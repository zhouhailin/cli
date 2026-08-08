# CLI UX 增强与 mvnd 工具链实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 完成 6 项 CLI 增强：无参数显示版本、生效命令提示、use 无参交互、update 权限防御、mvnd 工具链、Go 阿里云镜像。

**Architecture:** 全部改动落在现有模块内：lib.rs（clap 命令面）、core/shell.rs（提示函数）、core/versions.rs（通用版本解析）、core/tools/{self_update,mvnd,go,maven}.rs（工具逻辑）、release.yml（打包权限）。每任务独立 TDD 循环 + 提交。

**Tech Stack:** Rust + clap 4 + dialoguer；GitHub Actions；assert_cmd/predicates 集成测试。

## Global Constraints

- 中文注释与提示文案（与现有代码一致）
- 每个任务结束必须：全量 `cargo test` 全绿 + `cargo clippy --all-targets -- -D warnings` 零告警 + `cargo fmt --check` 通过
- 集成测试平台断言用 `cfg!` 动态构造，禁止写死平台字符串
- mvnd 平台映射：os=linux/darwin/windows、arch=amd64/aarch64（与 cli 内部 x86_64/aarch64 不同）
- mvnd 只提供纯数字点分稳定版（过滤 rc/beta/milestone）
- `cli update` 命令名已定（不保留 self-update 别名）
- 提交信息遵循现有风格：`feat:`/`fix:`/`docs:`/`refactor:` 前缀 + 中文描述

---

### Task 1: 无参数运行显示版本

**Files:**
- Modify: `src/lib.rs:13-18`（clap 属性）
- Test: `tests/cli_version.rs`（追加测试）

**Interfaces:**
- Consumes: 现有 `current_version()`（lib.rs）
- Produces: 无（纯行为变更）

- [ ] **Step 1: 手动确认 clap 无参数行为**

```bash
cd /opt/work/demo/cli && cargo build -q && ./target/debug/cli; echo "exit=$?"
```

预期：报错 `requires a subcommand`（exit 2）。当前版本号只在 `-V` 与 `version` 子命令可见。

- [ ] **Step 2: 写失败测试**

在 `tests/cli_version.rs` 追加：

```rust
#[test]
fn no_args_prints_version_in_help() {
    Command::cargo_bin("cli")
        .unwrap()
        .assert()
        .failure()
        .stdout(predicate::str::contains("版本: 0.1.0"));
}
```

- [ ] **Step 3: 运行测试确认失败**

Run: `cargo test --test cli_version no_args_prints_version_in_help -q`
Expected: FAIL（stdout 无"版本:"）

- [ ] **Step 4: 实现**

`src/lib.rs` 的 clap 定义改为（追加 `arg_required_else_help` + 自定义 `help_template`）：

```rust
#[derive(Parser)]
#[command(
    name = "cli",
    version = current_version(),
    about = "跨平台开发环境一键安装工具",
    arg_required_else_help = true,
    help_template = "{about-with-newline}\n版本: {version}\n\n{usage-heading} {usage}\n\n{all-args}{after-help}"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}
```

- [ ] **Step 5: 运行测试确认通过**

```bash
cargo test --test cli_version -q
```

若 Step 2 断言失败提示"stdout 无版本"，而实际输出在 stderr，将断言改为 `.stderr(predicate::str::contains("版本: 0.1.0"))` 后重跑（先运行 `./target/debug/cli 1>/tmp/o 2>/tmp/e; grep -c 版本 /tmp/o /tmp/e` 确认输出流）。

Expected: PASS（含版本行）

- [ ] **Step 6: 全量回归 + 提交**

```bash
cargo test -q 2>&1 | grep "test result" | awk -F'[ ;]' '{s+=$4} END {print "TOTAL:", s}'
cargo clippy --all-targets -- -D warnings 2>&1 | tail -1
cargo fmt --check
git add src/lib.rs tests/cli_version.rs
git commit -m "feat: 无参数运行 cli 时帮助信息显示版本号"
```

### Task 2: 生效命令提示

