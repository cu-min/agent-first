use std::time::Duration;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
};
use sqlx::Row;
use time::OffsetDateTime;
use tracing::info;
use uuid::Uuid;

use crate::{
    auth::{ListPrincipal, require_agent, require_developer, resolve_list_principal},
    authz::{
        can_read_memory_principal, can_read_row_principal, ensure_workspace_owner,
        load_memory_access,
    },
    error::{ApiError, ApiResult},
    models::{
        EvidenceRecord, FeedbackRecord, ListMemoriesQuery, MemoryCreatedOutput, MemoryDetail,
        MemoryImportInput, MemoryImportedOutput, MemoryInput, MemoryListOutput, RelationRecord,
    },
    ratelimit::ensure_rate,
    state::{AppState, IMPORT_BATCH_MAXIMUM},
    store::{fetch_memory_summaries, insert_memory},
    validation::validate_memory_input,
};

pub(crate) async fn create_memory(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<MemoryInput>,
) -> ApiResult<Json<MemoryCreatedOutput>> {
    let agent = require_agent(&state, &headers).await?;
    ensure_rate(
        &state,
        format!("memory:{}", agent.agent_id),
        30,
        Duration::from_secs(3600),
    )
    .await?;
    validate_memory_input(&input)?;
    let mut transaction = state.pool.begin().await?;
    let inserted = insert_memory(&state, &mut transaction, &agent, input, "agent").await?;
    transaction.commit().await?;
    Ok(Json(MemoryCreatedOutput {
        id: inserted.id,
        visibility: inserted.visibility.as_str().to_owned(),
        publication_state: if inserted.published {
            "published"
        } else if inserted.publication_requested {
            "pending_owner"
        } else {
            "private_or_shared"
        },
    }))
}

pub(crate) async fn import_memories(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<MemoryImportInput>,
) -> ApiResult<Json<MemoryImportedOutput>> {
    let agent = require_agent(&state, &headers).await?;
    ensure_rate(
        &state,
        format!("import:{}", agent.agent_id),
        5,
        Duration::from_secs(3600),
    )
    .await?;
    if input.memories.is_empty() {
        return Err(ApiError::bad_request("导入列表不能为空"));
    }
    if input.memories.len() > IMPORT_BATCH_MAXIMUM {
        return Err(ApiError::bad_request(format!(
            "单次最多导入 {IMPORT_BATCH_MAXIMUM} 条记忆"
        )));
    }
    for item in &input.memories {
        validate_memory_input(item)?;
    }
    let mut transaction = state.pool.begin().await?;
    let mut ids = Vec::with_capacity(input.memories.len());
    for item in input.memories {
        let source_type = if item.request_public {
            "public_import"
        } else {
            "agent"
        };
        ids.push(
            insert_memory(&state, &mut transaction, &agent, item, source_type)
                .await?
                .id,
        );
    }
    transaction.commit().await?;
    info!(agent_id = %agent.agent_id, count = ids.len(), "memories imported");
    Ok(Json(MemoryImportedOutput {
        imported: ids.len(),
        ids,
    }))
}

