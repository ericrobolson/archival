use std::path::{Path, PathBuf};

pub const SUMMARY_FILENAME: &str = "INDEX.md";

pub const ARCHIVAL_DIR: &str = ".archival";

pub const INSTRUCTION_FILENAME: &str = "INSTRUCTIONS.md";

pub const CONFIG_FILENAME: &str = "archival.toml";

/// Filenames that archival owns and should be excluded from scanning/hashing.
/// The .archival/ directory is excluded via DEFAULT_IGNORES; this list is a
/// safety net for archival-specific filenames that could appear in source trees.
pub const ARCHIVAL_FILES: &[&str] = &[SUMMARY_FILENAME];

/// Compute the path to the summary file for a given source directory.
/// Instead of storing the index next to the source files, we mirror the
/// directory structure under `<root>/.archival/`.
pub fn summary_path_for(dir: &Path, root: &Path) -> PathBuf {
    let rel = dir.strip_prefix(root).unwrap_or(dir);
    root.join(ARCHIVAL_DIR).join(rel).join(SUMMARY_FILENAME)
}

/// Patterns that are always excluded from scanning, regardless of config.
pub const DEFAULT_IGNORES: &[&str] = &[
    // Archival output
    ".archival/",
    // Version control
    ".git/",
    ".hg/",
    ".svn/",
    // Binary / compiled
    "*.bin",
    "*.exe",
    "*.dll",
    "*.so",
    "*.dylib",
    "*.o",
    "*.obj",
    "*.a",
    "*.lib",
    "*.class",
    "*.pyc",
    "*.pyo",
    // Archives
    "*.zip",
    "*.tar",
    "*.gz",
    "*.bz2",
    "*.xz",
    "*.7z",
    "*.rar",
    // Images
    "*.png",
    "*.jpg",
    "*.jpeg",
    "*.gif",
    "*.bmp",
    "*.ico",
    "*.svg",
    "*.webp",
    // Audio / video
    "*.mp3",
    "*.mp4",
    "*.wav",
    "*.avi",
    "*.mov",
    // Fonts
    "*.woff",
    "*.woff2",
    "*.ttf",
    "*.eot",
    "*.otf",
    // Data / database
    "*.sqlite",
    "*.db",
    // Package / dependency dirs
    "node_modules/",
    ".venv/",
    "venv/",
    "__pycache__/",
    "target/",
    // IDE / editor
    ".idea/",
    ".vscode/",
    "*.swp",
    "*.swo",
    // OS junk
    ".DS_Store",
    "Thumbs.db",
    // Lock files
    "*.lock",
    "package-lock.json",
];

pub const INSTRUCTION_FILE_CONTENTS: &str = r#"# Traversal Instructions
Index files are stored in the .archival/ directory, which mirrors the source directory structure.
When traversing, look at {summary-file} files in .archival/ before reading individual source files.
For example, the index for src/ is at .archival/src/{summary-file}.
If a file listed in an index looks relevant, then read it.
Otherwise skip it. Don't load the file tree like a maniac.
"#;