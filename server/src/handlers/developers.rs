use std::{net::SocketAddr, time::Duration};

use axum::{
    Json,
    extract::{ConnectInfo, Path, State},
    http::{HeaderMap, StatusCode},
};
use sqlx::Row;
use time::{Duration as TimeDuration, OffsetDateTime};
use tracing::info;
use uuid::Uuid;

use crate::{
    auth::require_developer,
    error::{ApiError, ApiResult},
    models::{
        AgentOverview, ClaimWorkspaceInput, DeveloperOverview, DeveloperSessionOutput, LoginInput,
        PolicyInput, RotatedWorkspaceInviteOutput, WorkspaceOverview,
    },
    net::client_ip,
    ratelimit::ensure_rate,
    security,
    state::AppState,
    store::fetch_memory_summaries,
    validation::is_unique_violation,
};

pub(crate) async fn claim_workspace(
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

pub(crate) async fn login(
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

pub(crate) async fn delete_developer_account(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<crate::models::DeleteAccountInput>,
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

pub(crate) async fn create_developer_session(
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

pub(crate) async fn developer_overview(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<DeveloperOverview>> {
    let developer = require_developer(&state, &headers).await?;
    let workspaces = sqlx::query_as::<_, WorkspaceOverview>(
        "SELECT id, name, publication_policy, created_at, updated_at FROM workspaces WHERE developer_id = $1 ORDER BY created_at DESC",
    ).bind(developer.developer_id).fetch_all(&state.pool).await?;
    let agents = sqlx::query_as::<_, AgentOverview>(
        "SELECT a.id, a.workspace_id, a.name, a.created_at, \
         (SELECT count(*) FROM memories m WHERE m.author_agent_id = a.id AND m.removed_at IS NULL) AS memory_count, \
         (SELECT count(*) FROM memories m WHERE m.author_agent_id = a.id AND m.removed_at IS NULL AND m.visibility = 'public') AS public_count, \
         (SELECT count(*) FROM memory_feedback f WHERE f.agent_id = a.id) AS feedback_count, \
         GREATEST(\
             (SELECT max(m.created_at) FROM memories m WHERE m.author_agent_id = a.id), \
             (SELECT max(f.created_at) FROM memory_feedback f WHERE f.agent_id = a.id)\
         ) AS last_active_at \
         FROM agents a JOIN workspaces w ON w.id = a.workspace_id WHERE w.developer_id = $1 AND a.revoked_at IS NULL ORDER BY a.created_at DESC",
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

pub(crate) async fn update_publication_policy(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(input): Json<PolicyInput>,
) -> ApiResult<Json<serde_json::Value>> {
    let developer = require_developer(&state, &headers).await?;
    if !matches!(input.publication_policy.as_str(), "manual" | "auto") {
        return Err(ApiError::bad_request("公开策略只能是 manual 或 auto"));
    }
    crate::authz::ensure_workspace_owner(&state.pool, id, developer.developer_id).await?;
    sqlx::query("UPDATE workspaces SET publication_policy = $1, updated_at = now() WHERE id = $2")
        .bind(&input.publication_policy)
        .bind(id)
        .execute(&state.pool)
        .await?;
    Ok(Json(
        serde_json::json!({ "workspace_id": id, "publication_policy": input.publication_policy }),
    ))
}

pub(crate) async fn rotate_workspace_invite(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<RotatedWorkspaceInviteOutput>> {
    let developer = require_developer(&state, &headers).await?;
    crate::authz::ensure_workspace_owner(&state.pool, id, developer.developer_id).await?;
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
