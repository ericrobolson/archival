use crate::constants::CONFIG_FILENAME;
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub ignore: Vec<String>,
    #[serde(default)]
    pub allows: Vec<String>,
    pub llm_cmd: Option<String>,
}

impl Config {
    pub fn load(path: &Path) -> Option<Config> {
        let content = fs::read_to_string(path).ok()?;
        toml::from_str(&content).ok()
    }

    /// Find .archival.toml by searching from root_dir upward.
    pub fn find(root_dir: &Path) -> Option<PathBuf> {
        let mut dir = root_dir.to_path_buf();
        loop {
            let candidate = dir.join(CONFIG_FILENAME);
            if candidate.is_file() {
                return Some(candidate);
            }
            if !dir.pop() {
                return None;
            }
        }
    }
}

/// Append a pattern to the allows array in .archival.toml, creating the file if needed.
pub fn add_allow_pattern(root_dir: &Path, pattern: &str) {
    let config_path = root_dir.join(CONFIG_FILENAME);
    let mut config: toml::Table = if config_path.is_file() {
        let content = fs::read_to_string(&config_path).unwrap_or_default();
        content.parse().unwrap_or_default()
    } else {
        toml::Table::new()
    };

    let allows = config
        .entry("allows")
        .or_insert_with(|| toml::Value::Array(Vec::new()));

    if let toml::Value::Array(arr) = allows {
        let pat_val = toml::Value::String(pattern.to_string());
        if !arr.contains(&pat_val) {
            arr.push(pat_val);
        }
    }

    let output = toml::to_string_pretty(&config).expect("failed to serialize config");
    fs::write(&config_path, output).expect("failed to write .archival.toml");
}

/// Append a pattern to the ignore array in .archival.toml, creating the file if needed.
pub fn add_ignore_pattern(root_dir: &Path, pattern: &str) {
    let config_path = root_dir.join(CONFIG_FILENAME);
    let mut config: toml::Table = if config_path.is_file() {
        let content = fs::read_to_string(&config_path).unwrap_or_default();
        content.parse().unwrap_or_default()
    } else {
        toml::Table::new()
    };

    let ignores = config
        .entry("ignore")
        .or_insert_with(|| toml::Value::Array(Vec::new()));

    if let toml::Value::Array(arr) = ignores {
        let pat_val = toml::Value::String(pattern.to_string());
        if !arr.contains(&pat_val) {
            arr.push(pat_val);
        }
    }

    let output = toml::to_string_pretty(&config).expect("failed to serialize config");
    fs::write(&config_path, output).expect("failed to write .archival.toml");
    println!("Added pattern '{}' to {}", pattern, config_path.display());
}
