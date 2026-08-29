use std::{net::SocketAddr, path::PathBuf};

use agent_first::{AppConfig, AppState, SearchThresholds, build_router};
use axum::{
    Router,
    body::{Body, to_bytes},
    extract::connect_info::MockConnectInfo,
    http::{
        HeaderValue, Method, Request, StatusCode,
        header::{AUTHORIZATION, CONTENT_SECURITY_POLICY, CONTENT_TYPE},
    },
    response::Response,
};
use serde_json::{Value, json};
use sqlx::PgPool;
use tower::ServiceExt;

fn test_config() -> AppConfig {
    AppConfig {
        database_url: "postgres://agentfirst@127.0.0.1:5433/agentfirst".to_owned(),
        bind_addr: "127.0.0.1:0".parse().unwrap(),
        app_origin: HeaderValue::from_static("http://localhost:5173"),
        static_dir: PathBuf::from("../web/dist"),
        embeddings: None,
        trusted_proxies: Vec::new(),
        thresholds: SearchThresholds {
            lexical_min: 0.0,
            semantic_min: 0.0,
            semantic_exact_min: 0.65,
            gap_min: 0.0,
        },
    }
}

fn test_app(pool: PgPool) -> Router {
    let state = AppState::new(
        pool,
        None,
        Vec::new(),
        SearchThresholds {
            lexical_min: 0.0,
            semantic_min: 0.0,
            semantic_exact_min: 0.65,
            gap_min: 0.0,
        },
    )
    .unwrap();
    build_router(state, &test_config()).layer(axum::Extension(MockConnectInfo(SocketAddr::from((
        [127, 0, 0, 1],
        60000,
    )))))
}

async fn send(
    app: &Router,
    method: Method,
    uri: &str,
    token: Option<&str>,
    payload: Option<Value>,
) -> Response {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(token) = token {
        builder = builder.header(AUTHORIZATION, format!("Bearer {token}"));
    }
    let request = match payload {
        Some(value) => builder
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(value.to_string()))
            .unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    };
    app.clone().oneshot(request).await.unwrap()
}

async fn payload(response: Response) -> Value {
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    serde_json::from_slice(&bytes).unwrap_or(Value::Null)
}

fn memory_input(problem: &str) -> Value {
    json!({
        "problem": problem,
        "conditions": { "technologies": ["postgres 17"], "os": "Ubuntu 24.04" },
        "action": "调整 max_connections 并滚动重启实例",
        "outcome": "连接数回落，服务恢复",
        "outcome_kind": "success",
        "language": "zh-CN",
        "tags": ["postgres"]
    })
}

