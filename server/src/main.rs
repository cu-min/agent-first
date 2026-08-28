mod security;

use std::{
    collections::{HashMap, HashSet},
    env,
    net::{IpAddr, SocketAddr},
    path::PathBuf,
    sync::{Arc, Mutex as StdMutex},
    time::{Duration, Instant},
};

use axum::{
    Json, Router,
    extract::{ConnectInfo, Path, Query, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{FromRow, PgPool, Row, postgres::PgPoolOptions};
use time::{Duration as TimeDuration, OffsetDateTime};
use tokio::sync::Mutex;
use tower_http::{
    cors::CorsLayer,
    limit::RequestBodyLimitLayer,
    services::{ServeDir, ServeFile},
    set_header::SetResponseHeaderLayer,
    trace::TraceLayer,
};
use tracing::{info, warn};
use uuid::Uuid;

const SEARCH_CANDIDATES: i64 = 20;
const RRF_K: f64 = 60.0;
const BREAKER_THRESHOLD: u32 = 3;
const BREAKER_COOLDOWN_SECS: u64 = 30;
// 与 migrations/0002 中 vector(1024) 列类型保持一致（bge-m3 输出 1024 维）
const EMBEDDING_DIM: usize = 1024;
const IMPORT_BATCH_MAXIMUM: usize = 100;

#[derive(Clone)]
struct AppState {
    pool: PgPool,
    embeddings: Option<EmbeddingConfig>,
    http: Client,
    limiter: Arc<RateLimiter>,
    embed_breaker: Arc<EmbeddingBreaker>,
    trusted_proxies: Vec<TrustedProxy>,
    thresholds: SearchThresholds,
}

#[derive(Clone, Copy)]
struct SearchThresholds {
    lexical_min: f64,
    semantic_min: f64,
}

#[derive(Clone, Copy)]
struct TrustedProxy {
    base: IpAddr,
    prefix: u8,
}

fn parse_trusted_proxies(raw: Option<String>) -> Vec<TrustedProxy> {
    let Some(raw) = raw else {
        return Vec::new();
    };
    raw.split(',')
        .filter_map(|entry| {
            let entry = entry.trim();
            if entry.is_empty() {
                return None;
            }
            let (base, prefix) = match entry.split_once('/') {
                Some((base, prefix)) => (base, prefix.parse::<u8>().ok()?),
                None => (entry, if entry.contains(':') { 128 } else { 32 }),
            };
            let base: IpAddr = base.parse().ok()?;
            let maximum = if base.is_ipv4() { 32 } else { 128 };
            if prefix > maximum {
                return None;
            }
            Some(TrustedProxy { base, prefix })
        })
        .collect()
}

fn ip_bits(ip: IpAddr) -> u128 {
    match ip {
        IpAddr::V4(value) => u32::from(value) as u128,
        IpAddr::V6(value) => u128::from(value),
    }
}

fn is_trusted_proxy(ip: IpAddr, trusted: &[TrustedProxy]) -> bool {
    trusted.iter().any(|proxy| {
        if proxy.base.is_ipv4() != ip.is_ipv4() {
            return false;
        }
        let width = if ip.is_ipv4() { 32 } else { 128 };
        let shift = width - u32::from(proxy.prefix);
        (ip_bits(ip) >> shift) == (ip_bits(proxy.base) >> shift)
    })
}

fn client_ip(address: &SocketAddr, headers: &HeaderMap, trusted: &[TrustedProxy]) -> IpAddr {
    let peer = address.ip();
    if !is_trusted_proxy(peer, trusted) {
        return peer;
    }
    if let Some(value) = headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
    {
        let candidates: Vec<IpAddr> = value
            .split(',')
            .filter_map(|entry| entry.trim().parse().ok())
            .collect();
        if let Some(client) = candidates
            .iter()
            .rev()
            .find(|candidate| !is_trusted_proxy(**candidate, trusted))
            .or_else(|| candidates.first())
        {
            return *client;
        }
    }
    if let Some(ip) = headers
        .get("x-real-ip")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse().ok())
    {
        return ip;
    }
    peer
}

#[derive(Clone)]
struct EmbeddingConfig {
    endpoint: String,
    api_key: String,
    model: String,
}

struct AppConfig {
    database_url: String,
    bind_addr: SocketAddr,
    app_origin: HeaderValue,
    static_dir: PathBuf,
    embeddings: Option<EmbeddingConfig>,
    trusted_proxies: Vec<TrustedProxy>,
    thresholds: SearchThresholds,
}

impl AppConfig {
    fn from_env() -> Result<Self, ApiError> {
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
            trusted_proxies: parse_trusted_proxies(env::var("TRUSTED_PROXIES").ok()),
            thresholds,
        })
    }
}

fn parse_score_env(name: &str, default: f64) -> f64 {
    match env::var(name).ok().and_then(|raw| raw.parse::<f64>().ok()) {
        Some(value) if (0.0..=1.0).contains(&value) => value,
        _ => default,
    }
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ApiError {}

impl ApiError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: "bad_request",
            message: message.into(),
        }
    }
    fn unauthorized() -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            code: "unauthorized",
            message: "需要有效身份凭证".to_owned(),
        }
    }
    fn forbidden(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            code: "forbidden",
            message: message.into(),
        }
    }
    fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            code: "not_found",
            message: message.into(),
        }
    }
    fn conflict(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            code: "conflict",
            message: message.into(),
        }
    }
    fn rate_limited() -> Self {
        Self {
            status: StatusCode::TOO_MANY_REQUESTS,
            code: "rate_limited",
            message: "请求过于频繁，请稍后再试".to_owned(),
        }
    }
    fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "internal_error",
            message: message.into(),
        }
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(error: sqlx::Error) -> Self {
        warn!(error = %error, "database operation failed");
        Self::internal("服务暂时无法处理请求")
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({ "error": { "code": self.code, "message": self.message } })),
        )
            .into_response()
    }
}

type ApiResult<T> = Result<T, ApiError>;

#[derive(Default)]
struct RateLimiter {
    entries: Mutex<HashMap<String, RateWindow>>,
}

struct RateWindow {
    opened_at: Instant,
    count: u32,
}

impl RateLimiter {
    // ponytail: 单进程限流；扩容为多个实例时改由网关或共享存储限流。
    async fn allow(&self, key: String, maximum: u32, window: Duration) -> bool {
        let mut entries = self.entries.lock().await;
        if entries.len() >= 4096 {
            // ponytail: 单进程内存保护；公网多实例时由网关按 IP/密钥限流。
            entries.retain(|_, item| item.opened_at.elapsed() < Duration::from_secs(3600));
        }
        if !entries.contains_key(&key) && entries.len() >= 8192 {
            return false;
        }
        let entry = entries.entry(key).or_insert(RateWindow {
            opened_at: Instant::now(),
            count: 0,
        });
        if entry.opened_at.elapsed() >= window {
            entry.opened_at = Instant::now();
            entry.count = 0;
        }
        entry.count += 1;
        entry.count <= maximum
    }
}

#[derive(Default)]
struct EmbeddingBreaker {
    inner: StdMutex<BreakerState>,
}

#[derive(Default)]
struct BreakerState {
    consecutive_failures: u32,
    open_until: Option<Instant>,
}

impl EmbeddingBreaker {
    fn allow(&self) -> bool {
        let mut state = self.inner.lock().unwrap();
        match state.open_until {
            Some(until) if until > Instant::now() => false,
            _ => {
                state.open_until = None;
                true
            }
        }
    }

    fn record_success(&self) {
        let mut state = self.inner.lock().unwrap();
        state.consecutive_failures = 0;
        state.open_until = None;
    }

