# install 无参交互选择、self-update 与版本同步 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `cli install` 无参时交互选择工具；新增 `cli self-update` 自更新；发布二进制版本号与 tag 同步；支持 `DEVKIT_CACHE_DIR` 缓存目录。

**Architecture:** CLI 层（lib.rs/commands）做参数化与交互分发，core 层（versions/paths/tools/self_update）提供纯函数与 IO 流程。self-update 复用现有 download/http_get_string/confirm 组件，版本比较复用 versions::compare。所有可测逻辑抽为纯函数，TDD 先行。

**Tech Stack:** Rust 2021 / clap 4 / dialoguer 0.11 / anyhow / ureq 2（已有）

## Global Constraints

- 设计文档：`docs/superpowers/specs/2026-08-08-install-interactive-and-self-update-design.md`
- 版本号：发布构建注入 `CLI_VERSION`（tag 值如 `v0.1.1`），显示时去掉 `v` 前缀；本地构建回退 Cargo.toml 版本
- 工具列表中文标签：Java / Node.js / Go / Maven / 自更新
- 非 TTY 且 install 无参：报错 `请指定工具名，例如: cli install java`，不抛交互错误
- 资产命名（与 release.yml 一致）：`cli-linux-x64` / `cli-linux-arm64` / `cli-macos-x64` / `cli-macos-arm64` / `cli-windows-x64.exe`
- 所有环境变量测试必须 `#[serial(env)]` 标注（serial_test）
- 每次提交前运行：`cargo test`（现有 91 个测试保持全绿）、`cargo clippy --all-targets -- -D warnings`、`cargo fmt --check`

---

### Task 1: 版本归一化 `parse_tag` + `current_version`

**Files:**
- Modify: `src/core/versions.rs`（新增 `parse_tag` + 测试）
- Modify: `src/lib.rs`（`VERSION` 常量改为 `current_version()` 函数）
- Modify: `src/commands/version.rs:6`（改用 `crate::current_version()`）

**Interfaces:**
- Produces: `pub fn parse_tag(tag: &str) -> &str`（去 `v`/`V` 前缀，无前缀原样返回）
- Produces: `pub fn current_version() -> &'static str`（lib.rs，`option_env!("CLI_VERSION")` 优先，回退 `CARGO_PKG_VERSION`，经 `parse_tag` 归一化）

- [ ] **Step 1: 写失败测试**（versions.rs 测试模块追加）

```rust
#[test]
fn parse_tag_strips_v_prefix() {
    assert_eq!(parse_tag("v0.1.1"), "0.1.1");
    assert_eq!(parse_tag("V1.2.3"), "1.2.3");
    assert_eq!(parse_tag("0.1.1"), "0.1.1");
    assert_eq!(parse_tag(""), "");
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test parse_tag_strips_v_prefix`
Expected: FAIL（`parse_tag` 未定义）

- [ ] **Step 3: 实现 `parse_tag`**（versions.rs，`parse_segment` 函数下方）

```rust
/// 去除版本号前缀 v/V（v0.1.1 -> 0.1.1），无前缀原样返回
pub fn parse_tag(tag: &str) -> &str {
    tag.strip_prefix('v').or_else(|| tag.strip_prefix('V')).unwrap_or(tag)
}
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test parse_tag_strips_v_prefix`
Expected: PASS

- [ ] **Step 5: lib.rs 改版本常量**

```rust
// 替换：pub const VERSION: &str = env!("CARGO_PKG_VERSION");
use crate::core::versions::parse_tag;

/// 当前版本：发布构建用 CLI_VERSION（tag），本地开发回退 Cargo.toml 版本
pub fn current_version() -> &'static str {
    parse_tag(option_env!("CLI_VERSION").unwrap_or(env!("CARGO_PKG_VERSION")))
}
```

（lib.rs 已有 `pub mod core;`，直接加 `use crate::core::versions::parse_tag;`）

- [ ] **Step 6: version.rs 改用新函数**

`src/commands/version.rs`：`println!("cli {}", crate::current_version());`

- [ ] **Step 7: 全量验证 + 提交**

