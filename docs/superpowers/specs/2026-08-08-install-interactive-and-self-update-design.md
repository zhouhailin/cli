# install 无参交互选择、self-update 与版本同步设计

日期：2026-08-08

## 背景与目标

用户反馈三个问题：

1. `cli install` 不带参数时报 clap 参数错误，体验生硬——期望无参时交互弹出工具列表选择。
2. 二进制内版本号固定来自 Cargo.toml（0.1.0），与发布 tag（v0.1.1）脱节——期望发布二进制显示的版本号与 tag 严格一致。
3. 下载的压缩包需要支持自定义存放位置，便于离线拷贝分发。

同时需求规格中规划的 self-update（CLI 自更新）一并实现，其版本比较依赖第 2 点的版本同步。

## 需求

- `cli install` 无参时交互选择工具：Java / Node.js / Go / Maven / 自更新（5 项）
- 新增 `cli self-update` 命令：检查 GitHub Releases 最新版，下载并替换自身二进制
- 版本号与发布 tag 同步：tag v0.1.1 发布的二进制显示 `cli 0.1.1`
- 支持 `DEVKIT_CACHE_DIR` 环境变量指定压缩包缓存目录

## 设计

### 1. 命令层与交互分发（lib.rs / commands/install.rs）

- `Command::Install { tool: Option<String> }`——`<tool>` 变为可选
- 有参：分发逻辑不变（`cli install java`、`cli install self-update` 均可）
- 无参：`select()` 交互列表（中文标签：Java / Node.js / Go / Maven / 自更新），选中后进入对应流程
- 非 TTY 且无参：报错 `请指定工具名，例如: cli install java`，不抛交互错误
- 新增 `Command::SelfUpdate` 子命令（`cli self-update`），与 install 列表入口共用同一实现

### 2. 版本与 tag 同步（lib.rs / release.yml）

- `lib.rs` 定义版本常量并归一化（去掉 `v` 前缀）：
  ```rust
  /// 当前版本：发布构建用 CLI_VERSION（tag），本地开发回退 Cargo.toml 版本
  pub fn current_version() -> &'static str {
      parse_tag(option_env!("CLI_VERSION").unwrap_or(env!("CARGO_PKG_VERSION")))
  }
  ```
  `parse_tag` 为 const 兼容的纯函数（`v0.1.1` -> `0.1.1`，无前缀原样返回），`version` 命令与 self-update 版本比较统一使用 `current_version()`
- `release.yml` build job 增加 job 级环境变量：
  ```yaml
  env:
    CLI_VERSION: ${{ github.ref_name }}   # v0.1.1
  ```
- 本地开发构建不受影响（无 CLI_VERSION 时回退 Cargo.toml 版本）

### 3. DEVKIT_CACHE_DIR 缓存目录配置（paths.rs / installer.rs）

- `DevkitPaths` 新增 `cache_dir()`：`DEVKIT_CACHE_DIR` 非空时原样使用，否则默认 `<root>/cache`
- `installer.rs` 的 `install_archive` 改用 `ctx.paths.cache_dir()`
- 相对路径按当前工作目录解释；目录不存在时自动创建（现有逻辑）
- 压缩包保留行为不变（安装后不删除），仅位置可配置

### 4. self-update 机制（新增 src/core/tools/self_update.rs）

流程：

1. 查询 `https://api.github.com/repos/zhouhailin/cli/releases/latest` 获取 `tag_name`
2. `parse_tag` 去 `v` 前缀，与 `current_version()` 比较（复用 `core::versions::compare`）
3. 无新版：提示 `已是最新版本 (x.y.z)`，退出 0
4. 有新版：显示当前/最新版本，`confirm()` 确认
5. `asset_name(platform)` 纯函数映射当前平台资产名（`cli-linux-x64` 等 5 个命名）
6. 下载到 cache 临时文件（复用 `core::download::download`），成功后原子替换 `std::env::current_exe()`
7. 完成提示，建议重新执行验证

平台差异与安全：

- Unix：直接覆盖运行中的二进制（POSIX 允许）
- Windows：运行中 exe 被锁，下载为 `cli.new.exe` 并提示用户手动替换
- 下载/替换失败：原二进制不受影响（先临时文件、成功后才替换）
- 资产无 SHA-256 校验文件（当前 Release 未生成），跳过校验

### 5. 测试

单元测试（纯函数）：

- `parse_tag`：`v0.1.1 -> 0.1.1`、无前缀原样返回、空串报错
- `asset_name`：5 种平台组合映射正确
- `cache_dir`：`DEVKIT_CACHE_DIR` 覆盖默认值（`#[serial]` 串行化环境变量测试）

集成测试：

- `install` 无参且非 TTY：报错含"请指定工具名"
- `install java` 有参行为不变

### 6. 文档

- README.md：命令详解补充无参交互、`cli self-update`、`DEVKIT_CACHE_DIR` 环境变量说明

## 影响文件

| 文件 | 改动 |
|------|------|
| src/lib.rs | `Install.tool` 改 Option；新增 `SelfUpdate` 子命令；VERSION 双来源 |
| src/commands/install.rs | 无参交互分发；self-update 路由 |
| src/commands/mod.rs | 新增 self_update 命令模块 |
| src/core/tools/self_update.rs | 新增：版本检查/下载/替换 |
| src/core/paths.rs | 新增 `cache_dir()` |
| src/core/installer.rs | 改用 `ctx.paths.cache_dir()` |
| .github/workflows/release.yml | build job 注入 `CLI_VERSION` |
| README.md | 文档更新 |

## 错误处理

- 非 TTY 无参 install：明确提示指定工具名
- 更新检查网络失败：报错退出，不影响现有二进制
- 已是最新版本：正常退出不下载
- Windows 替换限制：提示手动替换步骤

## 测试计划

1. `cargo test` 全量（新增约 8-10 个测试，现有 91 个保持全绿）
2. `cargo clippy --all-targets -- -D warnings`
3. `cargo fmt --check`
4. 推送后 CI 自动验证；打 tag 验证版本号与 tag 同步
