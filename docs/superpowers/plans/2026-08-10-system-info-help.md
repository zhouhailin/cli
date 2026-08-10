# help 展示本机系统信息 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `cli` 无参数 / `-h` / `--help` / `help` 时，在 help 的「版本」行下方新增一行「系统」信息（系统名称+版本，含麒麟 SP/代号；CPU 架构）。

**Architecture:** 新增 core/system_info.rs（解析纯函数 + 平台检测入口 os_display/help_line，检测失败逐级回退）；lib.rs 提供 render_top_help()（渲染 clap help 后在「版本」行后注入系统行）与 wants_top_level_help()（拦截判定）；main.rs 薄拦截输出（无参数 stderr+exit(2)，其余 stdout+exit(0)）。

**Tech Stack:** Rust + clap 4 derive + serde + anyhow + assert_cmd + predicates + tempfile + serial_test（与项目现有栈一致）。

## Global Constraints

- 系统信息行格式：`系统: {os_display} ({arch})`，arch 必须复用 `Platform::detect().arch_name()`（x86_64 / aarch64）
- 麒麟判定：存在 `/usr/bin/nkvers` 即麒麟；麒麟优先读 `/etc/.productinfo`（ProductName + ProductVersion + ProductVersionInfo 中 `(...)` 项按顺序去重拼接），失败回退 `Kylin Linux`
- 非麒麟 Linux：`/etc/os-release` 解析 NAME + VERSION_ID（去引号），失败回退 `Linux`
- macOS：`sw_vers` 解析 ProductName + ProductVersion，失败回退 `macOS`
- Windows：`cmd /c ver` 解析 `[Version x.y.z...]` 或 `[版本 x.y.z...]`，取前 3 段，失败回退 `Windows`
- 所有检测永不 panic、永不因系统信息失败阻断 help 输出
- 无参数 → stderr + exit(2)；`-h`/`--help`/`help` → stdout + exit(0)；`cli help <子命令>` 不拦截
- 集成测试平台断言使用 `cfg!` 动态构造，禁止硬编码平台字符串
- 门禁：`cargo test` 全绿、`cargo clippy --all-targets -- -D warnings` 0 错误、`cargo fmt --check` 通过

---

### Task 1: core/system_info.rs 检测模块

**Files:**
- Create: `src/core/system_info.rs`
- Modify: `src/core/mod.rs`（注册 `pub mod system_info;`，字母序位于 shell 之后、tools 之前）

**Interfaces:**
- Consumes: `crate::core::platform::Platform`（已有 `Platform::detect()`、`arch_name()`）
- Produces: `pub fn is_kylin(nkvers_path: &Path) -> bool`；`pub fn parse_productinfo(content: &str) -> String`；`pub fn parse_os_release(content: &str) -> String`；`pub fn parse_sw_vers(output: &str) -> String`；`pub fn parse_ver_output(output: &str) -> String`；`pub fn os_display() -> String`；`pub fn help_line() -> String`（Task 2 依赖 help_line）

- [ ] **Step 1: 写失败测试（单测 + 注册模块）**

