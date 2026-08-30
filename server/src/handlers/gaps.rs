use std::{collections::HashMap, time::Duration};

use axum::{
    Json,
    extract::{Path, Query, State},
    http::HeaderMap,
};
use sqlx::Row;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    auth::{read_scope, resolve_read_principal, ReadPrincipal},
    authz::{can_read_row_principal, load_gap_access},
    error::{ApiError, ApiResult},
    models::{
        CreatedId, GapDetail, GapInput, GapListItem, GapListOutput, GapRecord, ListGapsQuery,
        MemoryAccessRow, RelatedGap, Visibility,
    },
    ratelimit::ensure_rate,
    search::{flatten_json_values, gap_semantic_candidates},
    security,
    state::AppState,
    store::fetch_memory_summaries,
    validation::{validate_json, validate_optional_text},
};

const GAP_SCOPE: &str = "g.removed_at IS NULL AND (g.visibility = 'public' \
  OR (cardinality($1::uuid[]) > 0 AND g.workspace_id = ANY($1::uuid[]) AND g.visibility = 'developer_shared') \
  OR (cardinality($2::uuid[]) > 0 AND g.workspace_id = ANY($2::uuid[])) \
  OR ($3::uuid IS NOT NULL AND g.author_agent_id = $3 AND g.visibility = 'agent_private'))";

const LINK_READABLE: &str = "(SELECT count(*) FROM gap_memory_links l JOIN memories m ON m.id = l.memory_id \
  WHERE l.gap_id = g.id AND m.removed_at IS NULL AND (m.visibility = 'public' \
  OR (cardinality($1::uuid[]) > 0 AND m.workspace_id = ANY($1::uuid[]) AND m.visibility = 'developer_shared') \
  OR (cardinality($2::uuid[]) > 0 AND m.workspace_id = ANY($2::uuid[])) \
  OR ($3::uuid IS NOT NULL AND m.author_agent_id = $3 AND m.visibility = 'agent_private')))";

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
        return Err(ApiError::forbidden("工作区认领后才能创建公开经验缺口"));
    }
    let language = input.language.unwrap_or_else(|| "zh-CN".to_owned());
    security::validate_text(&language, "语言", 2, 20).map_err(ApiError::bad_request)?;
    let gap_text = format!(
        "{}\n{}\n{}",
        input.question.trim(),
        flatten_json_values(&input.context),
        input.attempted.as_deref().map(str::trim).unwrap_or("")
    );
    let embedding = crate::embed::embed(&state, &gap_text).await.ok().flatten();
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO experience_gaps (id, workspace_id, author_agent_id, visibility, question, context, attempted, language, embedding) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9::vector)")
        .bind(id).bind(agent.workspace_id).bind(agent.agent_id).bind(visibility.as_str()).bind(input.question.trim())
        .bind(input.context).bind(input.attempted.map(|value| value.trim().to_owned())).bind(language).bind(embedding)
        .execute(&state.pool).await?;
    Ok(Json(CreatedId { id }))
}

pub(crate) async fn related_gaps_for_query(
    state: &AppState,
    vector: &str,
    principal: &ReadPrincipal,
) -> ApiResult<Vec<RelatedGap>> {
    let hits = gap_semantic_candidates(state, vector, principal, 5).await?;
    if hits.is_empty() {
        return Ok(Vec::new());
    }
    let (shared_workspaces, full_workspaces, agent_id) = read_scope(principal);
    let ids: Vec<Uuid> = hits.iter().map(|(id, _)| *id).collect();
    let rows = sqlx::query(&format!(
        "SELECT g.id, g.question, {} AS linked_count FROM experience_gaps g WHERE g.id = ANY($4::uuid[])",
        LINK_READABLE
    ))
    .bind(&shared_workspaces)
    .bind(&full_workspaces)
    .bind(agent_id)
    .bind(&ids)
    .fetch_all(&state.pool)
    .await?;
    let details: HashMap<Uuid, (String, i64)> = rows
        .into_iter()
        .map(|row| {
            (
                row.get::<Uuid, _>("id"),
                (row.get::<String, _>("question"), row.get::<i64, _>("linked_count")),
            )
        })
        .collect();
    // hits 按余弦距离升序（分数降序）返回，filter_map 保持该顺序
    Ok(hits
        .into_iter()
        .filter_map(|(id, score)| {
            details.get(&id).map(|(question, linked)| RelatedGap {
                id,
                question: question.clone(),
                closed: *linked > 0,
                score,
            })
        })
        .collect())
}