    fn record_failure(&self) {
        let mut state = self.inner.lock().unwrap();
        state.consecutive_failures += 1;
        if state.consecutive_failures >= BREAKER_THRESHOLD {
            state.open_until = Some(Instant::now() + Duration::from_secs(BREAKER_COOLDOWN_SECS));
            state.consecutive_failures = 0;
        }
    }
}

#[derive(Clone, FromRow)]
struct AgentPrincipal {
    agent_id: Uuid,
    workspace_id: Uuid,
    developer_id: Option<Uuid>,
    publication_policy: String,
}

#[derive(Clone, FromRow)]
struct DeveloperPrincipal {
    developer_id: Uuid,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Visibility {
    AgentPrivate,
    DeveloperShared,
    Public,
}

impl Visibility {
    fn as_str(self) -> &'static str {
        match self {
            Self::AgentPrivate => "agent_private",
            Self::DeveloperShared => "developer_shared",
            Self::Public => "public",
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum OutcomeKind {
    Success,
    Failure,
    Partial,
    Unknown,
}

impl OutcomeKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Failure => "failure",
            Self::Partial => "partial",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum EvidenceKind {
    Log,
    Test,
    Link,
    HumanNote,
    Other,
}

impl EvidenceKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Log => "log",
            Self::Test => "test",
            Self::Link => "link",
            Self::HumanNote => "human_note",
            Self::Other => "other",
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum RelationKind {
    Patches,
    Contradicts,
    Supersedes,
    Expires,
}

impl RelationKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Patches => "patches",
            Self::Contradicts => "contradicts",
            Self::Supersedes => "supersedes",
            Self::Expires => "expires",
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum FeedbackVerdict {
    Useful,
    NotUseful,
    Worked,
    PartiallyWorked,
    Failed,
}

impl FeedbackVerdict {
    fn as_str(self) -> &'static str {
        match self {
            Self::Useful => "useful",
            Self::NotUseful => "not_useful",
            Self::Worked => "worked",
            Self::PartiallyWorked => "partially_worked",
            Self::Failed => "failed",
        }
    }
}

#[derive(Deserialize)]
struct RegisterAgentInput {
    name: String,
    invite_token: Option<String>,
}

#[derive(Serialize)]
struct RegisterAgentOutput {
    agent_id: Uuid,
    workspace_id: Uuid,
    api_key: String,
    claim_token: Option<String>,
    warning: &'static str,
}

#[derive(Serialize)]
struct RotatedAgentKeyOutput {
    api_key: String,
    warning: &'static str,
}

#[derive(Serialize)]
struct RotatedWorkspaceInviteOutput {
    workspace_invite_token: String,
    warning: &'static str,
}

#[derive(Deserialize)]
struct ClaimWorkspaceInput {
    claim_token: String,
    login_name: String,
    password: String,
}

#[derive(Deserialize)]
struct LoginInput {
    login_name: String,
    password: String,
}

#[derive(Serialize)]
struct DeveloperSessionOutput {
    developer_token: String,
    expires_at: OffsetDateTime,
    workspace_invite_token: Option<String>,
}

#[derive(Deserialize)]
struct SearchInput {
    query: String,
    language: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    technology: Option<String>,
    limit: Option<u8>,
}

#[derive(Serialize)]
struct SearchOutput {
    items: Vec<MemorySummary>,
    retrieval: &'static str,
    untrusted_content: bool,
}

#[derive(Deserialize)]
struct MemoryInput {
    problem: String,
    #[serde(default = "empty_object")]
    conditions: Value,
    action: String,
    outcome: String,
    outcome_kind: OutcomeKind,
    visibility: Option<Visibility>,
    #[serde(default)]
    request_public: bool,
    language: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    evidence: Vec<EvidenceInput>,
    #[serde(default)]
    relations: Vec<RelationInput>,
    gap_id: Option<Uuid>,
}

#[derive(Deserialize)]
struct EvidenceInput {
    kind: EvidenceKind,
    label: Option<String>,
    value: String,
}

#[derive(Deserialize)]
struct RelationInput {
    target_memory_id: Uuid,
    relation_type: RelationKind,
}

#[derive(Serialize)]
struct MemoryCreatedOutput {
    id: Uuid,
    visibility: String,
    publication_state: &'static str,
}

#[derive(Deserialize)]
struct FeedbackInput {
    verdict: FeedbackVerdict,
    note: Option<String>,
    evidence: Option<String>,
}

#[derive(Deserialize)]
struct GapInput {
    question: String,
    #[serde(default = "empty_object")]
    context: Value,
    attempted: Option<String>,
    visibility: Option<Visibility>,
    language: Option<String>,
}

#[derive(Serialize)]
struct CreatedId {
    id: Uuid,
}

#[derive(Serialize, FromRow)]
struct MemorySummary {
    id: Uuid,
    visibility: String,
    problem: String,
    conditions: Value,
    action: String,
    outcome: String,
    outcome_kind: String,
    source_type: String,
    language: String,
    tags: Vec<String>,
    created_at: OffsetDateTime,
    evidence_count: i64,
    agent_positive_feedback: i64,
    human_positive_feedback: i64,
}

#[derive(Serialize, FromRow)]
struct EvidenceRecord {
    id: Uuid,
    kind: String,
    label: Option<String>,
    value: String,
    created_at: OffsetDateTime,
}

#[derive(Serialize, FromRow)]
struct RelationRecord {
    target_memory_id: Uuid,
    relation_type: String,
    created_at: OffsetDateTime,
}

#[derive(Serialize)]
struct MemoryDetail {
    memory: MemorySummary,
    evidence: Vec<EvidenceRecord>,
    relations: Vec<RelationRecord>,
    untrusted_content: bool,
}

#[derive(FromRow)]
struct MemoryAccessRow {
    id: Uuid,
    workspace_id: Uuid,
    author_agent_id: Uuid,
    visibility: String,
    removed_at: Option<OffsetDateTime>,
}

#[derive(Serialize, FromRow)]
struct WorkspaceOverview {
    id: Uuid,
    name: String,
    publication_policy: String,
    created_at: OffsetDateTime,
    updated_at: OffsetDateTime,
}

#[derive(Serialize, FromRow)]
struct AgentOverview {
    id: Uuid,
    workspace_id: Uuid,
    name: String,
    created_at: OffsetDateTime,
}

#[derive(Serialize)]
struct DeveloperOverview {
    workspaces: Vec<WorkspaceOverview>,
    agents: Vec<AgentOverview>,
    pending_memories: Vec<MemorySummary>,
}

#[derive(Serialize, FromRow)]
struct GapRecord {
    id: Uuid,
    visibility: String,
    question: String,
    context: Value,
    attempted: Option<String>,
    language: String,
    created_at: OffsetDateTime,
}

#[derive(Serialize)]
struct GapDetail {
    gap: GapRecord,
    memories: Vec<MemorySummary>,
    untrusted_content: bool,
}

#[derive(Deserialize)]
struct PolicyInput {
    publication_policy: String,
}

fn empty_object() -> Value {
    json!({})
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _ = dotenvy::dotenv();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("agent_first=info".parse()?),
        )
        .init();
    let config = AppConfig::from_env()?;
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(&config.database_url)
        .await?;
    sqlx::migrate!("../migrations").run(&pool).await?;
    let state = AppState {
        pool,
        embeddings: config.embeddings,
        http: Client::builder().timeout(Duration::from_secs(3)).no_proxy().build()?,
        limiter: Arc::new(RateLimiter::default()),
        embed_breaker: Arc::new(EmbeddingBreaker::default()),
        trusted_proxies: config.trusted_proxies,
        thresholds: config.thresholds,
    };
    spawn_session_cleanup(state.pool.clone());
    let cors = CorsLayer::new()
        .allow_origin(config.app_origin)
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE]);
    let security_headers = SetResponseHeaderLayer::if_not_present(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'self'; base-uri 'self'; frame-ancestors 'none'; object-src 'none'",
        ),
    );
    let static_file = config.static_dir.join("index.html");
    let static_service =
        ServeDir::new(config.static_dir).not_found_service(ServeFile::new(static_file));
    let app = Router::new()
        .route("/healthz", get(health))
        .route("/skill.md", get(skill))
        .route("/.well-known/agent-first.json", get(discovery))
        .route("/v1/search", post(search))
        .route("/v1/agents/register", post(register_agent))
        .route("/v1/agents/{id}/keys/rotate", post(rotate_agent_key))
        .route("/v1/developers/claim", post(claim_workspace))
        .route("/v1/developers/login", post(login))
        .route("/v1/developer/overview", get(developer_overview))
        .route("/v1/developer/account", delete(delete_developer_account))
        .route(
            "/v1/workspaces/{id}/publication-policy",
            post(update_publication_policy),
        )
        .route(
            "/v1/workspaces/{id}/invite/rotate",
            post(rotate_workspace_invite),
        )
        .route("/v1/public/overview", get(public_overview))
        .route("/v1/public/memories", get(list_public_memories))
        .route("/v1/memories", get(list_memories).post(create_memory))
        .route("/v1/memories/import", post(import_memories))
        .route("/v1/memories/{id}", get(get_memory))
        .route(
            "/v1/memories/{id}/feedback",
            get(list_memory_feedback).post(create_feedback),
        )
        .route("/v1/memories/{id}/publish", post(publish_memory))
        .route("/v1/memories/{id}/remove", post(remove_memory))
        .route("/v1/gaps", post(create_gap))
        .route("/v1/gaps/{id}", get(get_gap))
        .fallback_service(static_service)
        .layer(RequestBodyLimitLayer::new(2 * 1024 * 1024))
        .layer(security_headers)
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state);
    let listener = tokio::net::TcpListener::bind(config.bind_addr).await?;
    info!(address = %config.bind_addr, "agent-first is listening");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;
    Ok(())
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

