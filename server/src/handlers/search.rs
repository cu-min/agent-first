use std::{collections::HashMap, net::SocketAddr, time::Duration};

use axum::{
    Json,
    extract::{ConnectInfo, State},
    http::HeaderMap,
};
use tracing::info;
use uuid::Uuid;

use crate::{
    auth::{ReadPrincipal, resolve_read_principal},
    embed::embed_with_breaker,
    error::{ApiError, ApiResult},
    handlers::gaps::related_gaps_for_query,
    models::{SearchDetail, SearchHit, SearchInput, SearchOutput},
    net::client_ip,
    ratelimit::ensure_rate,
    search::{grade_hit, lexical_candidates, merge_ranks, semantic_candidates},
    security,
    state::AppState,
    store::fetch_memory_summaries,
    validation::normalize_optional,
};

pub(crate) async fn search(
    State(state): State<AppState>,
    ConnectInfo(address): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(input): Json<SearchInput>,
) -> ApiResult<Json<SearchOutput>> {
    ensure_rate(
        &state,
        format!(
            "search:{}",
            client_ip(&address, &headers, &state.trusted_proxies)
        ),
        60,
        Duration::from_secs(60),
    )
    .await?;
    security::validate_text(&input.query, "查询", 2, 300).map_err(ApiError::bad_request)?;
    let tags = security::normalize_tags(input.tags).map_err(ApiError::bad_request)?;
    let language = normalize_optional(&input.language, "语言", 20)?;
    let technology = normalize_optional(&input.technology, "技术", 80)?;
    let limit = input.limit.unwrap_or(5).clamp(1, 20) as usize;
    let principal = resolve_read_principal(&state, &headers).await?;
    let lexical = lexical_candidates(
        &state,
        &input.query,
        language.as_deref(),
        &tags,
        technology.as_deref(),
        &principal,
    )
    .await?;
    let query_vector = embed_with_breaker(&state, &input.query).await;
    let semantic = match &query_vector {
        Some(vector) => {
            semantic_candidates(
                &state,
                vector,
                language.as_deref(),
                &tags,
                technology.as_deref(),
                &principal,
            )
            .await?
        }
        None => Vec::new(),
    };
    // 缺口常驻返回：不依赖经验命中数触发，语义向量缺失时为空数组
    let related_gaps = match &query_vector {
        Some(vector) => related_gaps_for_query(&state, vector, &principal).await?,
        None => Vec::new(),
    };
    let summaries =
        fetch_memory_summaries(&state.pool, &merge_ranks(&lexical, &semantic, limit)).await?;
    let include_action = input.detail == SearchDetail::Full;
    let semantic_scores: HashMap<Uuid, f64> = semantic.iter().copied().collect();
    let items: Vec<SearchHit> = summaries
        .into_iter()
        .map(|summary| {
            let score = semantic_scores.get(&summary.id).copied();
            SearchHit::from_summary(
                summary,
                include_action,
                score,
                grade_hit(score, state.thresholds.semantic_exact_min),
            )
        })
        .collect();
    let retrieval = if semantic.is_empty() {
        "lexical"
    } else {
        "hybrid_rrf"
    };
    // 查询日志：query 已过 validate_text 敏感拦截（含密钥/邮箱/手机号的请求在上方被 400 拒绝，不会到达这里）
    info!(
        query = %input.query,
        ip = %client_ip(&address, &headers, &state.trusted_proxies),
        principal = principal_kind(&principal),
        language = language.as_deref().unwrap_or("-"),
        hits = items.len(),
        gaps = related_gaps.len(),
        retrieval,
        "search served"
    );
    Ok(Json(SearchOutput {
        items,
        related_gaps,
        retrieval,
        untrusted_content: true,
    }))
}

fn principal_kind(principal: &ReadPrincipal) -> &'static str {
    match principal {
        ReadPrincipal::Agent(_) => "agent",
        ReadPrincipal::Developer { .. } => "developer",
        ReadPrincipal::Anonymous => "anonymous",
    }
}
