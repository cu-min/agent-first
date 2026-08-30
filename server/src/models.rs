use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::FromRow;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::validation::empty_object;

#[derive(Clone, FromRow)]
pub(crate) struct AgentPrincipal {
    pub(crate) agent_id: Uuid,
    pub(crate) workspace_id: Uuid,
    pub(crate) developer_id: Option<Uuid>,
    pub(crate) publication_policy: String,
}

#[derive(Clone, FromRow)]
pub(crate) struct DeveloperPrincipal {
    pub(crate) developer_id: Uuid,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Visibility {
    AgentPrivate,
    DeveloperShared,
    Public,
}

impl Visibility {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::AgentPrivate => "agent_private",
            Self::DeveloperShared => "developer_shared",
            Self::Public => "public",
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum OutcomeKind {
    Success,
    Failure,
    Partial,
    Unknown,
}

impl OutcomeKind {
    pub(crate) fn as_str(self) -> &'static str {
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
pub(crate) enum EvidenceKind {
    Log,
    Test,
    Link,
    HumanNote,
    Other,
}

impl EvidenceKind {
    pub(crate) fn as_str(self) -> &'static str {
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
pub(crate) enum RelationKind {
    Patches,
    Contradicts,
    Supersedes,
    Expires,
}

impl RelationKind {
    pub(crate) fn as_str(self) -> &'static str {
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
pub(crate) enum FeedbackVerdict {
    Useful,
    NotUseful,
    Worked,
    PartiallyWorked,
    Failed,
}

impl FeedbackVerdict {
    pub(crate) fn as_str(self) -> &'static str {
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
pub(crate) struct RegisterAgentInput {
    pub(crate) name: String,
    pub(crate) invite_token: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct CreateAgentInput {
    pub(crate) workspace_id: Uuid,
    pub(crate) name: String,
}

#[derive(Deserialize)]
pub(crate) struct RenameAgentInput {
    pub(crate) name: String,
}

#[derive(Serialize)]
pub(crate) struct RegisterAgentOutput {
    pub(crate) agent_id: Uuid,
    pub(crate) workspace_id: Uuid,
    pub(crate) api_key: String,
    pub(crate) claim_token: Option<String>,
    pub(crate) warning: &'static str,
}

#[derive(Serialize)]
pub(crate) struct RotatedAgentKeyOutput {
    pub(crate) api_key: String,
    pub(crate) warning: &'static str,
}

#[derive(Serialize)]
pub(crate) struct RotatedWorkspaceInviteOutput {
    pub(crate) workspace_invite_token: String,
    pub(crate) warning: &'static str,
}

#[derive(Deserialize)]
pub(crate) struct ClaimWorkspaceInput {
    pub(crate) claim_token: String,
    pub(crate) login_name: String,
    pub(crate) password: String,
}

#[derive(Deserialize)]
pub(crate) struct LoginInput {
    pub(crate) login_name: String,
    pub(crate) password: String,
}

#[derive(Deserialize)]
pub(crate) struct DeleteAccountInput {
    pub(crate) password: String,
    pub(crate) confirmation: String,
}

#[derive(Serialize)]
pub(crate) struct DeveloperSessionOutput {
    pub(crate) developer_token: String,
    #[serde(with = "time::serde::rfc3339")]
    pub(crate) expires_at: OffsetDateTime,
    pub(crate) workspace_invite_token: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum SearchDetail {
    #[default]
    Fingerprint,
    Full,
}

#[derive(Deserialize)]
pub(crate) struct SearchInput {
    pub(crate) query: String,
    pub(crate) language: Option<String>,
    #[serde(default)]
    pub(crate) tags: Vec<String>,
    pub(crate) technology: Option<String>,
    pub(crate) limit: Option<u8>,
    #[serde(default)]
    pub(crate) detail: SearchDetail,
}

#[derive(Serialize)]
pub(crate) struct SearchOutput {
    pub(crate) items: Vec<SearchHit>,
    pub(crate) related_gaps: Vec<RelatedGap>,
    pub(crate) retrieval: &'static str,
    pub(crate) untrusted_content: bool,
}

#[derive(Serialize)]
pub(crate) struct RelatedGap {
    pub(crate) id: Uuid,
    pub(crate) question: String,
    pub(crate) closed: bool,
    pub(crate) score: f64,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SearchRelevance {
    Exact,
    Related,
}

#[cfg(test)]
impl SearchRelevance {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::Related => "related",
        }
    }
}

#[derive(Deserialize)]
pub(crate) struct MemoryInput {
    pub(crate) problem: String,
    #[serde(default = "empty_object")]
    pub(crate) conditions: Value,
    pub(crate) action: String,
    pub(crate) outcome: String,
    pub(crate) outcome_kind: OutcomeKind,
    pub(crate) visibility: Option<Visibility>,
    #[serde(default)]
    pub(crate) request_public: bool,
    pub(crate) language: Option<String>,
    #[serde(default)]
    pub(crate) tags: Vec<String>,
    #[serde(default)]
    pub(crate) evidence: Vec<EvidenceInput>,
    #[serde(default)]
    pub(crate) relations: Vec<RelationInput>,
    pub(crate) gap_id: Option<Uuid>,
}

#[derive(Deserialize)]
pub(crate) struct EvidenceInput {
    pub(crate) kind: EvidenceKind,
    pub(crate) label: Option<String>,
    pub(crate) value: String,
}

#[derive(Deserialize)]
pub(crate) struct RelationInput {
    pub(crate) target_memory_id: Uuid,
    pub(crate) relation_type: RelationKind,
}

#[derive(Deserialize)]
pub(crate) struct MemoryImportInput {
    #[serde(default)]
    pub(crate) memories: Vec<MemoryInput>,
}

#[derive(Serialize)]
pub(crate) struct MemoryImportedOutput {
    pub(crate) imported: usize,
    pub(crate) ids: Vec<Uuid>,
}

#[derive(Serialize)]
pub(crate) struct MemoryCreatedOutput {
    pub(crate) id: Uuid,
    pub(crate) visibility: String,
    pub(crate) publication_state: &'static str,
}

#[derive(Deserialize)]
pub(crate) struct FeedbackInput {
    pub(crate) verdict: FeedbackVerdict,
    pub(crate) note: Option<String>,
    pub(crate) evidence: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct GapInput {
    pub(crate) question: String,
    #[serde(default = "empty_object")]
    pub(crate) context: Value,
    pub(crate) attempted: Option<String>,
    pub(crate) visibility: Option<Visibility>,
    pub(crate) language: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct CreatedId {
    pub(crate) id: Uuid,
}

#[derive(Serialize, FromRow)]
pub(crate) struct MemorySummary {
    pub(crate) id: Uuid,
    pub(crate) visibility: String,
    pub(crate) problem: String,
    pub(crate) conditions: Value,
    pub(crate) action: String,
    pub(crate) outcome: String,
    pub(crate) outcome_kind: String,
    pub(crate) source_type: String,
    pub(crate) language: String,
    pub(crate) tags: Vec<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub(crate) created_at: OffsetDateTime,
    pub(crate) author_agent_name: Option<String>,
    pub(crate) evidence_count: i64,
    pub(crate) agent_positive_feedback: i64,
    pub(crate) human_positive_feedback: i64,
}

#[derive(Serialize)]
pub(crate) struct SearchHit {
    pub(crate) id: Uuid,
    pub(crate) visibility: String,
    pub(crate) problem: String,
    pub(crate) conditions: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) action: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) score: Option<f64>,
    pub(crate) relevance: SearchRelevance,
    pub(crate) outcome: String,
    pub(crate) outcome_kind: String,
    pub(crate) source_type: String,
    pub(crate) language: String,
    pub(crate) tags: Vec<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub(crate) created_at: OffsetDateTime,
    pub(crate) author_agent_name: Option<String>,
    pub(crate) evidence_count: i64,
    pub(crate) agent_positive_feedback: i64,
    pub(crate) human_positive_feedback: i64,
}

impl SearchHit {
    pub(crate) fn from_summary(
        summary: MemorySummary,
        include_action: bool,
        score: Option<f64>,
        relevance: SearchRelevance,
    ) -> Self {
        SearchHit {
            id: summary.id,
            visibility: summary.visibility,
            problem: summary.problem,
            conditions: summary.conditions,
            action: include_action.then_some(summary.action),
            score,
            relevance,
            outcome: summary.outcome,
            outcome_kind: summary.outcome_kind,
            source_type: summary.source_type,
            language: summary.language,
            tags: summary.tags,
            created_at: summary.created_at,
            author_agent_name: summary.author_agent_name,
            evidence_count: summary.evidence_count,
            agent_positive_feedback: summary.agent_positive_feedback,
            human_positive_feedback: summary.human_positive_feedback,
        }
    }
}

#[derive(Serialize, FromRow)]
pub(crate) struct EvidenceRecord {
    pub(crate) id: Uuid,
    pub(crate) kind: String,
    pub(crate) label: Option<String>,
    pub(crate) value: String,
    #[serde(with = "time::serde::rfc3339")]
    pub(crate) created_at: OffsetDateTime,
}

#[derive(Serialize, FromRow)]
pub(crate) struct RelationRecord {
    pub(crate) target_memory_id: Uuid,
    pub(crate) relation_type: String,
    #[serde(with = "time::serde::rfc3339")]
    pub(crate) created_at: OffsetDateTime,
}

#[derive(Serialize)]
pub(crate) struct MemoryDetail {
    pub(crate) memory: MemorySummary,
    pub(crate) evidence: Vec<EvidenceRecord>,
    pub(crate) relations: Vec<RelationRecord>,
    pub(crate) gaps: Vec<GapBacklink>,
    pub(crate) untrusted_content: bool,
}

#[derive(FromRow)]
pub(crate) struct MemoryAccessRow {
    pub(crate) id: Uuid,
    pub(crate) workspace_id: Uuid,
    pub(crate) author_agent_id: Uuid,
    pub(crate) visibility: String,
    pub(crate) removed_at: Option<OffsetDateTime>,
}

#[derive(Serialize, FromRow)]
pub(crate) struct WorkspaceOverview {
    pub(crate) id: Uuid,
    pub(crate) name: String,
    pub(crate) publication_policy: String,
    #[serde(with = "time::serde::rfc3339")]
    pub(crate) created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub(crate) updated_at: OffsetDateTime,
}

#[derive(Serialize, FromRow)]
pub(crate) struct AgentOverview {
    pub(crate) id: Uuid,
    pub(crate) workspace_id: Uuid,
    pub(crate) name: String,
    #[serde(with = "time::serde::rfc3339")]
    pub(crate) created_at: OffsetDateTime,
    pub(crate) memory_count: i64,
    pub(crate) public_count: i64,
    pub(crate) feedback_count: i64,
    #[serde(with = "time::serde::rfc3339::option")]
    pub(crate) last_active_at: Option<OffsetDateTime>,
}

#[derive(Serialize)]
pub(crate) struct DeveloperOverview {
    pub(crate) workspaces: Vec<WorkspaceOverview>,
    pub(crate) agents: Vec<AgentOverview>,
    pub(crate) pending_memories: Vec<MemorySummary>,
}

#[derive(Serialize, FromRow)]
pub(crate) struct GapRecord {
    pub(crate) id: Uuid,
    pub(crate) visibility: String,
    pub(crate) question: String,
    pub(crate) context: Value,
    pub(crate) attempted: Option<String>,
    pub(crate) language: String,
    #[serde(with = "time::serde::rfc3339")]
    pub(crate) created_at: OffsetDateTime,
}

#[derive(Serialize, FromRow)]
pub(crate) struct GapListItem {
    pub(crate) id: Uuid,
    pub(crate) visibility: String,
    pub(crate) question: String,
    pub(crate) context: Value,
    pub(crate) attempted: Option<String>,
    pub(crate) language: String,
    #[serde(with = "time::serde::rfc3339")]
    pub(crate) created_at: OffsetDateTime,
    pub(crate) linked_count: i64,
}

#[derive(Serialize)]
pub(crate) struct GapListOutput {
    pub(crate) items: Vec<GapListItem>,
    pub(crate) total: i64,
    pub(crate) limit: i64,
    pub(crate) offset: i64,
}

#[derive(Serialize, FromRow)]
pub(crate) struct GapBacklink {
    pub(crate) id: Uuid,
    pub(crate) question: String,
}

#[derive(Serialize)]
pub(crate) struct GapDetail {
    pub(crate) gap: GapRecord,
    pub(crate) memories: Vec<MemorySummary>,
    pub(crate) untrusted_content: bool,
}

#[derive(Deserialize)]
pub(crate) struct PolicyInput {
    pub(crate) publication_policy: String,
}

#[derive(Deserialize)]
pub(crate) struct ListMemoriesQuery {
    pub(crate) limit: Option<i64>,
    pub(crate) offset: Option<i64>,
    pub(crate) visibility: Option<String>,
    pub(crate) outcome_kind: Option<String>,
    pub(crate) since: Option<String>,
    pub(crate) until: Option<String>,
    pub(crate) order_by: Option<String>,
    /// 开发者控制台按 Agent 过滤；仅在开发者作用域下生效（受 w.developer_id 约束）
    pub(crate) agent_id: Option<Uuid>,
}

#[derive(Deserialize)]
pub(crate) struct ListGapsQuery {
    pub(crate) limit: Option<i64>,
    pub(crate) offset: Option<i64>,
    pub(crate) visibility: Option<String>,
    pub(crate) language: Option<String>,
    pub(crate) status: Option<String>,
    pub(crate) since: Option<String>,
    pub(crate) until: Option<String>,
    pub(crate) order_by: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct MemoryListOutput {
    pub(crate) items: Vec<MemorySummary>,
    pub(crate) total: i64,
    pub(crate) limit: i64,
    pub(crate) offset: i64,
}

#[derive(Serialize)]
pub(crate) struct PublicStats {
    pub(crate) public_memories: i64,
    pub(crate) agents: i64,
    pub(crate) reuse_total: i64,
}

#[derive(Serialize, FromRow)]
pub(crate) struct ActivityItem {
    pub(crate) kind: String,
    #[serde(with = "time::serde::rfc3339")]
    pub(crate) at: OffsetDateTime,
    pub(crate) problem: String,
    pub(crate) agent_name: Option<String>,
    pub(crate) actor_kind: Option<String>,
    pub(crate) verdict: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct PublicOverview {
    pub(crate) stats: PublicStats,
    pub(crate) activity: Vec<ActivityItem>,
    pub(crate) top: Vec<MemorySummary>,
}

#[derive(Serialize, FromRow)]
pub(crate) struct FeedbackRecord {
    pub(crate) source_type: String,
    pub(crate) verdict: String,
    pub(crate) note: Option<String>,
    pub(crate) evidence: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub(crate) created_at: OffsetDateTime,
}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! snake_case_round_trip {
        ($name:ident, $ty:ty, [$($variant:expr => $raw:expr),+ $(,)?]) => {
            #[test]
            fn $name() {
                $(
                    let encoded = serde_json::to_string(&$variant).unwrap();
                    assert_eq!(encoded, concat!('"', $raw, '"'));
                    let decoded: $ty = serde_json::from_str(&encoded).unwrap();
                    assert_eq!(serde_json::to_string(&decoded).unwrap(), encoded);
                    assert_eq!($variant.as_str(), $raw);
                )+
            }
        };
    }

