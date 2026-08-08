# 默认安装根目录调整实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Linux 默认安装根目录从 `~/.devkit` 调整为 `/opt/.devkit`，macOS/Windows 保持不变，权限失败时明确提示不降级。

**Architecture:** `src/core/paths.rs` 新增 `default_root()` 平台分支函数与 `ensure_writable()` 可写性校验，`DevkitPaths::new()` 解析链变为「DEVKIT_ROOT → default_root()」两步；`commands/install.rs` 入口前置校验并包装权限提示；README 同步默认路径说明。

**Tech Stack:** Rust 2021 / anyhow / cfg 平台分支 / assert_cmd + tempfile（集成测试）/ serial_test（环境变量测试）

## Global Constraints

- 环境变量机制不做调整：`DEVKIT_ROOT` 非空 → 用之；未设置 → `default_root()`；不新增 `CLI_HOME`/`DEVKIT_HOME`
- Linux 默认 `/opt/.devkit`；macOS/Windows 默认 `~/.devkit`（`#[cfg(target_os = "linux")]` 判定）
- 权限错误**报错不降级**，错误文案固定为：`创建安装目录 {root} 失败: {err}\n提示: 请使用 sudo 运行，或设置 DEVKIT_ROOT 指定可写目录，例如 DEVKIT_ROOT=$HOME/.devkit`
- `ensure_writable()` 必须用探针文件验证真实可写性（`create_dir_all` 对已存在目录不做权限校验）
- 修改环境变量的单测加 `#[serial(env)]`（serial_test = "3" 已在 dev-dependencies）
- 集成测试平台相关断言用 `cfg!` 动态构造，禁止硬编码平台字符串

---

### Task 1: paths.rs 默认根目录平台化（default_root + new 两步解析）

**Files:**
- Modify: `src/core/paths.rs`（新增 `default_root()`；`new()` 第 31-38 行改造；测试模块 111-119 行改造 + 新增 2 测试）

**Interfaces:**
- Produces: `pub(crate) fn default_root() -> Result<PathBuf>` —— Linux 返回 `/opt/.devkit`，其他平台返回 `home_dir()?.join(".devkit")`；Task 1 内部 `new()` 使用

- [ ] **Step 1: 写失败测试（新增 2 个 + 改造 1 个）**

在 `src/core/paths.rs` 测试模块新增（`use super::*` 与 `use serial_test::serial;` 已存在）：

```rust
#[cfg(target_os = "linux")]
#[test]
fn default_root_linux_is_opt_devkit() {
    assert_eq!(default_root().unwrap(), PathBuf::from("/opt/.devkit"));
}

#[cfg(not(target_os = "linux"))]
#[serial(env)]
#[test]
fn default_root_other_platforms_is_home_devkit() {
    let home = tempfile::tempdir().unwrap();
    std::env::set_var("HOME", home.path());
    let root = default_root().unwrap();
    assert_eq!(root, home.path().join(".devkit"));
}
```

将现有 `new_falls_back_to_home_devkit` 整体替换为（按平台分支断言，不再写死 home）：

```rust
#[serial(env)]
#[test]
fn new_falls_back_to_default_root() {
    std::env::remove_var("DEVKIT_ROOT");
    #[cfg(target_os = "linux")]
    {
        let paths = DevkitPaths::new().unwrap();
        assert_eq!(paths.root(), Path::new("/opt/.devkit"));
    }
    #[cfg(not(target_os = "linux"))]
    {
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("HOME", home.path());
        let paths = DevkitPaths::new().unwrap();
        assert_eq!(paths.root(), home.path().join(".devkit"));
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test --lib core::paths -q`
Expected: FAIL —— `default_root` 未定义（编译错误）+ `new_falls_back_to_default_root` 无法编译

- [ ] **Step 3: 最小实现**

在 `home_dir()` 之后、`DevkitPaths` 之前新增：

```rust
/// 默认安装根目录：Linux 为 /opt/.devkit（系统级共享），
/// 其他平台沿用用户主目录下的 .devkit
pub(crate) fn default_root() -> Result<PathBuf> {
    #[cfg(target_os = "linux")]
    {
        Ok(PathBuf::from("/opt/.devkit"))
    }
    #[cfg(not(target_os = "linux"))]
    {
        Ok(home_dir()?.join(".devkit"))
    }
}
```

`new()` 第 37 行 `Ok(Self::with_root(home_dir()?.join(".devkit")))` 替换为：

```rust
        Ok(Self::with_root(default_root()?))
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test --lib core::paths -q`
Expected: PASS（4 个测试：with_root_sets_root / layout_paths_are_derived_from_root / new_reads_devkit_root_env / new_falls_back_to_default_root + 平台相关的 default_root 测试）

- [ ] **Step 5: 全量回归 + 提交**

Run: `cargo test -q`（预期全绿，114 个测试）、`cargo clippy --all-targets -- -D warnings`（0 警告）、`cargo fmt --check`（clean）
Expected: 全部通过

```bash
git add src/core/paths.rs
git commit -m "feat: 默认安装根目录按平台区分（Linux /opt/.devkit）"
```

---

### Task 2: ensure_writable 权限校验 + install 入口接线

**Files:**
- Modify: `src/core/paths.rs`（新增 `ensure_writable()` + 1 个单元测试）
- Modify: `src/commands/install.rs`（`run()` 开头调用）
- Test: `tests/cli_install.rs`（追加 1 个集成测试）

**Interfaces:**
- Consumes: 无（不依赖 Task 1 产出；`default_root()` 由 `new()` 内部使用）
- Produces: `pub fn ensure_writable(&self) -> Result<()>` —— 创建 root 目录并探针验证可写，失败返回含固定提示文案的错误；`commands/install.rs` 消费

