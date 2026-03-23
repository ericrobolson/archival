use crate::constants::{ARCHIVAL_FILES, summary_path_for};
use crate::hasher;
use crate::scanner::DirNode;
use std::collections::BTreeMap;
use std::path::Path;

/// Result of comparing current state against stored hashes for a directory.
#[derive(Debug)]
pub struct DiffResult {
    /// Current hashes for all files and subdirs.
    pub current_hashes: BTreeMap<String, String>,
    /// Files whose hash changed or are new.
    pub changed_files: Vec<String>,
    /// Subdirs whose hash changed or are new.
    pub changed_dirs: Vec<String>,
    /// Whether the directory needs regeneration.
    pub is_dirty: bool,
}

/// Compare current file/subdir hashes against stored hashes in the existing index file.
pub fn diff(node: &DirNode, root: &Path) -> DiffResult {
    let summary_path = summary_path_for(&node.path, root);
    let stored = hasher::parse_stored_hashes(&summary_path);

    let mut current_hashes = BTreeMap::new();
    let mut changed_files = Vec::new();
    let mut changed_dirs = Vec::new();
    let mut is_dirty = false;

    // Hash each file
    for file in &node.files {
        let file_name = file.path.file_name().unwrap_or_default().to_string_lossy();
        let key = format!("file:{}", file_name);

        if let Some(hash) = hasher::hash_file(&file.path) {
            current_hashes.insert(key.clone(), hash.clone());
            match stored.get(&key) {
                Some(old_hash) if old_hash == &hash => {} // unchanged
                _ => {
                    changed_files.push(file_name.to_string());
                    is_dirty = true;
                }
            }
        }
    }

    // Hash each subdirectory's index file (excluding SYSTEM-HASHES section)
    for subdir in &node.subdirs {
        let dir_name = subdir.file_name().unwrap_or_default().to_string_lossy();
        let key = format!("dir:{}", dir_name);
        let sub_summary = summary_path_for(subdir, root);

        match hasher::hash_summary(&sub_summary) {
            Some(hash) => {
                current_hashes.insert(key.clone(), hash.clone());
                match stored.get(&key) {
                    Some(old_hash) if old_hash == &hash => {} // unchanged
                    _ => {
                        changed_dirs.push(dir_name.to_string());
                        is_dirty = true;
                    }
                }
            }
            None => {
                // Subdirectory exists but has no index file yet — mark as changed
                changed_dirs.push(dir_name.to_string());
                is_dirty = true;
            }
        }
    }

    // Check for removed entries
    for key in stored.keys() {
        if !current_hashes.contains_key(key) {
            is_dirty = true;
        }
    }

    // Missing index file means dirty
    if !summary_path.is_file() {
        is_dirty = true;
    }

    DiffResult {
        current_hashes,
        changed_files,
        changed_dirs,
        is_dirty,
    }
}

/// Check if a source directory is empty (no files or subdirs) but still has
/// an index file in the .archival/ mirror. Since index files are no longer
/// stored next to source files, we just check if the source dir is empty.
pub fn is_orphan_summary(dir: &Path, root: &Path) -> bool {
    let summary_path = summary_path_for(dir, root);
    if !summary_path.is_file() {
        return false;
    }
    // If the source directory doesn't exist at all, it's an orphan
    if !dir.is_dir() {
        return true;
    }
    // Check if the source directory has any non-archival entries
    let entries: Vec<_> = match std::fs::read_dir(dir) {
        Ok(rd) => rd.filter_map(|e| e.ok()).collect(),
        Err(_) => return true,
    };
    // With index files in .archival/, only ARCHIVAL_FILES that might
    // still be in the source dir are the config and instruction files.
    !entries.iter().any(|e| {
        let name = e.file_name();
        let name = name.to_string_lossy();
        !ARCHIVAL_FILES.iter().any(|&f| f == name.as_ref())
    })
}
