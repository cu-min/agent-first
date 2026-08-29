use std::{
    sync::Mutex as StdMutex,
    time::{Duration, Instant},
};

use serde::Deserialize;
use serde_json::json;
use tracing::warn;

use crate::{
    error::{ApiError, ApiResult},
    state::AppState,
};

const BREAKER_THRESHOLD: u32 = 3;
const BREAKER_COOLDOWN_SECS: u64 = 30;
// 与 migrations/0002 中 vector(1024) 列类型保持一致（bge-m3 输出 1024 维）
pub(crate) const EMBEDDING_DIM: usize = 1024;

#[derive(Default)]
pub(crate) struct EmbeddingBreaker {
    inner: StdMutex<BreakerState>,
}

#[derive(Default)]
struct BreakerState {
    consecutive_failures: u32,
    open_until: Option<Instant>,
}

impl EmbeddingBreaker {
    pub(crate) fn allow(&self) -> bool {
        let mut state = self.inner.lock().unwrap();
        match state.open_until {
            Some(until) if until > Instant::now() => false,
            _ => {
                state.open_until = None;
                true
            }
        }
    }

    pub(crate) fn record_success(&self) {
        let mut state = self.inner.lock().unwrap();
        state.consecutive_failures = 0;
        state.open_until = None;
    }

    pub(crate) fn record_failure(&self) {
        let mut state = self.inner.lock().unwrap();
        state.consecutive_failures += 1;
        if state.consecutive_failures >= BREAKER_THRESHOLD {
            state.open_until = Some(Instant::now() + Duration::from_secs(BREAKER_COOLDOWN_SECS));
            state.consecutive_failures = 0;
        }
    }
}

pub(crate) async fn embed_with_breaker(state: &AppState, input: &str) -> Option<String> {
    if state.embeddings.is_none() {
        return None;
    }
    if !state.embed_breaker.allow() {
        warn!("embedding circuit open; skipping semantic retrieval");
        return None;
    }
    match embed(state, input).await {
        Ok(Some(vector)) => {
            state.embed_breaker.record_success();
            Some(vector)
        }
        Ok(None) => None,
        Err(error) => {
            warn!(error = %error, "embedding failed; recording breaker failure");
            state.embed_breaker.record_failure();
            None
        }
    }
}

pub(crate) async fn embed(state: &AppState, input: &str) -> ApiResult<Option<String>> {
    let Some(config) = &state.embeddings else {
        return Ok(None);
    };
    let response = state
        .http
        .post(&config.endpoint)
        .bearer_auth(&config.api_key)
        // 显式锁定维度：数据库列固定 vector(1024)，模型侧若不传 dimensions
        // （如智谱 embedding-3）会默认返回其他维度，导致写入/检索被维度校验拒绝。
        .json(&json!({ "model": config.model, "input": input, "dimensions": EMBEDDING_DIM }))
        .send()
        .await
        .map_err(|_| ApiError::internal("Embedding 服务暂时不可用"))?;
    if !response.status().is_success() {
        return Err(ApiError::internal("Embedding 服务返回异常"));
    }
    #[derive(Deserialize)]
    struct EmbeddingResponse {
        data: Vec<EmbeddingData>,
    }
    #[derive(Deserialize)]
    struct EmbeddingData {
        embedding: Vec<f32>,
    }
    let data: EmbeddingResponse = response
        .json()
        .await
        .map_err(|_| ApiError::internal("Embedding 响应格式无效"))?;
    let vector = data
        .data
        .into_iter()
        .next()
        .map(|item| item.embedding)
        .unwrap_or_default();
    if vector.len() != EMBEDDING_DIM || vector.iter().any(|item| !item.is_finite()) {
        return Err(ApiError::internal(format!(
            "Embedding 向量维度应为 {EMBEDDING_DIM}"
        )));
    }
    Ok(Some(format!(
        "[{}]",
        vector
            .iter()
            .map(|item| item.to_string())
            .collect::<Vec<_>>()
            .join(",")
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn circuit_breaker_opens_after_repeated_failures() {
        let breaker = EmbeddingBreaker::default();
        assert!(breaker.allow());
        breaker.record_failure();
        assert!(breaker.allow());
        breaker.record_failure();
        breaker.record_failure();
        assert!(!breaker.allow());
    }

    #[test]
    fn circuit_breaker_recovers_after_success() {
        let breaker = EmbeddingBreaker::default();
        for _ in 0..BREAKER_THRESHOLD {
            breaker.record_failure();
        }
        assert!(!breaker.allow());
        breaker.record_success();
        assert!(breaker.allow());
    }

    #[test]
    fn two_failures_keep_circuit_closed() {
        let breaker = EmbeddingBreaker::default();
        breaker.record_failure();
        breaker.record_failure();
        assert!(breaker.allow());
    }
}
