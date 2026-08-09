use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};

use crate::core::cache::{self, CacheManifest};
use crate::core::config::Config;
use crate::core::download::{download, extract_archive, sha256_of, verify_sha256};
use crate::core::links::set_current_link;
use crate::core::offline;
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

/// 统一安装流程：解析缓存路径（下载/复用/离线）→ 解压安装 → 注册 → 注入环境
pub fn install_archive(
    url: &str,
    sha256: Option<&str>,
    tool: &str,
    version: &str,
    ctx: &mut InstallContext,
    inject: bool,
) -> Result<()> {
    let tool_dir = ctx.paths.tool_dir(tool, version);
    if tool_dir.exists() {
        return Err(anyhow!("{tool} {version} 已安装，请先卸载或使用其他版本"));
    }
    let archive_path = resolve_archive_path(url, sha256, tool, version, ctx)?;
    install_from_archive(ctx, tool, version, &archive_path, inject)
}

/// 解析压缩包路径：在线下载（缓存命中复用）/ 离线查清单
fn resolve_archive_path(
    url: &str,
    sha256: Option<&str>,
    tool: &str,
    version: &str,
    ctx: &InstallContext,
) -> Result<PathBuf> {
    let cache_dir = ctx.paths.cache_dir();
    std::fs::create_dir_all(&cache_dir)?;
    let archive_name = url
        .rsplit('/')
        .next()
        .filter(|n| !n.is_empty())
        .ok_or_else(|| anyhow!("无法从 URL 解析文件名: {url}"))?;
    let archive_path = cache_dir.join(archive_name);
    debug_log!("安装 {tool} {version}: 归档文件 {}", archive_path.display());
    if offline::is_offline() {
        return offline_archive_path(ctx, tool, version);
    }
    // 在线：缓存命中复用（官方 sha 优先，其次清单 sha；无任何校验依据时文件存在即复用）
    let hit = archive_path.exists().then(|| {
        if let Some(expected) = sha256 {
            verify_sha256(&archive_path, expected).is_ok()
        } else {
            let manifest = cache::load(&cache_dir).unwrap_or_default();
            match cache::find(&manifest, tool, version) {
                Some(e) => verify_sha256(&archive_path, &e.sha256).is_ok(),
                None => true,
            }
        }
    });
    if hit == Some(true) {
        debug_log!("缓存命中复用: {}", archive_path.display());
        return Ok(archive_path);
    }
    // 下载（内部带重试 + 校验 + 原子 rename），成功后计算哈希并更新清单
    download(url, &archive_path, sha256, &format!("{tool} {version}"))?;
    let actual = sha256_of(&archive_path)?;
    let mut manifest = cache::load(&cache_dir).unwrap_or_default();
    cache::add(&mut manifest, tool, version, archive_name, &actual);
    if let Err(e) = cache::save(&cache_dir, &manifest) {
        eprintln!("警告: 更新版本清单失败: {e}");
    }
    Ok(archive_path)
}

/// 离线路径：从清单解析缓存文件并校验
fn offline_archive_path(ctx: &InstallContext, tool: &str, version: &str) -> Result<PathBuf> {
    let cache_dir = ctx.paths.cache_dir();
    let manifest = cache::load(&cache_dir).unwrap_or_default();
    let entry = cache::find(&manifest, tool, version).ok_or_else(|| {
        anyhow!(
            "离线模式缺少 {tool} {version} 的缓存，请先在联网机器执行 cli download {tool} {version} 预热"
        )
    })?;
    let archive_path = cache_dir.join(&entry.file);
    if !archive_path.exists() {
        return Err(anyhow!(
            "离线模式缺少 {tool} {version} 的缓存文件 {}，请重新拷贝缓存目录",
            archive_path.display()
        ));
    }
    verify_sha256(&archive_path, &entry.sha256).map_err(|_| {
        anyhow!("缓存文件损坏或不完整（sha256 不匹配），请重新预热")
    })?;
    Ok(archive_path)
}

/// 离线安装入口：清单查版本 → 校验 → 解压安装（install.rs 调用）
pub fn install_offline(ctx: &mut InstallContext, tool: &str, version: &str) -> Result<()> {
    let tool_dir = ctx.paths.tool_dir(tool, version);
    if tool_dir.exists() {
        return Err(anyhow!("{tool} {version} 已安装，请先卸载或使用其他版本"));
    }
    let archive_path = offline_archive_path(ctx, tool, version)?;
    install_from_archive(ctx, tool, version, &archive_path, true)
}

