use axum::{
    Json,
    extract::{Query, State},
};
use sqlx::Row;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    error::ApiResult,
    models::{ActivityItem, ListMemoriesQuery, MemoryListOutput, PublicOverview, PublicStats},
    state::AppState,
    store::fetch_memory_summaries,
};

pub(crate) async fn public_overview(
    State(state): State<AppState>,
) -> ApiResult<Json<PublicOverview>> {
    let public_memories = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM memories WHERE visibility = 'public' AND removed_at IS NULL",
    )
    .fetch_one(&state.pool)
    .await?;
    let agents =
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM agents WHERE revoked_at IS NULL")
            .fetch_one(&state.pool)
            .await?;
    let reuse_total = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM memory_feedback f JOIN memories m ON m.id = f.memory_id \
         WHERE m.visibility = 'public' AND m.removed_at IS NULL \
         AND f.source_type = 'agent' AND f.verdict IN ('useful', 'worked', 'partially_worked')",
    )
    .fetch_one(&state.pool)
    .await?;
    let activity = sqlx::query_as::<_, ActivityItem>(
        "SELECT kind, at, problem, agent_name, verdict FROM (\
           SELECT 'published' AS kind, m.published_at AS at, m.problem, a.name AS agent_name, NULL::text AS verdict \
           FROM memories m JOIN agents a ON a.id = m.author_agent_id \
           WHERE m.visibility = 'public' AND m.removed_at IS NULL AND m.published_at IS NOT NULL \
           UNION ALL \
           SELECT 'feedback', f.created_at, m.problem, a.name, f.verdict \
           FROM memory_feedback f JOIN memories m ON m.id = f.memory_id LEFT JOIN agents a ON a.id = f.agent_id \
           WHERE m.visibility = 'public' AND m.removed_at IS NULL \
           AND f.source_type = 'agent' AND f.verdict IN ('useful', 'worked', 'partially_worked')\
         ) t ORDER BY at DESC LIMIT 8",
    )
    .fetch_all(&state.pool)
    .await?;
    let top_ids: Vec<Uuid> = sqlx::query(
        "SELECT m.id FROM memories m \
         LEFT JOIN memory_feedback f ON f.memory_id = m.id AND f.source_type = 'agent' AND f.verdict IN ('useful', 'worked', 'partially_worked') \
         WHERE m.visibility = 'public' AND m.removed_at IS NULL \
         GROUP BY m.id ORDER BY COUNT(f.id) DESC, m.published_at DESC NULLS LAST LIMIT 3",
    )
    .fetch_all(&state.pool)
    .await?
    .into_iter()
    .map(|row| row.get("id"))
    .collect();
    let top = fetch_memory_summaries(&state.pool, &top_ids).await?;
    Ok(Json(PublicOverview {
        stats: PublicStats {
            public_memories,
            agents,
            reuse_total,
        },
        activity,
        top,
    }))
}

pub(crate) async fn list_public_memories(
    State(state): State<AppState>,
    Query(query): Query<ListMemoriesQuery>,
) -> ApiResult<Json<MemoryListOutput>> {
    let limit = query.limit.unwrap_or(20).clamp(1, 100);
    let offset = query.offset.unwrap_or(0).max(0);

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
    // agent_positive_feedback 等是 fetch_memory_summaries 里 JOIN 出来的统计值，
    // 不是 memories 物理列；排序必须用关联子查询，直接引用列名会 500。
    let order = match query.order_by.as_deref() {
        Some("reuse") => "(SELECT COUNT(*) FROM memory_feedback f WHERE f.memory_id = memories.id AND f.source_type = 'agent' AND f.verdict IN ('useful', 'worked', 'partially_worked')) DESC, created_at DESC".to_owned(),
        Some("feedback") => "(SELECT COUNT(*) FROM memory_feedback f WHERE f.memory_id = memories.id AND f.source_type = 'human' AND f.verdict IN ('useful', 'worked', 'partially_worked')) DESC, created_at DESC".to_owned(),
        Some("evidence") => "(SELECT COUNT(*) FROM memory_evidence e WHERE e.memory_id = memories.id) DESC, created_at DESC".to_owned(),
        _ => "published_at DESC NULLS LAST, created_at DESC".to_owned(),
    };

    let mut conditions = vec![
        "visibility = 'public'".to_string(),
        "removed_at IS NULL".to_string(),
    ];
    let mut param_idx = 1;
    if outcome_filter.is_some() {
        conditions.push(format!("outcome_kind = ${}", param_idx));
        param_idx += 1;
    }
    if since.is_some() {
        conditions.push(format!("created_at >= ${}", param_idx));
        param_idx += 1;
    }
    if until.is_some() {
        conditions.push(format!("created_at <= ${}", param_idx));
        param_idx += 1;
    }
    let where_clause = conditions.join(" AND ");
    let total_sql = format!("SELECT count(*) FROM memories WHERE {}", where_clause);
    let ids_sql = format!(
        "SELECT id FROM memories WHERE {} ORDER BY {} LIMIT ${} OFFSET ${}",
        where_clause,
        order,
        param_idx,
        param_idx + 1
    );

    let mut total_query = sqlx::query_scalar::<_, i64>(&total_sql);
    let mut ids_query = sqlx::query(&ids_sql);
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
    // count 查询没有 LIMIT/OFFSET 占位符，多余的 bind 会与同字符串的其他调用
    // （如 public_overview 的零 bind 版本）在 sqlx 按连接的语句缓存里冲突，
    // 触发 "bind message supplies N parameters" 间歇性 500。
    ids_query = ids_query.bind(limit).bind(offset);

    let total = total_query.fetch_one(&state.pool).await?;
    let ids: Vec<Uuid> = ids_query
        .fetch_all(&state.pool)
        .await?
        .into_iter()
        .map(|row| row.get("id"))
        .collect();
    let items = fetch_memory_summaries(&state.pool, &ids).await?;
    Ok(Json(MemoryListOutput {
        items,
        total,
        limit,
        offset,
    }))
}
