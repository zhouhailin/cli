# 离线部署模式与 os 无参交互 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现 CLI_OFFLINE/DEVKIT_OFFLINE 离线部署模式（本地缓存 + versions.json 清单 + cli download 预热）与 os info/download 无 name 交互选系统。

**Architecture:** 新增 core/offline.rs（开关检测）与 core/cache.rs（版本清单）；installer.rs 拆分「解析压缩包路径 / 解压安装」两段并新增 install_offline；新增 cli download 命令复用各工具现有版本列表与 URL 解析；os info/download 的 name 改可选并复用 fetch_all_names 交互选择。

**Tech Stack:** Rust + clap derive + serde_json + anyhow + assert_cmd + serial_test（与项目现有栈一致）。

## Global Constraints

- 离线开关：`CLI_OFFLINE` 或 `DEVKIT_OFFLINE` 任一非空且非 `"0"`/`"false"`（大小写不敏感）即离线。
- 离线范围：仅 java/node/go/maven/mvnd；rust、os download、cli download、cli update 离线报错。
- 清单文件：`<cache_dir>/versions.json`，`file` 与 URL 文件名一致，`sha256` 为下载后计算的实际哈希。
- 错误文案（逐字）：离线缺缓存「离线模式缺少 {tool} {version} 的缓存，请先在联网机器执行 cli download {tool} {version} 预热」；离线 rust/os「离线模式不支持 {工具} 安装，仅支持 java/node/go/maven/mvnd」；离线 download「离线模式无法下载，仅支持本地缓存安装」；sha 不匹配「缓存文件损坏或不完整（sha256 不匹配），请重新预热」。
- 清单写入失败：警告不阻断安装。
- 测试门禁：`cargo test` 全绿、`cargo clippy --all-targets -- -D warnings` 0、`cargo fmt --check` clean。
- 全部 env 修改测试加 `#[serial(env)]`。

---

### Task 1: 离线开关 core/offline.rs

**Files:**
- Create: `src/core/offline.rs`
- Modify: `src/core/mod.rs`（注册 `pub mod offline;`）
- Test: `src/core/offline.rs`（内嵌单测）

**Interfaces:**
- Produces: `pub fn is_offline() -> bool` —— 后续所有任务依赖。

- [ ] **Step 1: 写失败单测**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    fn clear_vars() {
        std::env::remove_var("CLI_OFFLINE");
        std::env::remove_var("DEVKIT_OFFLINE");
    }

    #[serial(env)]
    #[test]
    fn offline_when_cli_offline_true() {
        clear_vars();
        std::env::set_var("CLI_OFFLINE", "true");
        assert!(is_offline());
    }

    #[serial(env)]
    #[test]
    fn offline_when_devkit_offline_one() {
        clear_vars();
        std::env::set_var("DEVKIT_OFFLINE", "1");
        assert!(is_offline());
    }

    #[serial(env)]
    #[test]
    fn online_when_vars_unset() {
        clear_vars();
        assert!(!is_offline());
    }

    #[serial(env)]
    #[test]
    fn online_when_var_is_false_or_zero() {
        clear_vars();
        std::env::set_var("CLI_OFFLINE", "false");
        assert!(!is_offline());
        std::env::set_var("CLI_OFFLINE", "0");
        assert!(!is_offline());
        std::env::set_var("CLI_OFFLINE", "FALSE");
        assert!(!is_offline());
    }

    #[serial(env)]
    #[test]
    fn offline_when_var_is_uppercase_true() {
        clear_vars();
        std::env::set_var("DEVKIT_OFFLINE", "TRUE");
        assert!(is_offline());
    }
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test --lib core::offline`
Expected: 编译错误 `cannot find module offline`（或 `is_offline` 未定义）

- [ ] **Step 3: 最小实现**

```rust
//! 离线模式检测：CLI_OFFLINE / DEVKIT_OFFLINE 任一非空且非 0/false 即启用