pub(crate) async fn list_gaps(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ListGapsQuery>,
) -> ApiResult<Json<GapListOutput>> {
    let limit = query.limit.unwrap_or(20).clamp(1, 100);
    let offset = query.offset.unwrap_or(0).max(0);
    let principal = resolve_read_principal(&state, &headers).await?;
    let (shared_workspaces, full_workspaces, agent_id) = read_scope(&principal);

    let vis_filter = query
        .visibility
        .as_deref()
        .filter(|v| ["public", "developer_shared", "agent_private"].contains(v));
    let status_filter = query
        .status
        .as_deref()
        .filter(|v| ["open", "closed"].contains(v));
    let language = query
        .language
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let since = query
        .since
        .as_deref()
        .and_then(|s| OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339).ok());
    let until = query
        .until
        .as_deref()
        .and_then(|s| OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339).ok());

    let mut conditions = vec![GAP_SCOPE.to_owned()];
    let mut param_idx = 4;
    if vis_filter.is_some() {
        conditions.push(format!("g.visibility = ${}", param_idx));
        param_idx += 1;
    }
    if language.is_some() {
        conditions.push(format!("g.language = ${}", param_idx));
        param_idx += 1;
    }
    if since.is_some() {
        conditions.push(format!("g.created_at >= ${}", param_idx));
        param_idx += 1;
    }
    if until.is_some() {
        conditions.push(format!("g.created_at <= ${}", param_idx));
        param_idx += 1;
    }
    if let Some(status) = status_filter {
        let compare = if status == "open" { "= 0" } else { "> 0" };
        conditions.push(format!("{} {}", LINK_READABLE, compare));
    }
    let order = match query.order_by.as_deref() {
        Some("linked") => format!("{} DESC, g.created_at DESC", LINK_READABLE),
        _ => "g.created_at DESC".to_owned(),
    };
    let where_clause = conditions.join(" AND ");
    let total_sql = format!("SELECT count(*) FROM experience_gaps g WHERE {}", where_clause);
    let list_sql = format!(
        "SELECT g.id, g.visibility, g.question, g.context, g.attempted, g.language, g.created_at, {} AS linked_count \
         FROM experience_gaps g WHERE {} ORDER BY {} LIMIT ${} OFFSET ${}",
        LINK_READABLE, where_clause, order, param_idx, param_idx + 1
    );

    let mut total_query = sqlx::query_scalar::<_, i64>(&total_sql)
        .bind(&shared_workspaces)
        .bind(&full_workspaces)
        .bind(agent_id);
    let mut list_query = sqlx::query_as::<_, GapListItem>(&list_sql)
        .bind(&shared_workspaces)
        .bind(&full_workspaces)
        .bind(agent_id);
    if let Some(v) = vis_filter {
        total_query = total_query.bind(v);
        list_query = list_query.bind(v);
    }
    if let Some(v) = language {
        total_query = total_query.bind(v);
        list_query = list_query.bind(v);
    }
    if let Some(s) = since {
        total_query = total_query.bind(s);
        list_query = list_query.bind(s);
    }
    if let Some(u) = until {
        total_query = total_query.bind(u);
        list_query = list_query.bind(u);
    }
    // count 查询不含 LIMIT/OFFSET 占位符，多余 bind 与语句缓存冲突（见 public.rs）
    let total = total_query.fetch_one(&state.pool).await?;
    let items = list_query
        .bind(limit)
        .bind(offset)
        .fetch_all(&state.pool)
        .await?;
    Ok(Json(GapListOutput {
        items,
        total,
        limit,
        offset,
    }))
}

pub(crate) async fn get_gap(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<GapDetail>> {
    let principal = resolve_read_principal(&state, &headers).await?;
    let access = load_gap_access(&state.pool, id).await?;
    if !can_read_row_principal(&access, &principal) {
        return Err(ApiError::not_found("经验缺口不存在或不可访问"));
    }
    let gap = sqlx::query_as::<_, GapRecord>(
        "SELECT id, visibility, question, context, attempted, language, created_at FROM experience_gaps WHERE id = $1 AND removed_at IS NULL",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| ApiError::not_found("经验缺口不存在"))?;
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
                    .is_some_and(|row| can_read_row_principal(row, &principal))
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
