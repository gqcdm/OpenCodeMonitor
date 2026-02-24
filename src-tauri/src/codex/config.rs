use std::path::{Path, PathBuf};

use serde_json::Value as JsonValue;

use crate::files::io::read_text_file_within;
use crate::files::ops::write_with_policy;
use crate::files::policy::{policy_for_with_root, FileKind, FileScope};

pub(crate) fn read_steer_enabled() -> Result<Option<bool>, String> {
    read_json_bool_field("steer")
}

pub(crate) fn read_collab_enabled() -> Result<Option<bool>, String> {
    read_json_bool_field("collab")
}

pub(crate) fn read_collaboration_modes_enabled() -> Result<Option<bool>, String> {
    read_json_bool_field("collaboration_modes")
}

pub(crate) fn read_unified_exec_enabled() -> Result<Option<bool>, String> {
    read_json_bool_field("unified_exec")
}

pub(crate) fn read_apps_enabled() -> Result<Option<bool>, String> {
    read_json_bool_field("apps")
}

pub(crate) fn read_personality() -> Result<Option<String>, String> {
    let Some(root) = resolve_default_codex_home() else {
        return Ok(None);
    };
    let contents = read_config_contents_from_root(&root)?;
    Ok(contents
        .as_deref()
        .and_then(parse_personality_from_json)
        .map(|value| value.to_string()))
}

pub(crate) fn write_steer_enabled(enabled: bool) -> Result<(), String> {
    write_json_bool_field("steer", enabled)
}

pub(crate) fn write_collab_enabled(enabled: bool) -> Result<(), String> {
    write_json_bool_field("collab", enabled)
}

pub(crate) fn write_collaboration_modes_enabled(enabled: bool) -> Result<(), String> {
    write_json_bool_field("collaboration_modes", enabled)
}

pub(crate) fn write_unified_exec_enabled(enabled: bool) -> Result<(), String> {
    write_json_bool_field("unified_exec", enabled)
}

pub(crate) fn write_apps_enabled(enabled: bool) -> Result<(), String> {
    write_json_bool_field("apps", enabled)
}

pub(crate) fn write_personality(personality: &str) -> Result<(), String> {
    let Some(root) = resolve_default_codex_home() else {
        return Ok(());
    };
    let policy = config_policy(&root)?;
    let response = read_text_file_within(
        &root,
        &policy.filename,
        policy.root_may_be_missing,
        policy.root_context,
        &policy.filename,
        policy.allow_external_symlink_target,
    )?;
    let contents = if response.exists {
        response.content
    } else {
        String::from("{}")
    };
    let normalized = normalize_personality_value(personality);
    let updated = match normalized {
        Some(value) => upsert_json_string_field(&contents, "personality", value)?,
        None => remove_json_field(&contents, "personality")?,
    };
    write_with_policy(&root, policy, &updated)
}

fn read_json_bool_field(key: &str) -> Result<Option<bool>, String> {
    let Some(root) = resolve_default_codex_home() else {
        return Ok(None);
    };
    let contents = read_config_contents_from_root(&root)?;
    Ok(contents
        .as_deref()
        .and_then(|c| parse_json_bool_field(c, key)))
}

fn write_json_bool_field(key: &str, enabled: bool) -> Result<(), String> {
    let Some(root) = resolve_default_codex_home() else {
        return Ok(());
    };
    let policy = config_policy(&root)?;
    let response = read_text_file_within(
        &root,
        &policy.filename,
        policy.root_may_be_missing,
        policy.root_context,
        &policy.filename,
        policy.allow_external_symlink_target,
    )?;
    let contents = if response.exists {
        response.content
    } else {
        String::from("{}")
    };
    let updated = upsert_json_bool_field(&contents, key, enabled)?;
    write_with_policy(&root, policy, &updated)
}

pub(crate) fn config_json_path() -> Option<PathBuf> {
    let root = resolve_default_codex_home()?;
    let policy = config_policy(&root).ok()?;
    Some(root.join(&policy.filename))
}