/// 解压安装 → 剥离单顶层目录 → 注册激活 → current 链接 → PATH 注入
pub fn install_from_archive(
    ctx: &mut InstallContext,
    tool: &str,
    version: &str,
    archive_path: &Path,
    inject: bool,
) -> Result<()> {
    let tool_dir = ctx.paths.tool_dir(tool, version);
    extract_archive(archive_path, &tool_dir)?;
    flatten_single_top_dir(&tool_dir)?;
    debug_log!("已安装到 {}", tool_dir.display());
    ctx.config.add_installed(tool, version);
    ctx.config.set_active(tool, version);
    ctx.save()?;
    let link = ctx.paths.current_link(tool);
    let rel_target = format!("../{tool}/{version}");
    set_current_link(&link, Path::new(&rel_target))?;
    if inject {
        let rc_file = rc_file_for_shell()?;
        let link = ctx.paths.current_link(tool);
        inject_path(&rc_file, &link.join("bin"))?;
        debug_log!("已注入 PATH: {}", rc_file.display());
        crate::core::shell::print_activation_hint()?;
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

    #[cfg(unix)]
    #[test]
    fn install_archive_creates_current_link() {
        let (mut ctx, _dir) = test_ctx();
        let base = mock_server(make_tar_gz_bytes(), 200);
        install_archive(
            &format!("{base}/pkg.tar.gz"),
            None,
            "maven",
            "3.9.9",
            &mut ctx,
            false,
        )
        .unwrap();
        let link = ctx.paths.current_link("maven");
        let target = std::fs::read_link(&link).unwrap();
        assert_eq!(target, std::path::Path::new("../maven/3.9.9"));
        // 再次安装其他版本：链接更新指向新版本（安装即激活）
        let base2 = mock_server(make_tar_gz_bytes(), 200);
        install_archive(
            &format!("{base2}/pkg.tar.gz"),
            None,
            "maven",
            "3.9.10",
            &mut ctx,
            false,
        )
        .unwrap();
        assert_eq!(
            std::fs::read_link(&link).unwrap(),
            std::path::Path::new("../maven/3.9.10")
        );
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

    /// 预置缓存目录：写入 tar.gz 文件与 versions.json 清单
    fn seed_cache(
        ctx: &InstallContext,
        tool: &str,
        version: &str,
        file: &str,
        body: &[u8],
    ) -> String {
        use crate::core::cache;
        let cache_dir = ctx.paths.cache_dir();
        std::fs::create_dir_all(&cache_dir).unwrap();
        std::fs::write(cache_dir.join(file), body).unwrap();
        let mut manifest = cache::CacheManifest::new();
        cache::add(
            &mut manifest,
            tool,
            version,
            file,
            &crate::core::download::sha256_of(&cache_dir.join(file)).unwrap(),
        );
        cache::save(&cache_dir, &manifest).unwrap();
        cache_dir.join(file).display().to_string()
    }

    #[serial(env)]
    #[test]
    fn install_offline_installs_from_cache_without_network() {
        let (mut ctx, dir) = test_ctx();
        // 隔离 rc 注入环境（install_offline 内部 inject=true），避免污染真实 HOME
        std::env::set_var("HOME", dir.path());
        std::env::set_var("SHELL", "/bin/zsh");
        let body = make_tar_gz_bytes();
        seed_cache(
            &ctx,
            "node",
            "v22.11.0",
            "node-v22.11.0-linux-x64.tar.gz",
            &body,
        );
        install_offline(&mut ctx, "node", "v22.11.0").unwrap();
        let installed = ctx.paths.tool_dir("node", "v22.11.0");
        assert_eq!(
            std::fs::read_to_string(installed.join("hello.txt")).unwrap(),
            "data"
        );
        assert_eq!(ctx.config.installed["node"], vec!["v22.11.0".to_string()]);
    }

    #[test]
    fn install_offline_fails_with_warmup_hint_when_missing() {
        let (mut ctx, _dir) = test_ctx();
        let err = install_offline(&mut ctx, "node", "v20.0.0").unwrap_err();
        assert!(err.to_string().contains("离线模式缺少 node v20.0.0 的缓存"));
        assert!(err.to_string().contains("cli download node v20.0.0 预热"));
    }

    #[test]
    fn install_offline_fails_on_sha_mismatch() {
        let (mut ctx, _dir) = test_ctx();
        let body = make_tar_gz_bytes();
        let cache_dir = ctx.paths.cache_dir();
        std::fs::create_dir_all(&cache_dir).unwrap();
        std::fs::write(cache_dir.join("pkg.tar.gz"), &body).unwrap();
        let mut manifest = crate::core::cache::CacheManifest::new();
        crate::core::cache::add(
            &mut manifest,
            "node",
            "v22.11.0",
            "pkg.tar.gz",
            "0000000000000000000000000000000000000000000000000000000000000000",
        );
        crate::core::cache::save(&cache_dir, &manifest).unwrap();
        let err = install_offline(&mut ctx, "node", "v22.11.0").unwrap_err();
        assert!(err.to_string().contains("缓存文件损坏或不完整"));
    }

    #[test]
    fn install_archive_reuses_cache_when_sha_matches() {
        // 预置缓存：文件名与 URL 末尾一致 + 清单 sha 匹配 → 复用，不发起网络请求。
        // mock 服务器返回 500：若代码仍尝试下载，则安装必然失败，从而证明未访问网络。
        let (mut ctx, _dir) = test_ctx();
        let body = make_tar_gz_bytes();
        seed_cache(&ctx, "go", "1.22.0", "go1.22.0.tar.gz", &body);
        let base = mock_server(body, 500);
        install_archive(
            &format!("{base}/go1.22.0.tar.gz"),
            None,
            "go",
            "1.22.0",
            &mut ctx,
            false,
        )
        .unwrap();
        let installed = ctx.paths.tool_dir("go", "1.22.0");
        assert_eq!(
            std::fs::read_to_string(installed.join("hello.txt")).unwrap(),
            "data"
        );
    }
}
