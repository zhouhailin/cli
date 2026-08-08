use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};

use crate::core::config::Config;
use crate::core::download::{download, extract_archive};
use crate::core::paths::DevkitPaths;
use crate::core::shell::{inject_path, rc_file_for_shell};
use crate::debug_log;

pub struct InstallContext {
    pub paths: DevkitPaths,
    pub config: Config,
}

impl InstallContext {
    pub fn load() -> Result<InstallContext> {
        let paths = DevkitPaths::new()?;
        let config = Config::load(&paths)?;
        Ok(InstallContext { paths, config })
    }

    pub fn save(&self) -> Result<()> {
        self.config.save(&self.paths)
    }
}

/// 统一安装流程：检查已安装 → 下载 → 校验 → 解压 → 剥离单顶层目录 → 注册 → 注入环境
pub fn install_archive(
    url: &str,
    sha256: Option<&str>,
    tool: &str,
    version: &str,
    ctx: &mut InstallContext,
    inject: bool,
) -> Result<()> {
    // 先检查已安装，避免白白下载
    let tool_dir = ctx.paths.tool_dir(tool, version);
    if tool_dir.exists() {
        return Err(anyhow!("{tool} {version} 已安装，请先卸载或使用其他版本"));
    }
    let cache_dir = ctx.paths.cache_dir();
    std::fs::create_dir_all(&cache_dir)?;
    let archive_name = url
        .rsplit('/')
        .next()
        .filter(|n| !n.is_empty())
        .ok_or_else(|| anyhow!("无法从 URL 解析文件名: {url}"))?;
    let archive_path = cache_dir.join(archive_name);
    debug_log!("安装 {tool} {version}: 归档文件 {}", archive_path.display());
    // 下载（内部带重试 + 校验 + 原子 rename）
    download(url, &archive_path, sha256)?;
    // 解压到工具目录（若目录已存在则跳过已安装检测交由上层）
    extract_archive(&archive_path, &tool_dir)?;
    // 剥离单顶层目录（node-v22.12.0/、apache-maven-3.9.9/ 等），使 bin/conf 直接位于 tool_dir 下
    flatten_single_top_dir(&tool_dir)?;
    debug_log!("已安装到 {}", tool_dir.display());
    // 注册配置并激活
    ctx.config.add_installed(tool, version);
    ctx.config.set_active(tool, version);
    ctx.save()?;
    // 注入 PATH（指向 current 链）
    if inject {
        let rc_file = rc_file_for_shell()?;
        let link = ctx.paths.current_link(tool);
        inject_path(&rc_file, &link.join("bin"))?;
        debug_log!("已注入 PATH: {}", rc_file.display());
    }
    Ok(())
}