**Files:**
- Modify: `src/core/shell.rs`（新增 `print_activation_hint`）
- Modify: `src/commands/use_cmd.rs:39`（替换固定文案）
- Modify: `src/core/installer.rs:67-73`（inject 分支）
- Modify: `src/core/tools/node.rs:110`、`src/core/tools/maven.rs:93`、`src/core/tools/go.rs:104`、`src/core/tools/java.rs`（注入后调用）
- Create: `tests/cli_use.rs`

**Interfaces:**
- Produces: `pub fn print_activation_hint() -> Result<()>`（shell.rs，打印含 `source <rc>` 的提示行；rc 检测失败时降级提示）

- [ ] **Step 1: 写失败测试**

创建 `tests/cli_use.rs`：

```rust
use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::tempdir;

#[test]
fn use_switches_version_and_prints_source_hint() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("java/21/bin")).unwrap();
    fs::write(
        dir.path().join("config.json"),
        r#"{"installed":{"java":["21"]},"active":{"java":"21"}}"#,
    )
    .unwrap();
    Command::cargo_bin("cli")
        .unwrap()
        .env("DEVKIT_ROOT", dir.path())
        .args(["use", "java", "21"])
        .assert()
        .success()
        .stdout(predicate::str::contains("已切换到 java 21"))
        .stdout(predicate::str::contains("source"));
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test --test cli_use -q`
Expected: FAIL（当前提示文案为"新终端或 source 当前 shell 配置文件后生效"，不含具体 `source` 命令——断言 `contains("source")` 实际会过！需先跑一次确认）。若 PASS，临时将断言改为 `contains("source .")`（要求带路径）确保 RED。

- [ ] **Step 3: 实现 print_activation_hint**

`src/core/shell.rs` 的 `rc_file_for_shell` 之后新增：

```rust
/// 打印激活提示：新终端或执行 source 当前 shell 配置文件后生效
pub fn print_activation_hint() -> Result<()> {
    match rc_file_for_shell() {
        Ok(rc) => {
            println!("提示: 新终端或执行 source {} 后生效", rc.display());
            Ok(())
        }
        Err(e) => {
            println!("提示: 新终端后生效（{e}）");
            Ok(())
        }
    }
}
```

- [ ] **Step 4: 替换/新增调用点**

`use_cmd.rs:39`：

```rust
    println!("已切换到 {tool} {version}");
    crate::core::shell::print_activation_hint()?;
```

`installer.rs` inject 分支（第 67-73 行区域）：

```rust
    if inject {
        let rc_file = rc_file_for_shell()?;
        let link = ctx.paths.current_link(tool);
        inject_path(&rc_file, &link.join("bin"))?;
        debug_log!("已注入 PATH: {}", rc_file.display());
        crate::core::shell::print_activation_hint()?;
    }
```

`node.rs:108-111`（inject_path 之后）：

```rust
    let rc_file = rc_file_for_shell()?;
    inject_path(&rc_file, &ctx.paths.current_link("node").join("bin"))?;
    crate::core::shell::print_activation_hint()?;
    println!("Node.js {version} 安装完成");
```

`maven.rs:92-94`（inject_path 之后）：

```rust
    let rc_file = crate::core::shell::rc_file_for_shell()?;
    crate::core::shell::inject_path(&rc_file, &ctx.paths.current_link("maven").join("bin"))?;
    crate::core::shell::print_activation_hint()?;
    println!("Maven {version} 安装完成（已配置阿里云镜像与本地仓库）");
```

`go.rs:103-105`（inject_path 之后）：

```rust
    let rc_file = rc_file_for_shell()?;
    inject_path(&rc_file, &ctx.paths.current_link("go").join("bin"))?;
    crate::core::shell::print_activation_hint()?;
    println!("Go {version} 安装完成");
```

`java.rs`：找到 JAVA_HOME 与 PATH 注入完成的最后一行（`inject_path` 或 `inject_env_var` 之后、`println!` 安装完成之前），追加：

```rust
    crate::core::shell::print_activation_hint()?;
```

- [ ] **Step 5: 运行测试确认通过**

Run: `cargo test --test cli_use -q`
Expected: PASS（stdout 含"已切换到 java 21"与"source <路径>"）

- [ ] **Step 6: 全量回归 + 提交**

