use std::collections::HashMap;

use serde_json::Value;
use sqlx::Row;
use uuid::Uuid;

use crate::{
    auth::{ReadPrincipal, read_scope},
    error::ApiResult,
    state::AppState,
};

pub(crate) const SEARCH_CANDIDATES: i64 = 20;
const RRF_K: f64 = 60.0;

fn collect_json_values(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for value in map.values() {
                collect_json_values(value, out);
            }
        }
        Value::Array(items) => {
            for value in items {
                collect_json_values(value, out);
            }
        }
        Value::String(text) => out.push(text.clone()),
        Value::Number(number) => out.push(number.to_string()),
        Value::Bool(flag) => out.push(flag.to_string()),
        Value::Null => {}
    }
}

pub(crate) fn flatten_json_values(value: &Value) -> String {
    let mut parts = Vec::new();
    collect_json_values(value, &mut parts);
    parts.join(" ")
}

pub(crate) fn tokenize_query(query: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    for token in query.split(|character: char| !character.is_alphanumeric()) {
        let token = token.trim();
        if !token.is_empty() && !tokens.contains(&token.to_owned()) {
            tokens.push(token.to_owned());
        }
    }
    tokens.truncate(8);
    tokens
}

pub(crate) async fn lexical_candidates(
    state: &AppState,
    query: &str,
    language: Option<&str>,
    tags: &[String],
    technology: Option<&str>,
    principal: &ReadPrincipal,
) -> ApiResult<Vec<(Uuid, f64)>> {
    let mut patterns: Vec<String> = tokenize_query(query)
        .into_iter()
        .map(|token| format!("%{}%", token))
        .collect();
    if patterns.is_empty() {
        patterns.push(format!("%{}%", query));
    }
    let (shared_workspaces, full_workspaces, agent_id) = read_scope(principal);
    let rows = sqlx::query(
        "SELECT m.id, \
         GREATEST(similarity(m.search_text, $4), \
           (SELECT count(*) FROM unnest($8::text[]) AS p(pattern) WHERE m.search_text ILIKE p.pattern)::float8 \
           / cardinality($8::text[])) \
         + CASE m.outcome_kind WHEN 'success' THEN 0.05 WHEN 'partial' THEN 0.02 ELSE 0.0 END::float8 AS score \
         FROM memories m WHERE m.removed_at IS NULL \
         AND ($1::text IS NULL OR m.language = $1) \
         AND (cardinality($2::text[]) = 0 OR m.tags && $2) \
         AND ($3::text IS NULL OR m.conditions->'technologies' ? $3) \
         AND (m.search_text ILIKE ANY($8::text[]) OR similarity(m.search_text, $4) > 0.05) \
         AND (m.visibility = 'public' \
           OR (cardinality($5::uuid[]) > 0 AND m.workspace_id = ANY($5::uuid[]) AND m.visibility = 'developer_shared') \
           OR (cardinality($6::uuid[]) > 0 AND m.workspace_id = ANY($6::uuid[])) \
           OR ($7::uuid IS NOT NULL AND m.author_agent_id = $7 AND m.visibility = 'agent_private')) \
         ORDER BY score DESC, m.created_at DESC LIMIT $9",
    )
    .bind(language).bind(tags).bind(technology).bind(query)
    .bind(&shared_workspaces).bind(&full_workspaces).bind(agent_id).bind(&patterns).bind(SEARCH_CANDIDATES)
    .fetch_all(&state.pool).await?;
    let minimum = state.thresholds.lexical_min;
    Ok(rows
        .into_iter()
        .map(|row| (row.get::<Uuid, _>("id"), row.get::<f64, _>("score")))
        .filter(|(_, score)| *score >= minimum)
        .collect())
}

pub(crate) async fn semantic_candidates(
    state: &AppState,
    vector: &str,
    language: Option<&str>,
    tags: &[String],
    technology: Option<&str>,
    principal: &ReadPrincipal,
) -> ApiResult<Vec<(Uuid, f64)>> {
    let (shared_workspaces, full_workspaces, agent_id) = read_scope(principal);
    let rows = sqlx::query(
        "SELECT m.id, 1 - (m.embedding <=> $7::vector) AS score FROM memories m WHERE m.removed_at IS NULL AND m.embedding IS NOT NULL \
         AND ($1::text IS NULL OR m.language = $1) \
         AND (cardinality($2::text[]) = 0 OR m.tags && $2) \
         AND ($3::text IS NULL OR m.conditions->'technologies' ? $3) \
         AND (m.visibility = 'public' \
           OR (cardinality($4::uuid[]) > 0 AND m.workspace_id = ANY($4::uuid[]) AND m.visibility = 'developer_shared') \
           OR (cardinality($5::uuid[]) > 0 AND m.workspace_id = ANY($5::uuid[])) \
           OR ($6::uuid IS NOT NULL AND m.author_agent_id = $6 AND m.visibility = 'agent_private')) \
         ORDER BY m.embedding <=> $7::vector ASC LIMIT $8",
    )
    .bind(language).bind(tags).bind(technology).bind(&shared_workspaces).bind(&full_workspaces).bind(agent_id)
    .bind(vector).bind(SEARCH_CANDIDATES)
    .fetch_all(&state.pool).await?;
    let minimum = state.thresholds.semantic_min;
    Ok(rows
        .into_iter()
        .map(|row| (row.get::<Uuid, _>("id"), row.get::<f64, _>("score")))
        .filter(|(_, score)| *score >= minimum)
        .collect())
}

