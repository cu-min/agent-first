use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng},
};
use once_cell::sync::Lazy;
use regex::Regex;
use sha2::{Digest, Sha256};
use url::Url;
use uuid::Uuid;

static SENSITIVE_PATTERNS: Lazy<Vec<Regex>> = Lazy::new(|| {
    vec![
        Regex::new(r"(?i)-----BEGIN [A-Z ]*PRIVATE KEY-----").unwrap(),
        Regex::new(r"\bsk-[A-Za-z0-9_-]{20,}\b").unwrap(),
        Regex::new(r"\bgh[pousr]_[A-Za-z0-9_]{20,}\b").unwrap(),
        Regex::new(r"\bgithub_pat_[A-Za-z0-9_]{20,}\b").unwrap(),
        Regex::new(r"\bAKIA[0-9A-Z]{16}\b").unwrap(),
        Regex::new(r"\baf_(?:live|dev|claim|invite)_[A-Za-z0-9_-]{20,}\b").unwrap(),
        Regex::new(r"(?i)\bbearer\s+[A-Za-z0-9._~+/-]{16,}\b").unwrap(),
        Regex::new(r"\beyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\b").unwrap(),
        Regex::new(r"(?i)\b(?:postgres(?:ql)?|mysql|redis)://[^:\s/]+:[^@\s/]+@").unwrap(),
        Regex::new(
            r#"(?i)\b(?:password|passwd|api[_-]?key|access[_-]?token|secret|token|credential)\b[\"']?\s*[:=]\s*[\"']?[^\s,\"'}\]]{8,}"#,
        )
        .unwrap(),
        Regex::new(
            r#"(?:密码|口令|密钥|令牌|访问令牌|访问密钥)[\"']?\s*[:：=]\s*[\"']?[^\s,\"'}，、】【]{6,}"#,
        )
        .unwrap(),
        Regex::new(r"\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}\b").unwrap(),
        Regex::new(r"(?:^|[^0-9])1[3-9]\d{9}(?:$|[^0-9])").unwrap(),
    ]
});

pub fn new_token(prefix: &str) -> String {
    format!(
        "{prefix}_{}{}",
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple()
    )
}

pub fn hash_token(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

pub fn token_prefix(value: &str) -> String {
    value.chars().take(16).collect()
}

pub fn hash_password(password: &str) -> Result<String, String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|_| "无法安全地处理密码".to_owned())
}

pub fn verify_password(password: &str, password_hash: &str) -> bool {
    let Ok(parsed_hash) = PasswordHash::new(password_hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed_hash)
        .is_ok()
}

pub fn reject_sensitive(value: &str) -> Result<(), &'static str> {
    if SENSITIVE_PATTERNS
        .iter()
        .any(|pattern| pattern.is_match(value))
    {
        return Err("内容疑似包含密钥或个人信息，请脱敏后再提交");
    }
    Ok(())
}

pub fn validate_text(value: &str, field: &str, min: usize, max: usize) -> Result<(), String> {
    let length = value.chars().count();
    if !(min..=max).contains(&length) {
        return Err(format!("{field} 长度应在 {min} 到 {max} 个字符之间"));
    }
    reject_sensitive(value).map_err(str::to_owned)
}

pub fn validate_login_name(value: &str) -> Result<(), &'static str> {
    let valid = !value.trim().is_empty() && value.chars().count() <= 64;
    valid.then_some(()).ok_or("请输入不超过 64 个字符的登录名")
}

pub fn validate_password(value: &str) -> Result<(), &'static str> {
    (value.chars().count() >= 6 && value.chars().count() <= 256)
        .then_some(())
        .ok_or("密码至少需要 6 个字符")
}

pub fn validate_https_url(value: &str) -> Result<(), &'static str> {
    let parsed = Url::parse(value).map_err(|_| "证据链接格式无效")?;
    (parsed.scheme() == "https" && parsed.host_str().is_some())
        .then_some(())
        .ok_or("证据链接只能使用 HTTPS")
}

pub fn normalize_tags(tags: Vec<String>) -> Result<Vec<String>, String> {
    if tags.len() > 12 {
        return Err("标签最多 12 个".to_owned());
    }
    let mut normalized = Vec::new();
    for tag in tags {
        let tag = tag.trim().to_lowercase();
        if tag.is_empty() {
            continue;
        }
        if tag.chars().count() > 48 {
            return Err("单个标签不能超过 48 个字符".to_owned());
        }
        reject_sensitive(&tag).map_err(str::to_owned)?;
        if !normalized.contains(&tag) {
            normalized.push(tag);
        }
    }
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_common_secrets_and_personal_data() {
        assert!(reject_sensitive("sk-abcdefghijklmnopqrstuvwxyz012345").is_err());
        assert!(reject_sensitive(r#"{"api_key": "secret-value-123456"}"#).is_err());
        assert!(reject_sensitive("Bearer abcdefghijklmnopqrstuvwxyz").is_err());
        assert!(reject_sensitive("af_live_abcdefghijklmnopqrstuvwxyz123456").is_err());
        assert!(reject_sensitive("someone@example.com").is_err());
        assert!(reject_sensitive("普通的失败日志").is_ok());
    }

    #[test]
    fn token_hash_is_stable_but_secret_is_not_plaintext() {
        let token = new_token("af_live");
        assert!(token.starts_with("af_live_"));
        assert_ne!(hash_token(&token), token);
    }

    #[test]
    fn accepts_simple_account_credentials() {
        assert!(validate_login_name("我的账号").is_ok());
        assert!(validate_password("123456").is_ok());
        assert!(validate_password("12345").is_err());
    }
}
