# Rust (rustup) 双源安装 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 为 `cli` 增加 `cli install rust`，通过 rustup-init 脚本安装 Rust 到 devkit 目录，支持阿里源/官方源双源选择并持久化环境变量与 PATH。

**Architecture:** 新增 `src/core/tools/rust.rs`（纯函数层 + 安装流程），复用 core 层 `interact::select`、`shell::{inject_env_var, inject_path, rc_file_for_shell, print_activation_hint}`、`paths::DevkitPaths`。执行 `curl | sh` 安装（进程级注入 `RUSTUP_HOME`/`CARGO_HOME`/镜像变量），安装成功后持久化到 rc 文件 devkit 块。`commands/install.rs` 增加 "rust" 分发。

**Tech Stack:** Rust（clap/serde 现有依赖不变）、dialoguer（交互选择）、assert_cmd + TcpListener mock（集成测试）、serial_test（env 测试串行化）。

## Global Constraints

- 安装位置：`RUSTUP_HOME=<root>/rustup`、`CARGO_HOME=<root>/cargo`（`<root>` = `DevkitPaths::root()`）
- 官方源脚本 URL：`https://sh.rustup.rs`；阿里源脚本 URL：`https://mirrors.aliyun.com/repo/rust/rustup-init.sh`
- 阿里源镜像变量：`RUSTUP_UPDATE_ROOT=https://mirrors.aliyun.com/rustup/rustup`、`RUSTUP_DIST_SERVER=https://mirrors.aliyun.com/rustup`
- 安装命令统一追加 `-y --no-modify-path`；PATH 由 cli 统一注入 rc（禁止 rustup 改 rc）
- 源选择：TTY 交互 `select`（`["阿里源（国内加速）", "官方源"]`，下标 0=阿里源）；非 TTY 默认官方源并打印提示
- 范围：仅 install；不纳入 list/uninstall/use/config.json
- 平台：仅 macOS/Linux；Windows 返回错误「Windows 暂不支持自动安装 Rust，请手动运行 rustup-init.exe」
- 测试钩子：环境变量 `DEVKIT_RUSTUP_SCRIPT` 非空时覆盖脚本 URL
- 已安装检测：`<root>/rustup` 目录存在 → 报错「Rust (rustup) 已安装于 <path>，如需重装请先手动删除该目录与 rc 中相关注入」
- rc 注入失败：打印警告（`eprintln!`）不阻断，仍返回成功
- 项目门禁：`cargo test`、`cargo clippy -- -D warnings`、`cargo fmt --check` 全绿

---

### Task 1: rust.rs 纯函数层（源类型/命令构建/环境变量）

**Files:**
- Create: `src/core/tools/rust.rs`（本任务只含类型与纯函数 + `#[cfg(test)]`）
- Modify: `src/core/tools/mod.rs`（增加 `pub mod rust;`）

**Interfaces:**
- Produces（本任务，后续任务消费）:
  - `pub enum RustSource { Official, Aliyun }`
  - `impl RustSource { pub fn label(self) -> &'static str; pub fn script_url(self) -> String; pub fn choose() -> Result<RustSource> }`
  - `pub fn install_command(source: RustSource) -> String`
  - `pub fn rustup_home_dir(root: &Path) -> PathBuf`、`pub fn cargo_home_dir(root: &Path) -> PathBuf`
  - `pub fn install_env_vars(source: RustSource, root: &Path) -> Vec<(String, String)>`
  - `pub fn aliyun_env_vars() -> Vec<(&'static str, &'static str)>`
  - `pub fn is_installed(root: &Path) -> bool`

- [ ] **Step 1: 在 mod.rs 注册模块**

在 `src/core/tools/mod.rs` 的模块声明区（与 `go`、`java` 等并列）加入一行：

```rust
pub mod rust;
```

- [ ] **Step 2: 写失败测试（rust.rs 测试模块）**