Run: `cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Expected: 全绿（tests/cli_version.rs 断言 `cli 0.1.0` 不受影响——本地无 CLI_VERSION 时回退 Cargo.toml 版本）

```bash
git add src/core/versions.rs src/lib.rs src/commands/version.rs
git commit -m "feat: 版本号支持 CLI_VERSION 注入（发布构建与 tag 同步）"
```

---

### Task 2: DEVKIT_CACHE_DIR 缓存目录配置

**Files:**
- Modify: `src/core/paths.rs`（新增 `cache_dir()` + 测试）
- Modify: `src/core/installer.rs:37`（改用 `ctx.paths.cache_dir()`）

**Interfaces:**
- Consumes: `DevkitPaths`（已有 `root()`）
- Produces: `pub fn cache_dir(&self) -> PathBuf`——`DEVKIT_CACHE_DIR` 非空时原样使用（相对路径按当前工作目录解释），否则 `<root>/cache`

- [ ] **Step 1: 写失败测试**（paths.rs 测试模块追加）

```rust
#[test]
fn cache_dir_defaults_to_root_cache() {
    let paths = DevkitPaths::with_root(PathBuf::from("/tmp/x"));
    assert_eq!(paths.cache_dir(), PathBuf::from("/tmp/x/cache"));
}

