# uninstall 命令实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现 `cli uninstall [tool] [version]` 命令：按版本卸载工具，全量清理安装目录、config 注册、current 链接，工具无残留版本时清理 shell 注入。

**Architecture:** 命令层（`commands/uninstall.rs`）负责交互解析（无参选择工具/版本、删除确认）与结果提示；核心删除逻辑 `remove_version` 为纯函数（不经 confirm，可单测）。shell 清理复用现有 devkit 块机制，扩展 `shell.rs` 新增 `remove_block` + `remove_tool_injections`，`links.rs` 新增 `remove_link`。

**Tech Stack:** Rust 2021 / clap 4 derive / anyhow / dialoguer 0.11（select/confirm）/ serial_test / tempfile / assert_cmd

## Global Constraints

- 现有 99 个测试保持全绿；每任务完成需通过 `cargo test`、`cargo clippy --all-targets`、`cargo fmt --check`
- 中文提示文案（与现有命令一致）；错误与提示风格参考 `use_cmd.rs`
- 不引入新依赖；不触碰 cache 压缩包；不清理服务注册（未实现）
- 删除顺序不可变：删目录 → 更新 config → 删 current 链接 → 无残留时清理 shell 注入；删目录失败必须中止
- shell 清理失败（如无法识别 shell）降级为警告，不阻断卸载
- PATH 注入行匹配条件：行以 `export PATH="` 开头且包含 `/current/<tool>/bin:$PATH`
- JAVA_HOME 注入行匹配条件：`tool == "java"` 且行以 `export JAVA_HOME="` 开头且包含 `/current/java`（避免误删用户手动设置的其他 JAVA_HOME）
- 现有模式参照：`commands/use_cmd.rs`（版本交互选择）、`tests/cli_install.rs`（非 TTY 守卫）

---

### Task 1: shell.rs 扩展 remove_block + remove_tool_injections

**Files:**
- Modify: `src/core/shell.rs`

**Interfaces:**
- Produces:
  - `pub fn remove_block(rc_file: &Path, marker: &str) -> Result<bool>`——移除整块；无块返回 false，移除成功返回 true
  - `pub fn remove_tool_injections(rc_file: &Path, tool: &str) -> Result<bool>`——移除 devkit 块中该工具的 PATH 行与（仅 java）JAVA_HOME 行；块清空则整块移除；无修改返回 false

- [ ] **Step 1: 写失败测试**

在 `src/core/shell.rs` 的 `#[cfg(test)] mod tests` 末尾追加 6 个测试：

```rust
    #[test]
    fn remove_block_removes_whole_block() {
        let dir = tempdir().unwrap();
        let rc = dir.path().join(".zshrc");
        upsert_block(&rc, "devkit", "export FOO=1").unwrap();
        assert!(remove_block(&rc, "devkit").unwrap());
        let text = std::fs::read_to_string(&rc).unwrap();
        assert!(!text.contains("cli devkit"));
    }

    #[test]
    fn remove_block_noop_when_absent() {
        let dir = tempdir().unwrap();
        let rc = dir.path().join(".zshrc");
        std::fs::write(&rc, "export FOO=1\n").unwrap();
        assert!(!remove_block(&rc, "devkit").unwrap());
        assert_eq!(std::fs::read_to_string(&rc).unwrap(), "export FOO=1\n");
    }

    #[test]
    fn remove_tool_injections_removes_tool_lines() {
        let dir = tempdir().unwrap();
        let rc = dir.path().join(".zshrc");
        let java_bin = dir.path().join("current/java/bin");
        inject_path(&rc, &java_bin).unwrap();
        inject_env_var(&rc, "JAVA_HOME", &dir.path().join("current/java").to_string_lossy()).unwrap();
        assert!(remove_tool_injections(&rc, "java").unwrap());
        let text = std::fs::read_to_string(&rc).unwrap();
        assert!(!text.contains("cli devkit")); // 两条行都删后整块移除
    }

    #[test]
    fn remove_tool_injections_keeps_other_tools() {
        let dir = tempdir().unwrap();
        let rc = dir.path().join(".zshrc");
        let java_bin = dir.path().join("current/java/bin");
        let node_bin = dir.path().join("current/node/bin");
        inject_path(&rc, &java_bin).unwrap();
        inject_path(&rc, &node_bin).unwrap();
        assert!(remove_tool_injections(&rc, "java").unwrap());
        let text = std::fs::read_to_string(&rc).unwrap();
        assert!(!text.contains("current/java"));
        assert!(text.contains(&format!("export PATH=\"{}\"", node_bin.display())));
    }

    #[test]
    fn remove_tool_injections_keeps_custom_java_home() {
        let dir = tempdir().unwrap();
        let rc = dir.path().join(".zshrc");
        let java_bin = dir.path().join("current/java/bin");
        inject_path(&rc, &java_bin).unwrap();
        // 用户手动设置的 JAVA_HOME（不指向 current/java）
        let custom = format!("export JAVA_HOME=\"{}\"", dir.path().join("mymanual").display());
        let current = read_block(&rc, "devkit").unwrap();
        upsert_block(&rc, "devkit", &format!("{current}\n{custom}")).unwrap();
        // 卸载 node 不清 JAVA_HOME；卸载 java 只清指向 current/java 的行
        assert!(!remove_tool_injections(&rc, "node").unwrap());
        assert!(remove_tool_injections(&rc, "java").unwrap());
        let text = std::fs::read_to_string(&rc).unwrap();
        assert!(text.contains("mymanual"));
    }

    #[test]
    fn remove_tool_injections_is_idempotent() {
        let dir = tempdir().unwrap();
        let rc = dir.path().join(".zshrc");
        let java_bin = dir.path().join("current/java/bin");
        inject_path(&rc, &java_bin).unwrap();
        assert!(remove_tool_injections(&rc, "java").unwrap());
        assert!(!remove_tool_injections(&rc, "java").unwrap());
    }
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test --quiet --lib core::shell 2>&1 | tail -8`
Expected: FAIL——`remove_block` / `remove_tool_injections` 未定义（编译错误 E0425）