在 `src/core/system_info.rs` 写入测试与空实现占位（`fn is_kylin...` 等均返回空值，使编译通过但断言失败）：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn is_kylin_detects_nkvers_file() {
        let dir = tempdir().unwrap();
        let nkvers = dir.path().join("nkvers");
        assert!(!is_kylin(&nkvers));
        std::fs::write(&nkvers, b"#!/bin/sh\n").unwrap();
        assert!(is_kylin(&nkvers));
    }

    #[test]
    fn parse_productinfo_full_with_sp_and_codename() {
        let content = "\
ProductName=Kylin Linux Advanced Server
ProductVersion=V10
ProductType=Server
ProductVersionInfo[0]=Kylin Linux Advanced Server V10
ProductVersionInfo[1]=(SP1)
ProductVersionInfo[2]=(Halberd)
ProductVersionInfo[3]=Kylin Linux Advanced Server V10
ProductVersionInfo[4]=(SP1)
ProductVersionInfo[5]=(Halberd)";
        assert_eq!(
            parse_productinfo(content),
            "Kylin Linux Advanced Server V10 (SP1) (Halberd)"
        );
    }

    #[test]
    fn parse_productinfo_missing_version_keeps_name_and_marks() {
        let content = "ProductName=Kylin Linux Desktop\nProductVersionInfo[1]=(SP1)";
        assert_eq!(parse_productinfo(content), "Kylin Linux Desktop (SP1)");
    }

    #[test]
    fn parse_productinfo_empty_returns_empty() {
        assert_eq!(parse_productinfo(""), "");
        assert_eq!(parse_productinfo("ProductType=Server\n"), "");
    }

    #[test]
    fn parse_os_release_quoted() {
        let content = "NAME=\"Ubuntu\"\nVERSION_ID=\"22.04\"\n";
        assert_eq!(parse_os_release(content), "Ubuntu 22.04");
    }

    #[test]
    fn parse_os_release_unquoted_and_missing_version() {
        assert_eq!(parse_os_release("NAME=CentOS Linux\n"), "CentOS Linux");
        assert_eq!(parse_os_release("VERSION_ID=7\n"), "");
    }

    #[test]
    fn parse_sw_vers_standard() {
        let output = "ProductName:\t\tmacOS\nProductVersion:\t\t15.5\nBuildVersion:\t\t24D60\n";
        assert_eq!(parse_sw_vers(output), "macOS 15.5");
    }

    #[test]
    fn parse_ver_output_english() {
        let output = "Microsoft Windows [Version 10.0.22631.4460]\n";
        assert_eq!(parse_ver_output(output), "Windows 10.0.22631");
    }

    #[test]
    fn parse_ver_output_chinese() {
        let output = "Microsoft Windows [版本 10.0.22631]\n";
        assert_eq!(parse_ver_output(output), "Windows 10.0.22631");
    }

    #[test]
    fn parse_ver_output_unmatched_returns_empty() {
        assert_eq!(parse_ver_output("not windows output"), "");
    }

    #[test]
    fn os_display_never_empty() {
        assert!(!os_display().is_empty());
    }

    #[test]
    fn help_line_contains_arch() {
        let line = help_line();
        assert!(line.starts_with("系统: "));
        assert!(
            line.contains("x86_64") || line.contains("aarch64"),
            "help_line 必须包含架构: {line}"
        );
    }
}
```

- [ ] **Step 2: 运行测试确认 RED**

Run: `cargo test --lib core::system_info 2>&1 | tail -15`
Expected: 断言失败（占位实现返回空值/常量）

- [ ] **Step 3: 最小实现**

在 `src/core/system_info.rs` 写入实现。**保留 Step 1 已写入的测试模块不变**，在测试模块上方加入以下全部函数：

```rust
//! 本机系统信息检测：help「系统」行展示用。检测失败逐级回退，永不 panic。

use std::path::Path;

use crate::core::platform::Platform;

/// 麒麟操作系统判定：存在 nkvers 命令即判定为麒麟
pub fn is_kylin(nkvers_path: &Path) -> bool {
    nkvers_path.exists()
}

/// 解析 /etc/.productinfo（key=value 格式）：
/// ProductName + ProductVersion 组合基础名；ProductVersionInfo[i] 中的 `(...)` 项（SP/代号）按顺序去重拼接
pub fn parse_productinfo(content: &str) -> String {
    let mut name: Option<String> = None;
    let mut version: Option<String> = None;
    let mut marks: Vec<String> = Vec::new();
    for line in content.lines() {
        let Some((k, v)) = line.split_once('=') else { continue };
        let v = v.trim().trim_matches('"').to_string();
        match k {
            "ProductName" => name = Some(v),
            "ProductVersion" => version = Some(v),
            _ if k.starts_with("ProductVersionInfo") => {
                let t = v.trim();
                if t.starts_with('(') && t.ends_with(')') && !marks.contains(&t) {
                    marks.push(t.to_string());
                }
            }
            _ => {}
        }
    }
    let Some(name) = name else { return String::new() };
    let mut out = name;
    if let Some(v) = version {
        out.push(' ');
        out.push_str(&v);
    }
    for m in marks {
        out.push(' ');
        out.push_str(&m);
    }
    out
}

