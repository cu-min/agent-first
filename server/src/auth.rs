use axum::http::{HeaderMap, header};
use uuid::Uuid;

use crate::{
    error::{ApiError, ApiResult},
    models::{AgentPrincipal, DeveloperPrincipal},
    security,
    state::AppState,
};

pub(crate) enum ListPrincipal {
    Agent(AgentPrincipal),
    Developer(DeveloperPrincipal),
}

pub(crate) enum ReadPrincipal {
    Agent(AgentPrincipal),
    Developer { workspaces: Vec<Uuid> },
    Anonymous,
}

pub(crate) async fn resolve_list_principal(
    state: &AppState,
    headers: &HeaderMap,
) -> ApiResult<ListPrincipal> {
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

pub(crate) async fn resolve_read_principal(
    state: &AppState,
    headers: &HeaderMap,
) -> ApiResult<ReadPrincipal> {
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
        let workspaces =
            sqlx::query_scalar::<_, Uuid>("SELECT id FROM workspaces WHERE developer_id = $1")
                .bind(developer.developer_id)
                .fetch_all(&state.pool)
                .await?;
        return Ok(ReadPrincipal::Developer { workspaces });
    }
    Err(ApiError::unauthorized())
}

pub(crate) fn read_scope(principal: &ReadPrincipal) -> (Vec<Uuid>, Vec<Uuid>, Option<Uuid>) {
    match principal {
        ReadPrincipal::Agent(agent) => (vec![agent.workspace_id], Vec::new(), Some(agent.agent_id)),
        ReadPrincipal::Developer { workspaces } => (Vec::new(), workspaces.clone(), None),
        ReadPrincipal::Anonymous => (Vec::new(), Vec::new(), None),
    }
}

pub(crate) async fn optional_agent(
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
    )
    .bind(security::hash_token(token))
    .fetch_optional(&state.pool)
    .await?;
    if agent.is_none() {
        return Err(ApiError::unauthorized());
    }
    Ok(agent)
}

pub(crate) async fn require_agent(
    state: &AppState,
    headers: &HeaderMap,
) -> ApiResult<AgentPrincipal> {
    optional_agent(state, headers)
        .await?
        .ok_or_else(ApiError::unauthorized)
}

pub(crate) async fn require_developer(
    state: &AppState,
    headers: &HeaderMap,
) -> ApiResult<DeveloperPrincipal> {
    let developer = sqlx::query_as::<_, DeveloperPrincipal>(
        "SELECT d.id AS developer_id FROM developer_sessions s JOIN developers d ON d.id = s.developer_id \
         WHERE s.token_hash = $1 AND s.revoked_at IS NULL AND s.expires_at > now()",
    )
    .bind(security::hash_token(bearer_token(headers)?))
    .fetch_optional(&state.pool)
    .await?;
    developer.ok_or_else(ApiError::unauthorized)
}

pub(crate) fn optional_bearer_token(headers: &HeaderMap) -> ApiResult<Option<&str>> {
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

pub(crate) fn bearer_token(headers: &HeaderMap) -> ApiResult<&str> {
    optional_bearer_token(headers)?.ok_or_else(ApiError::unauthorized)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn auth_headers(token: &str) -> HeaderMap {
        let mut map = HeaderMap::new();
        map.insert(
            header::AUTHORIZATION,
            axum::http::HeaderValue::from_str(&format!("Bearer {token}")).unwrap(),
        );
        map
    }

    #[test]
    fn missing_authorization_yields_none() {
        assert!(optional_bearer_token(&HeaderMap::new()).unwrap().is_none());
    }

    #[test]
    fn bearer_prefix_is_required() {
        let mut map = HeaderMap::new();
        map.insert(
            header::AUTHORIZATION,
            axum::http::HeaderValue::from_static("Basic dXNlcjpwYXNz"),
        );
        assert!(optional_bearer_token(&map).is_err());
    }

    #[test]
    fn empty_bearer_token_is_rejected() {
        assert!(optional_bearer_token(&auth_headers("")).is_err());
    }

    #[test]
    fn valid_token_is_extracted() {
        assert_eq!(
            optional_bearer_token(&auth_headers("af_live_token")).unwrap(),
            Some("af_live_token")
        );
    }
}