#[serial(env)]
#[test]
fn cache_dir_reads_env_override() {
    let paths = DevkitPaths::with_root(PathBuf::from("/tmp/x"));
    std::env::set_var("DEVKIT_CACHE_DIR", "/data/pkg-cache");
    assert_eq!(paths.cache_dir(), PathBuf::from("/data/pkg-cache"));
    std::env::remove_var("DEVKIT_CACHE_DIR");
    assert_eq!(paths.cache_dir(), PathBuf::from("/tmp/x/cache"));
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test cache_dir_`
Expected: FAIL（`cache_dir` 未定义）

- [ ] **Step 3: 实现 `cache_dir()`**（paths.rs，`config_file()` 附近）

```rust
/// 压缩包缓存目录：DEVKIT_CACHE_DIR 优先，否则默认 <root>/cache
pub fn cache_dir(&self) -> PathBuf {
    if let Ok(env_cache) = std::env::var("DEVKIT_CACHE_DIR") {
        if !env_cache.is_empty() {
            return PathBuf::from(env_cache);
        }
    }
    self.root.join("cache")
}
```

- [ ] **Step 4: installer.rs 改用 cache_dir()**

`src/core/installer.rs` 的 `install_archive` 中：`let cache_dir = ctx.paths.root().join("cache");` 替换为 `let cache_dir = ctx.paths.cache_dir();`

- [ ] **Step 5: 运行测试确认通过**

Run: `cargo test`
Expected: PASS（含现有 `install_archive_creates_cache_and_removes_part_file`——默认路径不变）

- [ ] **Step 6: 提交**

```bash
git add src/core/paths.rs src/core/installer.rs
git commit -m "feat: 支持 DEVKIT_CACHE_DIR 环境变量指定压缩包缓存目录"
```

---

### Task 3: `cli install` 无参交互选择 + `SelfUpdate` 子命令

**Files:**
- Modify: `src/lib.rs`（`Install.tool` 改 `Option<String>`；新增 `SelfUpdate` 子命令；分发）
- Modify: `src/commands/install.rs`（无参交互分发 + self-update 路由）
- Modify: `src/commands/mod.rs`（注册 `self_update` 模块）
- Create: `src/commands/self_update.rs`（命令入口）
- Create: `tests/cli_install.rs`（集成测试）

**Interfaces:**
- Consumes: `crate::core::interact::select`、`crate::core::interact::is_interactive`（均已存在）
- Consumes (Task 4): `crate::core::tools::self_update::run() -> Result<()>`
- Produces: `Command::Install { tool: Option<String> }`、`Command::SelfUpdate`
- Produces: `commands::install::run(tool: Option<String>)`——无参时 select 中文标签列表，映射下标到工具名再分发；`"self-update"` 字面量进入自更新
- Produces: `commands::self_update::run()`——仅转发 `tools::self_update::run()`

- [ ] **Step 1: 写失败集成测试**（tests/cli_install.rs）

```rust
use assert_cmd::Command;

#[test]
fn install_without_tool_reports_hint_when_non_tty() {
    // 测试进程 stdin 非 TTY（CI/管道环境），若本地为 TTY 则跳过
    if std::io::stdin().is_terminal() && std::io::stderr().is_terminal() {
        return;
    }
    Command::cargo_bin("cli")
        .unwrap()
        .arg("install")
        .assert()
        .failure()
        .stderr(predicates::prelude::predicate::str::contains("请指定工具名"));
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test --test cli_install`
Expected: FAIL（当前 clap 报 required arguments 错误）

- [ ] **Step 3: lib.rs 改造**

```rust
/// 交互式安装开发工具（无参数时弹出工具列表选择）
Install {
    /// 工具名（不填则交互选择）
    tool: Option<String>,
},
/// 自更新：检查并升级到 GitHub Releases 最新版
SelfUpdate,
```

`run()` 分发更新：

```rust
Command::Install { tool } => commands::install::run(tool),
Command::SelfUpdate => commands::self_update::run(),
```

- [ ] **Step 4: 实现 install.rs 交互分发**

```rust
use anyhow::{anyhow, Result};

use crate::core::interact::{is_interactive, select};
use crate::core::tools::{go, java, maven, node};

/// 交互列表：中文标签 -> 工具内部名
const TOOL_CHOICES: [(&str, &str); 5] = [
    ("Java", "java"),
    ("Node.js", "node"),
    ("Go", "go"),
    ("Maven", "maven"),
    ("自更新", "self-update"),
];

pub fn run(tool: Option<String>) -> Result<()> {
    let tool = match tool {
        Some(t) => t,
        None => {
            if !is_interactive() {
                return Err(anyhow!("请指定工具名，例如: cli install java"));
            }
            let labels: Vec<&str> = TOOL_CHOICES.iter().map(|(label, _)| *label).collect();
            let idx = select("请选择要安装的工具", &labels)?;
            TOOL_CHOICES[idx].1.to_string()
        }
    };
    match tool.as_str() {
        "java" => java::install(None, None),
        "node" => node::install(None),
        "go" => go::install(None),
        "maven" => maven::install(None),
        "self-update" => crate::core::tools::self_update::run(),
        _ => Err(anyhow!("暂不支持的安装目标: {tool}")),
    }
}
```

- [ ] **Step 5: 新增 self_update 命令入口与模块注册**

`src/commands/self_update.rs`：

```rust
use anyhow::Result;

pub fn run() -> Result<()> {
    crate::core::tools::self_update::run()
}
```

`src/commands/mod.rs`：追加 `pub mod self_update;`
`src/core/tools/mod.rs`：追加 `pub mod self_update;`（Task 4 实现，先建最小文件）

Task 4 完成前，`src/core/tools/self_update.rs` 先放最小实现：

```rust
use anyhow::{bail, Result};

pub fn run() -> Result<()> {
    bail!("自更新功能尚未实现")
}
```

- [ ] **Step 6: 运行测试确认通过**

Run: `cargo test --test cli_install && cargo test`
Expected: PASS（无参非 TTY 报"请指定工具名"；有参路径不变）

- [ ] **Step 7: 提交**

```bash
git add src/lib.rs src/commands/install.rs src/commands/mod.rs src/commands/self_update.rs src/core/tools/mod.rs src/core/tools/self_update.rs tests/cli_install.rs
git commit -m "feat: cli install 无参交互选择工具列表，新增 self-update 子命令"
```

---

### Task 4: self-update 核心实现

**Files:**
- Modify: `src/core/tools/self_update.rs`（完整实现）
- Test: 模块内测试

**Interfaces:**
- Consumes: `crate::core::versions::compare`、`crate::core::versions::parse_tag`、`crate::current_version()`
- Consumes: `crate::core::download::http_get_string`、`crate::core::download::download`（均已存在）
- Consumes: `crate::core::interact::confirm`、`crate::core::platform::Platform`
- Produces: `pub fn run() -> Result<()>`（完整更新流程）
- Produces: `pub fn asset_name(platform: &Platform) -> &'static str`（平台 → 资产名）
- Produces: `pub fn parse_latest_release(json: &str) -> Result<String>`（提取 tag_name）

- [ ] **Step 1: 写失败测试（纯函数）**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::platform::{Arch, Os, Platform};

    #[test]
    fn asset_name_maps_all_platforms() {
        let cases = [
            ((Os::Linux, Arch::X86_64), "cli-linux-x64"),
            ((Os::Linux, Arch::Aarch64), "cli-linux-arm64"),
            ((Os::MacOs, Arch::X86_64), "cli-macos-x64"),
            ((Os::MacOs, Arch::Aarch64), "cli-macos-arm64"),
            ((Os::Windows, Arch::X86_64), "cli-windows-x64.exe"),
        ];
        for ((os, arch), expected) in cases {
            let p = Platform { os, arch };
            assert_eq!(asset_name(&p), expected);
        }
    }

    #[test]
    fn parse_latest_release_extracts_tag() {
        let json = r#"{"tag_name": "v0.1.1", "name": "v0.1.1"}"#;
        assert_eq!(parse_latest_release(json).unwrap(), "v0.1.1");
        assert!(parse_latest_release("{}").is_err());
    }
}
```

（需确认 `Platform` 字段为 pub——见 `src/core/platform.rs` 结构定义，若 `os`/`arch` 非 pub 则用 `Platform::detect()` 无法构造测试平台，改用 `with_os_arch` 辅助或直接断言 os_name/arch_name 组合；现有 java.rs 测试已用 `Platform { os, arch }` 构造，字段必为 pub）

- [ ] **Step 2: 运行确认失败**

Run: `cargo test self_update`
Expected: FAIL（函数未定义）

- [ ] **Step 3: 实现纯函数与完整流程**

```rust
use anyhow::{anyhow, bail, Result};

use crate::core::download::{download, http_get_string};
use crate::core::interact::confirm;
use crate::core::platform::Platform;
use crate::core::versions::{compare, parse_tag};
use std::cmp::Ordering;

const REPO: &str = "zhouhailin/cli";
const LATEST_API: &str = "https://api.github.com/repos/zhouhailin/cli/releases/latest";

/// 平台 → Release 资产名（与 release.yml 命名一致）
pub fn asset_name(platform: &Platform) -> &'static str {
    match (platform.os, platform.arch) {
        (crate::core::platform::Os::Linux, crate::core::platform::Arch::X86_64) => "cli-linux-x64",
        (crate::core::platform::Os::Linux, crate::core::platform::Arch::Aarch64) => "cli-linux-arm64",
        (crate::core::platform::Os::MacOs, crate::core::platform::Arch::X86_64) => "cli-macos-x64",
        (crate::core::platform::Os::MacOs, crate::core::platform::Arch::Aarch64) => "cli-macos-arm64",
        (crate::core::platform::Os::Windows, crate::core::platform::Arch::X86_64) => {
            "cli-windows-x64.exe"
        }
    }
}

