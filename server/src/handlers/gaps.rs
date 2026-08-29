use std::{collections::HashMap, time::Duration};

use axum::{
    Json,
    extract::{Path, State},
    http::HeaderMap,
};
use sqlx::Row;
use uuid::Uuid;

use crate::{
    auth::optional_agent,
    authz::{can_read_gap_with_optional, can_read_row},
    error::{ApiError, ApiResult},
    models::{CreatedId, GapDetail, GapInput, GapRecord, MemoryAccessRow, Visibility},
    ratelimit::ensure_rate,
    security,
    state::AppState,
    store::fetch_memory_summaries,
    validation::{validate_json, validate_optional_text},
};

pub(crate) async fn create_gap(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<GapInput>,
) -> ApiResult<Json<CreatedId>> {
    let agent = crate::auth::require_agent(&state, &headers).await?;
    ensure_rate(
        &state,
        format!("gap:{}", agent.agent_id),
        20,
        Duration::from_secs(3600),
    )
    .await?;
    security::validate_text(&input.question, "缺口问题", 2, 1600).map_err(ApiError::bad_request)?;
    validate_json(&input.context, "缺口条件", 6000)?;
    validate_optional_text(&input.attempted, "已尝试内容", 2000)?;
    let visibility = input.visibility.unwrap_or(Visibility::DeveloperShared);
    if visibility == Visibility::Public && agent.developer_id.is_none() {
        return Err(ApiError::forbidden("工作区认领后才能创建公共经验缺口"));
    }
    let language = input.language.unwrap_or_else(|| "zh-CN".to_owned());
    security::validate_text(&language, "语言", 2, 20).map_err(ApiError::bad_request)?;
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO experience_gaps (id, workspace_id, author_agent_id, visibility, question, context, attempted, language) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)")
        .bind(id).bind(agent.workspace_id).bind(agent.agent_id).bind(visibility.as_str()).bind(input.question.trim())
        .bind(input.context).bind(input.attempted.map(|value| value.trim().to_owned())).bind(language)
        .execute(&state.pool).await?;
    Ok(Json(CreatedId { id }))
}

pub(crate) async fn get_gap(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<GapDetail>> {
    let agent = optional_agent(&state, &headers).await?;
    if !can_read_gap_with_optional(&state.pool, id, agent.as_ref()).await? {
        return Err(ApiError::not_found("经验缺口不存在或不可访问"));
    }
    let gap = sqlx::query_as::<_, GapRecord>(
        "SELECT id, visibility, question, context, attempted, language, created_at FROM experience_gaps WHERE id = $1 AND removed_at IS NULL",
    ).bind(id).fetch_optional(&state.pool).await?.ok_or_else(|| ApiError::not_found("经验缺口不存在"))?;
    let ids: Vec<Uuid> = sqlx::query(
        "SELECT memory_id FROM gap_memory_links WHERE gap_id = $1 ORDER BY created_at DESC",
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await?
    .into_iter()
    .map(|row| row.get("memory_id"))
    .collect();
    let readable_ids: Vec<Uuid> = if ids.is_empty() {
        Vec::new()
    } else {
        let rows = sqlx::query_as::<_, MemoryAccessRow>(
            "SELECT id, workspace_id, author_agent_id, visibility, removed_at FROM memories WHERE id = ANY($1)",
        )
        .bind(&ids)
        .fetch_all(&state.pool)
        .await?;
        let access: HashMap<Uuid, MemoryAccessRow> =
            rows.into_iter().map(|row| (row.id, row)).collect();
        ids.into_iter()
            .filter(|memory_id| {
                access
                    .get(memory_id)
                    .is_some_and(|row| can_read_row(row, agent.as_ref()))
            })
            .collect()
    };
    let memories = fetch_memory_summaries(&state.pool, &readable_ids).await?;
    Ok(Json(GapDetail {
        gap,
        memories,
        untrusted_content: true,
    }))
}
