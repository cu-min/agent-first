use std::sync::Arc;

use reqwest::Client;
use sqlx::PgPool;

use crate::{
    config::EmbeddingConfig, embed::EmbeddingBreaker, net::TrustedProxy, ratelimit::RateLimiter,
};

pub(crate) const IMPORT_BATCH_MAXIMUM: usize = 100;

#[derive(Clone)]
pub struct AppState {
    pub(crate) pool: PgPool,
    pub(crate) embeddings: Option<EmbeddingConfig>,
    pub(crate) http: Client,
    pub(crate) limiter: Arc<RateLimiter>,
    pub(crate) embed_breaker: Arc<EmbeddingBreaker>,
    pub(crate) trusted_proxies: Vec<TrustedProxy>,
    pub(crate) thresholds: SearchThresholds,
}

impl AppState {
    pub fn new(
        pool: PgPool,
        embeddings: Option<EmbeddingConfig>,
        trusted_proxies: Vec<TrustedProxy>,
        thresholds: SearchThresholds,
    ) -> Result<Self, reqwest::Error> {
        Ok(Self {
            pool,
            embeddings,
            http: build_http_client()?,
            limiter: Arc::new(RateLimiter::default()),
            embed_breaker: Arc::new(EmbeddingBreaker::default()),
            trusted_proxies,
            thresholds,
        })
    }
}

#[derive(Clone, Copy)]
pub struct SearchThresholds {
    pub lexical_min: f64,
    pub semantic_min: f64,
}

fn build_http_client() -> Result<Client, reqwest::Error> {
    Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .no_proxy()
        .build()
}