```bash
cargo test -q 2>&1 | grep "test result" | awk -F'[ ;]' '{s+=$4} END {print "TOTAL:", s}'
cargo clippy --all-targets -- -D warnings 2>&1 | tail -1
cargo fmt --check
git add src/core/shell.rs src/commands/use_cmd.rs src/core/installer.rs src/core/tools/node.rs src/core/tools/maven.rs src/core/tools/go.rs src/core/tools/java.rs tests/cli_use.rs
git commit -m "feat: 安装/切换后提示具体生效命令（source <rc 文件>）"
```

### Task 3: cli use 无参数交互

**Files:**
- Modify: `src/lib.rs:31-37`（Use 枚举 tool 改 Option）
- Modify: `src/commands/use_cmd.rs:9-17`（无参交互选择）
- Test: `tests/cli_use.rs`（追加）

**Interfaces:**
- Consumes: `Config::load` 的 `installed: HashMap<String, Vec<String>>`（config.rs）、`interact::is_interactive/select`
- Produces: `use_cmd::run(tool: Option<String>, version: Option<String>)`

- [ ] **Step 1: 写失败测试**

`tests/cli_use.rs` 追加：

```rust
#[test]
fn use_without_tool_prompts_in_non_tty() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("config.json"),
        r#"{"installed":{},"active":{}}"#,
    )
    .unwrap();
    Command::cargo_bin("cli")
        .unwrap()
        .env("DEVKIT_ROOT", dir.path())
        .arg("use")
        .assert()
        .failure()
        .stderr(predicate::str::contains("请指定工具名"));
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test --test cli_use use_without_tool -q`
Expected: FAIL（clap 报"requires a subcommand"或"unexpected argument"，非"请指定工具名"）

- [ ] **Step 3: 实现**

`src/lib.rs`：

```rust
    /// 切换工具激活版本
    Use {
        /// 工具名（不填则交互选择已安装工具）
        tool: Option<String>,
        /// 目标版本（不填则交互选择）
        version: Option<String>,
    },
```

`src/commands/use_cmd.rs` 开头（新增 import 后）改为：

```rust
use crate::core::config::Config;
use crate::core::interact::{is_interactive, select};
use crate::core::links::set_current_link;
use crate::core::paths::DevkitPaths;

pub fn run(tool: Option<String>, version: Option<String>) -> Result<()> {
    let paths = DevkitPaths::new()?;
    let mut config = Config::load(&paths)?;
    let tool = match tool {
        Some(t) => t,
        None => {
            if !is_interactive() {
                return Err(anyhow!("请指定工具名，例如: cli use java"));
            }
            let mut installed_tools: Vec<String> = config.installed.keys().cloned().collect();
            installed_tools.sort();
            if installed_tools.is_empty() {
                return Err(anyhow!("尚未安装任何工具，请先执行 cli install"));
            }
            let labels: Vec<String> = installed_tools.iter().map(|t| t.to_string()).collect();
            let idx = select("请选择要切换的工具", &labels)?;
            installed_tools[idx].clone()
        }
    };
    let installed = config.installed.get(&tool).cloned().unwrap_or_default();
    // ... 后续版本选择/链接/激活逻辑保持不变 ...
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test --test cli_use -q`
Expected: PASS（use_without_tool 与 use_switches 均过）

- [ ] **Step 5: 手动验证交互路径**

```bash
cargo build -q && printf '\n' | script -q /dev/null env DEVKIT_ROOT=/tmp/use-e2e ./target/debug/cli use
```

用真实 TTY 观察工具选择交互（临时构造 config 后可省略，非阻塞）。

- [ ] **Step 6: 全量回归 + 提交**

```bash
cargo test -q 2>&1 | grep "test result" | awk -F'[ ;]' '{s+=$4} END {print "TOTAL:", s}'
cargo clippy --all-targets -- -D warnings 2>&1 | tail -1
cargo fmt --check
git add src/lib.rs src/commands/use_cmd.rs tests/cli_use.rs
git commit -m "feat: cli use 无参数时交互选择已安装工具与版本"
```

### Task 4: update 权限防御 + 打包执行位

**Files:**
- Modify: `src/core/tools/self_update.rs:84-94`（replace_binary 重写）
- Modify: `.github/workflows/release.yml`（chmod 步骤）
- Test: `src/core/tools/self_update.rs` tests 模块

