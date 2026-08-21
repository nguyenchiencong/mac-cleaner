use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("Configuration file already exists at {0}")]
    AlreadyExists(PathBuf),

    #[error("Configuration file not found at {0}")]
    NotFound(PathBuf),

    #[error("Failed to parse configuration TOML: {0}")]
    ParseError(#[from] toml::de::Error),

    #[error("Failed to serialize configuration TOML: {0}")]
    SerializeError(#[from] toml::ser::Error),

    #[error("I/O error with configuration file: {0}")]
    Io(#[from] std::io::Error),
}

/// A configured workspace root containing source code repositories.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceRoot {
    pub path: PathBuf,
    #[serde(default = "default_true")]
    pub recursive: bool,
    #[serde(default)]
    pub max_depth: Option<usize>,
}

fn default_true() -> bool {
    true
}

impl WorkspaceRoot {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            recursive: true,
            max_depth: Some(5),
        }
    }
}

/// A validated set of workspace roots, age thresholds, exclusions, and scanner toggles.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigProfile {
    #[serde(default)]
    pub workspace_roots: Vec<WorkspaceRoot>,
    #[serde(default)]
    pub age_threshold_days: Option<u32>,
    #[serde(default)]
    pub exclusions: Vec<String>,
    #[serde(default)]
    pub enabled_scanners: Vec<String>,
    #[serde(default)]
    pub disabled_scanners: Vec<String>,
}

impl Default for ConfigProfile {
    fn default() -> Self {
        let mut default_roots = Vec::new();
        if let Some(home) = std::env::var_os("HOME") {
            let home_path = PathBuf::from(home);
            let dev_path = home_path.join("Dev");
            let src_path = home_path.join("src");
            let projects_path = home_path.join("Projects");

            if dev_path.exists() {
                default_roots.push(WorkspaceRoot::new(dev_path));
            } else if src_path.exists() {
                default_roots.push(WorkspaceRoot::new(src_path));
            } else if projects_path.exists() {
                default_roots.push(WorkspaceRoot::new(projects_path));
            } else {
                default_roots.push(WorkspaceRoot::new(home_path));
            }
        }

        Self {
            workspace_roots: default_roots,
            age_threshold_days: Some(30),
            exclusions: vec![
                "**/.git/**".to_string(),
                "**/node_modules/**/.bin/**".to_string(),
            ],
            enabled_scanners: Vec::new(),
            disabled_scanners: Vec::new(),
        }
    }
}

impl ConfigProfile {
    /// Loads configuration from standard path or explicit path.
    pub fn load_from_file(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let path = path.as_ref();
        if !path.exists() {
            return Err(ConfigError::NotFound(path.to_path_buf()));
        }
        let content = std::fs::read_to_string(path)?;
        let profile: Self = toml::from_str(&content)?;
        Ok(profile)
    }

    /// Saves configuration profile to the specified path.
    pub fn save_to_file(&self, path: impl AsRef<Path>) -> Result<(), ConfigError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let toml_str = toml::to_string_pretty(self)?;
        std::fs::write(path, toml_str)?;
        Ok(())
    }

    /// Explicitly initializes a default configuration file if it does not already exist.
    pub fn init_file(path: impl AsRef<Path>, force: bool) -> Result<Self, ConfigError> {
        let path = path.as_ref();
        if path.exists() && !force {
            return Err(ConfigError::AlreadyExists(path.to_path_buf()));
        }
        let default_config = Self::default();
        default_config.save_to_file(path)?;
        Ok(default_config)
    }

    /// Returns standard configuration path in `~/.config/mac-cleaner/config.toml`.
    pub fn default_config_path() -> PathBuf {
        if let Some(home) = std::env::var_os("HOME") {
            PathBuf::from(home)
                .join(".config")
                .join("mac-cleaner")
                .join("config.toml")
        } else {
            PathBuf::from(".mac-cleaner.toml")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_config_lifecycle() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_path_buf();

        let profile = ConfigProfile::init_file(&path, true).unwrap();
        assert!(!profile.workspace_roots.is_empty() || !profile.exclusions.is_empty());

        let loaded = ConfigProfile::load_from_file(&path).unwrap();
        assert_eq!(profile, loaded);

        // Fail to init over existing file without force
        let err = ConfigProfile::init_file(&path, false);
        assert!(matches!(err, Err(ConfigError::AlreadyExists(_))));
    }
}