    snake_case_round_trip!(visibility_round_trips, Visibility, [
        Visibility::AgentPrivate => "agent_private",
        Visibility::DeveloperShared => "developer_shared",
        Visibility::Public => "public",
    ]);

    snake_case_round_trip!(outcome_kind_round_trips, OutcomeKind, [
        OutcomeKind::Success => "success",
        OutcomeKind::Failure => "failure",
        OutcomeKind::Partial => "partial",
        OutcomeKind::Unknown => "unknown",
    ]);

    snake_case_round_trip!(evidence_kind_round_trips, EvidenceKind, [
        EvidenceKind::Log => "log",
        EvidenceKind::Test => "test",
        EvidenceKind::Link => "link",
        EvidenceKind::HumanNote => "human_note",
        EvidenceKind::Other => "other",
    ]);

    snake_case_round_trip!(relation_kind_round_trips, RelationKind, [
        RelationKind::Patches => "patches",
        RelationKind::Contradicts => "contradicts",
        RelationKind::Supersedes => "supersedes",
        RelationKind::Expires => "expires",
    ]);

    snake_case_round_trip!(feedback_verdict_round_trips, FeedbackVerdict, [
        FeedbackVerdict::Useful => "useful",
        FeedbackVerdict::NotUseful => "not_useful",
        FeedbackVerdict::Worked => "worked",
        FeedbackVerdict::PartiallyWorked => "partially_worked",
        FeedbackVerdict::Failed => "failed",
    ]);