pub(crate) fn read_config_model(codex_home: Option<PathBuf>) -> Result<Option<String>, String> {
    let root = codex_home.or_else(resolve_default_codex_home);
    let Some(root) = root else {
        return Err("Unable to resolve config home (OPENCODE_HOME)".to_string());
    };
    read_config_model_from_root(&root)
}

fn resolve_default_codex_home() -> Option<PathBuf> {
    crate::codex::home::resolve_default_codex_home()
}

fn config_policy(root: &Path) -> Result<crate::files::policy::FilePolicy, String> {
    policy_for_with_root(FileScope::Global, FileKind::Config, Some(root))
}

fn read_config_contents_from_root(root: &Path) -> Result<Option<String>, String> {
    let policy = config_policy(root)?;
    let response = read_text_file_within(
        root,
        &policy.filename,
        policy.root_may_be_missing,
        policy.root_context,
        &policy.filename,
        policy.allow_external_symlink_target,
    )?;
    if response.exists {
        Ok(Some(response.content))
    } else {
        Ok(None)
    }
}

fn read_config_model_from_root(root: &Path) -> Result<Option<String>, String> {
    let contents = read_config_contents_from_root(root)?;
    Ok(contents.as_deref().and_then(parse_model_from_json))
}

fn parse_model_from_json(contents: &str) -> Option<String> {
    let parsed: JsonValue = strip_jsonc_comments_and_parse(contents).ok()?;
    let model = parsed.get("model")?.as_str()?;
    let trimmed = model.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn parse_personality_from_json(contents: &str) -> Option<&'static str> {
    let parsed: JsonValue = strip_jsonc_comments_and_parse(contents).ok()?;
    let value = parsed.get("personality")?.as_str()?;
    normalize_personality_value(value)
}

fn parse_json_bool_field(contents: &str, key: &str) -> Option<bool> {
    let parsed: JsonValue = strip_jsonc_comments_and_parse(contents).ok()?;
    parsed.get(key)?.as_bool()
}

fn normalize_personality_value(value: &str) -> Option<&'static str> {
    match value.trim().to_ascii_lowercase().as_str() {
        "friendly" => Some("friendly"),
        "pragmatic" => Some("pragmatic"),
        _ => None,
    }
}

fn strip_jsonc_comments(contents: &str) -> String {
    let mut result = String::with_capacity(contents.len());
    let mut in_string = false;
    let mut escape_next = false;
    let mut chars = contents.chars().peekable();

    while let Some(c) = chars.next() {
        if escape_next {
            result.push(c);
            escape_next = false;
            continue;
        }

        if c == '\\' && in_string {
            result.push(c);
            escape_next = true;
            continue;
        }

        if c == '"' {
            in_string = !in_string;
            result.push(c);
            continue;
        }

        if !in_string && c == '/' {
            if chars.peek() == Some(&'/') {
                chars.next();
                while let Some(&next) = chars.peek() {
                    if next == '\n' {
                        break;
                    }
                    chars.next();
                }
                continue;
            } else if chars.peek() == Some(&'*') {
                chars.next();
                while let Some(next) = chars.next() {
                    if next == '*' && chars.peek() == Some(&'/') {
                        chars.next();
                        break;
                    }
                }
                continue;
            }
        }

        result.push(c);
    }

    result
}

fn strip_jsonc_comments_and_parse(contents: &str) -> Result<JsonValue, String> {
    let stripped = strip_jsonc_comments(contents);
    serde_json::from_str(&stripped).map_err(|e| format!("Failed to parse config JSON: {e}"))
}

fn upsert_json_bool_field(contents: &str, key: &str, value: bool) -> Result<String, String> {
    let mut parsed: JsonValue = strip_jsonc_comments_and_parse(contents)?;

    if let Some(obj) = parsed.as_object_mut() {
        obj.insert(key.to_string(), JsonValue::Bool(value));
    }

    serde_json::to_string_pretty(&parsed)
        .map_err(|e| format!("Failed to serialize config JSON: {e}"))
}

