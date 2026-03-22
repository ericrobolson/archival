pub const SUMMARY_FILENAME: &str = ".archival.INDEX.md";


pub const INSTRUCTION_FILENAME: &str = ".archival.INSTRUCTIONS.md";

pub const CONFIG_FILENAME: &str = ".archival.toml";

/// Files that archival owns and should always be excluded from scanning/hashing.
pub const ARCHIVAL_FILES: &[&str] = &[SUMMARY_FILENAME, INSTRUCTION_FILENAME, CONFIG_FILENAME];

/// Patterns that are always excluded from scanning, regardless of config.
pub const DEFAULT_IGNORES: &[&str] = &[
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
When traversing, look at the {summary-file} index files before reading individual files. 
Recursively traverse the directory tree, looking at the {summary-file} index files in subfolders before reading individual files.
If a file in a {summary-file} looks relevant, then read it. 
Otherwise skip it. Don't load the file tree like a maniac.
"#;