创建 `src/core/tools/rust.rs`，先写测试模块（`use std::path::{Path, PathBuf};`、`use serial_test::serial;`）：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[serial(env)]
    #[test]
    fn install_command_official_source() {
        assert_eq!(
            install_command(RustSource::Official),
            "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path"
        );
    }

    #[serial(env)]
    #[test]
    fn install_command_aliyun_source() {
        assert_eq!(
            install_command(RustSource::Aliyun),
            "curl --proto '=https' --tlsv1.2 -sSf https://mirrors.aliyun.com/repo/rust/rustup-init.sh | sh -s -- -y --no-modify-path"
        );
    }

    #[serial(env)]
    #[test]
    fn install_command_uses_script_override() {
        std::env::set_var("DEVKIT_RUSTUP_SCRIPT", "http://127.0.0.1:9/rustup-init.sh");
        assert!(
            install_command(RustSource::Official)
                .contains("http://127.0.0.1:9/rustup-init.sh")
        );
        std::env::remove_var("DEVKIT_RUSTUP_SCRIPT");
    }

    #[test]
    fn aliyun_env_vars_returns_two_entries() {
        assert_eq!(
            aliyun_env_vars(),
            vec![
                ("RUSTUP_UPDATE_ROOT", "https://mirrors.aliyun.com/rustup/rustup"),
                ("RUSTUP_DIST_SERVER", "https://mirrors.aliyun.com/rustup"),
            ]
        );
    }

    #[test]
    fn common_env_vars_include_home_and_cargo() {
        let root = Path::new("/tmp/devkit");
        let vars = install_env_vars(RustSource::Official, root);
        assert_eq!(vars.len(), 2);
        assert!(vars.contains(&("RUSTUP_HOME".to_string(), "/tmp/devkit/rustup".to_string())));
        assert!(vars.contains(&("CARGO_HOME".to_string(), "/tmp/devkit/cargo".to_string())));
    }

    #[test]
    fn install_env_vars_aliyun_adds_mirror_vars() {
        let root = Path::new("/tmp/devkit");
        let vars = install_env_vars(RustSource::Aliyun, root);
        assert_eq!(vars.len(), 4);
        assert!(vars.iter().any(|(k, v)| {
            k == "RUSTUP_UPDATE_ROOT" && v == "https://mirrors.aliyun.com/rustup/rustup"
        }));
        assert!(vars.iter().any(|(k, v)| {
            k == "RUSTUP_DIST_SERVER" && v == "https://mirrors.aliyun.com/rustup"
        }));
    }

    #[test]
    fn rustup_home_dir_from_root() {
        assert_eq!(
            rustup_home_dir(Path::new("/tmp/devkit")),
            PathBuf::from("/tmp/devkit/rustup")
        );
    }

    #[test]
    fn is_installed_detects_rustup_dir() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_installed(dir.path()));
        std::fs::create_dir_all(dir.path().join("rustup")).unwrap();
        assert!(is_installed(dir.path()));
    }

    #[test]
    fn source_labels_are_chinese() {
        assert_eq!(RustSource::Official.label(), "官方源");
        assert_eq!(RustSource::Aliyun.label(), "阿里源");
    }
}
```

- [ ] **Step 3: 运行测试确认失败**

Run: `cargo test --lib tools::rust -v`
Expected: FAIL（编译错误 `cannot find ...` / 未定义符号）

- [ ] **Step 4: 实现纯函数层**

在测试模块上方写入实现：

```rust
//! Rust (rustup) 安装支持：阿里源/官方源双源选择，安装到 devkit 目录

use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::core::interact::{is_interactive, select};

/// 官方源安装脚本 URL
pub const OFFICIAL_SCRIPT_URL: &str = "https://sh.rustup.rs";
/// 阿里源安装脚本 URL
pub const ALIYUN_SCRIPT_URL: &str = "https://mirrors.aliyun.com/repo/rust/rustup-init.sh";
/// 阿里源发行版镜像根（RUSTUP_DIST_SERVER）
pub const ALIYUN_DIST_SERVER: &str = "https://mirrors.aliyun.com/rustup";
/// 阿里源更新元数据（RUSTUP_UPDATE_ROOT）
pub const ALIYUN_UPDATE_ROOT: &str = "https://mirrors.aliyun.com/rustup/rustup";

/// Rust 下载源
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RustSource {
    Official,
    Aliyun,
}

