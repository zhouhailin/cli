# 默认安装根目录调整设计文档

**日期**：2026-08-08
**状态**：已批准
**版本**：v0.1.4 目标功能

## 1. 背景

CLI 工具的安装根目录默认是用户主目录下的 `~/.devkit`。Linux 服务器场景（root 运行、多用户共享）下，工具链装在用户目录不利于系统级使用与统一管理。需求：**Linux 默认安装根目录调整为 `/opt/.devkit`**，macOS/Windows 保持 `~/.devkit` 不变。

## 2. 需求

1. **默认路径按平台区分**：Linux 默认 `/opt/.devkit`；其他平台保持 `~/.devkit`
2. **环境变量机制不做调整**：`DEVKIT_ROOT` 覆盖优先级链完全不变（非空 → 用之；未设置 → 默认路径），不新增 `CLI_HOME`/`DEVKIT_HOME`
3. **权限错误明确提示，不降级**：Linux 无 `/opt` 写权限时明确报错并给出指引，绝不静默回退 `~/.devkit`

## 3. 关键事实

- 当前解析链（`src/core/paths.rs` `DevkitPaths::new()`）：`DEVKIT_ROOT`（非空）→ `home_dir()?.join(".devkit")`
- 所有布局（config.json、`<tool>/<version>`、`current/<tool>` 链接、etc、services、cache）均从 root 派生，root 变化自动跟随
- shell 注入行（PATH/JAVA_HOME）内容来自实际 root 值（`<root>/current/<tool>/bin`），非硬编码，默认值变化后注入自动指向新位置
- root 目录为**惰性创建**：config save / download / cache 各自 `create_dir_all`，失败时返回原始 io 错误，缺少权限指引

## 4. 设计

### 4.1 路径解析修订（paths.rs）

新增 `pub(crate) fn default_root() -> Result<PathBuf>`（与现有 `home_dir()` 同模式，平台分支 + 单一职责）：

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

`DevkitPaths::new()` 解析链变为两步（优先级不变）：

```
DEVKIT_ROOT（非空） → default_root() 平台默认
```

### 4.2 权限错误处理（报错不降级）

root 目录惰性创建导致错误散落深层且缺指引。在 **install 命令入口集中前置检查**：

- `commands/install.rs` 的 `run()` 开头调用新增的 `DevkitPaths::ensure_writable()`（内部 `create_dir_all(root)`），失败时返回包装错误：

  ```
  创建安装目录 /opt/.devkit 失败: 权限被拒绝 (os error 13)
  提示: 请使用 sudo 运行，或设置 DEVKIT_ROOT 指定可写目录，例如 DEVKIT_ROOT=$HOME/.devkit
  ```

- 其他命令（use/uninstall/service）不新增检查：通常在安装成功后运行，root 已存在；目录缺失/不可写时现有错误已足够
- 不降级：绝不静默回退 `~/.devkit`，行为可预期

### 4.3 边界行为

| 场景 | 行为 |
|------|------|
| Linux 普通用户 install | 明确报错 + 提示 sudo / DEVKIT_ROOT |
| 已有 `~/.devkit` 旧环境（Linux） | 不迁移；可通过 `DEVKIT_ROOT=$HOME/.devkit` 继续访问，或手动 mv 数据到 `/opt/.devkit`（文档说明） |
| macOS / Windows | 行为完全不变 |
| `DEVKIT_CACHE_DIR` | 缓存 env 覆盖逻辑不变 |
| version 输出"根目录" | 自动跟随 root 值，无需改动 |

### 4.4 测试计划

**单元测试（paths.rs）**：

| 测试 | 断言 | 平台守卫 |
|------|------|----------|
| `default_root_linux_is_opt_devkit` | 返回 `/opt/.devkit` | `#[cfg(target_os = "linux")]` |
| `default_root_other_platforms_is_home_devkit` | 返回 `HOME/.devkit` | `#[cfg(not(target_os = "linux"))]` |
| `new_falls_back_to_default_root`（改现有测试） | 无 DEVKIT_ROOT 时返回 default_root() 而非写死 home | 按平台分支断言 |
| `ensure_writable_creates_root` | tempdir 下创建成功、已存在时幂等 | 无 |

**集成测试（权限提示）**：
- `#[cfg(unix)]` 守卫（Windows 无 chmod 语义）
- `DEVKIT_ROOT` 指向 `chmod 0o500` 只读 tempdir 模拟 `/opt` 不可写 → `cli install java` → 断言退出非 0 且错误包含"提示"文案
- CI runner 非 root，chmod 只读有效

### 4.5 文档更新（README）

- 环境变量表 `DEVKIT_ROOT` 行：默认 `~/.devkit` → 默认 Linux `/opt/.devkit`，macOS/Windows `~/.devkit`
- 目录布局示例：标注平台差异 + 旧数据访问方式
- 其他含 `~/.devkit` 的说明同步更新

## 5. 影响文件

| 文件 | 变更 |
|------|------|
| `src/core/paths.rs` | 新增 `default_root()`、`ensure_writable()`；`new()` 改两步解析；测试 |
| `src/commands/install.rs` | 入口调用 `ensure_writable()` 包装错误 |
| `tests/cli_install.rs` | 追加权限提示集成测试 |
| `README.md` | 默认路径文档 |

## 6. YAGNI

- 不做 `CLI_HOME`/`DEVKIT_HOME` 新环境变量（用户明确不做调整）
- 不做旧环境自动迁移
- 不做默认路径检测/回退（不降级）
- 不修改 shell.rs、config.rs、download.rs（root 值自动跟随）
