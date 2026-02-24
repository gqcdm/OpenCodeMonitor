//! Shared diff generation utilities.
//!
//! Used by both live SSE event translation and session replay to generate
//! unified diffs for file edit operations.

use serde_json::Value;
use similar::TextDiff;

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