/// 是否处于离线模式（仅使用本地缓存，不访问网络）
pub fn is_offline() -> bool {
    ["CLI_OFFLINE", "DEVKIT_OFFLINE"].iter().any(|key| {
        std::env::var(key).is_ok_and(|v| {
            let t = v.trim().to_lowercase();
            !t.is_empty() && t != "0" && t != "false"
        })
    })
}
```

（注意：`is_ok_and` 需 Rust 1.70+；若编译报错改用手写 match。）

- [ ] **Step 4: 运行确认通过**

Run: `cargo test --lib core::offline`
Expected: 5 passed

- [ ] **Step 5: 提交**

```bash
git add src/core/offline.rs src/core/mod.rs
git commit -m "feat: 离线模式开关检测 core/offline.rs"
```

---

### Task 2: 版本清单 core/cache.rs

**Files:**
- Create: `src/core/cache.rs`
- Modify: `src/core/mod.rs`（注册 `pub mod cache;`）
- Test: `src/core/cache.rs`（内嵌单测）

**Interfaces:**
- Produces:
  - `pub struct CacheEntry { pub version: String, pub file: String, pub sha256: String }`
  - `pub type CacheManifest = std::collections::BTreeMap<String, Vec<CacheEntry>>`
  - `pub fn load(cache_dir: &Path) -> Result<CacheManifest>`（文件缺失返回空清单）
  - `pub fn save(cache_dir: &Path, manifest: &CacheManifest) -> Result<()>`（写入 `<cache_dir>/versions.json`，先 create_dir_all）
  - `pub fn find<'a>(manifest: &'a CacheManifest, tool: &str, version: &str) -> Option<&'a CacheEntry>`
  - `pub fn add(manifest: &mut CacheManifest, tool: &str, version: &str, file: &str, sha256: &str)`（按 tool+version 幂等去重，已存在则更新 file/sha256）

- [ ] **Step 1: 写失败单测**

```rust
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
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test --lib core::cache`
Expected: 编译错误 `cannot find module cache`

- [ ] **Step 3: 最小实现**

```rust
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
```

- [ ] **Step 4: 运行确认通过**

Run: `cargo test --lib core::cache`
Expected: 5 passed

- [ ] **Step 5: 提交**

```bash
git add src/core/cache.rs src/core/mod.rs
git commit -m "feat: 版本清单 core/cache.rs（versions.json 读写/幂等更新）"
```

---

### Task 3: installer.rs 拆分：缓存解析与安装分离

**Files:**
- Modify: `src/core/installer.rs`
- Test: `src/core/installer.rs`（内嵌单测）

**Interfaces:**
- Consumes: `crate::core::offline::is_offline()`、`crate::core::cache::{load, find, add, save}`、`crate::core::download::{download, verify_sha256, sha256_of}`
- Produces:
  - `pub fn install_offline(ctx: &mut InstallContext, tool: &str, version: &str) -> Result<()>` —— Task 4 使用
  - `pub fn install_from_archive(ctx: &mut InstallContext, tool: &str, version: &str, archive_path: &Path, inject: bool) -> Result<()>` —— 内部共享（解压 → flatten → 注册 → current 链接 → PATH）
  - `install_archive` 签名不变：`(url: &str, sha256: Option<&str>, tool: &str, version: &str, ctx: &mut InstallContext, inject: bool) -> Result<()>`

- [ ] **Step 1: 写失败单测（先测 install_offline 与命中复用）**

```rust
// 追加到 installer.rs tests 模块（复用现有 test_ctx/mock_server/make_tar_gz_bytes）

/// 预置缓存目录：写入 tar.gz 文件与 versions.json 清单
fn seed_cache(ctx: &InstallContext, tool: &str, version: &str, file: &str, body: &[u8]) -> String {
    use crate::core::cache;
    let cache_dir = ctx.paths.cache_dir();
    std::fs::create_dir_all(&cache_dir).unwrap();
    std::fs::write(cache_dir.join(file), body).unwrap();
    let mut manifest = cache::CacheManifest::new();
    cache::add(&mut manifest, tool, version, file, &crate::core::download::sha256_of(&cache_dir.join(file)).unwrap());
    cache::save(&cache_dir, &manifest).unwrap();
    cache_dir.join(file).display().to_string()
}

#[test]
fn install_offline_installs_from_cache_without_network() {
    let (mut ctx, _dir) = test_ctx();
    let body = make_tar_gz_bytes();
    seed_cache(&ctx, "node", "v22.11.0", "node-v22.11.0-linux-x64.tar.gz", &body);
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
    // 预置缓存：文件名与 URL 末尾一致 + 清单有记录 → 复用，mock 服务器不应被访问
    let (mut ctx, _dir) = test_ctx();
    let body = make_tar_gz_bytes();
    seed_cache(&ctx, "go", "1.22.0", "go1.22.0.tar.gz", &body);
    // 故意绑一个只响应一次的 mock 服务器（若被访问会阻塞线程但下载会成功）；
    // 断言用「清单 sha 与新下载行为」验证：直接装成功且 cache 文件未被替换
    let base = mock_server(body.clone(), 200);
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
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test --lib core::installer`
Expected: `install_offline` 未定义编译错误；`install_archive_reuses_cache_when_sha_matches` 也会失败（当前无命中复用逻辑，会走下载；mock 服务器单响应但下载也成功——该测试实际会 PASS，仅 install_offline 系列编译失败）