**Interfaces:**
- Consumes: 现有 `replace_binary(staging, exe)` 签名不变
- Produces: 重写后的 `replace_binary`（原 exe 无执行位时 0755 兜底 + 替换后自检）

- [ ] **Step 1: 写失败测试（RED）**

`src/core/tools/self_update.rs` tests 模块追加：

```rust
    #[cfg(unix)]
    #[test]
    fn replace_binary_ensures_execute_when_original_missing_exec() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("cli");
        let staging = dir.path().join("cli.update");
        std::fs::write(&exe, b"old").unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o644)).unwrap();
        std::fs::write(&staging, b"new").unwrap();
        std::fs::set_permissions(&staging, std::fs::Permissions::from_mode(0o644)).unwrap();
        replace_binary(&staging, &exe).unwrap();
        let mode = std::fs::metadata(&exe).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o755);
    }
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test --lib replace_binary_ensures_execute_when_original_missing_exec -q`
Expected: FAIL（当前实现对齐 0644，断言 `0o755` 不匹配）

- [ ] **Step 3: 重写 replace_binary**

```rust
/// Unix 原子替换：staging 权限继承原 exe；原 exe 无执行位或不可读时 0755 兜底，替换后自检补位
/// （HTTP 下载默认 0644，直接 rename 会丢失执行位导致更新后无法运行）
#[cfg(not(windows))]
pub fn replace_binary(staging: &std::path::Path, exe: &std::path::Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mode = match std::fs::metadata(exe) {
        Ok(meta) => {
            let bits = meta.permissions().mode() & 0o777;
            if bits & 0o111 == 0 { 0o755 } else { bits }
        }
        Err(_) => 0o755,
    };
    std::fs::set_permissions(staging, std::fs::Permissions::from_mode(mode))
        .map_err(|e| anyhow!("设置临时文件权限失败: {e}"))?;
    std::fs::rename(staging, exe).map_err(|e| anyhow!("替换二进制失败: {e}"))?;
    // 替换后自检：仍无执行位则补（防御边界场景）
    let after = std::fs::metadata(exe)?.permissions().mode() & 0o777;
    if after & 0o111 == 0 {
        std::fs::set_permissions(exe, std::fs::Permissions::from_mode(after | 0o111))?;
    }
    crate::debug_log!("更新后二进制权限: {mode:o}");
    Ok(())
}
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test --lib replace_binary -q`
Expected: PASS（3 个测试：0755 继承、0700 保留、0644 兜底）

- [ ] **Step 5: release.yml 加打包执行位**

`.github/workflows/release.yml` 的"重命名为带平台后缀"步骤之后、"Alpine 冒烟验证"之前插入：

```yaml
      - name: 设置可执行权限
        if: matrix.strip
        run: chmod +x ${{ matrix.asset }}
```

（Windows 矩阵 `strip: false` 自动跳过）

- [ ] **Step 6: 全量回归 + 提交**

```bash
cargo test -q 2>&1 | grep "test result" | awk -F'[ ;]' '{s+=$4} END {print "TOTAL:", s}'
cargo clippy --all-targets -- -D warnings 2>&1 | tail -1
cargo fmt --check
git add src/core/tools/self_update.rs .github/workflows/release.yml
git commit -m "fix: update 替换二进制强制保证执行位（原二进制无执行位时 0755 兜底）；打包产物设置执行权限"
```

### Task 5: parse_version_dirs 通用解析提取

**Files:**
- Modify: `src/core/versions.rs`（新增 `parse_version_dirs`）
- Modify: `src/core/tools/maven.rs:8-29`（parse_maven_versions 改为委托）
- Test: `src/core/versions.rs` tests 模块

**Interfaces:**
- Produces: `pub fn parse_version_dirs(html: &str) -> Result<Vec<String>>`（versions.rs；纯数字点分目录名降序；空结果报"未解析到任何版本"）
- Consumes: 现有 `compare(a, b)`

- [ ] **Step 1: 写失败测试**

`src/core/versions.rs` tests 模块追加：

