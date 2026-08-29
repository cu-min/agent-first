use axum::{
    Router,
    http::{Method, header},
    routing::{delete, get, patch, post},
};
use tower_http::{
    cors::CorsLayer,
    limit::RequestBodyLimitLayer,
    services::{ServeDir, ServeFile},
    set_header::SetResponseHeaderLayer,
    trace::TraceLayer,
};

use crate::{
    config::AppConfig,
    handlers::{
        agents::{create_agent, register_agent, rename_agent, rotate_agent_key},
        developers::{
            claim_workspace, delete_developer_account, developer_overview, login,
            rotate_workspace_invite, update_publication_policy,
        },
        feedback::create_feedback,
        gaps::{create_gap, get_gap, list_gaps},
        memories::{
            create_memory, get_memory, import_memories, list_memories, list_memory_feedback,
            publish_memory, remove_memory,
        },
        meta::{discovery, health, skill},
        public::{list_public_memories, public_overview},
        search::search,
    },
    state::AppState,
};

pub fn build_router(state: AppState, config: &AppConfig) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(config.app_origin.clone())
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::PATCH, Method::DELETE])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE]);
    let security_headers = SetResponseHeaderLayer::if_not_present(
        header::CONTENT_SECURITY_POLICY,
        axum::http::HeaderValue::from_static(
            "default-src 'self'; base-uri 'self'; frame-ancestors 'none'; object-src 'none'",
        ),
    );
    let static_file = config.static_dir.join("index.html");
    let static_service =
        ServeDir::new(config.static_dir.clone()).not_found_service(ServeFile::new(static_file));
    Router::new()
        .route("/healthz", get(health))
        .route("/skill.md", get(skill))
        .route("/.well-known/agent-first.json", get(discovery))
        .route("/v1/search", post(search))
        .route("/v1/agents/register", post(register_agent))
        .route("/v1/agents", post(create_agent))
        .route("/v1/agents/{id}", patch(rename_agent))
        .route("/v1/agents/{id}/keys/rotate", post(rotate_agent_key))
        .route("/v1/developers/claim", post(claim_workspace))
        .route("/v1/developers/login", post(login))
        .route("/v1/developer/overview", get(developer_overview))
        .route("/v1/developer/account", delete(delete_developer_account))
        .route(
            "/v1/workspaces/{id}/publication-policy",
            post(update_publication_policy),
        )
        .route(
            "/v1/workspaces/{id}/invite/rotate",
            post(rotate_workspace_invite),
        )
        .route("/v1/public/overview", get(public_overview))
        .route("/v1/public/memories", get(list_public_memories))
        .route("/v1/memories", get(list_memories).post(create_memory))
        .route("/v1/memories/import", post(import_memories))
        .route("/v1/memories/{id}", get(get_memory))
        .route(
            "/v1/memories/{id}/feedback",
            get(list_memory_feedback).post(create_feedback),
        )
        .route("/v1/memories/{id}/publish", post(publish_memory))
        .route("/v1/memories/{id}/remove", post(remove_memory))
        .route("/v1/gaps", get(list_gaps).post(create_gap))
        .route("/v1/gaps/{id}", get(get_gap))
        .fallback_service(static_service)
        .layer(RequestBodyLimitLayer::new(2 * 1024 * 1024))
        .layer(security_headers)
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