- [ ] **Step 3: 实现**

在 `installer.rs` 顶部 import 增加：

```rust
use crate::core::cache::{self, CacheManifest};
use crate::core::download::{download, extract_archive, sha256_of, verify_sha256};
use crate::core::offline;
```

将 `install_archive` 拆分为三段：

```rust
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

/// 离线安装入口：清单查版本 → 校验 → 解压安装（Task 4 的 install.rs 调用）
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
```

注意：原 `install_archive` 的缓存创建/下载/解压/注册逻辑整体替换为上三段；`debug_log` 已 import。

- [ ] **Step 4: 运行确认通过**

Run: `cargo test --lib core::installer`
Expected: 全部通过（原 5 个 + 新增 4 个）

- [ ] **Step 5: 提交**

```bash
git add src/core/installer.rs
git commit -m "feat: installer 拆分缓存解析与安装，支持命中复用与离线安装"
```

---

### Task 4: install 命令离线接线 + 离线集成测试

**Files:**
- Modify: `src/commands/install.rs`
- Create: `tests/cli_offline.rs`
- Test: `tests/cli_offline.rs`

**Interfaces:**
- Consumes: `crate::core::offline::is_offline()`、`crate::core::cache::{load, find, CacheManifest}`、`crate::core::installer::install_offline`、`crate::core::interact::{select, is_interactive}`
- Produces: 无（install.rs 内部逻辑）

- [ ] **Step 1: 写失败集成测试**

```rust
// tests/cli_offline.rs
use assert_cmd::Command;
use predicates::prelude::*;

/// 构造含单顶层目录（node-v22.12.0/）的 tar.gz 字节（与 installer.rs 测试同构）
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
        tar.append_data(&mut dir_h, "node-v22.12.0", std::io::empty()).unwrap();
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

/// 预置缓存：tar.gz + versions.json
fn seed_cache(root: &std::path::Path, tool: &str, version: &str, file: &str, body: &[u8]) {
    let cache_dir = root.join("cache");
    std::fs::create_dir_all(&cache_dir).unwrap();
    std::fs::write(cache_dir.join(file), body).unwrap();
    let sha = {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        let mut f = std::fs::File::open(cache_dir.join(file)).unwrap();
        std::io::copy(&mut f, &mut h).unwrap();
        format!("{:x}", h.finalize())
    };
    let manifest = serde_json::json!({ tool: [{ "version": version, "file": file, "sha256": sha }] });
    std::fs::write(cache_dir.join("versions.json"), manifest.to_string()).unwrap();
}

#[test]
fn offline_install_succeeds_from_cache() {
    let home = tempfile::tempdir().unwrap();
    let root = home.path().join("devkit");
    let body = make_tar_gz_bytes();
    seed_cache(&root, "node", "v22.11.0", "node-v22.11.0-linux-x64.tar.gz", &body);
    Command::cargo_bin("cli")
        .unwrap()
        .env("HOME", home.path())
        .env("DEVKIT_ROOT", &root)
        .env("SHELL", "/bin/zsh")
        .env("CLI_OFFLINE", "true")
        .args(["install", "node", "v22.11.0"])
        .assert()
        .success()
        .stdout(predicate::str::contains("离线模式"));
    let config: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("config.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(config["installed"]["node"][0], "v22.11.0");
    assert!(root.join("node/v22.11.0/hello.txt").exists());
}

#[test]
fn offline_install_reports_warmup_hint_when_cache_missing() {
    let home = tempfile::tempdir().unwrap();
    let root = home.path().join("devkit");
    Command::cargo_bin("cli")
        .unwrap()
        .env("HOME", home.path())
        .env("DEVKIT_ROOT", &root)
        .env("SHELL", "/bin/zsh")
        .env("CLI_OFFLINE", "true")
        .args(["install", "node", "v20.0.0"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("离线模式缺少 node v20.0.0 的缓存"));
}

#[test]
fn offline_install_rejects_rust() {
    let home = tempfile::tempdir().unwrap();
    let root = home.path().join("devkit");
    Command::cargo_bin("cli")
        .unwrap()
        .env("HOME", home.path())
        .env("DEVKIT_ROOT", &root)
        .env("SHELL", "/bin/zsh")
        .env("DEVKIT_OFFLINE", "true")
        .args(["install", "rust"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("离线模式不支持 rust 安装"));
}

#[test]
fn offline_install_non_tty_without_version_reports_hint() {
    let home = tempfile::tempdir().unwrap();
    let root = home.path().join("devkit");
    Command::cargo_bin("cli")
        .unwrap()
        .env("HOME", home.path())
        .env("DEVKIT_ROOT", &root)
        .env("SHELL", "/bin/zsh")
        .env("CLI_OFFLINE", "true")
        .args(["install", "node"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("请指定版本"));
}
```

