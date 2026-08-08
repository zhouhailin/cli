use std::collections::BTreeMap;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::core::paths::DevkitPaths;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    pub installed: BTreeMap<String, Vec<String>>,
    pub active: BTreeMap<String, String>,
    pub goproxy: Option<String>,
    pub mirror: Option<String>,
}

impl Config {
    pub fn load(paths: &DevkitPaths) -> Result<Config> {
        let path = paths.config_file();
        if !path.exists() {
            return Ok(Config::default());
        }
        let text = std::fs::read_to_string(&path)?;
        Ok(serde_json::from_str(&text)?)
    }

    pub fn save(&self, paths: &DevkitPaths) -> Result<()> {
        let path = paths.config_file();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(self)?;
        std::fs::write(path, text)?;
        Ok(())
    }

    pub fn add_installed(&mut self, tool: &str, version: &str) {
        let list = self.installed.entry(tool.to_string()).or_default();
        if !list.iter().any(|v| v == version) {
            list.push(version.to_string());
        }
    }

    pub fn remove_installed(&mut self, tool: &str, version: &str) {
        if let Some(list) = self.installed.get_mut(tool) {
            list.retain(|v| v != version);
        }
    }

    pub fn set_active(&mut self, tool: &str, version: &str) {
        self.active.insert(tool.to_string(), version.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_paths() -> DevkitPaths {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.into_path();
        std::fs::create_dir_all(root.join("java/21")).unwrap();
        DevkitPaths::with_root(root)
    }

    #[test]
    fn load_returns_default_when_file_missing() {
        let paths = test_paths();
        let config = Config::load(&paths).unwrap();
        assert!(config.installed.is_empty());
        assert!(config.active.is_empty());
        assert!(config.goproxy.is_none());
    }

    #[test]
    fn save_then_load_roundtrip() {
        let paths = test_paths();
        let mut config = Config::default();
        config.add_installed("java", "21");
        config.set_active("java", "21");
        config.save(&paths).unwrap();
        let loaded = Config::load(&paths).unwrap();
        assert_eq!(loaded.installed.get("java").unwrap(), &vec!["21".to_string()]);
        assert_eq!(loaded.active.get("java").unwrap(), "21");
    }

    #[test]
    fn add_installed_keeps_unique_versions() {
        let mut config = Config::default();
        config.add_installed("node", "22.11.0");
        config.add_installed("node", "22.11.0");
        config.add_installed("node", "23.0.0");
        assert_eq!(config.installed["node"], vec!["22.11.0".to_string(), "23.0.0".to_string()]);
    }

    #[test]
    fn remove_installed_deletes_version() {
        let mut config = Config::default();
        config.add_installed("node", "22.11.0");
        config.add_installed("node", "23.0.0");
        config.remove_installed("node", "22.11.0");
        assert_eq!(config.installed["node"], vec!["23.0.0".to_string()]);
    }

    #[test]
    fn load_parses_existing_file() {
        let paths = test_paths();
        std::fs::write(
            paths.config_file(),
            r#"{"installed":{"java":["21"]},"active":{"java":"21"},"goproxy":"https://goproxy.cn,direct","mirror":null}"#,
        )
        .unwrap();
        let config = Config::load(&paths).unwrap();
        assert_eq!(config.goproxy.as_deref(), Some("https://goproxy.cn,direct"));
    }
}
