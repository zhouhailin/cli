//! 版本清单：<cache_dir>/versions.json，记录缓存压缩包与 sha256，供离线安装使用

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Result;
use serde::{Deserialize, Serialize};

/// 清单中一个缓存条目
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntry {
    pub version: String,
    pub file: String,
    pub sha256: String,
}

/// 版本清单：tool -> 缓存条目列表
pub type CacheManifest = BTreeMap<String, Vec<CacheEntry>>;

const MANIFEST_FILE: &str = "versions.json";

pub fn load(cache_dir: &Path) -> Result<CacheManifest> {
    let path = cache_dir.join(MANIFEST_FILE);
    if !path.exists() {
        return Ok(CacheManifest::new());
    }
    let text = std::fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&text).unwrap_or_default())
}

pub fn save(cache_dir: &Path, manifest: &CacheManifest) -> Result<()> {
    std::fs::create_dir_all(cache_dir)?;
    let text = serde_json::to_string_pretty(manifest)?;
    std::fs::write(cache_dir.join(MANIFEST_FILE), text)?;
    Ok(())
}

pub fn find<'a>(manifest: &'a CacheManifest, tool: &str, version: &str) -> Option<&'a CacheEntry> {
    manifest
        .get(tool)?
        .iter()
        .find(|e| e.version == version)
}

pub fn add(manifest: &mut CacheManifest, tool: &str, version: &str, file: &str, sha256: &str) {
    let list = manifest.entry(tool.to_string()).or_default();
    if let Some(e) = list.iter_mut().find(|e| e.version == version) {
        e.file = file.to_string();
        e.sha256 = sha256.to_string();
    } else {
        list.push(CacheEntry {
            version: version.to_string(),
            file: file.to_string(),
            sha256: sha256.to_string(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn load_returns_empty_when_file_missing() {
        let dir = tempdir().unwrap();
        let m = load(dir.path()).unwrap();
        assert!(m.is_empty());
    }

    #[test]
    fn save_then_load_roundtrip() {
        let dir = tempdir().unwrap();
        let mut m = CacheManifest::new();
        add(&mut m, "node", "v22.11.0", "node-v22.11.0-linux-x64.tar.gz", "abc123");
        save(dir.path(), &m).unwrap();
        let loaded = load(dir.path()).unwrap();
        let e = find(&loaded, "node", "v22.11.0").unwrap();
        assert_eq!(e.file, "node-v22.11.0-linux-x64.tar.gz");
        assert_eq!(e.sha256, "abc123");
    }

    #[test]
    fn add_is_idempotent_and_updates() {
        let mut m = CacheManifest::new();
        add(&mut m, "go", "1.22.0", "go1.22.0.linux-amd64.tar.gz", "old");
        add(&mut m, "go", "1.22.0", "go1.22.0.linux-amd64.tar.gz", "old");
        assert_eq!(m["go"].len(), 1);
        // 再次添加同版本不同哈希 → 更新
        add(&mut m, "go", "1.22.0", "go1.22.0.linux-amd64.tar.gz", "new");
        assert_eq!(m["go"].len(), 1);
        assert_eq!(m["go"][0].sha256, "new");
    }

    #[test]
    fn find_miss_returns_none() {
        let m = CacheManifest::new();
        assert!(find(&m, "node", "v20.0.0").is_none());
    }

    #[test]
    fn load_ignores_corrupt_file() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("versions.json"), "not-json").unwrap();
        let m = load(dir.path()).unwrap();
        assert!(m.is_empty());
    }
}