pub(crate) async fn list_memories(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ListMemoriesQuery>,
) -> ApiResult<Json<MemoryListOutput>> {
    let limit = query.limit.unwrap_or(20).clamp(1, 100);
    let offset = query.offset.unwrap_or(0).max(0);
    let principal = resolve_list_principal(&state, &headers).await?;

    let vis_filter = query
        .visibility
        .as_deref()
        .filter(|v| ["public", "developer_shared", "agent_private"].contains(v));
    let outcome_filter = query
        .outcome_kind
        .as_deref()
        .filter(|v| ["success", "failure", "partial", "unknown"].contains(v));
    let since = query.since.as_deref().and_then(|s| {
        OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339).ok()
    });
    let until = query.until.as_deref().and_then(|s| {
        OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339).ok()
    });
    let order = match query.order_by.as_deref() {
        Some("reuse") => "m.agent_positive_feedback DESC, m.created_at DESC",
        Some("feedback") => "m.human_positive_feedback DESC, m.created_at DESC",
        Some("evidence") => "m.evidence_count DESC, m.created_at DESC",
        _ => "m.created_at DESC",
    };

    let (total, ids) = match principal {
        ListPrincipal::Agent(agent) => {
            let mut conditions = vec!["m.removed_at IS NULL".to_string(), "(m.visibility = 'public' OR (m.workspace_id = $1 AND m.visibility = 'developer_shared') OR (m.author_agent_id = $2 AND m.visibility = 'agent_private'))".to_string()];
            let mut param_idx = 3;
            if let Some(_v) = vis_filter {
                conditions.push(format!("m.visibility = ${}", param_idx));
                param_idx += 1;
            }
            if let Some(_v) = outcome_filter {
                conditions.push(format!("m.outcome_kind = ${}", param_idx));
                param_idx += 1;
            }
            if since.is_some() {
                conditions.push(format!("m.created_at >= ${}", param_idx));
                param_idx += 1;
            }
            if until.is_some() {
                conditions.push(format!("m.created_at <= ${}", param_idx));
                param_idx += 1;
            }
            let where_clause = conditions.join(" AND ");
            let total_sql = format!("SELECT count(*) FROM memories m WHERE {}", where_clause);
            let ids_sql = format!(
                "SELECT m.id FROM memories m WHERE {} ORDER BY {} LIMIT ${} OFFSET ${}",
                where_clause,
                order,
                param_idx,
                param_idx + 1
            );

            let mut total_query = sqlx::query_scalar::<_, i64>(&total_sql)
                .bind(agent.workspace_id)
                .bind(agent.agent_id);
            let mut ids_query = sqlx::query(&ids_sql)
                .bind(agent.workspace_id)
                .bind(agent.agent_id);
            if let Some(v) = vis_filter {
                total_query = total_query.bind(v);
                ids_query = ids_query.bind(v);
            }
            if let Some(v) = outcome_filter {
                total_query = total_query.bind(v);
                ids_query = ids_query.bind(v);
            }
            if let Some(s) = since {
                total_query = total_query.bind(s);
                ids_query = ids_query.bind(s);
            }
            if let Some(u) = until {
                total_query = total_query.bind(u);
                ids_query = ids_query.bind(u);
            }
            total_query = total_query.bind(limit).bind(offset);
            ids_query = ids_query.bind(limit).bind(offset);

            let total = total_query.fetch_one(&state.pool).await?;
            let ids: Vec<Uuid> = ids_query
                .fetch_all(&state.pool)
                .await?
                .into_iter()
                .map(|row| row.get("id"))
                .collect();
            (total, ids)
        }
        ListPrincipal::Developer(developer) => {
            let mut conditions = vec![
                "m.removed_at IS NULL".to_string(),
                "w.developer_id = $1".to_string(),
            ];
            let mut param_idx = 2;
            if let Some(_v) = vis_filter {
                conditions.push(format!("m.visibility = ${}", param_idx));
                param_idx += 1;
            }
            if let Some(_v) = outcome_filter {
                conditions.push(format!("m.outcome_kind = ${}", param_idx));
                param_idx += 1;
            }
            if since.is_some() {
                conditions.push(format!("m.created_at >= ${}", param_idx));
                param_idx += 1;
            }
            if until.is_some() {
                conditions.push(format!("m.created_at <= ${}", param_idx));
                param_idx += 1;
            }
            let where_clause = conditions.join(" AND ");
            let total_sql = format!(
                "SELECT count(*) FROM memories m JOIN workspaces w ON w.id = m.workspace_id WHERE {}",
                where_clause
            );
            let ids_sql = format!(
                "SELECT m.id FROM memories m JOIN workspaces w ON w.id = m.workspace_id WHERE {} ORDER BY {} LIMIT ${} OFFSET ${}",
                where_clause,
                order,
                param_idx,
                param_idx + 1
            );

            let mut total_query =
                sqlx::query_scalar::<_, i64>(&total_sql).bind(developer.developer_id);
            let mut ids_query = sqlx::query(&ids_sql).bind(developer.developer_id);
            if let Some(v) = vis_filter {
                total_query = total_query.bind(v);
                ids_query = ids_query.bind(v);
            }
            if let Some(v) = outcome_filter {
                total_query = total_query.bind(v);
                ids_query = ids_query.bind(v);
            }
            if let Some(s) = since {
                total_query = total_query.bind(s);
                ids_query = ids_query.bind(s);
            }
            if let Some(u) = until {
                total_query = total_query.bind(u);
                ids_query = ids_query.bind(u);
            }
            total_query = total_query.bind(limit).bind(offset);
            ids_query = ids_query.bind(limit).bind(offset);

            let total = total_query.fetch_one(&state.pool).await?;
            let ids: Vec<Uuid> = ids_query
                .fetch_all(&state.pool)
                .await?
                .into_iter()
                .map(|row| row.get("id"))
                .collect();
            (total, ids)
        }
    };
    let items = fetch_memory_summaries(&state.pool, &ids).await?;
    Ok(Json(MemoryListOutput {
        items,
        total,
        limit,
        offset,
    }))
}

