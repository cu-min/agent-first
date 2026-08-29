//! agent-first 服务端：面向 Agent 的技术经验网络。
//! bin 侧仅保留启动入口，全部装配逻辑在此模块树中完成。

mod auth;
mod authz;
mod config;
mod embed;
mod error;
mod handlers;
mod models;
mod net;
mod ratelimit;
mod routes;
mod search;
mod security;
mod state;
mod store;
mod validation;

use std::{net::SocketAddr, time::Duration};

use sqlx::{PgPool, postgres::PgPoolOptions};
use tracing::{info, warn};

pub use crate::{
    config::AppConfig,
    routes::build_router,
    state::{AppState, SearchThresholds},
};

pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let _ = dotenvy::dotenv();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("agent_first=info".parse()?),
        )
        .init();
    let config = AppConfig::from_env()?;
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(&config.database_url)
        .await?;
    sqlx::migrate!("../migrations").run(&pool).await?;
    let state = AppState::new(
        pool,
        config.embeddings.clone(),
        config.trusted_proxies.clone(),
        config.thresholds,
    )?;
    spawn_session_cleanup(state.pool.clone());
    let app = build_router(state, &config);
    let listener = tokio::net::TcpListener::bind(config.bind_addr).await?;
    info!(address = %config.bind_addr, "agent-first is listening");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;
    Ok(())
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

fn spawn_session_cleanup(pool: PgPool) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(6 * 60 * 60));
        loop {
            ticker.tick().await;
            match sqlx::query(
                "DELETE FROM developer_sessions \
                 WHERE expires_at < now() - interval '7 days' \
                    OR revoked_at < now() - interval '30 days'",
            )
            .execute(&pool)
            .await
            {
                Ok(result) if result.rows_affected() > 0 => {
                    info!(
                        rows = result.rows_affected(),
                        "cleaned up expired developer sessions"
                    );
                }
                Ok(_) => {}
                Err(error) => warn!(error = %error, "session cleanup failed"),
            }
        }
    });
}