fn spawn_session_cleanup(pool: PgPool) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(6 * 60 * 60));
        loop {
            ticker.tick().await;
            match sqlx::query(
                "DELETE FROM developer_sessions \
                 WHERE expires_at < now() - interval '7 days' \
                    OR revoked_at < now() - interval '30 days'",
            )
            .execute(&pool)
            .await
            {
                Ok(result) if result.rows_affected() > 0 => {
                    info!(
                        rows = result.rows_affected(),
                        "cleaned up expired developer sessions"
                    );
                }
                Ok(_) => {}
                Err(error) => warn!(error = %error, "session cleanup failed"),
            }
        }
    });
}

async fn health(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(&state.pool)
        .await?;
    Ok(Json(json!({ "status": "ok", "database": "ok" })))
}

async fn skill() -> &'static str {
    include_str!("../../docs/SKILL.md")
}

async fn discovery() -> Json<Value> {
    Json(json!({
        "name": "Agent-first", "version": "v1", "skill": "/skill.md",
        "capabilities": ["memory_search", "memory_write", "experience_gap", "feedback"],
        "authentication": { "public_read": true, "agent_write": "Bearer API key" }
    }))
}

async fn register_agent(
    State(state): State<AppState>,
    ConnectInfo(address): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(input): Json<RegisterAgentInput>,
) -> ApiResult<Json<RegisterAgentOutput>> {
    ensure_rate(
        &state,
        format!(
            "register:{}",
            client_ip(&address, &headers, &state.trusted_proxies)
        ),
        8,
        Duration::from_secs(3600),
    )
    .await?;
    validate_name(&input.name, "Agent 名称")?;
    let mut transaction = state.pool.begin().await?;
    let (workspace_id, claim_token) = if let Some(invite_token) = input.invite_token.as_deref() {
        let workspace = sqlx::query(
            "SELECT id FROM workspaces WHERE invite_token_hash = $1 AND developer_id IS NOT NULL",
        )
        .bind(security::hash_token(invite_token))
        .fetch_optional(&mut *transaction)
        .await?;
        let Some(workspace) = workspace else {
            return Err(ApiError::forbidden("邀请令牌无效或工作区尚未认领"));
        };
        (workspace.get("id"), None)
    } else {
        let workspace_id = Uuid::new_v4();
        let claim_token = security::new_token("af_claim");
        sqlx::query("INSERT INTO workspaces (id, name, claim_token_hash) VALUES ($1, $2, $3)")
            .bind(workspace_id)
            .bind(format!("{} 的工作区", input.name.trim()))
            .bind(security::hash_token(&claim_token))
            .execute(&mut *transaction)
            .await?;
        (workspace_id, Some(claim_token))
    };
    let agent_id = Uuid::new_v4();
    let api_key = security::new_token("af_live");
    sqlx::query("INSERT INTO agents (id, workspace_id, name) VALUES ($1, $2, $3)")
        .bind(agent_id)
        .bind(workspace_id)
        .bind(input.name.trim())
        .execute(&mut *transaction)
        .await?;
    sqlx::query(
        "INSERT INTO agent_keys (id, agent_id, key_prefix, key_hash) VALUES ($1, $2, $3, $4)",
    )
    .bind(Uuid::new_v4())
    .bind(agent_id)
    .bind(security::token_prefix(&api_key))
    .bind(security::hash_token(&api_key))
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(Json(RegisterAgentOutput {
        agent_id,
        workspace_id,
        api_key,
        claim_token,
        warning: "api_key 和 claim_token 仅展示一次，请保存到安全位置",
    }))
}