/// 解析 /etc/os-release：NAME + VERSION_ID（去引号）；缺 VERSION_ID 只返回 NAME
pub fn parse_os_release(content: &str) -> String {
    let mut name: Option<String> = None;
    let mut version: Option<String> = None;
    for line in content.lines() {
        let Some((k, v)) = line.split_once('=') else { continue };
        let v = v.trim().trim_matches('"').to_string();
        match k {
            "NAME" => name = Some(v),
            "VERSION_ID" => version = Some(v),
            _ => {}
        }
    }
    match (name, version) {
        (Some(n), Some(v)) => format!("{n} {v}"),
        (Some(n), None) => n,
        (None, _) => String::new(),
    }
}

/// 解析 sw_vers 输出（ProductName: macOS / ProductVersion: 15.5）
pub fn parse_sw_vers(output: &str) -> String {
    let mut name: Option<String> = None;
    let mut version: Option<String> = None;
    for line in output.lines() {
        if let Some(v) = line.strip_prefix("ProductName:") {
            name = Some(v.trim().to_string());
        } else if let Some(v) = line.strip_prefix("ProductVersion:") {
            version = Some(v.trim().to_string());
        }
    }
    match (name, version) {
        (Some(n), Some(v)) => format!("{n} {v}"),
        (Some(n), None) => n,
        (None, _) => String::new(),
    }
}

/// 解析 Windows `ver` 输出（中文/英文），提取括号内版本号的前 3 段
pub fn parse_ver_output(output: &str) -> String {
    let Some(start) = output.find('[') else { return String::new() };
    let Some(end) = output.find(']') else { return String::new() };
    if end <= start + 1 {
        return String::new();
    }
    let inner = &output[start + 1..end];
    let Some(idx) = inner.find(|c: char| c.is_ascii_digit()) else { return String::new() };
    let parts: Vec<&str> = inner[idx..].split('.').take(3).collect();
    if parts.len() < 3 {
        return String::new();
    }
    format!("Windows {}", parts.join("."))
}

/// 系统名称+版本；检测失败回退基础名，永不 panic、永不为空
pub fn os_display() -> String {
    if cfg!(target_os = "macos") {
        macos_display()
    } else if cfg!(target_os = "linux") {
        linux_display()
    } else {
        windows_display()
    }
}

fn macos_display() -> String {
    match std::process::Command::new("sw_vers").output() {
        Ok(out) if out.status.success() => {
            let text = String::from_utf8_lossy(&out.stdout);
            let s = parse_sw_vers(&text);
            if s.is_empty() { "macOS".to_string() } else { s }
        }
        _ => "macOS".to_string(),
    }
}

fn linux_display() -> String {
    if is_kylin(Path::new("/usr/bin/nkvers")) {
        if let Ok(text) = std::fs::read_to_string("/etc/.productinfo") {
            let s = parse_productinfo(&text);
            if !s.is_empty() {
                return s;
            }
        }
        return "Kylin Linux".to_string();
    }
    if let Ok(text) = std::fs::read_to_string("/etc/os-release") {
        let s = parse_os_release(&text);
        if !s.is_empty() {
            return s;
        }
    }
    "Linux".to_string()
}

fn windows_display() -> String {
    #[cfg(windows)]
    let out = std::process::Command::new("cmd").args(["/c", "ver"]).output();
    #[cfg(not(windows))]
    let out = Err(std::io::Error::other("not windows"));
    match out {
        Ok(out) if out.status.success() => {
            let text = String::from_utf8_lossy(&out.stdout);
            let s = parse_ver_output(&text);
            if s.is_empty() { "Windows".to_string() } else { s }
        }
        _ => "Windows".to_string(),
    }
}