impl RustSource {
    /// 源的中文标签
    pub fn label(self) -> &'static str {
        match self {
            RustSource::Official => "官方源",
            RustSource::Aliyun => "阿里源",
        }
    }

    /// 安装脚本 URL；DEVKIT_RUSTUP_SCRIPT 非空时覆盖（测试钩子）
    pub fn script_url(self) -> String {
        if let Ok(url) = std::env::var("DEVKIT_RUSTUP_SCRIPT") {
            if !url.is_empty() {
                return url;
            }
        }
        match self {
            RustSource::Official => OFFICIAL_SCRIPT_URL.to_string(),
            RustSource::Aliyun => ALIYUN_SCRIPT_URL.to_string(),
        }
    }

    /// 交互选择下载源（阿里源在前）；非 TTY 默认官方源
    pub fn choose() -> Result<RustSource> {
        if !is_interactive() {
            println!("提示: 非交互模式默认使用官方源，可通过交互模式选择阿里源");
            return Ok(RustSource::Official);
        }
        let labels = ["阿里源（国内加速）", "官方源"];
        let idx = select("请选择 Rust 下载源", &labels)?;
        Ok(if idx == 0 {
            RustSource::Aliyun
        } else {
            RustSource::Official
        })
    }
}

/// 构建安装命令：curl 脚本 | sh 非交互安装；--no-modify-path 保证 rc 由 cli 统一注入
pub fn install_command(source: RustSource) -> String {
    format!(
        "curl --proto '=https' --tlsv1.2 -sSf {} | sh -s -- -y --no-modify-path",
        source.script_url()
    )
}

/// rustup 主目录：<root>/rustup
pub fn rustup_home_dir(root: &Path) -> PathBuf {
    root.join("rustup")
}

/// cargo 主目录：<root>/cargo
pub fn cargo_home_dir(root: &Path) -> PathBuf {
    root.join("cargo")
}

/// 安装进程注入的环境变量：RUSTUP_HOME/CARGO_HOME + 阿里源镜像变量
pub fn install_env_vars(source: RustSource, root: &Path) -> Vec<(String, String)> {
    let mut vars = vec![
        (
            "RUSTUP_HOME".to_string(),
            rustup_home_dir(root).display().to_string(),
        ),
        (
            "CARGO_HOME".to_string(),
            cargo_home_dir(root).display().to_string(),
        ),
    ];
    if source == RustSource::Aliyun {
        vars.push(("RUSTUP_UPDATE_ROOT".to_string(), ALIYUN_UPDATE_ROOT.to_string()));
        vars.push(("RUSTUP_DIST_SERVER".to_string(), ALIYUN_DIST_SERVER.to_string()));
    }
    vars
}

/// 阿里源镜像环境变量（安装成功后的 rc 持久化）
pub fn aliyun_env_vars() -> Vec<(&'static str, &'static str)> {
    vec![
        ("RUSTUP_UPDATE_ROOT", ALIYUN_UPDATE_ROOT),
        ("RUSTUP_DIST_SERVER", ALIYUN_DIST_SERVER),
    ]
}

/// 已安装检测：<root>/rustup 目录存在即视为已安装
pub fn is_installed(root: &Path) -> bool {
    rustup_home_dir(root).exists()
}
```

- [ ] **Step 5: 运行测试确认通过**

Run: `cargo test --lib tools::rust`
Expected: PASS（9 个测试全绿）

- [ ] **Step 6: 提交**

```bash
git add src/core/tools/rust.rs src/core/tools/mod.rs
git commit -m "feat: add rust install source selection and env helpers"
```

---

### Task 2: rust::install 执行流程与集成测试

**Files:**
- Modify: `src/core/tools/rust.rs`（追加 `pub fn install` + 引用 shell 模块）
- Modify: `src/commands/install.rs`（接线：TOOL_CHOICES + match 分发）
- Create: `tests/cli_rust.rs`

**Interfaces:**
- Consumes: Task 1 的全部函数；`crate::core::paths::DevkitPaths::new()`、`crate::core::shell::{inject_env_var, inject_path, rc_file_for_shell, print_activation_hint}`
- Produces: `pub fn install(_hint: Option<&str>) -> Result<()>`；install.rs 的 `TOOL_CHOICES` 增加 `("Rust (rustup)", "rust")`、match 增加 `"rust" => rust::install(None)`（端到端集成测试依赖此接线，故在本任务一并完成）

- [ ] **Step 1: 写失败集成测试**

创建 `tests/cli_rust.rs`（`assert_cmd`、`predicates`、`tempfile` 均为 dev-dependencies，无需新增）：

```rust
use assert_cmd::Command;
use predicates::prelude::*;
use std::io::{Read, Write};
use std::net::TcpListener;