/// 从 GitHub latest release API 响应提取 tag_name
pub fn parse_latest_release(json: &str) -> Result<String> {
    #[derive(serde::Deserialize)]
    struct LatestRelease {
        tag_name: Option<String>,
    }
    let parsed: LatestRelease =
        serde_json::from_str(json).map_err(|e| anyhow!("解析最新版本信息失败: {e}"))?;
    parsed
        .tag_name
        .ok_or_else(|| anyhow!("最新版本响应缺少 tag_name"))
}

pub fn run() -> Result<()> {
    let current = crate::current_version();
    let body = http_get_string(LATEST_API).map_err(|e| anyhow!("检查更新失败（{e}）"))?;
    let latest_tag = parse_latest_release(&body)?;
    let latest = parse_tag(&latest_tag);
    if compare(latest, current) != Ordering::Greater {
        println!("已是最新版本 ({current})");
        return Ok(());
    }
    println!("当前版本: {current} → 最新版本: {latest}");
    if !confirm("确认下载更新？", true)? {
        println!("已取消");
        return Ok(());
    }
    let platform = Platform::detect();
    let asset = asset_name(&platform);
    let url = format!("https://github.com/{REPO}/releases/download/{latest_tag}/{asset}");
    println!("下载地址: {url}");
    let exe = std::env::current_exe()?;
    let staging = exe.with_extension("update");
    download(&url, &staging, None)?;
    // Unix 直接原子替换；Windows 运行中 exe 被锁，提示手动替换
    #[cfg(not(windows))]
    {
        std::fs::rename(&staging, &exe)?;
        println!("更新完成，当前版本: {latest}");
    }
    #[cfg(windows)]
    {
        let new_exe = exe.with_extension("new.exe");
        std::fs::rename(&staging, &new_exe)?;
        println!("已下载新版到 {}，请手动替换当前 cli.exe 后重新运行", new_exe.display());
    }
    Ok(())
}
```

注意：`download` 内部用 `dest.with_extension("part")` 写临时文件，故 staging 用 `with_extension("update")` 避免与 `.part` 冲突；Unix 下 `rename` 原子覆盖运行中二进制可行。

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test self_update && cargo test`
Expected: PASS（91 + 2 新增全绿）