fn upsert_json_string_field(contents: &str, key: &str, value: &str) -> Result<String, String> {
    let mut parsed: JsonValue = strip_jsonc_comments_and_parse(contents)?;

    if let Some(obj) = parsed.as_object_mut() {
        obj.insert(key.to_string(), JsonValue::String(value.to_string()));
    }

    serde_json::to_string_pretty(&parsed)
        .map_err(|e| format!("Failed to serialize config JSON: {e}"))
}

fn remove_json_field(contents: &str, key: &str) -> Result<String, String> {
    let mut parsed: JsonValue = strip_jsonc_comments_and_parse(contents)?;

    if let Some(obj) = parsed.as_object_mut() {
        obj.remove(key);
    }

    serde_json::to_string_pretty(&parsed)
        .map_err(|e| format!("Failed to serialize config JSON: {e}"))
}

#[cfg(test)]
mod tests {
    use super::{
        parse_json_bool_field, parse_model_from_json, parse_personality_from_json,
        remove_json_field, strip_jsonc_comments, upsert_json_bool_field, upsert_json_string_field,
    };

    #[test]
    fn strip_jsonc_comments_removes_line_comments() {
        let input = r#"{
            // This is a comment
            "key": "value"
        }"#;
        let stripped = strip_jsonc_comments(input);
        assert!(!stripped.contains("// This is a comment"));
        assert!(stripped.contains("\"key\": \"value\""));
    }

    #[test]
    fn strip_jsonc_comments_removes_block_comments() {
        let input = r#"{
            /* block comment */
            "key": "value"
        }"#;
        let stripped = strip_jsonc_comments(input);
        assert!(!stripped.contains("/* block comment */"));
        assert!(stripped.contains("\"key\": \"value\""));
    }

    #[test]
    fn strip_jsonc_comments_preserves_strings_with_slashes() {
        let input = r#"{"url": "https://example.com"}"#;
        let stripped = strip_jsonc_comments(input);
        assert!(stripped.contains("https://example.com"));
    }

    #[test]
    fn parse_personality_reads_supported_values() {
        assert_eq!(
            parse_personality_from_json(r#"{"personality": "friendly"}"#),
            Some("friendly")
        );
        assert_eq!(
            parse_personality_from_json(r#"{"personality": "pragmatic"}"#),
            Some("pragmatic")
        );
        assert_eq!(
            parse_personality_from_json(r#"{"personality": "unknown"}"#),
            None
        );
    }

    #[test]
    fn parse_model_reads_model_field() {
        assert_eq!(
            parse_model_from_json(r#"{"model": "claude-3-opus"}"#),
            Some("claude-3-opus".to_string())
        );
        assert_eq!(parse_model_from_json(r#"{"model": ""}"#), None);
        assert_eq!(parse_model_from_json(r#"{}"#), None);
    }

    #[test]
    fn parse_json_bool_field_works() {
        assert_eq!(
            parse_json_bool_field(r#"{"steer": true}"#, "steer"),
            Some(true)
        );
        assert_eq!(
            parse_json_bool_field(r#"{"steer": false}"#, "steer"),
            Some(false)
        );
        assert_eq!(parse_json_bool_field(r#"{}"#, "steer"), None);
    }

    #[test]
    fn upsert_json_bool_field_adds_new_field() {
        let input = r#"{}"#;
        let updated = upsert_json_bool_field(input, "steer", true).unwrap();
        assert!(updated.contains("\"steer\": true"));
    }

    #[test]
    fn upsert_json_bool_field_updates_existing() {
        let input = r#"{"steer": false}"#;
        let updated = upsert_json_bool_field(input, "steer", true).unwrap();
        assert!(updated.contains("\"steer\": true"));
        assert!(!updated.contains("\"steer\": false"));
    }

    #[test]
    fn upsert_json_string_field_works() {
        let input = r#"{}"#;
        let updated = upsert_json_string_field(input, "personality", "friendly").unwrap();
        assert!(updated.contains("\"personality\": \"friendly\""));
    }

    #[test]
    fn remove_json_field_works() {
        let input = r#"{"personality": "friendly", "model": "test"}"#;
        let updated = remove_json_field(input, "personality").unwrap();
        assert!(!updated.contains("personality"));
        assert!(updated.contains("\"model\": \"test\""));
    }
}