- [ ] **Step 3: 实现**

在 `src/core/shell.rs` 的 `inject_env_var` 之后、`rc_file_for_shell` 之前追加：

```rust
/// 移除 rc 文件中由标记包裹的整个块；无块时返回 false，移除成功返回 true
pub fn remove_block(rc_file: &Path, marker: &str) -> Result<bool> {
    if !rc_file.exists() {
        return Ok(false);
    }
    let start_marker = format!("# >>> cli {marker} start >>>");
    let end_marker = format!("# <<< cli {marker} end <<<");
    let text = std::fs::read_to_string(rc_file)?;
    let lines: Vec<&str> = text.lines().collect();
    let start_idx = lines.iter().position(|l| l.trim() == start_marker);
    let end_idx = lines.iter().position(|l| l.trim() == end_marker);
    let (Some(s), Some(e)) = (start_idx, end_idx) else {
        return Ok(false);
    };
    if s >= e {
        return Ok(false);
    }
    let mut new_text = String::new();
    for (i, line) in lines.iter().enumerate() {
        if i >= s && i <= e {
            continue; // 跳过 start、块内容、end 行
        }
        new_text.push_str(line);
        new_text.push('\n');
    }
    std::fs::write(rc_file, new_text)?;
    Ok(true)
}

/// 移除 devkit 块中指定工具的注入行：PATH 行（含 /current/<tool>/bin:$PATH）
/// 与 JAVA_HOME 行（仅 tool == "java" 且值含 /current/java，避免误删手动配置）。
/// 块清空后整个块移除；无修改返回 false。
pub fn remove_tool_injections(rc_file: &Path, tool: &str) -> Result<bool> {
    let current = read_block(rc_file, "devkit")?;
    if current.is_empty() {
        return Ok(false);
    }
    let path_pattern = format!("/current/{tool}/bin:$PATH");
    let mut kept: Vec<&str> = Vec::new();
    let mut removed_any = false;
    for l in current.lines() {
        let is_tool_path = l.starts_with("export PATH=\"") && l.contains(&path_pattern);
        let is_java_home =
            tool == "java" && l.starts_with("export JAVA_HOME=\"") && l.contains("/current/java");
        if is_tool_path || is_java_home {
            removed_any = true;
            continue;
        }
        kept.push(l);
    }
    if !removed_any {
        return Ok(false);
    }
    if kept.is_empty() {
        return remove_block(rc_file, "devkit");
    }
    upsert_block(rc_file, "devkit", &kept.join("\n"))?;
    Ok(true)
}
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test --quiet --lib core::shell 2>&1 | tail -3`
Expected: PASS（原有 8 个 + 新增 6 个，共 14 个）

