//! Configuration management for grabstuff.
//!
//! Handles loading configuration from project-level and global config files,
//! with sensible defaults for ignore patterns.

use anyhow::Result;
use serde::Deserialize;

/// Application configuration loaded from `.grabstuff.yaml` files.
///
/// Configuration is searched in the following order:
/// 1. Project-level: `.grabstuff.yaml` in the current directory
/// 2. Global: `~/.grabstuff/config.yaml`
/// 3. Default values if no config file is found
#[derive(Debug, Deserialize, Clone, Default)]
pub struct Config {
    /// Default settings applied to all operations.
    #[serde(default)]
    pub defaults: Defaults,
}

/// Default output format from configuration.
#[derive(Debug, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum DefaultFormat {
    /// Markdown format.
    #[serde(alias = "markdown")]
    Md,
    /// Plain text format.
    #[serde(alias = "text")]
    Plain,
    /// JSON format.
    Json,
}

fn default_format() -> DefaultFormat {
    DefaultFormat::Md
}

/// Default settings for file matching and output.
#[derive(Debug, Deserialize, Clone)]
pub struct Defaults {
    /// Default output format.
    #[serde(default = "default_format")]
    pub format: DefaultFormat,
    /// Glob patterns for files and directories to ignore.
    /// Defaults to common directories like `.git/`, `node_modules/`, `target/`.
    #[serde(default = "default_ignores")]
    pub ignore: Vec<String>,
}

fn default_ignores() -> Vec<String> {
    vec![
        ".git/".to_string(),
        "node_modules/".to_string(),
        "target/".to_string(),
        ".env".to_string(),
    ]
}

impl Default for Defaults {
    fn default() -> Self {
        Defaults {
            format: default_format(),
            ignore: default_ignores(),
        }
    }
}

impl Config {
    /// Loads configuration from available config files.
    ///
    /// Searches for configuration in order of precedence:
    /// 1. `.grabstuff.yaml` in the current working directory
    /// 2. `~/.grabstuff/config.yaml` in the user's home directory
    /// 3. Returns default configuration if no files are found
    ///
    /// # Errors
    ///
    /// Returns an error if a config file exists but cannot be read or parsed.
    pub fn load() -> Result<Self> {
        // Try project-level config first
        let project_config = std::env::current_dir()?.join(".grabstuff.yaml");
        if project_config.exists() {
            let content = std::fs::read_to_string(&project_config)?;
            let config: Config = serde_yaml::from_str(&content)?;
            return Ok(config);
        }

        // Try global config
        if let Some(home) = dirs::home_dir() {
            let global_config = home.join(".grabstuff").join("config.yaml");
            if global_config.exists() {
                let content = std::fs::read_to_string(&global_config)?;
                let config: Config = serde_yaml::from_str(&content)?;
                return Ok(config);
            }
        }

        // Return default config
        Ok(Config::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.defaults.format, DefaultFormat::Md);
        assert!(!config.defaults.ignore.is_empty());
        assert!(config.defaults.ignore.contains(&".git/".to_string()));
        assert!(config
            .defaults
            .ignore
            .contains(&"node_modules/".to_string()));
        assert!(config.defaults.ignore.contains(&"target/".to_string()));
    }

    #[test]
    fn test_default_ignores() {
        let ignores = default_ignores();
        assert_eq!(ignores.len(), 4);
        assert!(ignores.contains(&".git/".to_string()));
        assert!(ignores.contains(&".env".to_string()));
    }

    #[test]
    fn test_parse_default_format_from_config() {
        let config: Config = serde_yaml::from_str(
            r#"
defaults:
  format: json
"#,
        )
        .unwrap();

        assert_eq!(config.defaults.format, DefaultFormat::Json);
    }

    #[test]
    fn test_load_returns_default_when_no_config() {
        // This test works because we don't have a config file in the test directory
        let config = Config::load();
        assert!(config.is_ok());
    }
}
