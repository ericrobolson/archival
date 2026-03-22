use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

const HASH_SECTION_HEADER: &str = "# SYSTEM-HASHES";
const IGNORE_SECTION: &str = "<!--AI: Ignore the below section. It is used only for system tracking.-->";

/// Compute SHA-256 hex digest of raw file bytes.
pub fn hash_file(path: &Path) -> Option<String> {
    let bytes = match fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("warning: cannot read {}: {}", path.display(), e);
            return None;
        }
    };
    Some(hash_bytes(&bytes))
}

/// Compute SHA-256 hex digest of a byte slice.
pub fn hash_bytes(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

/// Hash a subdirectory's index file content, excluding everything after SYSTEM-HASHES.
pub fn hash_summary(summary_path: &Path) -> Option<String> {
    let content = match fs::read_to_string(summary_path) {
        Ok(c) => c,
        Err(_) => return None,
    };
    let summary_text = strip_hash_section(&content);
    Some(hash_bytes(summary_text.as_bytes()))
}

/// Remove the IGNORE_SECTION (and everything after it, including SYSTEM-HASHES).
/// Falls back to stripping from SYSTEM-HASHES if IGNORE_SECTION is absent.
pub fn strip_hash_section(content: &str) -> &str {
    if let Some(pos) = content.find(IGNORE_SECTION) {
        content[..pos].trim_end()
    } else if let Some(pos) = content.find(HASH_SECTION_HEADER) {
        content[..pos].trim_end()
    } else {
        content.trim_end()
    }
}

/// Parse stored hashes from an existing index file.
/// Returns a map of "file:name" or "dir:name" -> hash string.
pub fn parse_stored_hashes(summary_path: &Path) -> BTreeMap<String, String> {
    let mut result = BTreeMap::new();
    let content = match fs::read_to_string(summary_path) {
        Ok(c) => c,
        Err(_) => return result,
    };

    let mut in_hash_section = false;
    for line in content.lines() {
        if line.trim() == HASH_SECTION_HEADER {
            in_hash_section = true;
            continue;
        }
        if !in_hash_section {
            continue;
        }
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // Format: "file:name hash" or "dir:name hash"
        if let Some((key, hash)) = line.split_once(' ') {
            result.insert(key.to_string(), hash.to_string());
        }
    }
    result
}

/// Format hash entries as the SYSTEM-HASHES section content.
/// Entries are sorted alphabetically by key.
pub fn format_hash_section(hashes: &BTreeMap<String, String>) -> String {
    let mut out = String::new();
    out.push_str(HASH_SECTION_HEADER);
    out.push_str("\n\n");
    for (key, hash) in hashes {
        out.push_str(key);
        out.push(' ');
        out.push_str(hash);
        out.push('\n');
    }
    out
}

/// Parse existing per-file and per-subdir summary text from a index file.
/// Returns a map of filename -> summary line, and dirname -> summary block.
pub fn parse_existing_summaries(summary_path: &Path) -> (BTreeMap<String, String>, BTreeMap<String, String>) {
    let mut file_summaries = BTreeMap::new();
    let mut dir_summaries = BTreeMap::new();

    let content = match fs::read_to_string(summary_path) {
        Ok(c) => c,
        Err(_) => return (file_summaries, dir_summaries),
    };

    let summary_text = strip_hash_section(&content);
    let mut current_dir: Option<String> = None;
    let mut current_dir_lines: Vec<String> = Vec::new();

    for line in summary_text.lines() {
        // Skip the auto-generated comment and the title heading
        if line.starts_with("<!--") || line.starts_with("# ") {
            // Flush any pending dir summary
            if let Some(dir_name) = current_dir.take() {
                let text = current_dir_lines.join("\n");
                if !text.trim().is_empty() {
                    dir_summaries.insert(dir_name, text.trim().to_string());
                }
                current_dir_lines.clear();
            }
            continue;
        }

        // File summary line: "- **filename** — description"
        if line.starts_with("- **") {
            // Flush any pending dir summary
            if let Some(dir_name) = current_dir.take() {
                let text = current_dir_lines.join("\n");
                if !text.trim().is_empty() {
                    dir_summaries.insert(dir_name, text.trim().to_string());
                }
                current_dir_lines.clear();
            }

            if let Some(end) = line.find("**\u{a0}") // non-breaking space
                .or_else(|| line.find("** —"))
                .or_else(|| line.find("** -"))
            {
                let start = 4; // skip "- **"
                let name = &line[start..end];
                file_summaries.insert(name.to_string(), line.to_string());
            } else if let Some(end) = line[4..].find("**") {
                let name = &line[4..4 + end];
                file_summaries.insert(name.to_string(), line.to_string());
            }
            continue;
        }

        // Subdir heading: "## dirname/"
        if line.starts_with("## ") {
            // Flush any pending dir summary
            if let Some(dir_name) = current_dir.take() {
                let text = current_dir_lines.join("\n");
                if !text.trim().is_empty() {
                    dir_summaries.insert(dir_name, text.trim().to_string());
                }
                current_dir_lines.clear();
            }
            let dir_name = line[3..].trim_end_matches('/').to_string();
            current_dir = Some(dir_name);
            continue;
        }

        // Lines belonging to current dir summary
        if current_dir.is_some() {
            current_dir_lines.push(line.to_string());
        }
    }

    // Flush last dir
    if let Some(dir_name) = current_dir.take() {
        let text = current_dir_lines.join("\n");
        if !text.trim().is_empty() {
            dir_summaries.insert(dir_name, text.trim().to_string());
        }
    }

    (file_summaries, dir_summaries)
}