- [ ] **Step 5: 全量验证 + 提交**

```bash
cargo test --quiet 2>&1 | grep -cE "^test result: ok"
cargo clippy --quiet --all-targets
cargo fmt --check
git add src/core/shell.rs
git commit -m "feat: shell 注入块移除能力（remove_block + remove_tool_injections）"
```

Expected: test result ok 全绿（105 个）、clippy 无警告、fmt 无 diff

---

### Task 2: links.rs remove_link + commands/uninstall.rs（核心逻辑）

**Files:**
- Modify: `src/core/links.rs`
- Create: `src/commands/uninstall.rs`

**Interfaces:**
- Consumes:
  - `remove_tool_injections(rc_file: &Path, tool: &str) -> Result<bool>`（Task 1）
  - `pub fn remove_link(link: &Path) -> Result<bool>`——本任务实现
  - 现有：`DevkitPaths::{with_root, tool_dir, current_link, config_file}`、`Config::{load, save, add_installed, remove_installed, set_active, active}`、`interact::{is_interactive, select, confirm}`、`links::set_current_link`、`shell::{inject_path, inject_env_var, rc_file_for_shell}`
- Produces:
  - `pub fn run(tool: Option<String>, version: Option<String>) -> Result<()>`——命令入口（交互 + 确认 + 编排 + 提示）
  - `pub struct UninstallOutcome { pub was_active: bool, pub has_remaining: bool }`
  - `pub fn remove_version(paths: &DevkitPaths, config: &mut Config, tool: &str, version: &str) -> Result<UninstallOutcome>`——核心删除逻辑，不经 confirm

- [ ] **Step 1: 写 remove_link 失败测试**

在 `src/core/links.rs` 的 `#[cfg(test)] mod tests` 追加：

```rust
    #[test]
    fn remove_link_removes_existing_link() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("java21");
        std::fs::create_dir_all(&target).unwrap();
        let link = dir.path().join("current").join("java");
        std::fs::create_dir_all(link.parent().unwrap()).unwrap();
        set_current_link(&link, &target).unwrap();
        assert!(remove_link(&link).unwrap());
        assert!(!link.symlink_metadata().is_ok());
    }

    #[test]
    fn remove_link_noop_when_absent() {
        let dir = tempdir().unwrap();
        let link = dir.path().join("current").join("java");
        assert!(!remove_link(&link).unwrap());
    }
```

- [ ] **Step 2: 运行确认失败 + 实现 remove_link**

Run: `cargo test --quiet --lib core::links 2>&1 | tail -6`
Expected: FAIL（`remove_link` 未定义）

在 `src/core/links.rs` 的 `set_current_link` 之后追加：

```rust
/// 移除 current 符号链接；不存在时返回 false，移除成功返回 true
pub fn remove_link(link: &Path) -> Result<bool> {
    let Ok(meta) = link.symlink_metadata() else {
        return Ok(false);
    };
    #[cfg(windows)]
    if meta.file_type().is_dir() {
        std::fs::remove_dir(link).map_err(|e| anyhow!("删除符号链接失败: {e}"))?;
    } else {
        std::fs::remove_file(link).map_err(|e| anyhow!("删除符号链接失败: {e}"))?;
    }
    #[cfg(not(windows))]
    std::fs::remove_file(link).map_err(|e| anyhow!("删除符号链接失败: {e}"))?;
    Ok(true)
}
```

- [ ] **Step 3: 写 remove_version 失败测试**

创建 `src/commands/uninstall.rs`，先只写测试（引用尚不存在的函数）：

