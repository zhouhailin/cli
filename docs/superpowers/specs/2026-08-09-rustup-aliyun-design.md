# Rust (rustup) 安装功能设计

> 状态: 已确认（2026-08-09）
> 决策记录: 安装位置纳入 ~/.devkit 管理（RUSTUP_HOME/CARGO_HOME）；仅提供 install，不纳入 list/uninstall

## 目标

为 `cli` 增加 `cli install rust` 子命令，通过 rustup-init 脚本安装 Rust 工具链，支持**阿里源**与**官方源**双源选择，安装到 devkit 自管理目录并持久化环境变量与 PATH。

## 背景

- Rustup 是 Rust 官方的跨平台 Rust 安装工具，通过安装脚本（rustup-init.sh）引导安装，与现有「下载压缩包 + 解压」的工具（java/node/go）安装方式不同，需单独实现。
- 阿里云镜像提供：
  - 安装脚本: `https://mirrors.aliyun.com/repo/rust/rustup-init.sh`
  - 发行版镜像根: `https://mirrors.aliyun.com/rustup/`（即 `RUSTUP_DIST_SERVER` 指向的目录）
  - 更新元数据: `https://mirrors.aliyun.com/rustup/rustup`（即 `RUSTUP_UPDATE_ROOT`）
- 参考先例: go.rs 的阿里云镜像模式（官方 API 列版本 + 镜像下载 + 安装后注入 GOPROXY）；shell.rs 的 `inject_env_var`/`inject_path` 幂等注入能力。

## 架构与命令面

- 新增模块 `src/core/tools/rust.rs`，遵循 tools 层模式（只依赖 core 层）。
- `cli install rust` 接线：
  - [src/commands/install.rs](src/commands/install.rs) 的 `TOOL_CHOICES` 增加 `("Rust (rustup)", "rust")`（数组长度 6 → 7），match 分发增加 `"rust" => rust::install(None)`。
- 平台限制：仅 macOS/Linux。Windows 上执行 `rust::install` 报「Windows 暂不支持，请手动运行 rustup-init.exe」。

## 安装流程

### 1. 已安装检测

`DevkitPaths::root()/rustup` 目录存在 → 返回错误「Rust (rustup) 已安装于 <path>，如需重装请先手动删除该目录与 rc 中相关注入」。

### 2. 源选择

- TTY 交互：`interact::select("请选择 Rust 下载源", &["阿里源（国内加速）", "官方源"])`。
- 非 TTY：默认**官方源**，打印说明「非交互模式默认使用官方源，可通过交互模式选择阿里源」。

### 3. 执行安装命令

统一追加 `-y --no-modify-path`（非交互安装；禁止 rustup 修改 rc 文件，PATH 由 cli 统一注入，保证集中管理）。

- 官方源: `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path`
- 阿里源: `curl --proto '=https' --tlsv1.2 -sSf https://mirrors.aliyun.com/repo/rust/rustup-init.sh | sh -s -- -y --no-modify-path`

执行细节：
- 用 `std::process::Command::new("sh")` + `-c` 执行上述管道命令（macOS/Linux 均内置 sh）。
- 进程级注入环境变量（`Command::env`，不污染 cli 自身进程）：
  - `RUSTUP_HOME=<root>/rustup`
  - `CARGO_HOME=<root>/cargo`
  - 阿里源额外注入：`RUSTUP_UPDATE_ROOT=https://mirrors.aliyun.com/rustup/rustup`、`RUSTUP_DIST_SERVER=https://mirrors.aliyun.com/rustup`
- stdout/stderr 透传（继承），非零退出码 → 返回错误「Rust 安装失败，退出码 <n>」。
- 测试钩子：环境变量 `DEVKIT_RUSTUP_SCRIPT` 非空时覆盖脚本 URL（集成测试用 mock HTTP server 提供假脚本）。

### 4. 环境变量与 PATH 持久化（rc 文件 devkit 块，幂等）

安装成功后写入 shell rc（`rc_file_for_shell()` 检测，仅 macOS/Linux）：
- `inject_env_var(rc, "RUSTUP_HOME", "<root>/rustup")`
- `inject_env_var(rc, "CARGO_HOME", "<root>/cargo")`
- 阿里源额外：`inject_env_var(rc, "RUSTUP_UPDATE_ROOT", "https://mirrors.aliyun.com/rustup/rustup")`、`inject_env_var(rc, "RUSTUP_DIST_SERVER", "https://mirrors.aliyun.com/rustup")`
- `inject_path(rc, "<root>/cargo/bin")`

最后打印 `print_activation_hint()`（新终端或 source rc 后生效）。

## 错误处理

| 场景 | 行为 |
|------|------|
| 已安装（root/rustup 存在） | 报错退出，提示手动卸载 |
| Windows | 报错退出，提示手动运行 rustup-init.exe |
| 脚本执行失败（非零退出码） | 报错退出；已注入的 rc 内容不清理（安装可能部分完成，留待用户处理） |
| rc 写入失败 | 打印警告「安装成功，但环境变量写入 <rc> 失败: <e>，请手动配置」，仍返回成功（安装本身已完成） |
| 非 TTY 无默认选择 | 使用官方源（安全默认） |

## 测试

### 单元测试（rust.rs 内，`#[cfg(test)]`）

1. `install_command_official_source`：官方源命令字符串精确匹配。
2. `install_command_aliyun_source`：阿里源命令字符串精确匹配（含 `mirrors.aliyun.com/repo/rust/rustup-init.sh`）。
3. `install_command_uses_script_override`：`DEVKIT_RUSTUP_SCRIPT` 覆盖脚本 URL。
4. `aliyun_env_vars_returns_two_entries`：`RUSTUP_UPDATE_ROOT`/`RUSTUP_DIST_SERVER` 精确值。
5. `common_env_vars_include_home_and_cargo`：`RUSTUP_HOME`/`CARGO_HOME` 指向 root/rustup、root/cargo。
6. `rustup_home_dir_from_root`：root 拼接正确。

### 集成测试（tests/cli_rust.rs 新增）

- mock HTTP server 提供假脚本（内容 `#!/bin/sh` + 标记输出，如 `echo mock-rustup-done`）。
- 环境隔离：`HOME`、`DEVKIT_ROOT`、`SHELL=/bin/zsh` 指向 tempdir（serial 环境变量测试，防并行污染）。
- 用例：
  1. 非 TTY 默认官方源：`cli install rust`（stdin 关闭）→ stdout 含 mock 标记、rc 文件含 `RUSTUP_HOME`/`CARGO_HOME`/`cargo/bin` PATH 行、不含 `RUSTUP_UPDATE_ROOT`。
  2. 已安装检测：预先创建 `DEVKIT_ROOT/rustup` → 报「已安装」。
  3. 阿里源选择：TTY 交互难以在集成测试模拟，仅验证非 TTY 默认官方源路径；阿里源命令与环境变量由单元测试覆盖。

## 范围边界（YAGNI）

- 不纳入 `cli uninstall` / `cli use`（rustup 自带 toolchain 管理；用户已确认仅 install）。
- `cli list` 展示：安装成功后写入 config.json（`installed.rust = ["rustup"]`），供 list 显示（2026-08-09 用户确认纳入展示）。
- 不配置 crates.io 稀疏索引镜像（用户未要求）。
- 不做官方源 fallback。
- Windows 暂不支持（与 shell rc 注入机制一致）。

## 文档

- README 的安装列表增加 Rust (rustup) 与双源说明；环境变量表增加 RUSTUP_HOME/CARGO_HOME/RUSTUP_UPDATE_ROOT/RUSTUP_DIST_SERVER。
