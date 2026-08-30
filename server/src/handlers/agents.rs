use std::{net::SocketAddr, time::Duration};

use axum::{
    Json,
    extract::{ConnectInfo, Path, State},
    http::HeaderMap,
};
use sqlx::Row;
use tracing::info;
use uuid::Uuid;

use crate::{
    auth::require_developer,
    authz::ensure_workspace_owner,
    error::{ApiError, ApiResult},
    models::{
        CreateAgentInput, RegisterAgentInput, RegisterAgentOutput, RenameAgentInput,
        RotatedAgentKeyOutput,
    },
    net::client_ip,
    ratelimit::ensure_rate,
    security,
    state::AppState,
    validation::validate_name,
};

pub(crate) async fn register_agent(
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
            return Err(ApiError::forbidden("邀请码无效或工作区尚未认领"));
        };
        (workspace.get("id"), None)
    } else {
        let workspace_id = Uuid::new_v4();
        let claim_token = security::new_token("af_claim");
        sqlx::query("INSERT INTO workspaces (id, name, claim_token_hash) VALUES ($1, $2, $3)")
            .bind(workspace_id)
            .bind("我的工作区")
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

pub(crate) async fn create_agent(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<CreateAgentInput>,
) -> ApiResult<Json<RegisterAgentOutput>> {
    let developer = require_developer(&state, &headers).await?;
    validate_name(&input.name, "Agent 名称")?;
    ensure_workspace_owner(&state.pool, input.workspace_id, developer.developer_id).await?;
    let mut transaction = state.pool.begin().await?;
    let agent_id = Uuid::new_v4();
    let api_key = security::new_token("af_live");
    sqlx::query("INSERT INTO agents (id, workspace_id, name) VALUES ($1, $2, $3)")
        .bind(agent_id)
        .bind(input.workspace_id)
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
    info!(agent_id = %agent_id, "agent created by developer");
    Ok(Json(RegisterAgentOutput {
        agent_id,
        workspace_id: input.workspace_id,
        api_key,
        claim_token: None,
        warning: "api_key 仅展示一次，请保存到安全位置",
    }))
}

pub(crate) async fn rename_agent(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(input): Json<RenameAgentInput>,
) -> ApiResult<Json<serde_json::Value>> {
    let developer = require_developer(&state, &headers).await?;
    validate_name(&input.name, "Agent 名称")?;
    let agent = sqlx::query(
        "SELECT a.workspace_id FROM agents a JOIN workspaces w ON w.id = a.workspace_id \
         WHERE a.id = $1 AND w.developer_id = $2 AND a.revoked_at IS NULL",
    )
    .bind(id)
    .bind(developer.developer_id)
    .fetch_optional(&state.pool)
    .await?;
    if agent.is_none() {
        return Err(ApiError::forbidden("该 Agent 不属于当前开发者或已被停用"));
    }
    sqlx::query("UPDATE agents SET name = $1 WHERE id = $2")
        .bind(input.name.trim())
        .bind(id)
        .execute(&state.pool)
        .await?;
    info!(agent_id = %id, "agent renamed");
    Ok(Json(serde_json::json!({ "agent_id": id, "name": input.name.trim() })))
}

pub(crate) async fn rotate_agent_key(
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
