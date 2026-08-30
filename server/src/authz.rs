use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    auth::ReadPrincipal,
    error::{ApiError, ApiResult},
    models::{AgentPrincipal, MemoryAccessRow},
};

pub(crate) async fn load_memory_access(pool: &PgPool, id: Uuid) -> ApiResult<MemoryAccessRow> {
    sqlx::query_as::<_, MemoryAccessRow>(
        "SELECT id, workspace_id, author_agent_id, visibility, removed_at FROM memories WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| ApiError::not_found("经验不存在"))
}

pub(crate) async fn load_gap_access(pool: &PgPool, id: Uuid) -> ApiResult<MemoryAccessRow> {
    sqlx::query_as::<_, MemoryAccessRow>(
        "SELECT id, workspace_id, author_agent_id, visibility, removed_at FROM experience_gaps WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| ApiError::not_found("经验缺口不存在"))
}

pub(crate) fn can_read_row(row: &MemoryAccessRow, agent: Option<&AgentPrincipal>) -> bool {
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

pub(crate) async fn can_read_memory(
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

pub(crate) fn can_read_row_principal(row: &MemoryAccessRow, principal: &ReadPrincipal) -> bool {
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

pub(crate) async fn can_read_memory_principal(
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

pub(crate) async fn ensure_workspace_owner(
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

pub(crate) async fn can_read_gap(
    pool: &PgPool,
    id: Uuid,
    agent: &AgentPrincipal,
) -> ApiResult<bool> {
    can_read_gap_with_optional(pool, id, Some(agent)).await
}

pub(crate) async fn can_read_gap_with_optional(
    pool: &PgPool,
    id: Uuid,
    agent: Option<&AgentPrincipal>,
) -> ApiResult<bool> {
    let Some(row) = sqlx::query_as::<_, MemoryAccessRow>(
        "SELECT id, workspace_id, author_agent_id, visibility, removed_at FROM experience_gaps WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    else {
        return Ok(false);
    };
    Ok(can_read_row(&row, agent))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::ReadPrincipal;
    use crate::models::{AgentPrincipal, MemoryAccessRow};
    use time::OffsetDateTime;

    fn row(visibility: &str, workspace_id: Uuid, author: Uuid) -> MemoryAccessRow {
        MemoryAccessRow {
            id: Uuid::new_v4(),
            workspace_id,
            author_agent_id: author,
            visibility: visibility.to_owned(),
            removed_at: None,
        }
    }

    fn agent(workspace_id: Uuid, agent_id: Uuid) -> AgentPrincipal {
        AgentPrincipal {
            agent_id,
            workspace_id,
            developer_id: None,
            publication_policy: "manual".to_owned(),
        }
    }

    #[test]
    fn public_rows_are_readable_by_everyone() {
        let row = row("public", Uuid::new_v4(), Uuid::new_v4());
        assert!(can_read_row(&row, None));
        assert!(can_read_row_principal(&row, &ReadPrincipal::Anonymous));
    }

    #[test]
    fn agent_private_requires_same_agent() {
        let workspace = Uuid::new_v4();
        let author = Uuid::new_v4();
        let row = row("agent_private", workspace, author);
        assert!(can_read_row(&row, Some(&agent(workspace, author))));
        assert!(!can_read_row(&row, Some(&agent(workspace, Uuid::new_v4()))));
        assert!(!can_read_row(&row, None));
    }

    #[test]
    fn developer_shared_requires_same_workspace() {
        let workspace = Uuid::new_v4();
        let row = row("developer_shared", workspace, Uuid::new_v4());
        assert!(can_read_row(&row, Some(&agent(workspace, Uuid::new_v4()))));
        assert!(!can_read_row(
            &row,
            Some(&agent(Uuid::new_v4(), Uuid::new_v4()))
        ));
    }

    #[test]
    fn developer_principal_reads_any_visibility_in_owned_workspaces() {
        let workspace = Uuid::new_v4();
        let principal = ReadPrincipal::Developer {
            workspaces: vec![workspace],
        };
        assert!(can_read_row_principal(
            &row("developer_shared", workspace, Uuid::new_v4()),
            &principal
        ));
        assert!(can_read_row_principal(
            &row("agent_private", workspace, Uuid::new_v4()),
            &principal
        ));
        assert!(!can_read_row_principal(
            &row("agent_private", Uuid::new_v4(), Uuid::new_v4()),
            &principal
        ));
    }

    #[test]
    fn removed_rows_are_never_readable() {
        let mut row = row("public", Uuid::new_v4(), Uuid::new_v4());
        row.removed_at = Some(OffsetDateTime::now_utc());
        assert!(!can_read_row(&row, None));
        assert!(!can_read_row_principal(&row, &ReadPrincipal::Anonymous));
    }
}