```rust
    #[test]
    fn parse_version_dirs_filters_non_numeric_and_sorts_desc() {
        let html = r#"<html><body>
          <a href="1.0.6/">1.0.6/</a>
          <a href="2.0.0-rc-3/">2.0.0-rc-3/</a>
          <a href="1.0-m6/">1.0-m6/</a>
          <a href="1.0.5/">1.0.5/</a>
          <a href="README.html">README</a>
        </body></html>"#;
        let list = parse_version_dirs(html).unwrap();
        assert_eq!(list, vec!["1.0.6", "1.0.5"]);
    }

    #[test]
    fn parse_version_dirs_rejects_empty() {
        assert!(parse_version_dirs("<html>no links</html>").is_err());
    }
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test --lib parse_version_dirs -q`
Expected: FAIL（函数未定义，编译错误）

- [ ] **Step 3: 实现**

`src/core/versions.rs` 顶部加 `use anyhow::{anyhow, Result};`，文件末尾（tests 模块之前）新增：

```rust
/// 从目录页 HTML 提取纯数字点分版本目录名（3.9.9、1.0.6），降序；过滤 rc/beta/milestone 等非纯数字段
pub fn parse_version_dirs(html: &str) -> Result<Vec<String>> {
    let mut versions: Vec<String> = Vec::new();
    let mut rest = html;
    while let Some(start) = rest.find(r#"<a href=""#) {
        let after = &rest[start + 9..];
        let Some(end) = after.find(r#"">"#) else {
            break;
        };
        let href = &after[..end];
        if let Some(dir) = href.strip_suffix('/') {
            if !dir.is_empty() && dir.chars().all(|c| c.is_ascii_digit() || c == '.') {
                versions.push(dir.to_string());
            }
        }
        rest = &after[end..];
    }
    if versions.is_empty() {
        return Err(anyhow!("未解析到任何版本"));
    }
    versions.sort_by(|a, b| compare(b, a));
    Ok(versions)
}
```

`src/core/tools/maven.rs` 的 `parse_maven_versions` 函数体替换为委托（保留 pub 签名与现有测试）：

```rust
/// 从 Apache archive 目录页 HTML 提取版本号（纯数字点分），降序
pub fn parse_maven_versions(html: &str) -> Result<Vec<String>> {
    crate::core::versions::parse_version_dirs(html)
}
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test --lib -q 2>&1 | grep -E "parse_version_dirs|parse_maven_versions|test result"`
Expected: PASS（versions 2 新测试 + maven 现有解析测试）

- [ ] **Step 5: 全量回归 + 提交**

```bash
cargo test -q 2>&1 | grep "test result" | awk -F'[ ;]' '{s+=$4} END {print "TOTAL:", s}'
cargo clippy --all-targets -- -D warnings 2>&1 | tail -1
cargo fmt --check
git add src/core/versions.rs src/core/tools/maven.rs
git commit -m "refactor: 提取通用版本目录解析 parse_version_dirs 到 core/versions.rs（maven/mvnd 复用）"
```

### Task 6: mvnd 工具链

**Files:**
- Create: `src/core/tools/mvnd.rs`
- Modify: `src/core/tools/mod.rs`（注册）
- Modify: `src/commands/install.rs`（TOOL_CHOICES + match）
- Modify: `README.md`（工具列表）
- Test: `src/core/tools/mvnd.rs` tests 模块

**Interfaces:**
- Consumes: `crate::core::versions::parse_version_dirs`（Task 5）、`install_archive(url, sha256, tool, version, ctx, inject)`、`print_activation_hint`（Task 2，inject=true 时由 install_archive 内部调用）
- Produces: `pub fn parse_sha256_text(text: &str) -> Result<String>`、`pub fn resolve_url(version: &str, platform: &Platform) -> String`、`pub fn install(version_hint: Option<&str>) -> Result<()>`

- [ ] **Step 1: 写失败测试**

创建 `src/core/tools/mvnd.rs`（先只含测试模块，函数未定义→编译失败即 RED）：