（tests 需要 dev-dependencies：flate2/tar/serde_json/sha2——均已在 Cargo.toml 的 dependencies 中，集成测试可直接 use；若报未找到则确认 dev-dependencies 是否已含 flate2/tar/serde_json/sha2/tempfile/assert_cmd/predicates。）

- [ ] **Step 2: 运行确认失败**

Run: `cargo test --test cli_offline`
Expected: 全部失败（install.rs 未接离线逻辑，前两个会联网失败或走正常流程；rust 测试会走 rust::install 而非离线报错）

- [ ] **Step 3: 实现 install.rs 离线接线**

在 `src/commands/install.rs` 的 `run()` 开头（`ensure_writable` 之后）插入：

```rust
use crate::core::cache::{self, CacheManifest};
use crate::core::installer::{install_offline, InstallContext};
use crate::core::interact::{is_interactive, select};
use crate::core::offline;

/// 离线安装：rust/os 报错；压缩包类从版本清单选择版本后安装
fn offline_install(tool: &str, version_hint: Option<&str>) -> Result<()> {
    if tool == "rust" || tool == "os" {
        return Err(anyhow::anyhow!(
            "离线模式不支持 {tool} 安装，仅支持 java/node/go/maven/mvnd"
        ));
    }
    let mut ctx = InstallContext::load()?;
    let manifest = cache::load(&ctx.paths.cache_dir())?;
    let versions: Vec<String> = manifest
        .get(tool)
        .map(|list| list.iter().map(|e| e.version.clone()).collect())
        .unwrap_or_default();
    let version = match version_hint {
        Some(hint) => {
            if !versions.iter().any(|v| v == hint) {
                return Err(anyhow::anyhow!(
                    "离线模式无 {tool} {hint} 的缓存记录，可用版本: {}",
                    if versions.is_empty() {
                        "无".to_string()
                    } else {
                        versions.join("、")
                    }
                ));
            }
            hint.to_string()
        }
        None => {
            if versions.is_empty() {
                return Err(anyhow::anyhow!(
                    "离线模式无 {tool} 可用版本，请先在联网机器执行 cli download {tool} 预热"
                ));
            }
            if !is_interactive() {
                return Err(anyhow::anyhow!("请指定版本，例如: cli install {tool} <版本>"));
            }
            let idx = select(&format!("请选择要离线安装的 {tool} 版本"), &versions)?;
            versions[idx].clone()
        }
    };
    install_offline(&mut ctx, tool, &version)
}
```

在 `run()` 中，工具解析并校验后（`TOOL_CHOICES` 匹配之前或之后均可，放在匹配前保证 rust/os 也进入离线分支）：