- [ ] **Step 5: 手动验证提示路径（可选，TTY 下）**

Run: `CLI_DEBUG=true cargo run -q -- self-update`
Expected: 显示当前版本 0.1.0 → 最新 0.1.1，等待确认（Ctrl-C 取消即可，不实际替换本地开发二进制）

- [ ] **Step 6: 提交**

```bash
git add src/core/tools/self_update.rs
git commit -m "feat: 实现 self-update（检查 GitHub Releases 最新版并替换自身）"
```

---

### Task 5: release.yml 注入 CLI_VERSION

**Files:**
- Modify: `.github/workflows/release.yml`（build job）

- [ ] **Step 1: build job 增加环境变量**

在 `runs-on: ${{ matrix.os }}` 之后、现有 `env:`（musl linker）合并：

```yaml
    runs-on: ${{ matrix.os }}
    env:
      CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER: musl-gcc
      CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER: musl-gcc
      CLI_VERSION: ${{ github.ref_name }}   # v0.1.1 -> 版本号与 tag 同步
```

（`github.ref_name` 为 tag 名如 `v0.1.1`；代码内 `current_version()` 自动去 `v`）

- [ ] **Step 2: 提交**

```bash
git add .github/workflows/release.yml
git commit -m "ci: release 构建注入 CLI_VERSION，二进制版本号与 tag 同步"
```

---

### Task 6: README 更新

**Files:**
- Modify: `README.md`

- [ ] **Step 1: 更新命令与配置文档**

- 命令详解 `cli install` 段：补充"不带参数时交互弹出工具列表（Java/Node.js/Go/Maven/自更新）"
- 新增 `cli self-update` 段：检查 GitHub Releases 最新版并自动替换自身
- 环境变量表补充：

```markdown
| `DEVKIT_CACHE_DIR` | 压缩包缓存目录（默认 `<根目录>/cache`），便于离线分发 |
```

- [ ] **Step 2: 验证 + 提交**

Run: `cargo fmt --check`
Expected: clean

```bash
git add README.md
git commit -m "docs: README 补充无参交互、self-update 与 DEVKIT_CACHE_DIR"
```

---

### Task 7: 全量验证与发布

- [ ] **Step 1: 全量本地验证**

Run: `cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Expected: 全绿

- [ ] **Step 2: 推送并等 CI 绿**

```bash
git push origin main
```
Run 后查询：`curl -s "https://api.github.com/repos/zhouhailin/cli/actions/runs?per_page=1"` 直到 CI conclusion = success

- [ ] **Step 3: 打 tag 验证版本同步**

```bash
git tag v0.1.2 && git push origin v0.1.2
```
Release 完成后下载 linux-x64 资产，运行 `./cli-linux-x64 version`，确认输出 `cli 0.1.2`（与 tag 同步）

- [ ] **Step 4: 提交收尾（无改动则跳过）**

```bash
git status --short
```