```rust
use anyhow::{anyhow, Result};

use crate::core::config::Config;
use crate::core::interact::{confirm, is_interactive, select};
use crate::core::links::remove_link;
use crate::core::paths::DevkitPaths;
use crate::core::shell::{rc_file_for_shell, remove_tool_injections};

/// 卸载结果：命令层据此输出提示
pub struct UninstallOutcome {
    /// 被卸载版本是否为激活版本
    pub was_active: bool,
    /// 该工具是否还有其他已安装版本
    pub has_remaining: bool,
}

pub fn run(tool: Option<String>, version: Option<String>) -> Result<()> {
    todo!()
}

/// 核心删除逻辑：删目录 → 更新 config → 删 current 链接 → 无残留时清理 shell 注入
pub fn remove_version(
    paths: &DevkitPaths,
    config: &mut Config,
    tool: &str,
    version: &str,
) -> Result<UninstallOutcome> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::links::set_current_link;
    use crate::core::shell::{inject_env_var, inject_path};
    use serial_test::serial;
    use tempfile::tempdir;

    /// 构造 root/java/21 + root/java/17 目录、current 链接指向 17、config active=17
    fn setup_two_versions(root: &std::path::Path) -> (DevkitPaths, Config) {
        let paths = DevkitPaths::with_root(root.to_path_buf());
        std::fs::create_dir_all(paths.tool_dir("java", "21")).unwrap();
        std::fs::create_dir_all(paths.tool_dir("java", "17")).unwrap();
        let link = paths.current_link("java");
        std::fs::create_dir_all(link.parent().unwrap()).unwrap();
        set_current_link(&link, std::path::Path::new("../java/17")).unwrap();
        let mut config = Config::default();
        config.add_installed("java", "21");
        config.add_installed("java", "17");
        config.set_active("java", "17");
        config.save(&paths).unwrap();
        (paths, config)
    }

    #[test]
    fn remove_non_active_version_keeps_link_and_shell() {
        let dir = tempdir().unwrap();
        let (paths, mut config) = setup_two_versions(dir.path());
        let outcome = remove_version(&paths, &mut config, "java", "21").unwrap();
        assert!(!outcome.was_active);
        assert!(outcome.has_remaining);
        assert!(!paths.tool_dir("java", "21").exists());
        assert!(paths.tool_dir("java", "17").exists());
        assert!(paths.current_link("java").symlink_metadata().is_ok()); // 链接保留
        let reloaded = Config::load(&paths).unwrap();
        assert_eq!(reloaded.installed.get("java").unwrap(), &vec!["17".to_string()]);
        assert_eq!(reloaded.active.get("java").unwrap(), "17");
    }

    #[test]
    fn remove_active_version_removes_link() {
        let dir = tempdir().unwrap();
        let (paths, mut config) = setup_two_versions(dir.path());
        let outcome = remove_version(&paths, &mut config, "java", "17").unwrap();
        assert!(outcome.was_active);
        assert!(outcome.has_remaining);
        assert!(!paths.current_link("java").symlink_metadata().is_ok()); // 链接已删
        let reloaded = Config::load(&paths).unwrap();
        assert!(!reloaded.active.contains_key("java"));
        assert_eq!(reloaded.installed.get("java").unwrap(), &vec!["21".to_string()]);
    }

    #[serial(env)]
    #[test]
    fn remove_last_version_cleans_injections() {
        let dir = tempdir().unwrap();
        let home = tempdir().unwrap();
        std::env::set_var("HOME", home.path());
        std::env::set_var("SHELL", "/bin/zsh");
        let (paths, mut config) = setup_two_versions(dir.path());
        // 预置 shell 注入（PATH + JAVA_HOME 指向 current 链）
        let rc_file = home.path().join(".zshrc");
        let link = paths.current_link("java");
        inject_path(&rc_file, &link.join("bin")).unwrap();
        inject_env_var(&rc_file, "JAVA_HOME", &link.to_string_lossy()).unwrap();

        // 先卸非激活 21（不触发 shell 清理），再卸激活 17（最后版本 → 清理）
        remove_version(&paths, &mut config, "java", "21").unwrap();
        let outcome = remove_version(&paths, &mut config, "java", "17").unwrap();
        assert!(outcome.was_active);
        assert!(!outcome.has_remaining);
        assert!(!link.symlink_metadata().is_ok());
        let text = std::fs::read_to_string(&rc_file).unwrap();
        assert!(!text.contains("cli devkit")); // 注入整块已移除
        assert!(!text.contains("current/java"));
    }
}
```

- [ ] **Step 4: 运行确认失败**

Run: `cargo test --quiet --lib commands::uninstall 2>&1 | tail -8`
Expected: FAIL——`todo!()` panic（3 个测试全失败，remove_link 测试已过）

- [ ] **Step 5: 实现 run + remove_version**

用真实实现替换 `run` 与 `remove_version` 的 `todo!()` 占位：