```rust
use anyhow::{anyhow, Result};

use crate::core::platform::Platform;

/// 从 .sha256 文本提取 64 位十六进制哈希（剥离文件名与换行）
pub fn parse_sha256_text(text: &str) -> Result<String> {
    text.split_whitespace()
        .find(|t| t.len() == 64 && t.chars().all(|c| c.is_ascii_hexdigit()))
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow!("无法从校验和文本解析 SHA-256: {text:?}"))
}

/// mvnd 下载 URL（archive 资产命名：maven-mvnd-<ver>-<os>-<arch>.tar.gz）
pub fn resolve_url(version: &str, platform: &Platform) -> String {
    let os = match platform.os {
        crate::core::platform::Os::MacOs => "darwin",
        crate::core::platform::Os::Linux => "linux",
        crate::core::platform::Os::Windows => "windows",
    };
    let arch = match platform.arch {
        crate::core::platform::Arch::X86_64 => "amd64",
        crate::core::platform::Arch::Aarch64 => "aarch64",
    };
    format!(
        "https://mirrors.aliyun.com/apache/maven/mvnd/{version}/maven-mvnd-{version}-{os}-{arch}.tar.gz"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::platform::{Arch, Os};

    #[test]
    fn parse_sha256_text_extracts_hash() {
        let hash = "a".repeat(64);
        assert_eq!(
            parse_sha256_text(&format!("{hash}  maven-mvnd-1.0.6-linux-amd64.tar.gz\n")).unwrap(),
            hash
        );
        assert!(parse_sha256_text("short").is_err());
    }

    #[test]
    fn resolve_url_maps_platforms() {
        let linux_x64 = Platform { os: Os::Linux, arch: Arch::X86_64 };
        assert_eq!(
            resolve_url("1.0.6", &linux_x64),
            "https://mirrors.aliyun.com/apache/maven/mvnd/1.0.6/maven-mvnd-1.0.6-linux-amd64.tar.gz"
        );
        let mac_arm = Platform { os: Os::MacOs, arch: Arch::Aarch64 };
        assert_eq!(
            resolve_url("1.0.6", &mac_arm),
            "https://mirrors.aliyun.com/apache/maven/mvnd/1.0.6/maven-mvnd-1.0.6-darwin-aarch64.tar.gz"
        );
        let win_x64 = Platform { os: Os::Windows, arch: Arch::X86_64 };
        assert_eq!(
            resolve_url("1.0.6", &win_x64),
            "https://mirrors.aliyun.com/apache/maven/mvnd/1.0.6/maven-mvnd-1.0.6-windows-amd64.tar.gz"
        );
    }
}
```

- [ ] **Step 2: 注册模块并运行测试**

`src/core/tools/mod.rs` 加 `pub mod mvnd;`。

Run: `cargo test --lib mvnd -q`
Expected: PASS（parse_sha256_text / resolve_url）

- [ ] **Step 3: 实现 install 流程**

`mvnd.rs` 顶部 import 补充并新增：

```rust
use crate::core::download::http_get_string;
use crate::core::installer::{install_archive, InstallContext};
use crate::core::interact::{confirm, select};

pub fn install(version_hint: Option<&str>) -> Result<()> {
    let platform = Platform::detect();
    let body = http_get_string("https://mirrors.aliyun.com/apache/maven/mvnd/")?;
    let list = crate::core::versions::parse_version_dirs(&body)?;
    let version = if let Some(hint) = version_hint {
        if !list.contains(&hint.to_string()) {
            return Err(anyhow!("版本 {hint} 不可用，请从列表中选择"));
        }
        hint.to_string()
    } else {
        let labels: Vec<String> = list
            .iter()
            .map(|v| format!("Maven Daemon (mvnd) {v}"))
            .collect();
        let idx = select("请选择 mvnd 版本", &labels)?;
        list[idx].clone()
    };
    let url = resolve_url(&version, &platform);
    // archive 提供 .sha256 侧车文件，先取哈希再下载校验
    let sha_text = http_get_string(&format!("{url}.sha256"))?;
    let sha = parse_sha256_text(&sha_text)?;
    println!("准备安装 mvnd {version}...");
    println!("下载地址: {url}");
    if !confirm("确认开始下载安装？", true)? {
        println!("已取消");
        return Ok(());
    }
    let mut ctx = InstallContext::load()?;
    install_archive(&url, Some(&sha), "mvnd", &version, &mut ctx, true)?;
    if !ctx.config.active.contains_key("java") {
        println!("提示: mvnd 依赖 Java，请先执行 cli install java");
    }
    println!("mvnd {version} 安装完成");
    Ok(())
}
```