/// 脚本 mock：任何请求返回假 rustup-init 脚本（输出标记行）
fn mock_script() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 4096];
        let _ = stream.read(&mut buf);
        let body = "#!/bin/sh\necho mock-rustup-done\n";
        let resp = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(resp.as_bytes());
    });
    format!("http://{addr}/rustup-init.sh")
}

#[test]
fn rust_install_non_tty_defaults_official_source() {
    let base = mock_script();
    let home = tempfile::tempdir().unwrap();
    let root = home.path().join("devkit");
    Command::cargo_bin("cli")
        .unwrap()
        .env("HOME", home.path())
        .env("DEVKIT_ROOT", &root)
        .env("SHELL", "/bin/zsh")
        .env("NO_PROXY", "127.0.0.1")
        .env("DEVKIT_RUSTUP_SCRIPT", &base)
        .args(["install", "rust"])
        .assert()
        .success()
        .stdout(predicate::str::contains("mock-rustup-done"))
        .stdout(predicate::str::contains("安装完成"));
    let rc = std::fs::read_to_string(home.path().join(".zshrc")).unwrap();
    assert!(rc.contains(&format!(
        "export RUSTUP_HOME=\"{}\"",
        root.join("rustup").display()
    )));
    assert!(rc.contains(&format!(
        "export CARGO_HOME=\"{}\"",
        root.join("cargo").display()
    )));
    assert!(rc.contains(&format!(
        "export PATH=\"{}/bin:$PATH\"",
        root.join("cargo").display()
    )));
    // 官方源不注入镜像变量
    assert!(!rc.contains("RUSTUP_UPDATE_ROOT"));
}

