//! cli download：下载工具压缩包到缓存目录并更新版本清单（不安装），用于离线部署预热

use anyhow::{anyhow, Result};

use crate::core::cache::{self, CacheManifest};
use crate::core::download::{download, sha256_of};
use crate::core::interact::{is_interactive, select};
use crate::core::offline;
use crate::core::paths::DevkitPaths;
use crate::core::platform::Platform;

/// 支持的下载工具（压缩包类，与离线安装范围一致）
const DOWNLOAD_TOOLS: [(&str, &str); 5] = [
    ("Java", "java"),
    ("Node.js", "node"),
    ("Go", "go"),
    ("Maven", "maven"),
    ("Maven Daemon (mvnd)", "mvnd"),
];

pub fn run(tool: Option<String>, version: Option<String>) -> Result<()> {
    if offline::is_offline() {
        return Err(anyhow!("离线模式无法下载，仅支持本地缓存安装"));
    }
    let tool = match tool {
        Some(t) => t,
        None => {
            if !is_interactive() {
                return Err(anyhow!("请指定工具名，例如: cli download node v22.11.0"));
            }
            let labels: Vec<&str> = DOWNLOAD_TOOLS.iter().map(|(l, _)| *l).collect();
            let idx = select("请选择要预热的工具", &labels)?;
            DOWNLOAD_TOOLS[idx].1.to_string()
        }
    };
    if !DOWNLOAD_TOOLS.iter().any(|(_, id)| *id == tool) {
        return Err(anyhow!(
            "暂不支持下载 {tool}，仅支持: {}",
            DOWNLOAD_TOOLS
                .iter()
                .map(|(_, id)| *id)
                .collect::<Vec<_>>()
                .join("/")
        ));
    }
    let platform = Platform::detect();
    let paths = DevkitPaths::new()?;
    match tool.as_str() {
        "node" => {
            let list = crate::core::tools::node::fetch_lts_list()?;
            let versions: Vec<String> = list.iter().map(|n| n.version.clone()).collect();
            let v = pick_version(&versions, version.as_deref())?;
            let url = crate::core::tools::node::resolve_url(&v, &platform);
            fetch_and_cache(&paths, &tool, &v, &url, None)?;
        }
        "go" => {
            let list = crate::core::tools::go::fetch_versions(&platform)?;
            let v = pick_version(&list, version.as_deref())?;
            let url = crate::core::tools::go::resolve_url(&v, &platform);
            fetch_and_cache(&paths, &tool, &v, &url, None)?;
        }
        "maven" => {
            let list = crate::core::tools::maven::fetch_versions()?;
            let v = pick_version(&list, version.as_deref())?;
            let url = crate::core::tools::maven::resolve_url(&v);
            fetch_and_cache(&paths, &tool, &v, &url, None)?;
        }
        "mvnd" => {
            let list = crate::core::tools::mvnd::fetch_versions()?;
            let v = pick_version(&list, version.as_deref())?;
            let url = crate::core::tools::mvnd::resolve_url(&v, &platform);
            let sha = crate::core::tools::mvnd::fetch_sha256(&v, &platform)?;
            fetch_and_cache(&paths, &tool, &v, &url, Some(&sha))?;
        }
        "java" => {
            let vendors = crate::core::tools::java::vendors();
            let labels: Vec<String> =
                vendors.iter().map(|v| format!("{}（{}）", v.label, v.name)).collect();
            let idx = if is_interactive() && version.is_none() {
                select("请选择 Java 发行版", &labels)?
            } else {
                0 // 非交互默认第一个（或按 hint 后续扩展）
            };
            let vendor = &vendors[idx];
            let versions: Vec<String> = crate::core::tools::java::available_versions(vendor)
                .iter()
                .map(|s| s.to_string())
                .collect();
            let v = pick_version(&versions, version.as_deref())?;
            let url = crate::core::tools::java::resolve_url(&vendor.name, &v, &platform)?;
            // java 各发行版 sha 获取路径差异大，统一不传官方 sha（下载后清单记录实际哈希）
            fetch_and_cache(&paths, &tool, &v, &url, None)?;
        }
        _ => unreachable!(),
    }
    Ok(())
}

/// 版本选择：显式参数校验/交互选择
fn pick_version(list: &[String], hint: Option<&str>) -> Result<String> {
    match hint {
        Some(h) => {
            if !list.iter().any(|v| v == h) {
                return Err(anyhow!("版本 {h} 不可用，请从列表中选择"));
            }
            Ok(h.to_string())
        }
        None => {
            if !is_interactive() {
                return Err(anyhow!(
                    "非终端环境请指定版本，例如: cli download <tool> <版本>"
                ));
            }
            let idx = select("请选择版本", list)?;
            Ok(list[idx].clone())
        }
    }
}

/// 下载到缓存目录并更新清单（不安装）
pub fn fetch_and_cache(
    paths: &DevkitPaths,
    tool: &str,
    version: &str,
    url: &str,
    official_sha: Option<&str>,
) -> Result<()> {
    let cache_dir = paths.cache_dir();
    std::fs::create_dir_all(&cache_dir)?;
    let file = url
        .rsplit('/')
        .next()
        .filter(|n| !n.is_empty())
        .ok_or_else(|| anyhow!("无法从 URL 解析文件名: {url}"))?;
    let archive_path = cache_dir.join(file);
    // 预热场景总是重新下载覆盖，保证拿到最新文件
    download(url, &archive_path, official_sha, &format!("{tool} {version}"))?;
    let actual = sha256_of(&archive_path)?;
    let mut manifest = cache::load(&cache_dir).unwrap_or_default();
    cache::add(&mut manifest, tool, version, file, &actual);
    cache::save(&cache_dir, &manifest)?;
    println!("缓存就绪: {} ({}) -> {}", tool, version, archive_path.display());
    println!("已更新版本清单: {}", cache_dir.join("versions.json").display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::cache;
    use crate::core::paths::DevkitPaths;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    /// mock 服务器：返回 tar.gz 字节
    fn mock_server(body: Vec<u8>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf);
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(resp.as_bytes());
            let _ = stream.write_all(&body);
        });
        format!("http://{addr}")
    }

    #[test]
    fn download_file_updates_manifest_without_installing() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("devkit");
        let paths = DevkitPaths::with_root(root.clone());
        let body = b"fake-archive-bytes".to_vec();
        let base = mock_server(body.clone());
        let url = format!("{base}/node-v22.11.0-linux-x64.tar.gz");
        let tool = "node";
        let version = "v22.11.0";
        let file = "node-v22.11.0-linux-x64.tar.gz";
        // 核心流程函数：下载 → 算 sha → 更新清单（不注册 config）
        fetch_and_cache(&paths, tool, version, &url, None).unwrap();
        let cache_dir = paths.cache_dir();
        assert!(cache_dir.join(file).exists());
        let manifest = cache::load(&cache_dir).unwrap();
        let entry = cache::find(&manifest, tool, version).unwrap();
        assert_eq!(entry.file, file);
        assert_eq!(
            entry.sha256,
            crate::core::download::sha256_of(&cache_dir.join(file)).unwrap()
        );
        // 未安装：config.json 不存在
        assert!(!root.join("config.json").exists());
    }
}