/// help「系统」行：`系统: {os_display} ({arch})`
pub fn help_line() -> String {
    format!("系统: {} ({})", os_display(), Platform::detect().arch_name())
}
```

- [ ] **Step 4: 运行测试确认 GREEN**

Run: `cargo test --lib core::system_info 2>&1 | tail -10`
Expected: 全部通过

- [ ] **Step 5: 注册模块并跑全量单测**

在 `src/core/mod.rs` 的 `pub mod shell;` 后插入 `pub mod system_info;`。

Run: `cargo test --lib 2>&1 | grep -E "test result|FAILED" | head -3`
Expected: 全绿，无 FAILED

- [ ] **Step 6: 提交**

```bash
git add src/core/system_info.rs src/core/mod.rs
git commit -m "feat: 本机系统信息检测 core/system_info.rs（含麒麟 productinfo）"
```

---

### Task 2: lib.rs help 渲染注入 + main.rs 拦截

**Files:**
- Modify: `src/lib.rs`（新增 `render_top_help()`、`inject_system_line()`、`wants_top_level_help()` + 单测）
- Modify: `src/main.rs`（拦截顶层 help 场景并输出）
- Test: `tests/cli_version.rs`（追加 3 个集成测试）

**Interfaces:**
- Consumes: Task 1 的 `crate::core::system_info::help_line()`；已有 `crate::current_version()`、`Cli::command()`（clap CommandFactory）
- Produces: `pub fn wants_top_level_help(args: &[String]) -> bool`（main.rs 调用）；`pub fn render_top_help() -> String`（main.rs 调用）

- [ ] **Step 1: 写失败测试（lib.rs 单测）**

在 `src/lib.rs` 的 `mod tests` 中追加：

```rust
    #[test]
    fn wants_top_level_help_true_for_empty() {
        assert!(wants_top_level_help(&[]));
        assert!(wants_top_level_help(&["-h".to_string()]));
        assert!(wants_top_level_help(&["--help".to_string()]));
        assert!(wants_top_level_help(&["help".to_string()]));
    }

    #[test]
    fn wants_top_level_help_false_for_other_args() {
        assert!(!wants_top_level_help(&["install".to_string()]));
        assert!(!wants_top_level_help(&["--version".to_string()]));
        assert!(!wants_top_level_help(&["help".to_string(), "install".to_string()]));
    }

    #[test]
    fn inject_system_line_inserts_after_version_line() {
        let help = "跨平台开发环境一键安装工具\n版本: v0.1.0\n\nUsage: cli <COMMAND>\n";
        let out = inject_system_line(help);
        assert!(out.contains("版本: v0.1.0\n系统: "));
        assert!(out.contains("\n\nUsage: cli <COMMAND>"));
    }

    #[test]
    fn inject_system_line_unchanged_without_version_line() {
        let help = "Usage: cli <COMMAND>\n";
        assert_eq!(inject_system_line(help), help);
    }

    #[test]
    fn render_top_help_contains_version_and_system() {
        let help = render_top_help();
        assert!(help.contains("版本:"));
        assert!(help.contains("系统: "));
    }
```

- [ ] **Step 2: 运行测试确认 RED**

Run: `cargo test --lib 2>&1 | grep -E "error\[|FAILED" | head -5`
Expected: 编译失败（`wants_top_level_help` / `inject_system_line` / `render_top_help` 未定义）

- [ ] **Step 3: 实现 lib.rs 注入逻辑**

在 `src/lib.rs` 的 `run` 函数之后、`mod tests` 之前插入：

```rust
/// 是否顶层 help 场景（无参数或 -h/--help/help）；`cli help <子命令>` 不拦截
pub fn wants_top_level_help(args: &[String]) -> bool {
    args.is_empty() || (args.len() == 1 && matches!(args[0].as_str(), "-h" | "--help" | "help"))
}

/// 顶层 help 文本：渲染 clap help 后在「版本」行后注入系统信息行
pub fn render_top_help() -> String {
    inject_system_line(&Cli::command().render_help().to_string())
}