- [ ] **Step 1: 写失败测试**

`src/core/paths.rs` 测试模块新增：

```rust
#[test]
fn ensure_writable_creates_root_and_probes() {
    let dir = tempfile::tempdir().unwrap();
    let nested = dir.path().join("a/b");
    let paths = DevkitPaths::with_root(nested.clone());
    paths.ensure_writable().unwrap();
    assert!(nested.is_dir());
    // 幂等：已存在且可写时再次调用成功
    paths.ensure_writable().unwrap();
    // 探针文件已清理
    assert!(!nested.join(".devkit-write-probe").exists());
}
```

`tests/cli_install.rs` 末尾追加（文件顶部已有 `use assert_cmd::Command;`，predicates 用全限定名）：

```rust
#[test]
#[cfg(unix)]
fn install_reports_permission_hint_when_root_unwritable() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o500)).unwrap();
    Command::cargo_bin("cli")
        .unwrap()
        .env("DEVKIT_ROOT", dir.path())
        .arg("install")
        .arg("java")
        .assert()
        .failure()
        .stderr(predicates::prelude::predicate::str::contains("创建安装目录"))
        .stderr(predicates::prelude::predicate::str::contains("提示"));
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test --lib core::paths::ensure_writable -q && cargo test --test cli_install -q`
Expected: FAIL —— `ensure_writable` 方法未定义（编译错误）；集成测试同样编译失败

- [ ] **Step 3: 最小实现**

`src/core/paths.rs` 的 `DevkitPaths` impl 中 `new()` 之后新增：

```rust
    /// 确保安装根目录存在且可写；失败时返回含权限指引的错误信息
    pub fn ensure_writable(&self) -> Result<()> {
        let hint = "提示: 请使用 sudo 运行，或设置 DEVKIT_ROOT 指定可写目录，例如 DEVKIT_ROOT=$HOME/.devkit";
        std::fs::create_dir_all(&self.root).map_err(|e| {
            anyhow!("创建安装目录 {} 失败: {e}\n{hint}", self.root.display())
        })?;
        // create_dir_all 对已存在目录不校验权限，写探针文件确认真实可写
        let probe = self.root.join(".devkit-write-probe");
        std::fs::write(&probe, b"").map_err(|e| {
            anyhow!("安装目录 {} 不可写: {e}\n{hint}", self.root.display())
        })?;
        std::fs::remove_file(&probe).ok();
        Ok(())
    }
```

`src/commands/install.rs` 的 `run()` 第一行（`let tool = match tool {` 之前）插入：

```rust
    let paths = crate::core::paths::DevkitPaths::new()?;
    paths.ensure_writable()?;
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test --lib core::paths -q && cargo test --test cli_install -q`
Expected: PASS（paths 4+1 个单测；cli_install 2 个集成测试：非 TTY 提示 + 权限提示）

- [ ] **Step 5: 全量回归 + 提交**

Run: `cargo test -q`（预期全绿，116 个测试）、`cargo clippy --all-targets -- -D warnings`、`cargo fmt --check`
Expected: 全部通过

```bash
git add src/core/paths.rs src/commands/install.rs tests/cli_install.rs
git commit -m "feat: install 前置校验安装目录可写并给出权限提示"
```

---

### Task 3: README 默认路径文档同步

**Files:**
- Modify: `README.md:125`（环境变量表）、`README.md:136-144`（目录结构）

**Interfaces:**
- Consumes: 无（纯文档）

- [ ] **Step 1: 更新环境变量表**

`README.md` 第 125 行替换为：

```markdown
| `DEVKIT_ROOT` | 自定义安装根目录（默认 Linux `/opt/.devkit`，macOS/Windows `~/.devkit`），适合多环境隔离或 CI 测试 |
```

- [ ] **Step 2: 更新目录结构**

`README.md` 第 136-144 行代码块替换为：

````markdown
## 目录结构

```
~/.devkit/            # macOS / Windows 默认；Linux 默认为 /opt/.devkit
├── config.json          # 已安装工具与激活版本记录
├── cache/               # 下载的归档文件
├── <tool>/<version>/    # 各工具安装目录
└── current/<tool>/      # 指向当前激活版本的软链（PATH 注入点）
```

> Linux 从旧版升级后默认目录为 `/opt/.devkit`：原有 `~/.devkit` 数据可通过 `DEVKIT_ROOT=$HOME/.devkit` 继续访问，或自行迁移（如 `sudo mv ~/.devkit /opt/.devkit`）。普通用户无 `/opt` 写权限时，请使用 `sudo` 运行或设置 `DEVKIT_ROOT`。
````

- [ ] **Step 3: 验证文档渲染**

Run: `grep -n "\.devkit" README.md`
Expected: 125 行、139 行已更新；131 行 `DEVKIT_ROOT=/data/devkit` 示例保持不变；新增引用块存在

- [ ] **Step 4: 提交**

```bash
git add README.md
git commit -m "docs: README 同步 Linux 默认安装根目录 /opt/.devkit"
```

---

## 验证汇总

- 单元测试：`default_root` 平台分支 ×2、`new_falls_back_to_default_root`（改造）、`ensure_writable` ×1
- 集成测试：`install_reports_permission_hint_when_root_unwritable` ×1（cfg unix）
- 总数：112 → 116（Task 1 +2、Task 2 +2）
- Linux CI（ubuntu runner）实际执行 Linux 分支断言；macOS/Windows 执行各自分支
