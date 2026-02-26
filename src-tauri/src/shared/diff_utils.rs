//! Shared diff generation utilities.
//!
//! Used by both live SSE event translation and session replay to generate
//! unified diffs for file edit operations.

use serde_json::{json, Value};
use similar::TextDiff;

#[derive(Debug, Clone)]
struct ApplyPatchChange {
    path: String,
    kind: &'static str,
    diff: Option<String>,
}

#[derive(Debug, Clone)]
struct ApplyPatchSection {
    old_path: String,
    new_path: String,
    kind: &'static str,
    raw_lines: Vec<String>,
}

/// Generate a unified diff from oldString/newString edit input.
/// Returns a unified diff string with @@ hunk headers that the frontend can render.
pub(crate) fn generate_edit_diff(raw_input: &Value, file_path: &str) -> Option<String> {
    let old_string = raw_input.get("oldString").and_then(|v| v.as_str())?;
    let new_string = raw_input.get("newString").and_then(|v| v.as_str())?;

    // Don't generate diff for empty old/new (pure create or delete)
    if old_string.is_empty() && new_string.is_empty() {
        return None;
    }

    let diff = TextDiff::from_lines(old_string, new_string);
    let mut output = String::new();

    // Add file header
    output.push_str(&format!("--- a/{file_path}\n"));
    output.push_str(&format!("+++ b/{file_path}\n"));

    // Generate unified diff hunks
    for hunk in diff.unified_diff().context_radius(3).iter_hunks() {
        output.push_str(&hunk.to_string());
    }

    if output.contains("@@") {
        Some(output)
    } else {
        None
    }
}

pub(crate) fn generate_apply_patch_changes(raw_input: &Value) -> Option<Vec<Value>> {
    let patch_text = raw_input.get("patchText").and_then(|v| v.as_str())?;
    let parsed = parse_apply_patch_sections(patch_text);
    if parsed.is_empty() {
        return Some(Vec::new());
    }

    Some(
        parsed
            .into_iter()
            .map(|change| {
                let mut value = json!({ "path": change.path, "kind": change.kind });
                if let Some(diff) = change.diff {
                    value["diff"] = json!(diff);
                }
                value
            })
            .collect(),
    )
}

fn parse_apply_patch_sections(patch_text: &str) -> Vec<ApplyPatchChange> {
    let mut changes = Vec::new();
    let mut current: Option<ApplyPatchSection> = None;

    for line in patch_text.lines() {
        if line == "*** Begin Patch" || line == "*** End Patch" {
            finish_apply_patch_section(&mut current, &mut changes);
            continue;
        }

        if let Some(path) = line.strip_prefix("*** Add File: ") {
            finish_apply_patch_section(&mut current, &mut changes);
            let path = path.trim();
            if !path.is_empty() {
                current = Some(ApplyPatchSection {
                    old_path: path.to_string(),
                    new_path: path.to_string(),
                    kind: "add",
                    raw_lines: Vec::new(),
                });
            }
            continue;
        }

        if let Some(path) = line.strip_prefix("*** Update File: ") {
            finish_apply_patch_section(&mut current, &mut changes);
            let path = path.trim();
            if !path.is_empty() {
                current = Some(ApplyPatchSection {
                    old_path: path.to_string(),
                    new_path: path.to_string(),
                    kind: "modify",
                    raw_lines: Vec::new(),
                });
            }
            continue;
        }

        if let Some(path) = line.strip_prefix("*** Delete File: ") {
            finish_apply_patch_section(&mut current, &mut changes);
            let path = path.trim();
            if !path.is_empty() {
                current = Some(ApplyPatchSection {
                    old_path: path.to_string(),
                    new_path: path.to_string(),
                    kind: "delete",
                    raw_lines: Vec::new(),
                });
            }
            continue;
        }

        if let Some(new_path) = line.strip_prefix("*** Move to: ") {
            if let Some(section) = current.as_mut() {
                let new_path = new_path.trim();
                if !new_path.is_empty() {
                    section.new_path = new_path.to_string();
                }
            }
            continue;
        }

        if let Some(section) = current.as_mut() {
            match section.kind {
                "add" => {
                    if line.starts_with('+') {
                        section.raw_lines.push(line.to_string());
                    }
                }
                "modify" => {
                    if line.starts_with("@@")
                        || line.starts_with('+')
                        || line.starts_with('-')
                        || line.starts_with(' ')
                        || line.starts_with('\\')
                    {
                        section.raw_lines.push(line.to_string());
                    }
                }
                "delete" => {
                    if line.starts_with("@@")
                        || line.starts_with('+')
                        || line.starts_with('-')
                        || line.starts_with(' ')
                        || line.starts_with('\\')
                    {
                        section.raw_lines.push(line.to_string());
                    }
                }
                _ => {}
            }
        }
    }

    finish_apply_patch_section(&mut current, &mut changes);
    changes
}

