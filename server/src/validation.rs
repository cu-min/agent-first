use std::collections::HashSet;

use serde_json::{Value, json};

use crate::{
    error::{ApiError, ApiResult},
    models::{EvidenceKind, MemoryInput},
    security,
};

pub(crate) fn empty_object() -> Value {
    json!({})
}

pub(crate) fn validate_memory_input(input: &MemoryInput) -> ApiResult<()> {
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

pub(crate) fn validate_json(value: &Value, field: &str, maximum: usize) -> ApiResult<()> {
    let raw = serde_json::to_string(value)
        .map_err(|_| ApiError::bad_request(format!("{field} 格式无效")))?;
    security::validate_text(&raw, field, 1, maximum).map_err(ApiError::bad_request)
}

pub(crate) fn validate_optional_text(
    value: &Option<String>,
    field: &str,
    maximum: usize,
) -> ApiResult<()> {
    if let Some(value) = value {
        security::validate_text(value, field, 1, maximum).map_err(ApiError::bad_request)?;
    }
    Ok(())
}

pub(crate) fn validate_name(value: &str, field: &str) -> ApiResult<()> {
    security::validate_text(value, field, 1, 120).map_err(ApiError::bad_request)
}

pub(crate) fn normalize_optional(
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

pub(crate) fn is_unique_violation(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .and_then(|database| database.code())
        .is_some_and(|code| code == "23505")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{MemoryInput, OutcomeKind};
    use serde_json::json;

    fn input(problem: &str, relations: Vec<(String, String)>) -> MemoryInput {
        MemoryInput {
            problem: problem.to_owned(),
            conditions: json!({}),
            action: "执行操作".to_owned(),
            outcome: "成功".to_owned(),
            outcome_kind: OutcomeKind::Success,
            visibility: None,
            request_public: false,
            language: None,
            tags: Vec::new(),
            evidence: Vec::new(),
            relations: relations
                .into_iter()
                .map(|(id, _)| crate::models::RelationInput {
                    target_memory_id: Uuid::parse_str(&id).unwrap(),
                    relation_type: crate::models::RelationKind::Patches,
                })
                .collect(),
            gap_id: None,
        }
    }

    use uuid::Uuid;

    #[test]
    fn valid_input_passes() {
        assert!(validate_memory_input(&input("连接超时怎么办", vec![])).is_ok());
    }

    #[test]
    fn short_problem_is_rejected() {
        assert!(validate_memory_input(&input("短", vec![])).is_err());
    }

    #[test]
    fn duplicate_relations_are_rejected() {
        let id = Uuid::new_v4().to_string();
        let relations = vec![
            (id.clone(), "patches".to_owned()),
            (id, "patches".to_owned()),
        ];
        let error = validate_memory_input(&input("合法的问题描述", relations)).unwrap_err();
        assert_eq!(error.to_string(), "不能重复关联同一条记忆");
    }

    #[test]
    fn json_field_size_is_enforced() {
        let oversized = Value::String("长".repeat(6001));
        assert!(validate_json(&oversized, "条件", 6000).is_err());
        assert!(validate_json(&json!({ "ok": 1 }), "条件", 6000).is_ok());
    }

    #[test]
    fn link_evidence_requires_https_url() {
        let mut item = input("链接证据必须校验协议", vec![]);
        item.evidence = vec![crate::models::EvidenceInput {
            kind: crate::models::EvidenceKind::Link,
            label: None,
            value: "http://example.com/issue".to_owned(),
        }];
        assert!(validate_memory_input(&item).is_err());
        item.evidence[0].value = "https://example.com/issue".to_owned();
        assert!(validate_memory_input(&item).is_ok());
    }

    #[test]
    fn evidence_count_is_capped_at_eight() {
        let mut item = input("证据条数存在上限", vec![]);
        item.evidence = (0..9)
            .map(|_| crate::models::EvidenceInput {
                kind: crate::models::EvidenceKind::Log,
                label: None,
                value: "普通日志内容".to_owned(),
            })
            .collect();
        assert!(validate_memory_input(&item).is_err());
        item.evidence.truncate(8);
        assert!(validate_memory_input(&item).is_ok());
    }

    #[test]
    fn relation_count_is_capped_at_eight() {
        let mut item = input("关联条数存在上限", vec![]);
        item.relations = (0..9)
            .map(|_| crate::models::RelationInput {
                target_memory_id: Uuid::new_v4(),
                relation_type: crate::models::RelationKind::Patches,
            })
            .collect();
        assert!(validate_memory_input(&item).is_err());
    }

    #[test]
    fn sensitive_evidence_is_rejected() {
        let mut item = input("证据内容需要脱敏", vec![]);
        item.evidence = vec![crate::models::EvidenceInput {
            kind: crate::models::EvidenceKind::Log,
            label: None,
            value: "sk-abcdefghijklmnopqrstuvwxyz012345".to_owned(),
        }];
        assert!(validate_memory_input(&item).is_err());
    }
}