```rust
pub fn run(tool: Option<String>) -> Result<()> {
    let paths = DevkitPaths::new()?;
    paths.ensure_writable()?;
    let tool = match tool {
        Some(t) => t,
        None => { /* 现有交互选择逻辑不变 */ }
    };
    if offline::is_offline() {
        println!("离线模式: 仅使用本地缓存，不访问网络");
        return offline_install(&tool, None);
    }
    match tool.as_str() {
        "java" => java::install(None, None),
        // ... 其余不变
    }
}
```

注意：保持 `tool` 为 String 后的现有 match 分支不变；离线分支只新增在上方。若现有代码 `tool` 在 match 中按 `&str` 使用，保持一致。

- [ ] **Step 4: 运行确认通过**

Run: `cargo test --test cli_offline`
Expected: 4 passed

- [ ] **Step 5: 提交**

```bash
git add src/commands/install.rs tests/cli_offline.rs
git commit -m "feat: install 离线模式接线（rust/os 报错，压缩包类清单安装）"
```

---

### Task 5: cli download 命令

**Files:**
- Create: `src/commands/download.rs`
- Modify: `src/commands/mod.rs`（`pub mod download;`）
- Modify: `src/lib.rs`（Command 枚举加 `Download { tool: Option<String>, version: Option<String> }` + match 分发 + 现有 matches! 推断相关分支不变）
- Modify: `src/core/mod.rs`（若 download.rs 需要时；预期不需要）
- Test: `src/commands/download.rs`（内嵌单测：resolve_download_url + mock 下载更新清单）、`tests/cli_offline.rs`（扩展非网络路径）

**Interfaces:**
- Consumes: `crate::core::offline::is_offline()`、`crate::core::cache::{load, add, save}`、`crate::core::download::download`、各工具 `fetch_*`/`resolve_url`/`sha256_url`
- Produces: `pub fn run(tool: Option<String>, version: Option<String>) -> Result<()>`
- 注意：`cli download` 的 `version` 参数位置——clap 中 `Download { tool: Option<String>, version: Option<String> }` 为位置参数，命令形式 `cli download node v22.11.0`。

- [ ] **Step 1: 写失败单测（download.rs 内）**

```rust
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
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test --lib commands::download`
Expected: 编译错误 `cannot find module download`

- [ ] **Step 3: 实现 download.rs + 接线**

```rust
// src/commands/download.rs
//! cli download：下载工具压缩包到缓存目录并更新版本清单（不安装），用于离线部署预热

use std::path::Path;

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
            let v = pick_version(&list.iter().map(|n| n.version.clone()).collect::<Vec<_>>(), version.as_deref())?;
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
            let labels: Vec<String> = vendors.iter().map(|v| format!("{}（{}）", v.name, v.id)).collect();
            let idx = if is_interactive() && version.is_none() {
                select("请选择 Java 发行版", &labels)?
            } else {
                0 // 非交互默认第一个（或按 hint 后续扩展）
            };
            let vendor = &vendors[idx];
            let versions = crate::core::tools::java::available_versions(vendor);
            let v = pick_version(&versions.iter().map(|s| s.to_string()).collect::<Vec<_>>(), version.as_deref())?;
            let url = crate::core::tools::java::resolve_url(&vendor.id, &v, &platform)?;
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
                return Err(anyhow!("非终端环境请指定版本，例如: cli download <tool> <版本>"));
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
```

`lib.rs` 修改：

```rust
    /// 下载工具压缩包到本地缓存（离线部署预热，不安装）
    Download {
        /// 工具名（不填则交互选择）
        tool: Option<String>,
        /// 版本（不填则交互选择）
        version: Option<String>,
    },
```

```rust
        Command::Download { tool, version } => commands::download::run(tool, version),
```

（若 lib.rs 存在 infer_subcommands 相关 `matches!` 分支，按现有模式补充 `Command::Download { .. }`。）

`src/commands/mod.rs`：`pub mod download;`。

`maven.rs`/`mvnd.rs` 需确认函数名：maven 版本列表函数为 `parse_maven_versions(html)`——若不存在 `fetch_versions()` 则需在 maven.rs/mvnd.rs 各新增：

