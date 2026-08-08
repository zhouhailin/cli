use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};

/// 解析用户主目录：HOME → USERPROFILE → dirs 兜底。
/// 优先读环境变量使测试可在三端通过设置环境变量控制。
pub(crate) fn home_dir() -> Result<PathBuf> {
    if let Ok(h) = std::env::var("HOME") {
        if !h.is_empty() {
            return Ok(PathBuf::from(h));
        }
    }
    #[cfg(windows)]
    if let Ok(p) = std::env::var("USERPROFILE") {
        if !p.is_empty() {
            return Ok(PathBuf::from(p));
        }
    }
    dirs::home_dir().ok_or_else(|| anyhow!("无法获取用户主目录"))
}

pub struct DevkitPaths {
    root: PathBuf,
}

impl DevkitPaths {
    pub fn with_root(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn new() -> Result<Self> {
        if let Ok(env_root) = std::env::var("DEVKIT_ROOT") {
            if !env_root.is_empty() {
                return Ok(Self::with_root(PathBuf::from(env_root)));
            }
        }
        Ok(Self::with_root(home_dir()?.join(".devkit")))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn config_file(&self) -> PathBuf {
        self.root.join("config.json")
    }

    pub fn tool_dir(&self, tool: &str, version: &str) -> PathBuf {
        self.root.join(tool).join(version)
    }

    pub fn etc_dir(&self) -> PathBuf {
        self.root.join("etc")
    }

    pub fn services_dir(&self) -> PathBuf {
        self.root.join("services")
    }

    pub fn current_link(&self, tool: &str) -> PathBuf {
        self.root.join("current").join(tool)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_root_sets_root() {
        let paths = DevkitPaths::with_root(PathBuf::from("/tmp/x"));
        assert_eq!(paths.root(), Path::new("/tmp/x"));
    }

    #[test]
    fn layout_paths_are_derived_from_root() {
        let paths = DevkitPaths::with_root(PathBuf::from("/tmp/x"));
        assert_eq!(paths.config_file(), PathBuf::from("/tmp/x/config.json"));
        assert_eq!(paths.tool_dir("java", "21"), PathBuf::from("/tmp/x/java/21"));
        assert_eq!(paths.etc_dir(), PathBuf::from("/tmp/x/etc"));
        assert_eq!(paths.services_dir(), PathBuf::from("/tmp/x/services"));
        assert_eq!(paths.current_link("node"), PathBuf::from("/tmp/x/current/node"));
    }

    #[test]
    fn new_reads_devkit_root_env() {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("DEVKIT_ROOT", dir.path());
        let paths = DevkitPaths::new().unwrap();
        assert_eq!(paths.root(), dir.path());
    }

    #[test]
    fn new_falls_back_to_home_devkit() {
        let home = tempfile::tempdir().unwrap();
        std::env::remove_var("DEVKIT_ROOT");
        std::env::set_var("HOME", home.path());
        let paths = DevkitPaths::new().unwrap();
        assert_eq!(paths.root(), home.path().join(".devkit"));
    }
}
