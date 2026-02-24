use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FileScope {
    Workspace,
    Global,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FileKind {
    Agents,
    Config,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FilePolicy {
    pub(crate) filename: String,
    pub(crate) root_context: &'static str,
    pub(crate) root_may_be_missing: bool,
    pub(crate) create_root: bool,
    pub(crate) allow_external_symlink_target: bool,
}

const AGENTS_FILENAME: &str = "AGENTS.md";
const CONFIG_FILENAME_JSONC: &str = "opencode.jsonc";
const CONFIG_FILENAME_JSON: &str = "opencode.json";

pub(crate) fn policy_for(scope: FileScope, kind: FileKind) -> Result<FilePolicy, String> {
    policy_for_with_root(scope, kind, None)
}

pub(crate) fn policy_for_with_root(
    scope: FileScope,
    kind: FileKind,
    root: Option<&Path>,
) -> Result<FilePolicy, String> {
    match (scope, kind) {
        (FileScope::Workspace, FileKind::Agents) => Ok(FilePolicy {
            filename: AGENTS_FILENAME.to_string(),
            root_context: "workspace root",
            root_may_be_missing: false,
            create_root: false,
            allow_external_symlink_target: false,
        }),
        (FileScope::Global, FileKind::Agents) => Ok(FilePolicy {
            filename: AGENTS_FILENAME.to_string(),
            root_context: "OPENCODE_HOME",
            root_may_be_missing: true,
            create_root: true,
            allow_external_symlink_target: true,
        }),
        (FileScope::Global, FileKind::Config) => Ok(FilePolicy {
            filename: resolve_config_filename(root),
            root_context: "OPENCODE_HOME",
            root_may_be_missing: true,
            create_root: true,
            allow_external_symlink_target: false,
        }),
        (FileScope::Workspace, FileKind::Config) => {
            Err("opencode.json(c) is only supported for global scope".to_string())
        }
    }
}

fn resolve_config_filename(root: Option<&Path>) -> String {
    let Some(root) = root else {
        return CONFIG_FILENAME_JSONC.to_string();
    };

    let jsonc_path = root.join(CONFIG_FILENAME_JSONC);
    if jsonc_path.exists() {
        return CONFIG_FILENAME_JSONC.to_string();
    }

    let json_path = root.join(CONFIG_FILENAME_JSON);
    if json_path.exists() {
        return CONFIG_FILENAME_JSON.to_string();
    }

    CONFIG_FILENAME_JSONC.to_string()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{policy_for, policy_for_with_root, FileKind, FileScope};

    #[test]
    fn workspace_agents_policy_is_strict() {
        let policy = policy_for(FileScope::Workspace, FileKind::Agents).expect("policy");
        assert_eq!(policy.filename, "AGENTS.md");
        assert_eq!(policy.root_context, "workspace root");
        assert!(!policy.root_may_be_missing);
        assert!(!policy.create_root);
        assert!(!policy.allow_external_symlink_target);
    }

    #[test]
    fn global_agents_policy_creates_root() {
        let policy = policy_for(FileScope::Global, FileKind::Agents).expect("policy");
        assert_eq!(policy.filename, "AGENTS.md");
        assert_eq!(policy.root_context, "OPENCODE_HOME");
        assert!(policy.root_may_be_missing);
        assert!(policy.create_root);
        assert!(policy.allow_external_symlink_target);
    }

    #[test]
    fn global_config_policy_creates_root() {
        let policy = policy_for(FileScope::Global, FileKind::Config).expect("policy");
        assert_eq!(policy.filename, "opencode.jsonc");
        assert_eq!(policy.root_context, "OPENCODE_HOME");
        assert!(policy.root_may_be_missing);
        assert!(policy.create_root);
        assert!(!policy.allow_external_symlink_target);
    }

    #[test]
    fn global_config_prefers_jsonc_when_both_exist() {
        let temp_dir = std::env::temp_dir().join("policy-test-both");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).expect("create temp dir");
        fs::write(temp_dir.join("opencode.json"), "{}").expect("write json");
        fs::write(temp_dir.join("opencode.jsonc"), "{}").expect("write jsonc");

        let policy = policy_for_with_root(FileScope::Global, FileKind::Config, Some(&temp_dir))
            .expect("policy");
        assert_eq!(policy.filename, "opencode.jsonc");

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn global_config_falls_back_to_json() {
        let temp_dir = std::env::temp_dir().join("policy-test-json-only");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).expect("create temp dir");
        fs::write(temp_dir.join("opencode.json"), "{}").expect("write json");

        let policy = policy_for_with_root(FileScope::Global, FileKind::Config, Some(&temp_dir))
            .expect("policy");
        assert_eq!(policy.filename, "opencode.json");

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn global_config_defaults_to_jsonc_when_neither_exists() {
        let temp_dir = std::env::temp_dir().join("policy-test-empty");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).expect("create temp dir");

        let policy = policy_for_with_root(FileScope::Global, FileKind::Config, Some(&temp_dir))
            .expect("policy");
        assert_eq!(policy.filename, "opencode.jsonc");

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn workspace_config_is_rejected() {
        let result = policy_for(FileScope::Workspace, FileKind::Config);
        assert!(result.is_err());
    }
}