```rust
pub fn run(tool: Option<String>, version: Option<String>) -> Result<()> {
    let paths = DevkitPaths::new()?;
    let mut config = Config::load(&paths)?;

    // 工具解析：无参时交互选择（复用 use 命令模式）
    let tool = match tool {
        Some(t) => t,
        None => {
            if !is_interactive() {
                return Err(anyhow!("请指定工具名，例如: cli uninstall java"));
            }
            let tools: Vec<String> = config.installed.keys().cloned().collect();
            if tools.is_empty() {
                return Err(anyhow!("尚未安装任何工具，无需卸载"));
            }
            let idx = select("请选择要卸载的工具", &tools)?;
            tools[idx].clone()
        }
    };

    let installed = config.installed.get(&tool).cloned().unwrap_or_default();
    if installed.is_empty() {
        return Err(anyhow!(
            "{tool} 尚未安装任何版本，请先执行 cli install {tool}"
        ));
    }

    // 版本解析：指定版本需已安装；缺省时单版本直接卸、多版本交互选择
    let version = match version {
        Some(v) if installed.contains(&v) => v,
        Some(v) => {
            return Err(anyhow!(
                "{tool} {v} 未安装，可用版本: {}",
                installed.join(", ")
            ))
        }
        None => {
            if installed.len() == 1 {
                installed[0].clone()
            } else {
                let labels: Vec<String> = installed.iter().map(|v| format!("{tool} {v}")).collect();
                let idx = select("请选择要卸载的版本", &labels)?;
                installed[idx].clone()
            }
        }
    };

    // 删除确认（默认否）
    if !confirm(&format!("确认卸载 {tool} {version}？"), false)? {
        println!("已取消");
        return Ok(());
    }

    let outcome = remove_version(&paths, &mut config, &tool, &version)?;
    if outcome.was_active && outcome.has_remaining {
        println!("已卸载激活版本 {tool} {version}");
        println!("提示: 可用 `cli use {tool} <version>` 重新激活其他版本");
    } else {
        println!("已卸载 {tool} {version}");
        if !outcome.has_remaining {
            println!("该工具已无残留版本，环境已清理");
        }
    }
    Ok(())
}

/// 核心删除逻辑：删目录 → 更新 config → 删 current 链接 → 无残留时清理 shell 注入
pub fn remove_version(
    paths: &DevkitPaths,
    config: &mut Config,
    tool: &str,
    version: &str,
) -> Result<UninstallOutcome> {
    let tool_dir = paths.tool_dir(tool, version);
    if !tool_dir.exists() {
        return Err(anyhow!("{tool} {version} 未安装"));
    }
    // 1. 删版本目录（失败即中止，不动其他状态）
    std::fs::remove_dir_all(&tool_dir)?;

    // 2. 更新 config
    let was_active = config.active.get(tool).map(|s| s.as_str()) == Some(version);
    config.remove_installed(tool, version);
    if was_active {
        config.active.remove(tool);
    }
    config.save(paths)?;

    // 3. 激活版本被卸 → 删 current 链接
    if was_active {
        remove_link(&paths.current_link(tool))?;
    }

    // 4. 无残留版本 → 清理 shell 注入（失败降级警告，不阻断卸载）
    let has_remaining = config
        .installed
        .get(tool)
        .map(|v| !v.is_empty())
        .unwrap_or(false);
    if !has_remaining {
        if let Ok(rc_file) = rc_file_for_shell() {
            if let Err(e) = remove_tool_injections(&rc_file, tool) {
                println!("警告: 清理 shell 配置失败: {e}");
            }
        }
    }
    Ok(UninstallOutcome {
        was_active,
        has_remaining,
    })
}
```

- [ ] **Step 6: 运行测试确认通过**

Run: `cargo test --quiet --lib 2>&1 | grep -E "^test result" | tail -3`
Expected: PASS（links 2 个新测试 + uninstall 3 个新测试全过）

- [ ] **Step 7: 全量验证 + 提交**

```bash
cargo clippy --quiet --all-targets
cargo fmt --check
git add src/core/links.rs src/commands/uninstall.rs
git commit -m "feat: uninstall 核心逻辑（remove_version + 交互编排）"
```

Expected: clippy 无警告、fmt 无 diff

---

### Task 3: 命令接线 + 集成测试 + README