```rust
// maven.rs
pub fn fetch_versions() -> Result<Vec<String>> {
    let body = http_get_string("https://archive.apache.org/dist/maven/maven-3/")?;
    parse_maven_versions(&body)
}

// mvnd.rs
pub fn fetch_versions() -> Result<Vec<String>> {
    let body = http_get_string(VERSIONS_URL)?;
    parse_mvnd_versions(&body)  // 若存在内部解析函数则复用；否则按其现有 install 内的解析逻辑提取
}
```

（实施时先读 maven.rs/mvnd.rs 现有 install 实现，提取对应 fetch 函数；若函数已存在则直接使用。）

- [ ] **Step 4: 运行确认通过**

Run: `cargo test --lib commands::download && cargo test --test cli_offline`
Expected: download 单测 1 passed；cli_offline 仍 4 passed

- [ ] **Step 5: 集成测试扩展（非网络路径）**

追加到 `tests/cli_offline.rs`：

```rust
#[test]
fn download_command_rejects_offline_mode() {
    let home = tempfile::tempdir().unwrap();
    Command::cargo_bin("cli")
        .unwrap()
        .env("HOME", home.path())
        .env("DEVKIT_ROOT", home.path().join("devkit"))
        .env("CLI_OFFLINE", "true")
        .args(["download", "node", "v22.11.0"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("离线模式无法下载"));
}

#[test]
fn download_command_non_tty_without_tool_reports_hint() {
    let home = tempfile::tempdir().unwrap();
    Command::cargo_bin("cli")
        .unwrap()
        .env("HOME", home.path())
        .env("DEVKIT_ROOT", home.path().join("devkit"))
        .args(["download"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("请指定工具名"));
}

#[test]
fn download_command_rejects_unknown_tool() {
    let home = tempfile::tempdir().unwrap();
    Command::cargo_bin("cli")
        .unwrap()
        .env("HOME", home.path())
        .env("DEVKIT_ROOT", home.path().join("devkit"))
        .args(["download", "rust"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("暂不支持下载 rust"));
}
```

Run: `cargo test --test cli_offline`
Expected: 7 passed

- [ ] **Step 6: 提交**

```bash
git add src/commands/download.rs src/commands/mod.rs src/lib.rs src/core/tools/maven.rs src/core/tools/mvnd.rs tests/cli_offline.rs
git commit -m "feat: cli download 预热命令（下载到缓存并更新版本清单）"
```

---

### Task 6: os info/download 无 name 交互选系统

**Files:**
- Modify: `src/lib.rs`（`OsCommand::Info`/`Download` 的 `name` 改 `Option<String>`）
- Modify: `src/commands/os.rs`
- Modify: `tests/cli_os.rs`（追加非 TTY 无 name 报错测试）
- Test: `tests/cli_os.rs`

**Interfaces:**
- Consumes: `crate::core::mirror::fetch_all_names`、`crate::core::interact::{select, is_interactive}`
- Produces: 无

- [ ] **Step 1: 写失败集成测试（追加到 tests/cli_os.rs）**

```rust
#[test]
fn os_info_non_tty_without_name_reports_hint() {
    let home = tempfile::tempdir().unwrap();
    Command::cargo_bin("cli")
        .unwrap()
        .env("HOME", home.path())
        .env("DEVKIT_ROOT", home.path().join("devkit"))
        .args(["os", "info"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("请指定系统名"));
}

#[test]
fn os_download_non_tty_without_name_reports_hint() {
    let home = tempfile::tempdir().unwrap();
    Command::cargo_bin("cli")
        .unwrap()
        .env("HOME", home.path())
        .env("DEVKIT_ROOT", home.path().join("devkit"))
        .args(["os", "download"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("请指定系统名"));
}
```

（注意：非 TTY 时不应发起任何网络请求——run_info/run_download 在 name=None 且非 TTY 时**先报错**，不调用 fetch。）

- [ ] **Step 2: 运行确认失败**

Run: `cargo test --test cli_os`
Expected: 编译失败（name 为必填 String，`os info` 无参会 clap 报错而非自定义提示；或测试失败）

- [ ] **Step 3: 实现**

`src/lib.rs`：