#[test]
fn rust_install_detects_existing_installation() {
    let home = tempfile::tempdir().unwrap();
    let root = home.path().join("devkit");
    std::fs::create_dir_all(root.join("rustup")).unwrap();
    Command::cargo_bin("cli")
        .unwrap()
        .env("HOME", home.path())
        .env("DEVKIT_ROOT", &root)
        .env("SHELL", "/bin/zsh")
        .args(["install", "rust"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("已安装"));
}
```

- [ ] **Step 2: 运行集成测试确认失败**

Run: `cargo test --test cli_rust`
Expected: FAIL（编译错误 `cannot find function 'install' in module ...` 或 `clippy::` 无，先看编译失败）

- [ ] **Step 3: 实现 install 主流程**

在 `src/core/tools/rust.rs` 的 `is_installed` 之后追加：

```rust
/// 安装 Rust（rustup）：选择源 → 执行脚本 → 持久化环境变量与 PATH
pub fn install(_hint: Option<&str>) -> Result<()> {
    #[cfg(windows)]
    {
        return Err(anyhow::anyhow!(
            "Windows 暂不支持自动安装 Rust，请手动运行 rustup-init.exe（https://rustup.rs）"
        ));
    }
    #[cfg(not(windows))]
    {
        use crate::core::shell::{inject_env_var, inject_path, print_activation_hint, rc_file_for_shell};

        let paths = crate::core::paths::DevkitPaths::new()?;
        let root = paths.root();
        if is_installed(root) {
            return Err(anyhow::anyhow!(
                "Rust (rustup) 已安装于 {}，如需重装请先手动删除该目录与 rc 中相关注入",
                rustup_home_dir(root).display()
            ));
        }
        let source = RustSource::choose()?;
        println!("开始安装 Rust（{}）...", source.label());
        let status = std::process::Command::new("sh")
            .arg("-c")
            .arg(install_command(source))
            .envs(install_env_vars(source, root))
            .status()?;
        if !status.success() {
            let code = status
                .code()
                .map(|c| c.to_string())
                .unwrap_or_else(|| "信号终止".to_string());
            return Err(anyhow::anyhow!("Rust 安装失败，退出码 {code}"));
        }
        // 持久化环境变量与 PATH（失败仅警告，不阻断已完成的安装）
        let rc = rc_file_for_shell()?;
        for (key, value) in install_env_vars(source, root) {
            if let Err(e) = inject_env_var(&rc, &key, &value) {
                eprintln!(
                    "警告: 环境变量 {key} 写入 {} 失败: {e}，请手动配置",
                    rc.display()
                );
            }
        }
        if let Err(e) = inject_path(&rc, &cargo_home_dir(root).join("bin")) {
            eprintln!("警告: PATH 写入 {} 失败: {e}，请手动配置", rc.display());
        }
        println!("Rust (rustup) 安装完成（{}）", source.label());
        print_activation_hint()?;
        Ok(())
    }
}
```

- [ ] **Step 4: 接线 install.rs**

修改 `src/commands/install.rs`：

```rust
use crate::core::tools::{go, java, maven, node, rust};

/// 交互列表：中文标签 -> 工具内部名
const TOOL_CHOICES: [(&str, &str); 7] = [
    ("Java", "java"),
    ("Node.js", "node"),
    ("Go", "go"),
    ("Maven", "maven"),
    ("Maven Daemon (mvnd)", "mvnd"),
    ("Rust (rustup)", "rust"),
    ("自更新", "update"),
];
```

match 分发增加（`"mvnd" => ...` 之后、`"update" => ...` 之前）：

```rust
        "rust" => rust::install(None),
```

- [ ] **Step 5: 运行集成测试确认通过**

Run: `cargo test --test cli_rust`
Expected: PASS（2 个集成测试；curl 访问 mock 脚本 → sh 输出标记 → rc 注入断言通过；接线前会报「暂不支持的安装目标: rust」，接线后通过）

- [ ] **Step 6: 回归单测**

Run: `cargo test --lib tools::rust`
Expected: PASS（9 个单测仍全绿）

- [ ] **Step 7: 提交**

```bash
git add src/core/tools/rust.rs src/commands/install.rs tests/cli_rust.rs
git commit -m "feat: implement rustup install flow with rc injection"
```

---

### Task 3: README 文档与全量门禁

**Files:**
- Modify: `README.md:10,45,69-73,133-142`（工具列表与环境变量表）

**Interfaces:**
- Consumes: Task 2 完成的接线（`cli install rust` 已可达）

- [ ] **Step 1: 更新 README**

1. 第 10 行「多工具支持」改为：`Java（6 大发行版）、Node.js、Go、Maven、Rust (rustup)`
2. 第 45 行交互工具列表追加 `Rust (rustup)`；第 69 行 `支持：` 行改为 `java`、`node`、`go`、`maven`、`mvnd`、`rust`
3. 第 71 行后新增一条：`- **rust**：通过 rustup 安装到 `<根目录>/rustup` 与 `<根目录>/cargo`；交互选择阿里源（国内加速）或官方源，阿里源自动配置 `RUSTUP_UPDATE_ROOT`/`RUSTUP_DIST_SERVER` 镜像`
4. 环境变量表（第 141 行 `DEVKIT_MIRROR_API` 行后）追加两行：

```markdown
| `RUSTUP_HOME` | rustup 主目录（安装 Rust 后注入 `<根目录>/rustup`） |
| `CARGO_HOME` | cargo 主目录（安装 Rust 后注入 `<根目录>/cargo`） |
```

> 若 README 实际行号与计划有偏差，以匹配文本内容为准，保持表格/列表格式一致。

- [ ] **Step 2: 全量门禁**

Run: `cargo test && cargo clippy -- -D warnings && cargo fmt --check`
Expected: 全部通过（160 个既有测试 + 新增 9 单测 + 2 集成测试；clippy 0 warnings；fmt clean）

- [ ] **Step 3: 提交**

```bash
git add README.md
git commit -m "docs: document rust install and aliyun mirror"
```