/// 剥离解压产物的单顶层目录，使内容直接位于目标目录下
pub fn flatten_single_top_dir(dir: &Path) -> Result<()> {
    let entries: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    if entries.len() != 1 {
        return Ok(());
    }
    let inner = &entries[0];
    if !inner.is_dir() {
        return Ok(());
    }
    let staging = dir.join(".flatten-staging");
    if staging.exists() {
        std::fs::remove_dir_all(&staging)?;
    }
    std::fs::rename(inner, &staging)?;
    for entry in std::fs::read_dir(&staging)? {
        let entry = entry?;
        let dest = dir.join(entry.file_name());
        std::fs::rename(entry.path(), &dest)?;
    }
    std::fs::remove_dir(&staging)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use tempfile::tempdir;

    /// 本地 mock 服务：返回单个固定响应（供 install_archive 测试下载）
    fn mock_server(body: Vec<u8>, status: u16) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf);
            let reason = if status == 200 { "OK" } else { "Error" };
            let resp = format!(
                "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(resp.as_bytes());
            let _ = stream.write_all(&body);
        });
        format!("http://{addr}")
    }

    /// 构造含单顶层目录（模拟 node-v22.12.0/）的 tar.gz 字节流
    fn make_tar_gz_bytes() -> Vec<u8> {
        let mut out = Vec::new();
        {
            let gz = flate2::write::GzEncoder::new(&mut out, flate2::Compression::default());
            let mut tar = tar::Builder::new(gz);
            let mut dir_h = tar::Header::new_gnu();
            dir_h.set_entry_type(tar::EntryType::Directory);
            dir_h.set_mode(0o755);
            dir_h.set_size(0);
            dir_h.set_cksum();
            tar.append_data(&mut dir_h, "node-v22.12.0", std::io::empty())
                .unwrap();
            let mut header = tar::Header::new_gnu();
            header.set_size(4);
            header.set_mode(0o644);
            header.set_cksum();
            tar.append_data(
                &mut header,
                "node-v22.12.0/hello.txt",
                std::io::Cursor::new(b"data"),
            )
            .unwrap();
            tar.finish().unwrap();
        }
        out
    }

    fn test_ctx() -> (InstallContext, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let paths = DevkitPaths::with_root(root);
        let config = Config::default();
        (InstallContext { paths, config }, dir)
    }

    #[test]
    fn install_archive_unpacks_registers_and_activates() {
        let (mut ctx, _dir) = test_ctx();
        let body = make_tar_gz_bytes();
        let base = mock_server(body, 200);
        install_archive(
            &format!("{base}/pkg.tar.gz"),
            None,
            "node",
            "22.11.0",
            &mut ctx,
            false,
        )
        .unwrap();
        // 解压产物存在，且单顶层目录已被剥离
        let installed = ctx.paths.tool_dir("node", "22.11.0");
        assert_eq!(
            std::fs::read_to_string(installed.join("hello.txt")).unwrap(),
            "data"
        );
        assert!(
            !installed.join("node-v22.12.0").exists(),
            "单顶层目录应被剥离到 tool_dir 根"
        );
        // config 注册 + 激活
        assert_eq!(ctx.config.installed["node"], vec!["22.11.0".to_string()]);
        assert_eq!(ctx.config.active["node"], "22.11.0");
    }

    #[test]
    fn install_archive_creates_cache_and_removes_part_file() {
        let (mut ctx, _dir) = test_ctx();
        let base = mock_server(make_tar_gz_bytes(), 200);
        install_archive(
            &format!("{base}/go-1.22.0.tar.gz"),
            None,
            "go",
            "1.22.0",
            &mut ctx,
            false,
        )
        .unwrap();
        assert!(ctx.paths.root().join("cache/go-1.22.0.tar.gz").exists());
        assert!(!ctx
            .paths
            .root()
            .join("cache/go-1.22.0.tar.gz.part")
            .exists());
    }

    #[test]
    fn install_archive_fails_on_sha_mismatch_without_partial_state() {
        let (mut ctx, _dir) = test_ctx();
        let base = mock_server(make_tar_gz_bytes(), 200);
        let err = install_archive(
            &format!("{base}/pkg.tar.gz"),
            Some("0000000000000000000000000000000000000000000000000000000000000000"),
            "node",
            "22.11.0",
            &mut ctx,
            false,
        )
        .unwrap_err();
        assert!(err.to_string().contains("SHA-256 校验失败"));
        // 失败不产生注册/解压状态
        assert!(ctx.config.installed.is_empty());
        assert!(!ctx.paths.tool_dir("node", "22.11.0").exists());
    }

    #[serial(env)]
    #[test]
    fn install_archive_injects_path_when_requested() {
        let dir = tempdir().unwrap();
        // 构造 shell 环境：HOME 指向临时目录，SHELL=zsh，devkit root 位于 HOME/.devkit
        std::env::set_var("HOME", dir.path());
        std::env::set_var("SHELL", "/bin/zsh");
        let paths = DevkitPaths::with_root(dir.path().join(".devkit"));
        let config = Config::default();
        let mut ctx = InstallContext { paths, config };
        let base = mock_server(make_tar_gz_bytes(), 200);
        install_archive(
            &format!("{base}/pkg.tar.gz"),
            None,
            "maven",
            "3.9.9",
            &mut ctx,
            true,
        )
        .unwrap();
        let rc = rc_file_for_shell().unwrap();
        let text = std::fs::read_to_string(&rc).unwrap();
        let home_str = dir.path().to_string_lossy();
        assert!(text.contains(&format!(
            "export PATH=\"{home_str}/.devkit/current/maven/bin:$PATH\""
        )));
    }
}
