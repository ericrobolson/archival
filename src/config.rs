use crate::constants::{ARCHIVAL_DIR, CONFIG_FILENAME};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Compute the config file path: <root>/.archival/archival.toml
fn config_path(root_dir: &Path) -> PathBuf {
    root_dir.join(ARCHIVAL_DIR).join(CONFIG_FILENAME)
}

#[derive(Debug, Serialize, Deserialize, Default)]
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
        toml::from_str(&content)
            .map_err(|e| eprintln!("Warning: Failed to parse {}: {}", path.display(), e))
            .ok()
    }

    /// Find .archival/archival.toml by searching from root_dir upward.
    pub fn find(root_dir: &Path) -> Option<PathBuf> {
        let mut dir = root_dir.to_path_buf();
        loop {
            let candidate = config_path(&dir);
            if candidate.is_file() {
                return Some(candidate);
            }
            if !dir.pop() {
                return None;
            }
        }
    }

    pub fn save(&self, root_dir: &Path) {
        let path = config_path(root_dir);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("failed to create .archival directory");
        }
        let output = toml::to_string_pretty(self).expect("failed to serialize config");
        fs::write(&path, output).expect("failed to write archival.toml");
    }

    fn load_or_default(root_dir: &Path) -> Config {
        let path = config_path(root_dir);
        if path.is_file() {
            Config::load(&path).unwrap_or_default()
        } else {
            Config::default()
        }
    }
}

/// Append a pattern to the allows array in .archival/archival.toml, creating the file if needed.
pub fn add_allow_pattern(root_dir: &Path, pattern: &str) {
    let mut config = Config::load_or_default(root_dir);
    if !config.allows.contains(&pattern.to_string()) {
        config.allows.push(pattern.to_string());
    }
    config.save(root_dir);
}

/// Append a pattern to the ignore array in .archival/archival.toml, creating the file if needed.
pub fn add_ignore_pattern(root_dir: &Path, pattern: &str) {
    let mut config = Config::load_or_default(root_dir);
    if !config.ignore.contains(&pattern.to_string()) {
        config.ignore.push(pattern.to_string());
    }
    config.save(root_dir);
    println!("Added pattern '{}' to {}", pattern, config_path(root_dir).display());
}
