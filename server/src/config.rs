use std::{env, net::SocketAddr, path::PathBuf};

use axum::http::HeaderValue;

use crate::{error::ApiError, net::TrustedProxy, state::SearchThresholds};

#[derive(Clone)]
pub struct EmbeddingConfig {
    pub endpoint: String,
    pub api_key: String,
    pub model: String,
}

pub struct AppConfig {
    pub database_url: String,
    pub bind_addr: SocketAddr,
    pub app_origin: HeaderValue,
    pub static_dir: PathBuf,
    pub embeddings: Option<EmbeddingConfig>,
    pub trusted_proxies: Vec<TrustedProxy>,
    pub thresholds: SearchThresholds,
}

impl AppConfig {
    pub fn from_env() -> Result<Self, ApiError> {
        let database_url = env::var("DATABASE_URL")
            .map_err(|_| ApiError::internal("缺少 DATABASE_URL 环境变量"))?;
        let bind_addr = env::var("BIND_ADDR")
            .unwrap_or_else(|_| "127.0.0.1:8080".to_owned())
            .parse()
            .map_err(|_| ApiError::internal("BIND_ADDR 格式无效"))?;
        let app_origin = HeaderValue::from_str(
            &env::var("APP_ORIGIN").unwrap_or_else(|_| "http://localhost:5173".to_owned()),
        )
        .map_err(|_| ApiError::internal("APP_ORIGIN 格式无效"))?;
        let embedding_vars = (
            env::var("EMBEDDING_ENDPOINT").ok(),
            env::var("EMBEDDING_API_KEY").ok(),
            env::var("EMBEDDING_MODEL").ok(),
        );
        let embeddings = match embedding_vars {
            (Some(endpoint), Some(api_key), Some(model))
                if !endpoint.is_empty() && !api_key.is_empty() && !model.is_empty() =>
            {
                Some(EmbeddingConfig {
                    endpoint,
                    api_key,
                    model,
                })
            }
            _ => None,
        };
        let thresholds = SearchThresholds {
            lexical_min: parse_score_env("SEARCH_LEXICAL_MIN_SCORE", 0.10),
            semantic_min: parse_score_env("SEARCH_SEMANTIC_MIN_SCORE", 0.35),
        };
        Ok(Self {
            database_url,
            bind_addr,
            app_origin,
            static_dir: PathBuf::from(
                env::var("STATIC_DIR").unwrap_or_else(|_| "../web/dist".to_owned()),
            ),
            embeddings,
            trusted_proxies: crate::net::parse_trusted_proxies(env::var("TRUSTED_PROXIES").ok()),
            thresholds,
        })
    }
}

pub(crate) fn parse_score_env(name: &str, default: f64) -> f64 {
    match env::var(name).ok().and_then(|raw| raw.parse::<f64>().ok()) {
        Some(value) if (0.0..=1.0).contains(&value) => value,
        _ => default,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 独占变量名避免并行测试污染；edition 2024 中 env 写入是 unsafe。
    #[test]
    fn parse_score_env_accepts_only_in_range_values() {
        const NAME: &str = "AGENT_FIRST_TEST_SCORE";
        unsafe { env::remove_var(NAME) };
        assert_eq!(parse_score_env(NAME, 0.5), 0.5);
        unsafe { env::set_var(NAME, "0.42") };
        assert_eq!(parse_score_env(NAME, 0.5), 0.42);
        unsafe { env::set_var(NAME, "0") };
        assert_eq!(parse_score_env(NAME, 0.5), 0.0);
        unsafe { env::set_var(NAME, "1") };
        assert_eq!(parse_score_env(NAME, 0.5), 1.0);
    }

    #[test]
    fn parse_score_env_falls_back_on_invalid_input() {
        const NAME: &str = "AGENT_FIRST_TEST_INVALID_SCORE";
        for raw in ["1.5", "-0.1", "abc", ""] {
            unsafe { env::set_var(NAME, raw) };
            assert_eq!(parse_score_env(NAME, 0.25), 0.25, "raw = {raw:?}");
        }
        unsafe { env::remove_var(NAME) };
    }
}