pub(crate) async fn get_memory(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<MemoryDetail>> {
    let principal = crate::auth::resolve_read_principal(&state, &headers).await?;
    if !can_read_memory_principal(&state.pool, id, &principal).await? {
        return Err(ApiError::not_found("记忆不存在或不可访问"));
    }
    let memory = fetch_memory_summaries(&state.pool, &[id])
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| ApiError::not_found("记忆不存在"))?;
    let evidence = sqlx::query_as::<_, EvidenceRecord>(
        "SELECT id, kind, label, value, created_at FROM memory_evidence WHERE memory_id = $1 ORDER BY created_at ASC",
    ).bind(id).fetch_all(&state.pool).await?;
    let relations = sqlx::query_as::<_, RelationRecord>(
        "SELECT target_memory_id, relation_type, created_at FROM memory_relations WHERE source_memory_id = $1 ORDER BY created_at ASC",
    ).bind(id).fetch_all(&state.pool).await?;
    Ok(Json(MemoryDetail {
        memory,
        evidence,
        relations,
        untrusted_content: true,
    }))
}

pub(crate) async fn list_memory_feedback(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<FeedbackRecord>>> {
    let memory = load_memory_access(&state.pool, id).await?;
    let principal = crate::auth::resolve_read_principal(&state, &headers).await?;
    if !can_read_row_principal(&memory, &principal) {
        return Err(ApiError::not_found("记忆不存在或不可访问"));
    }
    let rows = sqlx::query_as::<_, FeedbackRecord>(
        "SELECT source_type, verdict, note, created_at FROM memory_feedback WHERE memory_id = $1 ORDER BY created_at DESC LIMIT 100",
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

pub(crate) async fn publish_memory(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    let developer = require_developer(&state, &headers).await?;
    let memory = load_memory_access(&state.pool, id).await?;
    ensure_workspace_owner(&state.pool, memory.workspace_id, developer.developer_id).await?;
    if memory.removed_at.is_some() {
        return Err(ApiError::bad_request("已移除的记忆不能公开"));
    }
    sqlx::query("UPDATE memories SET visibility = 'public', published_at = now(), publication_requested_at = NULL WHERE id = $1")
        .bind(id).execute(&state.pool).await?;
    Ok(Json(
        serde_json::json!({ "id": id, "visibility": "public" }),
    ))
}

pub(crate) async fn remove_memory(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    let developer = require_developer(&state, &headers).await?;
    let memory = load_memory_access(&state.pool, id).await?;
    ensure_workspace_owner(&state.pool, memory.workspace_id, developer.developer_id).await?;
    let mut transaction = state.pool.begin().await?;
    sqlx::query("DELETE FROM memory_evidence WHERE memory_id = $1")
        .bind(id)
        .execute(&mut *transaction)
        .await?;
    sqlx::query("DELETE FROM memory_feedback WHERE memory_id = $1")
        .bind(id)
        .execute(&mut *transaction)
        .await?;
    sqlx::query(
        "DELETE FROM memory_relations WHERE source_memory_id = $1 OR target_memory_id = $1",
    )
    .bind(id)
    .execute(&mut *transaction)
    .await?;
    sqlx::query("DELETE FROM gap_memory_links WHERE memory_id = $1")
        .bind(id)
        .execute(&mut *transaction)
        .await?;
    sqlx::query("UPDATE memories SET problem = '[已移除]', conditions = '{}'::jsonb, action = '[已移除]', outcome = '[已移除]', tags = '{}', search_text = '', embedding = NULL, removed_at = now(), removed_reason = 'owner_request' WHERE id = $1")
        .bind(id).execute(&mut *transaction).await?;
    transaction.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
