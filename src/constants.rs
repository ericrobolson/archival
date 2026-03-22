pub const SUMMARY_FILENAME: &str = ".archival.INDEX.md";


pub const INSTRUCTION_FILENAME: &str = ".archival.INSTRUCTIONS.md";

pub const CONFIG_FILENAME: &str = ".archival.toml";

/// Files that archival owns and should always be excluded from scanning/hashing.
pub const ARCHIVAL_FILES: &[&str] = &[SUMMARY_FILENAME, INSTRUCTION_FILENAME, CONFIG_FILENAME];

pub const INSTRUCTION_FILE_CONTENTS: &str = r#"# Traversal Instructions
When traversing, look at the {summary-file} index files before reading individual files. 
Recursively traverse the directory tree, looking at the {summary-file} index files in subfolders before reading individual files.
If a file in a {summary-file} looks relevant, then read it. 
Otherwise skip it. Don't load the file tree like a maniac.
"#;