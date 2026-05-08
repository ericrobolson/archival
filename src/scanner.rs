use crate::constants::ARCHIVAL_FILES;
use ignore::overrides::OverrideBuilder;
use ignore::WalkBuilder;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct FileEntry {
    pub path: PathBuf,
}

#[derive(Debug)]
pub struct DirNode {
    pub path: PathBuf,
    pub files: Vec<FileEntry>,
    pub subdirs: Vec<PathBuf>,
}

/// Walk the directory tree and collect all unique file extensions (sorted).
/// Respects .gitignore and extra ignore patterns.
/// Returns a BTreeMap of extension -> list of sample files (up to 3 per extension).
pub fn collect_extensions(
    root: &Path,
    extra_ignores: &[String],
) -> BTreeMap<String, Vec<PathBuf>> {
    let mut builder = WalkBuilder::new(root);
    builder
        .follow_links(false)
        .standard_filters(true);

    if !extra_ignores.is_empty() {
        let mut ob = OverrideBuilder::new(root);
        for pattern in extra_ignores {
            let _ = ob.add(&format!("!{}", pattern));
        }
        if let Ok(overrides) = ob.build() {
            builder.overrides(overrides);
        }
    }

    let mut extensions: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
    for entry in builder.build().filter_map(|e| e.ok()) {
        if entry.file_type().map_or(false, |ft| ft.is_file()) {
            let file_name = entry.file_name().to_string_lossy();
            if ARCHIVAL_FILES.iter().any(|&f| f == file_name.as_ref()) {
                continue;
            }
            if let Some(ext) = entry.path().extension() {
                let ext_str = ext.to_string_lossy().to_string();
                let files = extensions.entry(ext_str).or_insert_with(Vec::new);
                if files.len() < 3 {
                    files.push(entry.path().to_path_buf());
                }
            }
        }
    }
    extensions
}

/// Scan the directory tree bottom-up, returning DirNodes in post-order.
/// Respects .gitignore automatically via the `ignore` crate.
/// Additional ignore patterns can be supplied.
pub fn scan(
    root: &Path,
    extra_ignores: &[String],
) -> Vec<DirNode> {
    let mut builder = WalkBuilder::new(root);
    builder
        .follow_links(false)
        .standard_filters(true); // respects .gitignore

    // Use overrides to add extra ignore patterns (negated globs = ignores)
    if !extra_ignores.is_empty() {
        let mut ob = OverrideBuilder::new(root);
        for pattern in extra_ignores {
            // Prefix with ! to negate (ignore) the pattern
            let _ = ob.add(&format!("!{}", pattern));
        }
        if let Ok(overrides) = ob.build() {
            builder.overrides(overrides);
        }
    }

    // Collect all non-ignored file entries
    let mut dir_files: BTreeMap<PathBuf, Vec<FileEntry>> = BTreeMap::new();
    let mut dir_subdirs: BTreeMap<PathBuf, BTreeSet<PathBuf>> = BTreeMap::new();
    let mut all_dirs: BTreeSet<PathBuf> = BTreeSet::new();

    for entry in builder.build().filter_map(|e| e.ok()) {
        let path = entry.path().to_path_buf();

        // Skip the root itself from being added as a child
        if path == root {
            all_dirs.insert(path);
            continue;
        }

        // Check extra ignore patterns manually (the ignore crate handles .gitignore,
        // but CLI --ignore patterns need glob matching against relative paths)
        let rel = path.strip_prefix(root).unwrap_or(&path);
        let rel_str = rel.to_string_lossy();
        let skip = extra_ignores.iter().any(|pat| {
            glob_match(pat, &rel_str)
        });
        if skip {
            continue;
        }

        if entry.file_type().map_or(false, |ft| ft.is_dir()) {
            all_dirs.insert(path.clone());
            // Register this dir as a subdir of its parent
            if let Some(parent) = path.parent() {
                dir_subdirs
                    .entry(parent.to_path_buf())
                    .or_default()
                    .insert(path);
            }
        } else if entry.file_type().map_or(false, |ft| ft.is_file()) {
            let file_name = path.file_name().unwrap_or_default().to_string_lossy();
            // Skip archival-owned files and zero-byte files
            if ARCHIVAL_FILES.iter().any(|&f| f == file_name) {
                continue;
            }
            if let Ok(meta) = std::fs::metadata(&path) {
                if meta.len() == 0 {
                    continue;
                }
            }
            if let Some(parent) = path.parent() {
                dir_files
                    .entry(parent.to_path_buf())
                    .or_default()
                    .push(FileEntry { path });
            }
        }
    }

    // Build DirNodes and sort in post-order (deepest first)
    let mut nodes: Vec<DirNode> = Vec::new();
    for dir in &all_dirs {
        let files = dir_files.remove(dir).unwrap_or_default();
        let subdirs: Vec<PathBuf> = dir_subdirs
            .remove(dir)
            .unwrap_or_default()
            .into_iter()
            .collect();

        // Skip empty directories (no files, no subdirs)
        if files.is_empty() && subdirs.is_empty() {
            continue;
        }

        nodes.push(DirNode {
            path: dir.clone(),
            files,
            subdirs,
        });
    }

    // Sort by depth descending (post-order: deepest first)
    nodes.sort_by(|a, b| {
        let depth_a = a.path.components().count();
        let depth_b = b.path.components().count();
        depth_b.cmp(&depth_a)
    });

    nodes
}

/// Simple glob matching: supports * and ? wildcards, and trailing / for directory matching.
fn glob_match(pattern: &str, path: &str) -> bool {
    let pattern = pattern.trim_end_matches('/');
    // Check if any path component matches the pattern
    for component in std::path::Path::new(path).components() {
        let comp_str = component.as_os_str().to_string_lossy();
        if simple_glob(pattern, &comp_str) {
            return true;
        }
    }
    // Also try matching the full path
    simple_glob(pattern, path)
}

fn simple_glob(pattern: &str, text: &str) -> bool {
    fn do_match(
        pattern: &[char],
        text: &[char],
    ) -> bool {
        let mut pi = 0;
        let mut ti = 0;
        let mut star_pi = usize::MAX;
        let mut star_ti = 0;

        while ti < text.len() {
            if pi < pattern.len() && (pattern[pi] == '?' || pattern[pi] == text[ti]) {
                pi += 1;
                ti += 1;
            } else if pi < pattern.len() && pattern[pi] == '*' {
                star_pi = pi;
                star_ti = ti;
                pi += 1;
            } else if star_pi != usize::MAX {
                pi = star_pi + 1;
                star_ti += 1;
                ti = star_ti;
            } else {
                return false;
            }
        }
        while pi < pattern.len() && pattern[pi] == '*' {
            pi += 1;
        }
        pi == pattern.len()
    }

    let pc: Vec<char> = pattern.chars().collect();
    let tc: Vec<char> = text.chars().collect();
    do_match(&pc, &tc)
}