async fn register(app: &Router, name: &str) -> Value {
    let response = send(
        app,
        Method::POST,
        "/v1/agents/register",
        None,
        Some(json!({ "name": name })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    payload(response).await
}

#[sqlx::test(migrations = "../migrations")]
async fn healthz_returns_ok_with_security_headers(pool: PgPool) {
    let response = send(&test_app(pool), Method::GET, "/healthz", None, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    let csp = response
        .headers()
        .get(CONTENT_SECURITY_POLICY)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    assert!(csp.contains("default-src 'self'"), "CSP 头缺失: {csp}");
}

#[sqlx::test(migrations = "../migrations")]
async fn register_agent_persists_workspace_and_agent(pool: PgPool) {
    let app = test_app(pool.clone());
    let registration = register(&app, "集成测试 Agent").await;
    assert!(
        registration["api_key"]
            .as_str()
            .unwrap()
            .starts_with("af_live_")
    );
    assert!(
        registration["claim_token"]
            .as_str()
            .unwrap()
            .starts_with("af_claim_")
    );
    let agent_id = uuid::Uuid::parse_str(registration["agent_id"].as_str().unwrap()).unwrap();
    let workspaces: i64 = sqlx::query_scalar("SELECT count(*) FROM workspaces")
        .fetch_one(&pool)
        .await
        .unwrap();
    let agents: i64 = sqlx::query_scalar("SELECT count(*) FROM agents WHERE id = $1")
        .bind(agent_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(workspaces, 1);
    assert_eq!(agents, 1);
}

#[sqlx::test(migrations = "../migrations")]
async fn protected_routes_reject_missing_credentials(pool: PgPool) {
    let app = test_app(pool);
    let list = send(&app, Method::GET, "/v1/memories", None, None).await;
    assert_eq!(list.status(), StatusCode::UNAUTHORIZED);
    let create = send(
        &app,
        Method::POST,
        "/v1/memories",
        None,
        Some(memory_input("无凭证不应写入")),
    )
    .await;
    assert_eq!(create.status(), StatusCode::UNAUTHORIZED);
    let publish = send(
        &app,
        Method::POST,
        "/v1/memories/00000000-0000-0000-0000-000000000000/publish",
        None,
        None,
    )
    .await;
    assert_eq!(publish.status(), StatusCode::UNAUTHORIZED);
}

#[sqlx::test(migrations = "../migrations")]
async fn agent_memory_lifecycle_from_create_to_search(pool: PgPool) {
    let app = test_app(pool);
    let registration = register(&app, "生命周期 Agent").await;
    let api_key = registration["api_key"].as_str().unwrap().to_owned();

    let created = send(
        &app,
        Method::POST,
        "/v1/memories",
        Some(&api_key),
        Some(memory_input("Docker Compose 启动顺序失败导致服务无法启动")),
    )
    .await;
    assert_eq!(created.status(), StatusCode::OK);
    let created = payload(created).await;
    assert_eq!(created["visibility"], "agent_private");
    assert_eq!(created["publication_state"], "private_or_shared");
    let memory_id = created["id"].as_str().unwrap().to_owned();

    let detail = send(
        &app,
        Method::GET,
        &format!("/v1/memories/{memory_id}"),
        Some(&api_key),
        None,
    )
    .await;
    assert_eq!(detail.status(), StatusCode::OK);
    let detail = payload(detail).await;
    assert_eq!(
        detail["memory"]["problem"],
        "Docker Compose 启动顺序失败导致服务无法启动"
    );
    assert_eq!(detail["memory"]["language"], "zh-CN");
    assert_eq!(
        detail["memory"]["author_agent_name"],
        "生命周期 Agent",
        "记忆摘要应携带作者 Agent 名"
    );
    assert_eq!(detail["untrusted_content"], true);

    let anonymous = send(
        &app,
        Method::GET,
        &format!("/v1/memories/{memory_id}"),
        None,
        None,
    )
    .await;
    assert_eq!(anonymous.status(), StatusCode::NOT_FOUND);

    let search = send(
        &app,
        Method::POST,
        "/v1/search",
        Some(&api_key),
        Some(json!({ "query": "Docker Compose 启动顺序", "limit": 5 })),
    )
    .await;
    assert_eq!(search.status(), StatusCode::OK);
    let search = payload(search).await;
    assert_eq!(search["retrieval"], "lexical");
    assert!(
        search["related_gaps"].as_array().is_some(),
        "related_gaps 字段需常驻返回（即使语义检索不可用也应为空数组）"
    );
    let ids: Vec<&str> = search["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|item| item["id"].as_str())
        .collect();
    assert!(ids.contains(&memory_id.as_str()), "检索应命中新写入记忆");
    for item in search["items"].as_array().unwrap() {
        assert_eq!(
            item["relevance"], "related",
            "无语义通道（embedding 未配置）时命中条目应降级为 related"
        );
        assert!(
            item.get("score").is_none(),
            "仅词法通道命中不应携带语义分"
        );
    }
}

#[sqlx::test(migrations = "../migrations")]
async fn invalid_inputs_are_rejected_with_clear_messages(pool: PgPool) {
    let app = test_app(pool);
    let registration = register(&app, "校验 Agent").await;
    let api_key = registration["api_key"].as_str().unwrap().to_owned();

    let short_problem = send(
        &app,
        Method::POST,
        "/v1/memories",
        Some(&api_key),
        Some(memory_input("短")),
    )
    .await;
    assert_eq!(short_problem.status(), StatusCode::BAD_REQUEST);
    let message = payload(short_problem).await;
    assert!(
        message["error"]["message"]
            .as_str()
            .unwrap()
            .contains("问题")
    );

    let short_query = send(
        &app,
        Method::POST,
        "/v1/search",
        Some(&api_key),
        Some(json!({ "query": "a" })),
    )
    .await;
    assert_eq!(short_query.status(), StatusCode::BAD_REQUEST);

    let empty_import = send(
        &app,
        Method::POST,
        "/v1/memories/import",
        Some(&api_key),
        Some(json!({ "memories": [] })),
    )
    .await;
    assert_eq!(empty_import.status(), StatusCode::BAD_REQUEST);
    let message = payload(empty_import).await;
    assert_eq!(message["error"]["message"], "导入列表不能为空");
}

#[sqlx::test(migrations = "../migrations")]
async fn developer_claim_publish_and_remove_flow(pool: PgPool) {
    let app = test_app(pool);
    let registration = register(&app, "发布流程 Agent").await;
    let api_key = registration["api_key"].as_str().unwrap().to_owned();
    let claim_token = registration["claim_token"].as_str().unwrap().to_owned();

    let requested = send(
        &app,
        Method::POST,
        "/v1/memories",
        Some(&api_key),
        Some(json!({
            "problem": "pgvector 索引在数据量小时召回不稳定",
            "conditions": { "technologies": ["postgres 17"] },
            "action": "等数据量超过阈值后再建 HNSW 索引",
            "outcome": "召回率恢复",
            "outcome_kind": "partial",
            "request_public": true
        })),
    )
    .await;
    assert_eq!(requested.status(), StatusCode::OK);
    let requested = payload(requested).await;
    assert_eq!(requested["visibility"], "developer_shared");
    assert_eq!(requested["publication_state"], "pending_owner");
    let memory_id = requested["id"].as_str().unwrap().to_owned();

    let claim = send(
        &app,
        Method::POST,
        "/v1/developers/claim",
        None,
        Some(json!({ "claim_token": claim_token, "login_name": "it_dev", "password": "password123" })),
    )
    .await;
    assert_eq!(claim.status(), StatusCode::OK);
    let claim = payload(claim).await;
    let developer_token = claim["developer_token"].as_str().unwrap().to_owned();
    assert!(claim["workspace_invite_token"].as_str().is_some());

    let overview = send(
        &app,
        Method::GET,
        "/v1/developer/overview",
        Some(&developer_token),
        None,
    )
    .await;
    assert_eq!(overview.status(), StatusCode::OK);
    let overview = payload(overview).await;
    let pending_problems: Vec<&str> = overview["pending_memories"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|item| item["problem"].as_str())
        .collect();
    assert!(
        pending_problems.contains(&"pgvector 索引在数据量小时召回不稳定"),
        "待审核列表应包含申请公开的记忆"
    );

    let login = send(
        &app,
        Method::POST,
        "/v1/developers/login",
        None,
        Some(json!({ "login_name": "it_dev", "password": "password123" })),
    )
    .await;
    assert_eq!(login.status(), StatusCode::OK);

    let publish = send(
        &app,
        Method::POST,
        &format!("/v1/memories/{memory_id}/publish"),
        Some(&developer_token),
        None,
    )
    .await;
    assert_eq!(publish.status(), StatusCode::OK);
    assert_eq!(payload(publish).await["visibility"], "public");

    let anonymous = send(
        &app,
        Method::GET,
        &format!("/v1/memories/{memory_id}"),
        None,
        None,
    )
    .await;
    assert_eq!(anonymous.status(), StatusCode::OK);

    let public_overview = send(&app, Method::GET, "/v1/public/overview", None, None).await;
    assert_eq!(public_overview.status(), StatusCode::OK);
    let stats = payload(public_overview).await["stats"].clone();
    assert_eq!(stats["public_memories"], 1);
    assert_eq!(stats["agents"], 1);

    let remove = send(
        &app,
        Method::POST,
        &format!("/v1/memories/{memory_id}/remove"),
        Some(&developer_token),
        None,
    )
    .await;
    assert_eq!(remove.status(), StatusCode::NO_CONTENT);
    let after_remove = send(
        &app,
        Method::GET,
        &format!("/v1/memories/{memory_id}"),
        None,
        None,
    )
    .await;
    assert_eq!(after_remove.status(), StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "../migrations")]
async fn memory_creation_is_rate_limited_per_agent(pool: PgPool) {
    let app = test_app(pool);
    let registration = register(&app, "限流 Agent").await;
    let api_key = registration["api_key"].as_str().unwrap().to_owned();

    for round in 0..30 {
        let response = send(
            &app,
            Method::POST,
            "/v1/memories",
            Some(&api_key),
            Some(memory_input(&format!("限流压测第 {round} 条记忆"))),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK, "第 {round} 条不应被限流");
    }
    let blocked = send(
        &app,
        Method::POST,
        "/v1/memories",
        Some(&api_key),
        Some(memory_input("超过限额的写入")),
    )
    .await;
    assert_eq!(blocked.status(), StatusCode::TOO_MANY_REQUESTS);
    let message = payload(blocked).await;
    assert_eq!(message["error"]["code"], "rate_limited");
}

#[sqlx::test(migrations = "../migrations")]
async fn gap_lifecycle_from_create_to_closed_solution(pool: PgPool) {
    let app = test_app(pool);
    let registration = register(&app, "缺口流程 Agent").await;
    let api_key = registration["api_key"].as_str().unwrap().to_owned();

    let gap = send(
        &app,
        Method::POST,
        "/v1/gaps",
        Some(&api_key),
        Some(json!({
            "question": "本地后端进程常驻时如何安全执行 cargo 集成测试",
            "context": { "technologies": ["cargo", "Windows 11"] },
            "attempted": "直接运行集成测试报 os error 5"
        })),
    )
    .await;
    assert_eq!(gap.status(), StatusCode::OK);
    let gap = payload(gap).await;
    let gap_id = gap["id"].as_str().unwrap().to_owned();

    let anonymous_list = send(&app, Method::GET, "/v1/gaps", None, None).await;
    assert_eq!(anonymous_list.status(), StatusCode::OK);
    assert_eq!(payload(anonymous_list).await["total"], 0, "匿名不应看到 developer_shared 缺口");

    let open_list = send(
        &app,
        Method::GET,
        "/v1/gaps?status=open",
        Some(&api_key),
        None,
    )
    .await;
    assert_eq!(open_list.status(), StatusCode::OK);
    let open_list = payload(open_list).await;
    assert_eq!(open_list["total"], 1);
    assert_eq!(open_list["items"][0]["linked_count"], 0);

    let created = send(
        &app,
        Method::POST,
        "/v1/memories",
        Some(&api_key),
        Some(json!({
            "problem": "本地后端进程常驻时执行 cargo 集成测试被文件锁拒绝",
            "conditions": { "technologies": ["cargo", "Windows 11"] },
            "action": "先 Stop-Process 停后端再跑测试，测完重新拉起",
            "outcome": "集成测试全部通过",
            "outcome_kind": "success",
            "language": "zh-CN",
            "tags": ["cargo"],
            "gap_id": gap_id
        })),
    )
    .await;
    assert_eq!(created.status(), StatusCode::OK);
    let memory_id = payload(created).await["id"].as_str().unwrap().to_owned();

    let closed_list = send(
        &app,
        Method::GET,
        "/v1/gaps?status=closed",
        Some(&api_key),
        None,
    )
    .await;
    assert_eq!(closed_list.status(), StatusCode::OK);
    let closed_list = payload(closed_list).await;
    assert_eq!(closed_list["total"], 1);
    assert_eq!(closed_list["items"][0]["linked_count"], 1, "关联解法后缺口应转为已闭环");

    let still_open = send(
        &app,
        Method::GET,
        "/v1/gaps?status=open",
        Some(&api_key),
        None,
    )
    .await;
    assert_eq!(payload(still_open).await["total"], 0);

    let gap_detail = send(
        &app,
        Method::GET,
        &format!("/v1/gaps/{gap_id}"),
        Some(&api_key),
        None,
    )
    .await;
    assert_eq!(gap_detail.status(), StatusCode::OK);
    let gap_detail = payload(gap_detail).await;
    assert_eq!(
        gap_detail["gap"]["question"],
        "本地后端进程常驻时如何安全执行 cargo 集成测试"
    );
    assert_eq!(
        gap_detail["gap"]["attempted"],
        "直接运行集成测试报 os error 5"
    );
    let solutions: Vec<&str> = gap_detail["memories"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|item| item["id"].as_str())
        .collect();
    assert_eq!(solutions, vec![memory_id.as_str()], "缺口详情应包含关联解法");

    let memory_detail = send(
        &app,
        Method::GET,
        &format!("/v1/memories/{memory_id}"),
        Some(&api_key),
        None,
    )
    .await;
    assert_eq!(memory_detail.status(), StatusCode::OK);
    let memory_detail = payload(memory_detail).await;
    assert_eq!(memory_detail["gaps"][0]["id"].as_str().unwrap(), gap_id, "记忆详情应反向挂载缺口");

    let feedback = send(
        &app,
        Method::POST,
        &format!("/v1/memories/{memory_id}/feedback"),
        Some(&api_key),
        Some(json!({ "verdict": "worked", "note": "复用成功", "evidence": "cargo test 全绿" })),
    )
    .await;
    assert_eq!(feedback.status(), StatusCode::OK);

    let feedback_list = send(
        &app,
        Method::GET,
        &format!("/v1/memories/{memory_id}/feedback"),
        Some(&api_key),
        None,
    )
    .await;
    assert_eq!(feedback_list.status(), StatusCode::OK);
    let feedback_list = payload(feedback_list).await;
    assert_eq!(feedback_list[0]["source_type"], "agent");
    assert_eq!(feedback_list[0]["verdict"], "worked");
    assert_eq!(feedback_list[0]["evidence"], "cargo test 全绿");
}

#[sqlx::test(migrations = "../migrations")]
async fn gap_without_credentials_and_bad_filters_are_rejected(pool: PgPool) {
    let app = test_app(pool);

    let create = send(
        &app,
        Method::POST,
        "/v1/gaps",
        None,
        Some(json!({ "question": "匿名不应创建缺口" })),
    )
    .await;
    assert_eq!(create.status(), StatusCode::UNAUTHORIZED);

    let unknown = send(
        &app,
        Method::GET,
        "/v1/gaps/11111111-1111-1111-1111-111111111111",
        None,
        None,
    )
    .await;
    assert_eq!(unknown.status(), StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "../migrations")]
async fn unknown_memory_id_returns_not_found(pool: PgPool) {
    let app = test_app(pool);
    let response = send(
        &app,
        Method::GET,
        "/v1/memories/11111111-1111-1111-1111-111111111111",
        None,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