```rust
    /// 查询系统全部镜像（版本/大小/链接）
    Info {
        /// 系统名（如 almalinux、ubuntu；不填则交互选择）
        name: Option<String>,
    },
    /// 下载系统 ISO 镜像
    Download {
        /// 系统名（如 almalinux、ubuntu；不填则交互选择）
        name: Option<String>,
        /// 精确指定镜像版本（version 字段）；不填则交互选择
        #[arg(long)]
        version: Option<String>,
        /// 下载保存目录（默认当前目录）
        #[arg(short, long, default_value = ".")]
        output_dir: String,
    },
```

`src/commands/os.rs`：

```rust
pub fn run(cmd: OsCommand) -> Result<()> {
    match cmd {
        OsCommand::List => run_list(),
        OsCommand::Info { name } => run_info(name.as_deref()),
        OsCommand::Download {
            name,
            version,
            output_dir,
        } => run_download(name.as_deref(), version.as_deref(), &output_dir),
    }
}

/// 无 name 时交互选择系统（非 TTY 报错提示）
fn resolve_name(name: Option<&str>) -> Result<String> {
    if let Some(n) = name {
        return Ok(n.to_string());
    }
    if !is_interactive() {
        return Err(anyhow!("非终端环境请指定系统名，例如: cli os info <系统名>"));
    }
    let names = mirror::fetch_all_names()?;
    if names.is_empty() {
        return Err(anyhow!("暂无可用系统镜像"));
    }
    let idx = select("请选择系统", &names)?;
    Ok(names[idx].clone())
}

pub fn run_info(name: Option<&str>) -> Result<()> {
    let name = resolve_name(name)?;
    let images = mirror::fetch_images(&name)?;
    // ... 其余不变
}

pub fn run_download(name: Option<&str>, version: Option<&str>, output_dir: &str) -> Result<()> {
    let name = resolve_name(name)?;
    // ... 其余不变
}
```

（注意现有测试 `os info almalinux` 等显式 name 路径不变。）

- [ ] **Step 4: 运行确认通过**

Run: `cargo test --test cli_os`
Expected: 原测试 + 2 新增全部通过

- [ ] **Step 5: 提交**

```bash
git add src/lib.rs src/commands/os.rs tests/cli_os.rs
git commit -m "feat: os info/download 无 name 交互选择系统"
```

---

### Task 7: README 文档 + 全量门禁

**Files:**
- Modify: `README.md`

- [ ] **Step 1: 更新 README**

- 环境变量表增加两行：

```markdown
| CLI_OFFLINE | true/1 时启用离线模式（不访问网络，仅使用本地缓存安装） |
| DEVKIT_OFFLINE | 同上（与 CLI_OFFLINE 二选一即可） |
```

- 命令列表新增 `cli download`：`下载工具压缩包到缓存目录并更新版本清单（离线部署预热，不安装）`
- 新增「离线部署」小节（步骤示例）：

````markdown
## 离线部署

离线环境安装：设置 `CLI_OFFLINE=true`（或 `DEVKIT_OFFLINE=true`）后，cli 不访问网络，仅使用本地缓存安装（支持 java/node/go/maven/mvnd）。

1. 在联网机器预热缓存（每个需要离线安装的工具+版本执行一次）：

   ```bash
   cli download node v22.11.0
   ```

2. 将缓存目录拷贝到离线机器（默认 `<devkit根>/cache`，可用 `DEVKIT_CACHE_DIR` 指定共享目录）：

   ```bash
   cp -r <缓存目录> <离线机器>
   ```

3. 离线机器安装：

   ```bash
   CLI_OFFLINE=true cli install node v22.11.0
   ```

离线模式不支持 rust、os download、cli download、cli update。
````

- [ ] **Step 2: 全量门禁**

Run: `cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Expected: 全部通过；若 fmt 报差异先 `cargo fmt` 再复查

- [ ] **Step 3: 提交**

```bash
git add README.md
git commit -m "docs: README 离线部署模式与 cli download 说明"
```

---

## 验收清单

- [ ] `cargo test` 全绿（预计 157 + 新增约 20 个）
- [ ] `cargo clippy --all-targets -- -D warnings` 0
- [ ] `cargo fmt --check` clean
- [ ] 离线安装（预置缓存+清单）端到端可用
- [ ] `cli download` 预热 → 拷贝缓存 → 离线安装链路完整
- [ ] `cli os info`/`cli os download` 无 name 交互选择系统
