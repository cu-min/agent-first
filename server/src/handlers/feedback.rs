use std::time::Duration;

use axum::{
    Json,
    extract::{Path, State},
    http::HeaderMap,
};
use uuid::Uuid;

use crate::{
    auth::{bearer_token, require_agent},
    authz::{can_read_memory, ensure_workspace_owner, load_memory_access},
    error::{ApiError, ApiResult},
    models::{CreatedId, DeveloperPrincipal, FeedbackInput},
    ratelimit::ensure_rate,
    security,
    state::AppState,
    validation::validate_optional_text,
};

pub(crate) async fn create_feedback(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(input): Json<FeedbackInput>,
) -> ApiResult<Json<CreatedId>> {
    validate_optional_text(&input.note, "反馈说明", 1200)?;
    validate_optional_text(&input.evidence, "反馈证据", 2000)?;
    let memory = load_memory_access(&state.pool, id).await?;
    if memory.removed_at.is_some() {
        return Err(ApiError::not_found("记忆不存在"));
    }
    let token_hash = security::hash_token(bearer_token(&headers)?);
    let developer = sqlx::query_as::<_, DeveloperPrincipal>(
        "SELECT d.id AS developer_id FROM developer_sessions s JOIN developers d ON d.id = s.developer_id WHERE s.token_hash = $1 AND s.revoked_at IS NULL AND s.expires_at > now()",
    ).bind(&token_hash).fetch_optional(&state.pool).await?;
    let feedback_id = Uuid::new_v4();
    if let Some(developer) = developer {
        ensure_workspace_owner(&state.pool, memory.workspace_id, developer.developer_id).await?;
        ensure_rate(
            &state,
            format!("feedback-dev:{}", developer.developer_id),
            60,
            Duration::from_secs(3600),
        )
        .await?;
        sqlx::query("INSERT INTO memory_feedback (id, memory_id, source_type, developer_id, verdict, note, evidence) VALUES ($1, $2, 'human', $3, $4, $5, $6)")
            .bind(feedback_id).bind(id).bind(developer.developer_id).bind(input.verdict.as_str())
            .bind(input.note.map(|value| value.trim().to_owned())).bind(input.evidence.map(|value| value.trim().to_owned()))
            .execute(&state.pool).await?;
    } else {
        let agent = require_agent(&state, &headers).await?;
        ensure_rate(
            &state,
            format!("feedback:{}", agent.agent_id),
            60,
            Duration::from_secs(3600),
        )
        .await?;
        if !can_read_memory(&state.pool, id, Some(&agent)).await? {
            return Err(ApiError::forbidden("不能反馈不可访问的记忆"));
        }
        sqlx::query("INSERT INTO memory_feedback (id, memory_id, source_type, agent_id, verdict, note, evidence) VALUES ($1, $2, 'agent', $3, $4, $5, $6)")
            .bind(feedback_id).bind(id).bind(agent.agent_id).bind(input.verdict.as_str())
            .bind(input.note.map(|value| value.trim().to_owned())).bind(input.evidence.map(|value| value.trim().to_owned()))
            .execute(&state.pool).await?;
    }
    Ok(Json(CreatedId { id: feedback_id }))
}
