use std::collections::HashMap;

use sqlx::PgPool;
use time::OffsetDateTime;
use tracing::warn;
use uuid::Uuid;

use crate::{
    authz::{can_read_gap, can_read_memory},
    embed::embed,
    error::{ApiError, ApiResult},
    models::{AgentPrincipal, MemoryInput, MemorySummary, Visibility},
    search::flatten_json_values,
    state::AppState,
};

pub(crate) struct InsertedMemory {
    pub(crate) id: Uuid,
    pub(crate) visibility: Visibility,
    pub(crate) published: bool,
    pub(crate) publication_requested: bool,
}

pub(crate) async fn insert_memory(
    state: &AppState,
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    agent: &AgentPrincipal,
    input: MemoryInput,
    source_type: &str,
) -> ApiResult<InsertedMemory> {
    let tags = crate::security::normalize_tags(input.tags).map_err(ApiError::bad_request)?;
    let language = input.language.unwrap_or_else(|| "zh-CN".to_owned());
    crate::security::validate_text(&language, "语言", 2, 20).map_err(ApiError::bad_request)?;
    let requested_visibility = input.visibility.unwrap_or(Visibility::AgentPrivate);
    if requested_visibility == Visibility::Public {
        return Err(ApiError::bad_request(
            "不能直接公开记忆，请使用 request_public",
        ));
    }
    let (visibility, publication_requested, published_at) = if input.request_public {
        if agent.developer_id.is_some() && agent.publication_policy == "auto" {
            (Visibility::Public, false, Some(OffsetDateTime::now_utc()))
        } else {
            (Visibility::DeveloperShared, true, None)
        }
    } else {
        (requested_visibility, false, None)
    };
    let search_text = format!(
        "{}\n{}\n{}\n{}\n{}",
        input.problem,
        flatten_json_values(&input.conditions),
        input.action,
        input.outcome,
        tags.join(" ")
    );
    // 写入失败不能静默吞掉（智谱 key 失效 401 时曾无声退化词法检索）：
    // 记一条 warn，记忆照常落库，后续可用回填脚本补向量。
    let embedding = match embed(state, &search_text).await {
        Ok(vector) => vector,
        Err(error) => {
            warn!(error = %error, problem = %input.problem, "embedding failed on write; memory saved without vector");
            None
        }
    };
    for relation in &input.relations {
        if !can_read_memory(&state.pool, relation.target_memory_id, Some(agent)).await? {
            return Err(ApiError::forbidden("不能关联不可访问的记忆"));
        }
    }
    if let Some(gap_id) = input.gap_id {
        if !can_read_gap(&state.pool, gap_id, agent).await? {
            return Err(ApiError::forbidden("不能关联不可访问的经验缺口"));
        }
    }
    let memory_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO memories (id, workspace_id, author_agent_id, source_type, visibility, problem, conditions, action, outcome, outcome_kind, language, tags, search_text, embedding, publication_requested_at, published_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14::vector, $15, $16)",
    )
    .bind(memory_id).bind(agent.workspace_id).bind(agent.agent_id).bind(source_type).bind(visibility.as_str())
    .bind(input.problem.trim()).bind(input.conditions).bind(input.action.trim()).bind(input.outcome.trim())
    .bind(input.outcome_kind.as_str()).bind(language).bind(tags).bind(search_text).bind(embedding)
    .bind(publication_requested.then(OffsetDateTime::now_utc)).bind(published_at)
    .execute(&mut **transaction).await?;
    for evidence in input.evidence {
        sqlx::query("INSERT INTO memory_evidence (id, memory_id, kind, label, value) VALUES ($1, $2, $3, $4, $5)")
            .bind(Uuid::new_v4()).bind(memory_id).bind(evidence.kind.as_str())
            .bind(evidence.label.map(|value| value.trim().to_owned())).bind(evidence.value.trim())
            .execute(&mut **transaction).await?;
    }
    for relation in input.relations {
        sqlx::query("INSERT INTO memory_relations (source_memory_id, target_memory_id, relation_type) VALUES ($1, $2, $3)")
            .bind(memory_id).bind(relation.target_memory_id).bind(relation.relation_type.as_str())
            .execute(&mut **transaction).await?;
    }
    if let Some(gap_id) = input.gap_id {
        sqlx::query("INSERT INTO gap_memory_links (gap_id, memory_id) VALUES ($1, $2)")
            .bind(gap_id)
            .bind(memory_id)
            .execute(&mut **transaction)
            .await?;
    }
    Ok(InsertedMemory {
        id: memory_id,
        visibility,
        published: published_at.is_some(),
        publication_requested,
    })
}

pub(crate) async fn fetch_memory_summaries(
    pool: &PgPool,
    ids: &[Uuid],
) -> ApiResult<Vec<MemorySummary>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let rows = sqlx::query_as::<_, MemorySummary>(
        "SELECT m.id, m.visibility, m.problem, m.conditions, m.action, m.outcome, m.outcome_kind, m.source_type, m.language, m.tags, m.created_at, \
         a.name AS author_agent_name, \
         COUNT(DISTINCT e.id)::bigint AS evidence_count, \
         COUNT(DISTINCT f.id) FILTER (WHERE f.source_type = 'agent' AND f.verdict IN ('useful', 'worked', 'partially_worked'))::bigint AS agent_positive_feedback, \
         COUNT(DISTINCT f.id) FILTER (WHERE f.source_type = 'human' AND f.verdict IN ('useful', 'worked', 'partially_worked'))::bigint AS human_positive_feedback \
         FROM memories m LEFT JOIN agents a ON a.id = m.author_agent_id LEFT JOIN memory_evidence e ON e.memory_id = m.id LEFT JOIN memory_feedback f ON f.memory_id = m.id \
         WHERE m.id = ANY($1) AND m.removed_at IS NULL GROUP BY m.id, a.name",
    ).bind(ids).fetch_all(pool).await?;
    let mut by_id: HashMap<Uuid, MemorySummary> =
        rows.into_iter().map(|row| (row.id, row)).collect();
    Ok(ids.iter().filter_map(|id| by_id.remove(id)).collect())
}