    snake_case_round_trip!(search_relevance_round_trips, SearchRelevance, [
        SearchRelevance::Exact => "exact",
        SearchRelevance::Related => "related",
    ]);

    #[test]
    fn unknown_enum_values_are_rejected() {
        assert!(serde_json::from_str::<Visibility>("\"private\"").is_err());
        assert!(serde_json::from_str::<OutcomeKind>("\"SUCCESS\"").is_err());
    }

    #[test]
    fn memory_input_applies_documented_defaults() {
        let input: MemoryInput = serde_json::from_str(
            r#"{"problem":"连接超时","action":"调整超时参数","outcome":"成功","outcome_kind":"success"}"#,
        )
        .unwrap();
        assert!(input.conditions.as_object().unwrap().is_empty());
        assert!(input.tags.is_empty());
        assert!(input.evidence.is_empty());
        assert!(input.relations.is_empty());
        assert!(!input.request_public);
        assert!(input.language.is_none());
        assert!(input.visibility.is_none());
        assert!(input.gap_id.is_none());
    }

    #[test]
    fn search_input_defaults_optionals() {
        let input: SearchInput = serde_json::from_str(r#"{"query":"docker 超时"}"#).unwrap();
        assert_eq!(input.query, "docker 超时");
        assert!(input.tags.is_empty());
        assert!(input.language.is_none());
        assert!(input.technology.is_none());
        assert!(input.limit.is_none());
        assert_eq!(input.detail, SearchDetail::Fingerprint);

        let full: SearchInput =
            serde_json::from_str(r#"{"query":"docker 超时","detail":"full"}"#).unwrap();
        assert_eq!(full.detail, SearchDetail::Full);
    }

    #[test]
    fn memory_import_input_defaults_to_empty_list() {
        let input: MemoryImportInput = serde_json::from_str("{}").unwrap();
        assert!(input.memories.is_empty());
    }
}
