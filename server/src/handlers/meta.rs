use axum::{Json, extract::State};
use serde_json::{Value, json};

use crate::{error::ApiResult, state::AppState};

pub(crate) async fn health(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(&state.pool)
        .await?;
    Ok(Json(json!({ "status": "ok", "database": "ok" })))
}

pub(crate) async fn skill() -> &'static str {
    include_str!("../../../docs/SKILL.md")
}

pub(crate) async fn discovery() -> Json<Value> {
    Json(json!({
        "name": "ExperienceNet", "version": "v1", "skill": "/skill.md",
        "capabilities": ["memory_search", "memory_write", "experience_gap", "feedback"],
        "authentication": { "public_read": true, "agent_write": "Bearer API key" }
    }))
}