- [ ] **Step 4: 接入命令面**

`src/commands/install.rs`：

```rust
const TOOL_CHOICES: [(&str, &str); 6] = [
    ("Java", "java"),
    ("Node.js", "node"),
    ("Go", "go"),
    ("Maven", "maven"),
    ("Maven Daemon (mvnd)", "mvnd"),
    ("自更新", "update"),
];
```

match 分支加：

```rust
        "mvnd" => crate::core::tools::mvnd::install(None),
```

- [ ] **Step 5: README 同步**

`README.md` 工具列表（`cli install` 交互列表描述处）的"Java / Node.js / Go / Maven / 自更新"改为"Java / Node.js / Go / Maven / Maven Daemon (mvnd) / 自更新"（共 2 处：约 44 行与 68 行）。

- [ ] **Step 6: 全量回归 + 提交**

```bash
cargo test -q 2>&1 | grep "test result" | awk -F'[ ;]' '{s+=$4} END {print "TOTAL:", s}'
cargo clippy --all-targets -- -D warnings 2>&1 | tail -1
cargo fmt --check
git add src/core/tools/mvnd.rs src/core/tools/mod.rs src/commands/install.rs README.md
git commit -m "feat: 新增 mvnd 工具链（稳定版列表 + sha256 校验 + Java 依赖提示）"
```

### Task 7: Go 下载源换阿里云

**Files:**
- Modify: `src/core/tools/go.rs:56-68`（resolve_url）
- Test: `src/core/tools/go.rs` tests 模块

**Interfaces:**
- Consumes: 现有 `resolve_url(version, platform)` 签名不变
- Produces: 阿里云 URL

- [ ] **Step 1: 更新测试期望（RED）**

`src/core/tools/go.rs` tests 模块：

```rust
    #[test]
    fn resolve_url_macos_arm64() {
        assert_eq!(
            resolve_url("1.22.6", &mac_arm()),
            "https://mirrors.aliyun.com/golang/go1.22.6.darwin-arm64.tar.gz"
        );
    }
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test --lib resolve_url_macos_arm64 -q`
Expected: FAIL（当前返回 go.dev URL）

- [ ] **Step 3: 实现**

```rust
/// go 下载 URL：阿里云镜像（国内加速）https://mirrors.aliyun.com/golang/go<version>.<os>-<arch>.tar.gz
pub fn resolve_url(version: &str, platform: &Platform) -> String {
    let os = match platform.os {
        crate::core::platform::Os::MacOs => "darwin",
        crate::core::platform::Os::Linux => "linux",
        crate::core::platform::Os::Windows => "windows",
    };
    let arch = match platform.arch {
        crate::core::platform::Arch::X86_64 => "amd64",
        crate::core::platform::Arch::Aarch64 => "arm64",
    };
    format!("https://mirrors.aliyun.com/golang/go{version}.{os}-{arch}.tar.gz")
}
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test --lib resolve_url -q`
Expected: PASS

- [ ] **Step 5: 全量回归 + 提交**

```bash
cargo test -q 2>&1 | grep "test result" | awk -F'[ ;]' '{s+=$4} END {print "TOTAL:", s}'
cargo clippy --all-targets -- -D warnings 2>&1 | tail -1
cargo fmt --check
git add src/core/tools/go.rs
git commit -m "feat: Go 下载源切换为阿里云镜像（版本列表保持 go.dev 官方）"
```

---

## 计划自审记录

- **Spec 覆盖**：需求 1→Task 1；需求 2→Task 2；需求 3→Task 3；需求 4→Task 4（含 release.yml 打包权限）；需求 5→Task 5+6；需求 6→Task 7。spec 的"假设与边界"（mvnd 稳定版过滤、Java 依赖提示、curl 下载权限不在范围）均已落实。
- **Placeholder 扫描**：所有步骤含完整代码与命令，无 TBD/TODO。
- **类型一致性**：`parse_version_dirs`（Task 5 产出）在 Task 6 的 mvnd.rs 与 Task 5 内使用签名一致；`replace_binary` 签名不变；`use_cmd::run` 的 `Option<String>` 与 lib.rs 枚举同步。