/// 在「版本: {version}」行后插入系统信息行；找不到版本行时原样返回
fn inject_system_line(help: &str) -> String {
    let version_line = format!("版本: {}", current_version());
    let system_line = format!("{}\n{}", version_line, crate::core::system_info::help_line());
    if help.contains(&version_line) {
        help.replacen(&version_line, &system_line, 1)
    } else {
        help.to_string()
    }
}
```

- [ ] **Step 4: 运行 lib 测试确认 GREEN**

Run: `cargo test --lib 2>&1 | grep -E "test result|FAILED" | head -3`
Expected: 全绿

- [ ] **Step 5: 写失败集成测试**

在 `tests/cli_version.rs` 末尾追加：

```rust
#[test]
fn help_flag_shows_system_line() {
    Command::cargo_bin("cli")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("系统: "))
        .stdout(predicate::str::contains("x86_64").or(predicate::str::contains("aarch64")));
}

#[test]
fn help_subcommand_shows_system_line() {
    Command::cargo_bin("cli")
        .unwrap()
        .arg("help")
        .assert()
        .success()
        .stdout(predicate::str::contains("系统: "));
}

#[test]
fn no_args_shows_system_line() {
    Command::cargo_bin("cli")
        .unwrap()
        .assert()
        .failure()
        .stderr(predicate::str::contains("系统: "));
}
```

- [ ] **Step 6: 运行集成测试确认 RED**

Run: `cargo test --test cli_version 2>&1 | grep -E "FAILED|test result" | head -5`
Expected: 3 个新测试失败（main.rs 尚未拦截，help 无系统行）；原有测试仍通过

- [ ] **Step 7: 实现 main.rs 拦截**

将 `src/main.rs` 整体替换为：

```rust
use clap::Parser;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if cli::wants_top_level_help(&args) {
        let help = cli::render_top_help();
        // 无参数保持 clap 既有语义：stderr + exit(2)；-h/--help/help 走 stdout
        if args.is_empty() {
            eprint!("{help}");
            std::process::exit(2);
        }
        print!("{help}");
        return;
    }
    let cli = cli::Cli::parse();
    if let Err(e) = cli::run(cli) {
        eprintln!("错误: {e}");
        std::process::exit(1);
    }
}
```

- [ ] **Step 8: 运行集成测试确认 GREEN**

Run: `cargo test --test cli_version 2>&1 | grep -E "test result|FAILED" | head -3`
Expected: 全部通过（含原 4 个 + 新 3 个）

- [ ] **Step 9: 手动验证输出**

Run: `cargo run -- --help 2>/dev/null | head -4 && cargo run -- 2>&1 | head -4`
Expected: 版本行下出现 `系统: macOS 15.5 (aarch64)`（或对应平台）

- [ ] **Step 10: 全量回归 + 提交**

Run: `cargo test 2>&1 | grep -E "FAILED|error\[" | head -3; cargo test 2>&1 | grep -cE "test result: ok"`
Expected: 无失败，测试目标全 ok

```bash
git add src/lib.rs src/main.rs tests/cli_version.rs
git commit -m "feat: help 版本行下方展示本机系统信息（渲染后注入）"
```

---

### Task 3: README 文档 + 全量门禁

**Files:**
- Modify: `README.md`（功能特性增加一条）

- [ ] **Step 1: 更新 README**

在 `README.md` 功能特性列表中 `- **调试日志**：...` 一行后追加：

```markdown
- **help 系统信息**：`cli` 无参数或 help 时，在版本下方展示本机系统（macOS/Linux/Windows，麒麟系统含 SP/代号）
```

- [ ] **Step 2: 全量门禁**

Run:
```bash
cargo test 2>&1 | grep -E "test result|FAILED"
cargo clippy --all-targets -- -D warnings 2>&1 | grep -cE "^error"
cargo fmt --check && echo "FMT_CLEAN"
```
Expected: 测试全绿；clippy 输出 0；FMT_CLEAN

若 clippy 报错或 fmt 有差异，修复后重跑；若 `cargo fmt` 修改了文件，将格式化改动并入本任务提交。

- [ ] **Step 3: 提交**

```bash
git add README.md
git commit -m "docs: README 说明 help 系统信息展示"
```

- [ ] **Step 4: 确认工作树干净**

Run: `git status --short && git log --oneline -6`
Expected: 无未提交改动；最近 6 条提交含本计划 3 个 commit