async fn rotate_agent_key(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<RotatedAgentKeyOutput>> {
    let developer = require_developer(&state, &headers).await?;
    let mut transaction = state.pool.begin().await?;
    let owned = sqlx::query(
        "SELECT a.id FROM agents a JOIN workspaces w ON w.id = a.workspace_id \
         WHERE a.id = $1 AND w.developer_id = $2 AND a.revoked_at IS NULL FOR UPDATE",
    )
    .bind(id)
    .bind(developer.developer_id)
    .fetch_optional(&mut *transaction)
    .await?
    .is_some();
    if !owned {
        return Err(ApiError::forbidden("该 Agent 不属于当前开发者或已被停用"));
    }
    sqlx::query(
        "UPDATE agent_keys SET revoked_at = now() WHERE agent_id = $1 AND revoked_at IS NULL",
    )
    .bind(id)
    .execute(&mut *transaction)
    .await?;
    let api_key = security::new_token("af_live");
    sqlx::query(
        "INSERT INTO agent_keys (id, agent_id, key_prefix, key_hash) VALUES ($1, $2, $3, $4)",
    )
    .bind(Uuid::new_v4())
    .bind(id)
    .bind(security::token_prefix(&api_key))
    .bind(security::hash_token(&api_key))
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    info!(agent_id = %id, "agent API key rotated");
    Ok(Json(RotatedAgentKeyOutput {
        api_key,
        warning: "旧访问密钥已立即失效；新密钥仅展示一次，请立即替换 Agent 配置",
    }))
}

async fn claim_workspace(
    State(state): State<AppState>,
    ConnectInfo(address): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(input): Json<ClaimWorkspaceInput>,
) -> ApiResult<Json<DeveloperSessionOutput>> {
    ensure_rate(
        &state,
        format!(
            "claim:{}",
            client_ip(&address, &headers, &state.trusted_proxies)
        ),
        5,
        Duration::from_secs(3600),
    )
    .await?;
    security::validate_login_name(&input.login_name).map_err(ApiError::bad_request)?;
    security::validate_password(&input.password).map_err(ApiError::bad_request)?;
    let mut transaction = state.pool.begin().await?;
    let workspace = sqlx::query(
        "SELECT id FROM workspaces WHERE claim_token_hash = $1 AND developer_id IS NULL FOR UPDATE",
    )
    .bind(security::hash_token(&input.claim_token))
    .fetch_optional(&mut *transaction)
    .await?;
    let Some(workspace) = workspace else {
        return Err(ApiError::forbidden("认领令牌无效或工作区已经被认领"));
    };
    let developer_id = Uuid::new_v4();
    let password_hash = security::hash_password(&input.password).map_err(ApiError::internal)?;
    let insert_developer =
        sqlx::query("INSERT INTO developers (id, login_name, password_hash) VALUES ($1, $2, $3)")
            .bind(developer_id)
            .bind(input.login_name)
            .bind(password_hash)
            .execute(&mut *transaction)
            .await;
    if let Err(error) = insert_developer {
        if is_unique_violation(&error) {
            return Err(ApiError::conflict("该登录名已经被使用"));
        }
        return Err(error.into());
    }
    let invite_token = security::new_token("af_invite");
    sqlx::query("UPDATE workspaces SET developer_id = $1, claim_token_hash = NULL, invite_token_hash = $2 WHERE id = $3")
        .bind(developer_id)
        .bind(security::hash_token(&invite_token))
        .bind(workspace.get::<Uuid, _>("id"))
        .execute(&mut *transaction)
        .await?;
    let session =
        create_developer_session(&mut transaction, developer_id, Some(invite_token)).await?;
    transaction.commit().await?;
    Ok(Json(session))
}

async fn login(
    State(state): State<AppState>,
    ConnectInfo(address): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(input): Json<LoginInput>,
) -> ApiResult<Json<DeveloperSessionOutput>> {
    ensure_rate(
        &state,
        format!(
            "login:{}",
            client_ip(&address, &headers, &state.trusted_proxies)
        ),
        10,
        Duration::from_secs(600),
    )
    .await?;
    let row = sqlx::query("SELECT id, password_hash FROM developers WHERE login_name = $1")
        .bind(input.login_name)
        .fetch_optional(&state.pool)
        .await?;
    let Some(row) = row else {
        return Err(ApiError::unauthorized());
    };
    if !security::verify_password(
        &input.password,
        row.get::<String, _>("password_hash").as_str(),
    ) {
        return Err(ApiError::unauthorized());
    }
    let mut transaction = state.pool.begin().await?;
    let session = create_developer_session(&mut transaction, row.get("id"), None).await?;
    transaction.commit().await?;
    Ok(Json(session))
}

#[derive(Deserialize)]
struct DeleteAccountInput {
    password: String,
    confirmation: String,
}

async fn delete_developer_account(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<DeleteAccountInput>,
) -> ApiResult<StatusCode> {
    let developer = require_developer(&state, &headers).await?;
    if input.confirmation != "DELETE" {
        return Err(ApiError::bad_request("confirmation 字段必须为 DELETE"));
    }
    let row = sqlx::query("SELECT password_hash FROM developers WHERE id = $1")
        .bind(developer.developer_id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(ApiError::unauthorized)?;
    if !security::verify_password(
        &input.password,
        row.get::<String, _>("password_hash").as_str(),
    ) {
        return Err(ApiError::bad_request("密码不正确"));
    }
    let workspace_ids: Vec<Uuid> = sqlx::query("SELECT id FROM workspaces WHERE developer_id = $1")
        .bind(developer.developer_id)
        .fetch_all(&state.pool)
        .await?
        .into_iter()
        .map(|row| row.get("id"))
        .collect();
    let agent_ids: Vec<Uuid> = sqlx::query("SELECT id FROM agents WHERE workspace_id = ANY($1)")
        .bind(&workspace_ids)
        .fetch_all(&state.pool)
        .await?
        .into_iter()
        .map(|row| row.get("id"))
        .collect();
    let mut transaction = state.pool.begin().await?;
    sqlx::query("DELETE FROM memory_feedback WHERE developer_id = $1 OR agent_id = ANY($2)")
        .bind(developer.developer_id)
        .bind(&agent_ids)
        .execute(&mut *transaction)
        .await?;
    sqlx::query(
        "DELETE FROM memory_relations WHERE source_memory_id IN (SELECT id FROM memories WHERE workspace_id = ANY($1)) \
         OR target_memory_id IN (SELECT id FROM memories WHERE workspace_id = ANY($1))",
    )
    .bind(&workspace_ids)
    .execute(&mut *transaction)
    .await?;
    sqlx::query("DELETE FROM memory_evidence WHERE memory_id IN (SELECT id FROM memories WHERE workspace_id = ANY($1))")
        .bind(&workspace_ids)
        .execute(&mut *transaction)
        .await?;
    sqlx::query("DELETE FROM memory_feedback WHERE memory_id IN (SELECT id FROM memories WHERE workspace_id = ANY($1))")
        .bind(&workspace_ids)
        .execute(&mut *transaction)
        .await?;
    sqlx::query(
        "DELETE FROM gap_memory_links WHERE gap_id IN (SELECT id FROM experience_gaps WHERE workspace_id = ANY($1)) \
         OR memory_id IN (SELECT id FROM memories WHERE workspace_id = ANY($1))",
    )
    .bind(&workspace_ids)
    .execute(&mut *transaction)
    .await?;
    sqlx::query("DELETE FROM memories WHERE workspace_id = ANY($1)")
        .bind(&workspace_ids)
        .execute(&mut *transaction)
        .await?;
    sqlx::query("DELETE FROM experience_gaps WHERE workspace_id = ANY($1)")
        .bind(&workspace_ids)
        .execute(&mut *transaction)
        .await?;
    sqlx::query("DELETE FROM agent_keys WHERE agent_id = ANY($1)")
        .bind(&agent_ids)
        .execute(&mut *transaction)
        .await?;
    sqlx::query("DELETE FROM agents WHERE workspace_id = ANY($1)")
        .bind(&workspace_ids)
        .execute(&mut *transaction)
        .await?;
    sqlx::query("DELETE FROM developer_sessions WHERE developer_id = $1")
        .bind(developer.developer_id)
        .execute(&mut *transaction)
        .await?;
    sqlx::query("DELETE FROM workspaces WHERE developer_id = $1")
        .bind(developer.developer_id)
        .execute(&mut *transaction)
        .await?;
    sqlx::query("DELETE FROM developers WHERE id = $1")
        .bind(developer.developer_id)
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await?;
    info!(developer_id = %developer.developer_id, "developer account and all data deleted");
    Ok(StatusCode::NO_CONTENT)
}

async fn create_developer_session(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    developer_id: Uuid,
    workspace_invite_token: Option<String>,
) -> ApiResult<DeveloperSessionOutput> {
    let token = security::new_token("af_dev");
    let expires_at = OffsetDateTime::now_utc() + TimeDuration::days(14);
    sqlx::query("INSERT INTO developer_sessions (id, developer_id, token_hash, expires_at) VALUES ($1, $2, $3, $4)")
        .bind(Uuid::new_v4())
        .bind(developer_id)
        .bind(security::hash_token(&token))
        .bind(expires_at)
        .execute(&mut **transaction)
        .await?;
    Ok(DeveloperSessionOutput {
        developer_token: token,
        expires_at,
        workspace_invite_token,
    })
}

async fn search(
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
    let limit = input.limit.unwrap_or(5).clamp(1, 5) as usize;
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

async fn create_memory(
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

struct InsertedMemory {
    id: Uuid,
    visibility: Visibility,
    published: bool,
    publication_requested: bool,
}

async fn insert_memory(
    state: &AppState,
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    agent: &AgentPrincipal,
    input: MemoryInput,
    source_type: &str,
) -> ApiResult<InsertedMemory> {
    let tags = security::normalize_tags(input.tags).map_err(ApiError::bad_request)?;
    let language = input.language.unwrap_or_else(|| "zh-CN".to_owned());
    security::validate_text(&language, "语言", 2, 20).map_err(ApiError::bad_request)?;
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
    let embedding = embed(state, &search_text).await.ok().flatten();
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

#[derive(Deserialize)]
struct MemoryImportInput {
    #[serde(default)]
    memories: Vec<MemoryInput>,
}

#[derive(Serialize)]
struct MemoryImportedOutput {
    imported: usize,
    ids: Vec<Uuid>,
}

async fn import_memories(
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

#[derive(Deserialize)]
struct ListMemoriesQuery {
    limit: Option<i64>,
    offset: Option<i64>,
    visibility: Option<String>,
    outcome_kind: Option<String>,
    since: Option<String>,
    until: Option<String>,
    order_by: Option<String>,
}

#[derive(Serialize)]
struct MemoryListOutput {
    items: Vec<MemorySummary>,
    total: i64,
    limit: i64,
    offset: i64,
}

enum ListPrincipal {
    Agent(AgentPrincipal),
    Developer(DeveloperPrincipal),
}

async fn resolve_list_principal(state: &AppState, headers: &HeaderMap) -> ApiResult<ListPrincipal> {
    let Some(token) = optional_bearer_token(headers)? else {
        return Err(ApiError::unauthorized());
    };
    let token_hash = security::hash_token(token);
    if let Some(agent) = sqlx::query_as::<_, AgentPrincipal>(
        "SELECT a.id AS agent_id, a.workspace_id, w.developer_id, w.publication_policy \
         FROM agent_keys k JOIN agents a ON a.id = k.agent_id JOIN workspaces w ON w.id = a.workspace_id \
         WHERE k.key_hash = $1 AND k.revoked_at IS NULL AND a.revoked_at IS NULL",
    )
    .bind(&token_hash)
    .fetch_optional(&state.pool)
    .await?
    {
        return Ok(ListPrincipal::Agent(agent));
    }
    if let Some(developer) = sqlx::query_as::<_, DeveloperPrincipal>(
        "SELECT d.id AS developer_id FROM developer_sessions s JOIN developers d ON d.id = s.developer_id \
         WHERE s.token_hash = $1 AND s.revoked_at IS NULL AND s.expires_at > now()",
    )
    .bind(&token_hash)
    .fetch_optional(&state.pool)
    .await?
    {
        return Ok(ListPrincipal::Developer(developer));
    }
    Err(ApiError::unauthorized())
}

enum ReadPrincipal {
    Agent(AgentPrincipal),
    Developer { workspaces: Vec<Uuid> },
    Anonymous,
}

async fn resolve_read_principal(state: &AppState, headers: &HeaderMap) -> ApiResult<ReadPrincipal> {
    let Some(token) = optional_bearer_token(headers)? else {
        return Ok(ReadPrincipal::Anonymous);
    };
    let token_hash = security::hash_token(token);
    if let Some(agent) = sqlx::query_as::<_, AgentPrincipal>(
        "SELECT a.id AS agent_id, a.workspace_id, w.developer_id, w.publication_policy \
         FROM agent_keys k JOIN agents a ON a.id = k.agent_id JOIN workspaces w ON w.id = a.workspace_id \
         WHERE k.key_hash = $1 AND k.revoked_at IS NULL AND a.revoked_at IS NULL",
    )
    .bind(&token_hash)
    .fetch_optional(&state.pool)
    .await?
    {
        return Ok(ReadPrincipal::Agent(agent));
    }
    if let Some(developer) = sqlx::query_as::<_, DeveloperPrincipal>(
        "SELECT d.id AS developer_id FROM developer_sessions s JOIN developers d ON d.id = s.developer_id \
         WHERE s.token_hash = $1 AND s.revoked_at IS NULL AND s.expires_at > now()",
    )
    .bind(&token_hash)
    .fetch_optional(&state.pool)
    .await?
    {
        let workspaces = sqlx::query_scalar::<_, Uuid>("SELECT id FROM workspaces WHERE developer_id = $1")
            .bind(developer.developer_id)
            .fetch_all(&state.pool)
            .await?;
        return Ok(ReadPrincipal::Developer { workspaces });
    }
    Err(ApiError::unauthorized())
}

fn read_scope(principal: &ReadPrincipal) -> (Vec<Uuid>, Vec<Uuid>, Option<Uuid>) {
    match principal {
        ReadPrincipal::Agent(agent) => (vec![agent.workspace_id], Vec::new(), Some(agent.agent_id)),
        ReadPrincipal::Developer { workspaces } => (Vec::new(), workspaces.clone(), None),
        ReadPrincipal::Anonymous => (Vec::new(), Vec::new(), None),
    }
}

async fn list_memories(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ListMemoriesQuery>,
) -> ApiResult<Json<MemoryListOutput>> {
    let limit = query.limit.unwrap_or(20).clamp(1, 100);
    let offset = query.offset.unwrap_or(0).max(0);
    let principal = resolve_list_principal(&state, &headers).await?;

    let vis_filter = query.visibility.as_deref().filter(|v| ["public", "developer_shared", "agent_private"].contains(v));
    let outcome_filter = query.outcome_kind.as_deref().filter(|v| ["success", "failure", "partial", "unknown"].contains(v));
    let since = query.since.as_deref().and_then(|s| OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339).ok());
    let until = query.until.as_deref().and_then(|s| OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339).ok());
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
            let ids_sql = format!("SELECT m.id FROM memories m WHERE {} ORDER BY {} LIMIT ${} OFFSET ${}", where_clause, order, param_idx, param_idx + 1);

            let mut total_query = sqlx::query_scalar::<_, i64>(&total_sql).bind(agent.workspace_id).bind(agent.agent_id);
            let mut ids_query = sqlx::query(&ids_sql).bind(agent.workspace_id).bind(agent.agent_id);
            if let Some(v) = vis_filter { total_query = total_query.bind(v); ids_query = ids_query.bind(v); }
            if let Some(v) = outcome_filter { total_query = total_query.bind(v); ids_query = ids_query.bind(v); }
            if let Some(s) = since { total_query = total_query.bind(s); ids_query = ids_query.bind(s); }
            if let Some(u) = until { total_query = total_query.bind(u); ids_query = ids_query.bind(u); }
            total_query = total_query.bind(limit).bind(offset);
            ids_query = ids_query.bind(limit).bind(offset);

            let total = total_query.fetch_one(&state.pool).await?;
            let ids: Vec<Uuid> = ids_query.fetch_all(&state.pool).await?.into_iter().map(|row| row.get("id")).collect();
            (total, ids)
        }
        ListPrincipal::Developer(developer) => {
            let mut conditions = vec!["m.removed_at IS NULL".to_string(), "w.developer_id = $1".to_string()];
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
            let total_sql = format!("SELECT count(*) FROM memories m JOIN workspaces w ON w.id = m.workspace_id WHERE {}", where_clause);
            let ids_sql = format!("SELECT m.id FROM memories m JOIN workspaces w ON w.id = m.workspace_id WHERE {} ORDER BY {} LIMIT ${} OFFSET ${}", where_clause, order, param_idx, param_idx + 1);

            let mut total_query = sqlx::query_scalar::<_, i64>(&total_sql).bind(developer.developer_id);
            let mut ids_query = sqlx::query(&ids_sql).bind(developer.developer_id);
            if let Some(v) = vis_filter { total_query = total_query.bind(v); ids_query = ids_query.bind(v); }
            if let Some(v) = outcome_filter { total_query = total_query.bind(v); ids_query = ids_query.bind(v); }
            if let Some(s) = since { total_query = total_query.bind(s); ids_query = ids_query.bind(s); }
            if let Some(u) = until { total_query = total_query.bind(u); ids_query = ids_query.bind(u); }
            total_query = total_query.bind(limit).bind(offset);
            ids_query = ids_query.bind(limit).bind(offset);

            let total = total_query.fetch_one(&state.pool).await?;
            let ids: Vec<Uuid> = ids_query.fetch_all(&state.pool).await?.into_iter().map(|row| row.get("id")).collect();
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

#[derive(Serialize)]
struct PublicStats {
    public_memories: i64,
    agents: i64,
    reuse_total: i64,
}

#[derive(Serialize, FromRow)]
struct ActivityItem {
    kind: String,
    at: OffsetDateTime,
    problem: String,
    agent_name: Option<String>,
    verdict: Option<String>,
}

#[derive(Serialize)]
struct PublicOverview {
    stats: PublicStats,
    activity: Vec<ActivityItem>,
    top: Vec<MemorySummary>,
}

async fn public_overview(State(state): State<AppState>) -> ApiResult<Json<PublicOverview>> {
    let public_memories = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM memories WHERE visibility = 'public' AND removed_at IS NULL",
    )
    .fetch_one(&state.pool)
    .await?;
    let agents = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM agents WHERE revoked_at IS NULL",
    )
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

async fn list_public_memories(
    State(state): State<AppState>,
    Query(query): Query<ListMemoriesQuery>,
) -> ApiResult<Json<MemoryListOutput>> {
    let limit = query.limit.unwrap_or(20).clamp(1, 100);
    let offset = query.offset.unwrap_or(0).max(0);

    let outcome_filter = query.outcome_kind.as_deref().filter(|v| ["success", "failure", "partial", "unknown"].contains(v));
    let since = query.since.as_deref().and_then(|s| OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339).ok());
    let until = query.until.as_deref().and_then(|s| OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339).ok());
    let order = match query.order_by.as_deref() {
        Some("reuse") => "agent_positive_feedback DESC, created_at DESC",
        Some("feedback") => "human_positive_feedback DESC, created_at DESC",
        Some("evidence") => "evidence_count DESC, created_at DESC",
        _ => "published_at DESC NULLS LAST, created_at DESC",
    };

    let mut conditions = vec!["visibility = 'public'".to_string(), "removed_at IS NULL".to_string()];
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
    let ids_sql = format!("SELECT id FROM memories WHERE {} ORDER BY {} LIMIT ${} OFFSET ${}", where_clause, order, param_idx, param_idx + 1);

    let mut total_query = sqlx::query_scalar::<_, i64>(&total_sql);
    let mut ids_query = sqlx::query(&ids_sql);
    if let Some(v) = outcome_filter { total_query = total_query.bind(v); ids_query = ids_query.bind(v); }
    if let Some(s) = since { total_query = total_query.bind(s); ids_query = ids_query.bind(s); }
    if let Some(u) = until { total_query = total_query.bind(u); ids_query = ids_query.bind(u); }
    total_query = total_query.bind(limit).bind(offset);
    ids_query = ids_query.bind(limit).bind(offset);

    let total = total_query.fetch_one(&state.pool).await?;
    let ids: Vec<Uuid> = ids_query.fetch_all(&state.pool).await?.into_iter().map(|row| row.get("id")).collect();
    let items = fetch_memory_summaries(&state.pool, &ids).await?;
    Ok(Json(MemoryListOutput {
        items,
        total,
        limit,
        offset,
    }))
}

#[derive(Serialize, FromRow)]
struct FeedbackRecord {
    source_type: String,
    verdict: String,
    note: Option<String>,
    created_at: OffsetDateTime,
}

async fn list_memory_feedback(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<FeedbackRecord>>> {
    let memory = load_memory_access(&state.pool, id).await?;
    let principal = resolve_read_principal(&state, &headers).await?;
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

async fn get_memory(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<MemoryDetail>> {
    let principal = resolve_read_principal(&state, &headers).await?;
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

async fn publish_memory(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let developer = require_developer(&state, &headers).await?;
    let memory = load_memory_access(&state.pool, id).await?;
    ensure_workspace_owner(&state.pool, memory.workspace_id, developer.developer_id).await?;
    if memory.removed_at.is_some() {
        return Err(ApiError::bad_request("已移除的记忆不能公开"));
    }
    sqlx::query("UPDATE memories SET visibility = 'public', published_at = now(), publication_requested_at = NULL WHERE id = $1")
        .bind(id).execute(&state.pool).await?;
    Ok(Json(json!({ "id": id, "visibility": "public" })))
}

async fn remove_memory(
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

async fn create_feedback(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(input): Json<FeedbackInput>,
) -> ApiResult<Json<CreatedId>> {
    validate_optional_text(&input.note, "反馈说明", 1200)?;
    validate_optional_text(&input.evidence, "反馈证据", 2000)?;
    let memory = load_memory_access(&state.pool, id).await?;
    if memory.removed_at.is_some() {
        return Err(ApiError::not_found("记忆不存在"));
    }
    let token_hash = security::hash_token(bearer_token(&headers)?);
    let developer = sqlx::query_as::<_, DeveloperPrincipal>(
        "SELECT d.id AS developer_id FROM developer_sessions s JOIN developers d ON d.id = s.developer_id WHERE s.token_hash = $1 AND s.revoked_at IS NULL AND s.expires_at > now()",
    ).bind(&token_hash).fetch_optional(&state.pool).await?;
    let feedback_id = Uuid::new_v4();
    if let Some(developer) = developer {
        ensure_workspace_owner(&state.pool, memory.workspace_id, developer.developer_id).await?;
        ensure_rate(
            &state,
            format!("feedback-dev:{}", developer.developer_id),
            60,
            Duration::from_secs(3600),
        )
        .await?;
        sqlx::query("INSERT INTO memory_feedback (id, memory_id, source_type, developer_id, verdict, note, evidence) VALUES ($1, $2, 'human', $3, $4, $5, $6)")
            .bind(feedback_id).bind(id).bind(developer.developer_id).bind(input.verdict.as_str())
            .bind(input.note.map(|value| value.trim().to_owned())).bind(input.evidence.map(|value| value.trim().to_owned()))
            .execute(&state.pool).await?;
    } else {
        let agent = require_agent(&state, &headers).await?;
        ensure_rate(
            &state,
            format!("feedback:{}", agent.agent_id),
            60,
            Duration::from_secs(3600),
        )
        .await?;
        if !can_read_memory(&state.pool, id, Some(&agent)).await? {
            return Err(ApiError::forbidden("不能反馈不可访问的记忆"));
        }
        sqlx::query("INSERT INTO memory_feedback (id, memory_id, source_type, agent_id, verdict, note, evidence) VALUES ($1, $2, 'agent', $3, $4, $5, $6)")
            .bind(feedback_id).bind(id).bind(agent.agent_id).bind(input.verdict.as_str())
            .bind(input.note.map(|value| value.trim().to_owned())).bind(input.evidence.map(|value| value.trim().to_owned()))
            .execute(&state.pool).await?;
    }
    Ok(Json(CreatedId { id: feedback_id }))
}

async fn create_gap(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<GapInput>,
) -> ApiResult<Json<CreatedId>> {
    let agent = require_agent(&state, &headers).await?;
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
        return Err(ApiError::forbidden("工作区认领后才能创建公共经验缺口"));
    }
    let language = input.language.unwrap_or_else(|| "zh-CN".to_owned());
    security::validate_text(&language, "语言", 2, 20).map_err(ApiError::bad_request)?;
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO experience_gaps (id, workspace_id, author_agent_id, visibility, question, context, attempted, language) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)")
        .bind(id).bind(agent.workspace_id).bind(agent.agent_id).bind(visibility.as_str()).bind(input.question.trim())
        .bind(input.context).bind(input.attempted.map(|value| value.trim().to_owned())).bind(language)
        .execute(&state.pool).await?;
    Ok(Json(CreatedId { id }))
}

async fn get_gap(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<GapDetail>> {
    let agent = optional_agent(&state, &headers).await?;
    if !can_read_gap_with_optional(&state.pool, id, agent.as_ref()).await? {
        return Err(ApiError::not_found("经验缺口不存在或不可访问"));
    }
    let gap = sqlx::query_as::<_, GapRecord>(
        "SELECT id, visibility, question, context, attempted, language, created_at FROM experience_gaps WHERE id = $1 AND removed_at IS NULL",
    ).bind(id).fetch_optional(&state.pool).await?.ok_or_else(|| ApiError::not_found("经验缺口不存在"))?;
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
                    .is_some_and(|row| can_read_row(row, agent.as_ref()))
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

async fn developer_overview(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<DeveloperOverview>> {
    let developer = require_developer(&state, &headers).await?;
    let workspaces = sqlx::query_as::<_, WorkspaceOverview>(
        "SELECT id, name, publication_policy, created_at, updated_at FROM workspaces WHERE developer_id = $1 ORDER BY created_at DESC",
    ).bind(developer.developer_id).fetch_all(&state.pool).await?;
    let agents = sqlx::query_as::<_, AgentOverview>(
        "SELECT a.id, a.workspace_id, a.name, a.created_at FROM agents a JOIN workspaces w ON w.id = a.workspace_id WHERE w.developer_id = $1 AND a.revoked_at IS NULL ORDER BY a.created_at DESC",
    ).bind(developer.developer_id).fetch_all(&state.pool).await?;
    let pending_ids: Vec<Uuid> = sqlx::query(
        "SELECT m.id FROM memories m JOIN workspaces w ON w.id = m.workspace_id WHERE w.developer_id = $1 AND m.publication_requested_at IS NOT NULL AND m.visibility <> 'public' AND m.removed_at IS NULL ORDER BY m.publication_requested_at DESC",
    ).bind(developer.developer_id).fetch_all(&state.pool).await?.into_iter().map(|row| row.get("id")).collect();
    let pending_memories = fetch_memory_summaries(&state.pool, &pending_ids).await?;
    Ok(Json(DeveloperOverview {
        workspaces,
        agents,
        pending_memories,
    }))
}

async fn update_publication_policy(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(input): Json<PolicyInput>,
) -> ApiResult<Json<Value>> {
    let developer = require_developer(&state, &headers).await?;
    if !matches!(input.publication_policy.as_str(), "manual" | "auto") {
        return Err(ApiError::bad_request("公开策略只能是 manual 或 auto"));
    }
    ensure_workspace_owner(&state.pool, id, developer.developer_id).await?;
    sqlx::query("UPDATE workspaces SET publication_policy = $1, updated_at = now() WHERE id = $2")
        .bind(&input.publication_policy)
        .bind(id)
        .execute(&state.pool)
        .await?;
    Ok(Json(
        json!({ "workspace_id": id, "publication_policy": input.publication_policy }),
    ))
}

async fn rotate_workspace_invite(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<RotatedWorkspaceInviteOutput>> {
    let developer = require_developer(&state, &headers).await?;
    ensure_workspace_owner(&state.pool, id, developer.developer_id).await?;
    let invite_token = security::new_token("af_invite");
    sqlx::query("UPDATE workspaces SET invite_token_hash = $1 WHERE id = $2")
        .bind(security::hash_token(&invite_token))
        .bind(id)
        .execute(&state.pool)
        .await?;
    info!(workspace_id = %id, "workspace invite rotated");
    Ok(Json(RotatedWorkspaceInviteOutput {
        workspace_invite_token: invite_token,
        warning: "旧邀请码已立即失效；新邀请码只展示一次，请交给需要加入工作区的 Agent",
    }))
}

async fn optional_agent(
    state: &AppState,
    headers: &HeaderMap,
) -> ApiResult<Option<AgentPrincipal>> {
    let Some(token) = optional_bearer_token(headers)? else {
        return Ok(None);
    };
    let agent = sqlx::query_as::<_, AgentPrincipal>(
        "SELECT a.id AS agent_id, a.workspace_id, w.developer_id, w.publication_policy \
         FROM agent_keys k JOIN agents a ON a.id = k.agent_id JOIN workspaces w ON w.id = a.workspace_id \
         WHERE k.key_hash = $1 AND k.revoked_at IS NULL AND a.revoked_at IS NULL",
    ).bind(security::hash_token(token)).fetch_optional(&state.pool).await?;
    if agent.is_none() {
        return Err(ApiError::unauthorized());
    }
    Ok(agent)
}

async fn require_agent(state: &AppState, headers: &HeaderMap) -> ApiResult<AgentPrincipal> {
    optional_agent(state, headers)
        .await?
        .ok_or_else(ApiError::unauthorized)
}

async fn require_developer(state: &AppState, headers: &HeaderMap) -> ApiResult<DeveloperPrincipal> {
    let developer = sqlx::query_as::<_, DeveloperPrincipal>(
        "SELECT d.id AS developer_id FROM developer_sessions s JOIN developers d ON d.id = s.developer_id \
         WHERE s.token_hash = $1 AND s.revoked_at IS NULL AND s.expires_at > now()",
    ).bind(security::hash_token(bearer_token(headers)?)).fetch_optional(&state.pool).await?;
    developer.ok_or_else(ApiError::unauthorized)
}

fn optional_bearer_token(headers: &HeaderMap) -> ApiResult<Option<&str>> {
    let Some(value) = headers.get(header::AUTHORIZATION) else {
        return Ok(None);
    };
    let value = value.to_str().map_err(|_| ApiError::unauthorized())?;
    value
        .strip_prefix("Bearer ")
        .filter(|token| !token.is_empty())
        .map(Some)
        .ok_or_else(ApiError::unauthorized)
}

fn bearer_token(headers: &HeaderMap) -> ApiResult<&str> {
    optional_bearer_token(headers)?.ok_or_else(ApiError::unauthorized)
}

async fn ensure_rate(
    state: &AppState,
    key: String,
    maximum: u32,
    window: Duration,
) -> ApiResult<()> {
    if state.limiter.allow(key, maximum, window).await {
        Ok(())
    } else {
        Err(ApiError::rate_limited())
    }
}

fn flatten_json_values(value: &Value) -> String {
    let mut parts = Vec::new();
    collect_json_values(value, &mut parts);
    parts.join(" ")
}

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

fn tokenize_query(query: &str) -> Vec<String> {
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

async fn lexical_candidates(
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

async fn semantic_candidates(
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

fn merge_ranks(lexical: &[(Uuid, f64)], semantic: &[(Uuid, f64)], limit: usize) -> Vec<Uuid> {
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

async fn fetch_memory_summaries(pool: &PgPool, ids: &[Uuid]) -> ApiResult<Vec<MemorySummary>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let rows = sqlx::query_as::<_, MemorySummary>(
        "SELECT m.id, m.visibility, m.problem, m.conditions, m.action, m.outcome, m.outcome_kind, m.source_type, m.language, m.tags, m.created_at, \
         COUNT(DISTINCT e.id)::bigint AS evidence_count, \
         COUNT(DISTINCT f.id) FILTER (WHERE f.source_type = 'agent' AND f.verdict IN ('useful', 'worked', 'partially_worked'))::bigint AS agent_positive_feedback, \
         COUNT(DISTINCT f.id) FILTER (WHERE f.source_type = 'human' AND f.verdict IN ('useful', 'worked', 'partially_worked'))::bigint AS human_positive_feedback \
         FROM memories m LEFT JOIN memory_evidence e ON e.memory_id = m.id LEFT JOIN memory_feedback f ON f.memory_id = m.id \
         WHERE m.id = ANY($1) AND m.removed_at IS NULL GROUP BY m.id",
    ).bind(ids).fetch_all(pool).await?;
    let mut by_id: HashMap<Uuid, MemorySummary> =
        rows.into_iter().map(|row| (row.id, row)).collect();
    Ok(ids.iter().filter_map(|id| by_id.remove(id)).collect())
}

async fn load_memory_access(pool: &PgPool, id: Uuid) -> ApiResult<MemoryAccessRow> {
    sqlx::query_as::<_, MemoryAccessRow>(
        "SELECT id, workspace_id, author_agent_id, visibility, removed_at FROM memories WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| ApiError::not_found("记忆不存在"))
}

fn can_read_row(row: &MemoryAccessRow, agent: Option<&AgentPrincipal>) -> bool {
    if row.removed_at.is_some() {
        return false;
    }
    if row.visibility == "public" {
        return true;
    }
    let Some(agent) = agent else {
        return false;
    };
    (row.visibility == "developer_shared" && row.workspace_id == agent.workspace_id)
        || (row.visibility == "agent_private" && row.author_agent_id == agent.agent_id)
}

async fn can_read_memory(
    pool: &PgPool,
    id: Uuid,
    agent: Option<&AgentPrincipal>,
) -> ApiResult<bool> {
    let Some(row) = sqlx::query_as::<_, MemoryAccessRow>(
        "SELECT id, workspace_id, author_agent_id, visibility, removed_at FROM memories WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    else {
        return Ok(false);
    };
    Ok(can_read_row(&row, agent))
}

fn can_read_row_principal(row: &MemoryAccessRow, principal: &ReadPrincipal) -> bool {
    if row.removed_at.is_some() {
        return false;
    }
    if row.visibility == "public" {
        return true;
    }
    match principal {
        ReadPrincipal::Agent(agent) => {
            (row.visibility == "developer_shared" && row.workspace_id == agent.workspace_id)
                || (row.visibility == "agent_private" && row.author_agent_id == agent.agent_id)
        }
        ReadPrincipal::Developer { workspaces } => workspaces.contains(&row.workspace_id),
        ReadPrincipal::Anonymous => false,
    }
}

async fn can_read_memory_principal(
    pool: &PgPool,
    id: Uuid,
    principal: &ReadPrincipal,
) -> ApiResult<bool> {
    let Some(row) = sqlx::query_as::<_, MemoryAccessRow>(
        "SELECT id, workspace_id, author_agent_id, visibility, removed_at FROM memories WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    else {
        return Ok(false);
    };
    Ok(can_read_row_principal(&row, principal))
}

async fn ensure_workspace_owner(
    pool: &PgPool,
    workspace_id: Uuid,
    developer_id: Uuid,
) -> ApiResult<()> {
    let owned = sqlx::query("SELECT 1 FROM workspaces WHERE id = $1 AND developer_id = $2")
        .bind(workspace_id)
        .bind(developer_id)
        .fetch_optional(pool)
        .await?
        .is_some();
    owned
        .then_some(())
        .ok_or_else(|| ApiError::forbidden("该工作区不属于当前开发者"))
}

async fn can_read_gap(pool: &PgPool, id: Uuid, agent: &AgentPrincipal) -> ApiResult<bool> {
    can_read_gap_with_optional(pool, id, Some(agent)).await
}

async fn can_read_gap_with_optional(
    pool: &PgPool,
    id: Uuid,
    agent: Option<&AgentPrincipal>,
) -> ApiResult<bool> {
    let row = sqlx::query("SELECT workspace_id, author_agent_id, visibility, removed_at FROM experience_gaps WHERE id = $1")
        .bind(id).fetch_optional(pool).await?;
    let Some(row) = row else {
        return Ok(false);
    };
    let removed: Option<OffsetDateTime> = row.get("removed_at");
    if removed.is_some() {
        return Ok(false);
    }
    let visibility: String = row.get("visibility");
    if visibility == "public" {
        return Ok(true);
    }
    let Some(agent) = agent else {
        return Ok(false);
    };
    let workspace_id: Uuid = row.get("workspace_id");
    let author_agent_id: Uuid = row.get("author_agent_id");
    Ok(
        (visibility == "developer_shared" && workspace_id == agent.workspace_id)
            || (visibility == "agent_private" && author_agent_id == agent.agent_id),
    )
}

async fn embed_with_breaker(state: &AppState, input: &str) -> Option<String> {
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
            warn!(error = %error.message, "embedding failed; recording breaker failure");
            state.embed_breaker.record_failure();
            None
        }
    }
}

async fn embed(state: &AppState, input: &str) -> ApiResult<Option<String>> {
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

fn validate_memory_input(input: &MemoryInput) -> ApiResult<()> {
    security::validate_text(&input.problem, "问题", 2, 1000).map_err(ApiError::bad_request)?;
    validate_json(&input.conditions, "适用条件", 6000)?;
    security::validate_text(&input.action, "实际操作", 1, 2400).map_err(ApiError::bad_request)?;
    security::validate_text(&input.outcome, "实际结果", 1, 2400).map_err(ApiError::bad_request)?;
    if input.evidence.len() > 8 {
        return Err(ApiError::bad_request("证据最多 8 条"));
    }
    if input.relations.len() > 8 {
        return Err(ApiError::bad_request("关联最多 8 条"));
    }
    let mut targets = HashSet::new();
    for relation in &input.relations {
        if !targets.insert(relation.target_memory_id) {
            return Err(ApiError::bad_request("不能重复关联同一条记忆"));
        }
    }
    for evidence in &input.evidence {
        if let Some(label) = &evidence.label {
            security::validate_text(label, "证据标签", 1, 160).map_err(ApiError::bad_request)?;
        }
        security::validate_text(&evidence.value, "证据", 1, 2000).map_err(ApiError::bad_request)?;
        if matches!(evidence.kind, EvidenceKind::Link) {
            security::validate_https_url(&evidence.value).map_err(ApiError::bad_request)?;
        }
    }
    Ok(())
}

fn validate_json(value: &Value, field: &str, maximum: usize) -> ApiResult<()> {
    let raw = serde_json::to_string(value)
        .map_err(|_| ApiError::bad_request(format!("{field} 格式无效")))?;
    security::validate_text(&raw, field, 1, maximum).map_err(ApiError::bad_request)
}

fn validate_optional_text(value: &Option<String>, field: &str, maximum: usize) -> ApiResult<()> {
    if let Some(value) = value {
        security::validate_text(value, field, 1, maximum).map_err(ApiError::bad_request)?;
    }
    Ok(())
}

fn validate_name(value: &str, field: &str) -> ApiResult<()> {
    security::validate_text(value, field, 1, 120).map_err(ApiError::bad_request)
}

fn normalize_optional(
    value: &Option<String>,
    field: &str,
    maximum: usize,
) -> ApiResult<Option<String>> {
    value
        .as_ref()
        .map(|item| {
            let normalized = item.trim();
            security::validate_text(normalized, field, 1, maximum)
                .map_err(ApiError::bad_request)?;
            Ok(normalized.to_owned())
        })
        .transpose()
}

fn is_unique_violation(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .and_then(|database| database.code())
        .is_some_and(|code| code == "23505")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn rate_limiter_rejects_after_the_configured_limit() {
        let limiter = RateLimiter::default();
        assert!(
            limiter
                .allow("test-agent".to_owned(), 1, Duration::from_secs(60))
                .await
        );
        assert!(
            !limiter
                .allow("test-agent".to_owned(), 1, Duration::from_secs(60))
                .await
        );
    }

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
}