**Files:**
- Modify: `src/lib.rs`
- Modify: `src/commands/mod.rs`
- Create: `tests/cli_uninstall.rs`
- Modify: `README.md`

**Interfaces:**
- Consumes: `commands::uninstall::run(tool: Option<String>, version: Option<String>) -> Result<()>`（Task 2）
- Produces: clap 子命令 `Uninstall { tool: Option<String>, version: Option<String> }`

- [ ] **Step 1: clap 命令接线**

在 `src/lib.rs` 的 `Command` 枚举中、`SelfUpdate` 之前追加：

```rust
    /// 卸载已安装的工具版本（无参数时交互选择）
    Uninstall {
        /// 工具名（不填则交互选择）
        tool: Option<String>,
        /// 版本（不填则交互选择或卸载唯一版本）
        version: Option<String>,
    },
```

在 `src/lib.rs` 的 `run` 分发中追加：

```rust
        Command::Uninstall { tool, version } => commands::uninstall::run(tool, version),
```

在 `src/commands/mod.rs` 中追加：

```rust
pub mod uninstall;
```

- [ ] **Step 2: 写集成测试**

创建 `tests/cli_uninstall.rs`：

```rust
use std::io::IsTerminal;

use assert_cmd::Command;

#[test]
fn uninstall_without_tool_reports_hint_when_non_tty() {
    // 测试进程 stdin 非 TTY（CI/管道环境），若本地为 TTY 则跳过
    if std::io::stdin().is_terminal() && std::io::stderr().is_terminal() {
        return;
    }
    Command::cargo_bin("cli")
        .unwrap()
        .arg("uninstall")
        .assert()
        .failure()
        .stderr(predicates::prelude::predicate::str::contains(
            "请指定工具名",
        ));
}

#[test]
fn uninstall_unknown_tool_reports_not_installed() {
    // DEVKIT_ROOT 指向空目录，避免读真实 ~/.devkit 配置
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_str().unwrap().to_string();
    Command::cargo_bin("cli")
        .unwrap()
        .env("DEVKIT_ROOT", root)
        .args(["uninstall", "java", "99"])
        .assert()
        .failure()
        .stderr(predicates::prelude::predicate::str::contains(
            "尚未安装",
        ));
}
```

- [ ] **Step 3: 运行集成测试确认通过**

Run: `cargo test --quiet --test cli_uninstall 2>&1 | tail -3`
Expected: PASS（2 个测试；无参非 TTY 测试在 TTY 环境会跳过）

- [ ] **Step 4: README 命令详解**

在 `README.md` 的 `### \`cli install [tool]\`` 段之后插入：

```markdown
### `cli uninstall [tool] [version]`

卸载已安装的工具版本：删除安装目录、配置注册、current 链接；该工具最后一个版本被卸载时，同时清理 shell 配置文件中的 PATH/JAVA_HOME 注入。

- `cli uninstall java 17`：直接卸载指定版本
- `cli uninstall java`：卸载 java（多版本时交互选择）
- 无参数：交互选择工具和版本
- 删除前会确认，取消则不动作；cache 中下载的压缩包保留
```

- [ ] **Step 5: 全量验证 + 提交**

```bash
cargo test --quiet 2>&1 | grep -E "^test result: ok" | wc -l
cargo clippy --quiet --all-targets
cargo fmt --check
cargo build --release --quiet
git add src/lib.rs src/commands/mod.rs tests/cli_uninstall.rs README.md
git commit -m "feat: uninstall 命令接线、集成测试与文档"
```

Expected: 全部 test result ok（99 现有 + 6 shell + 2 links + 3 uninstall 逻辑 + 2 集成测试，共 112 个）、clippy/fmt clean、release 构建成功

---

## 验证总览（全部任务完成后）

```bash
cargo test --quiet 2>&1 | grep -E "^test result"
cargo clippy --quiet --all-targets
cargo fmt --check
```

手工冒烟（可选，真实环境谨慎操作——建议用 `DEVKIT_ROOT=/tmp/devkit-demo` 演练）：

```bash
DEVKIT_ROOT=/tmp/devkit-demo ./target/debug/cli uninstall java 21
DEVKIT_ROOT=/tmp/devkit-demo ./target/debug/cli uninstall
DEVKIT_ROOT=/tmp/devkit-demo ./target/debug/cli uninstall java 99   # 应报未安装
```