fn finish_apply_patch_section(
    current: &mut Option<ApplyPatchSection>,
    changes: &mut Vec<ApplyPatchChange>,
) {
    let Some(section) = current.take() else {
        return;
    };

    let path = section.new_path.clone();
    let diff = match section.kind {
        "add" => synthesize_add_file_unified_diff(&section.new_path, &section.raw_lines),
        "modify" => synthesize_patch_section_unified_diff(
            &section.old_path,
            &section.new_path,
            &section.raw_lines,
        ),
        "delete" => synthesize_delete_file_unified_diff(&section.old_path, &section.raw_lines),
        _ => None,
    };

    changes.push(ApplyPatchChange {
        path,
        kind: section.kind,
        diff,
    });
}

fn synthesize_add_file_unified_diff(path: &str, lines: &[String]) -> Option<String> {
    if lines.is_empty() {
        return None;
    }

    let mut output = String::new();
    output.push_str("--- /dev/null\n");
    output.push_str(&format!("+++ b/{path}\n"));
    output.push_str(&format!("@@ -0,0 +1,{} @@\n", lines.len()));
    for line in lines {
        output.push_str(line);
        output.push('\n');
    }
    Some(output)
}

fn synthesize_patch_section_unified_diff(
    old_path: &str,
    new_path: &str,
    lines: &[String],
) -> Option<String> {
    if !lines.iter().any(|line| line.starts_with("@@")) {
        return None;
    }

    let mut output = String::new();
    output.push_str(&format!("--- a/{old_path}\n"));
    output.push_str(&format!("+++ b/{new_path}\n"));
    for line in lines {
        output.push_str(line);
        output.push('\n');
    }
    Some(output)
}

fn synthesize_delete_file_unified_diff(path: &str, lines: &[String]) -> Option<String> {
    if !lines.iter().any(|line| line.starts_with("@@")) {
        return None;
    }

    let mut output = String::new();
    output.push_str(&format!("--- a/{path}\n"));
    output.push_str("+++ /dev/null\n");
    for line in lines {
        output.push_str(line);
        output.push('\n');
    }
    Some(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_apply_patch_changes_parses_multi_file_sections() {
        let raw_input = json!({
            "patchText": "*** Begin Patch\n*** Update File: src/main.rs\n@@ -1,1 +1,1 @@\n-fn main() {}\n+fn main(){ }\n*** Add File: notes.txt\n+hello\n+world\n*** Delete File: old.txt\n*** Update File: src/old_name.rs\n*** Move to: src/new_name.rs\n@@ -1,1 +1,1 @@\n-old\n+new\n*** End Patch"
        });

        let changes = generate_apply_patch_changes(&raw_input).expect("should parse patchText");
        assert_eq!(changes.len(), 4);

        assert_eq!(changes[0]["path"], "src/main.rs");
        assert_eq!(changes[0]["kind"], "modify");
        let update_diff = changes[0]["diff"].as_str().expect("modify diff");
        assert!(update_diff.contains("--- a/src/main.rs"));
        assert!(update_diff.contains("+++ b/src/main.rs"));

        assert_eq!(changes[1]["path"], "notes.txt");
        assert_eq!(changes[1]["kind"], "add");
        let add_diff = changes[1]["diff"].as_str().expect("add diff");
        assert!(add_diff.contains("--- /dev/null"));
        assert!(add_diff.contains("+++ b/notes.txt"));
        assert!(add_diff.contains("@@ -0,0 +1,2 @@"));

        assert_eq!(changes[2]["path"], "old.txt");
        assert_eq!(changes[2]["kind"], "delete");
        assert!(changes[2].get("diff").is_none());

        assert_eq!(changes[3]["path"], "src/new_name.rs");
        let rename_diff = changes[3]["diff"].as_str().expect("rename diff");
        assert!(rename_diff.contains("--- a/src/old_name.rs"));
        assert!(rename_diff.contains("+++ b/src/new_name.rs"));
    }
}