pub(crate) async fn gap_semantic_candidates(
    state: &AppState,
    vector: &str,
    principal: &ReadPrincipal,
    limit: usize,
) -> ApiResult<Vec<(Uuid, f64)>> {
    let (shared_workspaces, full_workspaces, agent_id) = read_scope(principal);
    let rows = sqlx::query(
        "SELECT g.id, 1 - (g.embedding <=> $4::vector) AS score FROM experience_gaps g \
         WHERE g.removed_at IS NULL AND g.embedding IS NOT NULL \
         AND (g.visibility = 'public' \
           OR (cardinality($1::uuid[]) > 0 AND g.workspace_id = ANY($1::uuid[]) AND g.visibility = 'developer_shared') \
           OR (cardinality($2::uuid[]) > 0 AND g.workspace_id = ANY($2::uuid[])) \
           OR ($3::uuid IS NOT NULL AND g.author_agent_id = $3 AND g.visibility = 'agent_private')) \
         ORDER BY g.embedding <=> $4::vector ASC LIMIT $5",
    )
    .bind(&shared_workspaces).bind(&full_workspaces).bind(agent_id)
    .bind(vector).bind(limit as i64)
    .fetch_all(&state.pool).await?;
    let minimum = state.thresholds.gap_min;
    Ok(rows
        .into_iter()
        .map(|row| (row.get::<Uuid, _>("id"), row.get::<f64, _>("score")))
        .filter(|(_, score)| *score >= minimum)
        .collect())
}

pub(crate) fn merge_ranks(
    lexical: &[(Uuid, f64)],
    semantic: &[(Uuid, f64)],
    limit: usize,
) -> Vec<Uuid> {
    let mut scores: HashMap<Uuid, f64> = HashMap::new();
    for (index, (id, _)) in lexical.iter().enumerate() {
        *scores.entry(*id).or_default() += 1.0 / (RRF_K + index as f64 + 1.0);
    }
    for (index, (id, _)) in semantic.iter().enumerate() {
        *scores.entry(*id).or_default() += 1.0 / (RRF_K + index as f64 + 1.0);
    }
    let mut ranked: Vec<_> = scores.into_iter().collect();
    ranked.sort_by(|left, right| {
        right
            .1
            .total_cmp(&left.1)
            .then_with(|| left.0.cmp(&right.0))
    });
    ranked.into_iter().take(limit).map(|(id, _)| id).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenize_splits_on_non_alphanumerics_and_dedupes() {
        let tokens = tokenize_query("Docker Compose 启动顺序-docker,失败");
        assert_eq!(
            tokens,
            vec!["Docker", "Compose", "启动顺序", "docker", "失败"]
                .into_iter()
                .map(str::to_owned)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn tokenize_keeps_at_most_eight_tokens() {
        let tokens = tokenize_query("a b c d e f g h i j");
        assert_eq!(tokens.len(), 8);
    }

    #[test]
    fn tokenize_empty_query_yields_no_tokens() {
        assert!(tokenize_query("!!! ---").is_empty());
    }

    #[test]
    fn flatten_json_values_collects_nested_strings() {
        let value = serde_json::json!({
            "technologies": ["postgres 17", "node 18"],
            "os": { "name": "Ubuntu 24.04", "memory_gb": 16 },
            "flag": true,
            "null_field": null
        });
        let flattened = flatten_json_values(&value);
        assert!(flattened.contains("postgres 17"));
        assert!(flattened.contains("Ubuntu 24.04"));
        assert!(flattened.contains("16"));
        assert!(flattened.contains("true"));
        assert!(!flattened.contains("null_field"));
    }

    #[test]
    fn merge_ranks_promotes_hits_in_both_channels() {
        let only_lexical = (Uuid::new_v4(), 0.9);
        let both = (Uuid::new_v4(), 0.5);
        let lexical = vec![only_lexical, both];
        let semantic = vec![both];
        let merged = merge_ranks(&lexical, &semantic, 2);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0], both.0);
    }

    #[test]
    fn merge_ranks_respects_limit() {
        let lexical: Vec<(Uuid, f64)> = (0..10).map(|_| (Uuid::new_v4(), 1.0)).collect();
        let merged = merge_ranks(&lexical, &[], 3);
        assert_eq!(merged.len(), 3);
    }

    #[test]
    fn merge_ranks_empty_inputs_return_empty() {
        assert!(merge_ranks(&[], &[], 5).is_empty());
    }

    #[test]
    fn merge_ranks_tie_breaks_by_id() {
        let left = Uuid::new_v4();
        let right = Uuid::new_v4();
        let (first, second) = if left < right {
            (left, right)
        } else {
            (right, left)
        };
        let merged = merge_ranks(&[(left, 0.5)], &[(right, 0.5)], 2);
        assert_eq!(merged, vec![first, second]);
    }
}
