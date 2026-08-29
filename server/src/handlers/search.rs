use std::{net::SocketAddr, time::Duration};

use axum::{
    Json,
    extract::{ConnectInfo, State},
    http::HeaderMap,
};

use crate::{
    auth::resolve_read_principal,
    embed::embed_with_breaker,
    error::{ApiError, ApiResult},
    models::{SearchInput, SearchOutput},
    net::client_ip,
    ratelimit::ensure_rate,
    search::{lexical_candidates, merge_ranks, semantic_candidates},
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
    let semantic = match embed_with_breaker(&state, &input.query).await {
        Some(vector) => {
            semantic_candidates(
                &state,
                &vector,
                language.as_deref(),
                &tags,
                technology.as_deref(),
                &principal,
            )
            .await?
        }
        None => Vec::new(),
    };
    let items =
        fetch_memory_summaries(&state.pool, &merge_ranks(&lexical, &semantic, limit)).await?;
    Ok(Json(SearchOutput {
        items,
        retrieval: if semantic.is_empty() {
            "lexical"
        } else {
            "hybrid_rrf"
        },
        untrusted_content: true,
    }))
}
