# help 展示本机系统信息设计

> 状态: 已确认（2026-08-10）
> 决策记录: `cli` 无参数 / `-h` / `--help` / `help` 时，在 help 的「版本」行下方新增一行「系统」信息（系统名称+版本、CPU 架构）；麒麟操作系统优先读取 /etc/.productinfo

## 目标

在 `cli` 无参数或 help 场景的版本信息下面，增加一行展示本机操作系统信息：系统名称+版本（含麒麟 SP/代号）、CPU 架构。

## 背景

- 现有 help 模板（lib.rs `help_template`）：`{about-with-newline}\n版本: {version}\n\n{usage-heading} {usage}\n\n{all-args}{after-help}`，版本行为第二行。
- clap 4 的 help 模板为静态字符串，不支持动态插入系统信息。
- `cli version` 命令已打印 `平台: {os} ({arch})`（编译期枚举，无系统版本号），本次不修改。
- 现有集成测试 `no_args_prints_version_in_help` 期待无参数时 failure（stderr 含「版本: 0.1.0」），行为需保持。

## 设计

### 方案：渲染后注入

在 `main.rs` 拦截 help 场景（无参数 / `-h` / `--help` / `help`），调用 `Cli::command().render_help()` 得到完整 help 字符串，在 `版本: {version}` 行后插入系统信息行，再输出。clap 渲染部分保持原样。

- 无参数 → stderr + exit(2)（保持现有 failure 语义）
- `-h` / `--help` / `help` → stdout + exit(0)
- `cli help <子命令>` 不拦截（子命令 help 无版本行，无需注入）

### 新模块：`src/core/system_info.rs`

对外接口：

- `pub fn os_display() -> String`：系统名称+版本；所有检测失败逐级回退，永不 panic
- `pub fn help_line() -> String`：`系统: {os_display} ({arch})`，arch 复用 `Platform::detect().arch_name()`（x86_64 / aarch64）

检测流程：

| 平台 | 流程 | 输出示例 |
|------|------|---------|
| macOS | 执行 `sw_vers`，解析 `ProductName` + `ProductVersion` | `macOS 15.5` |
| Linux | ① 存在 `/usr/bin/nkvers` → 麒麟系统：读取 `/etc/.productinfo`（key=value 格式），解析 `ProductName=` + `ProductVersion=` 组合基础名，再从 `ProductVersionInfo[i]` 数组提取 `(SPx)` 与代号（如 `(Halberd)`）全部拼接 → ② 否则常规 `/etc/os-release`（`NAME=` + `VERSION_ID=`，去引号） | `Kylin Linux Advanced Server V10 (SP1) (Halberd)` / `Ubuntu 22.04` |
| Windows | 执行 `cmd /c ver`，正则提取版本号 | `Windows 10.0.22631` |

### 错误处理

| 场景 | 行为 |
|------|------|
| `/usr/bin/nkvers` 不存在 | 走常规 os-release 流程 |
| `/etc/.productinfo` 缺失/解析失败 | 回退 `Kylin Linux`（nkvers 存在即判定麒麟） |
| `/etc/os-release` 缺失/解析失败 | 回退 `Linux` |
| `sw_vers` 执行失败 | 回退 `macOS` |
| `ver` 执行失败/正则不匹配 | 回退 `Windows` |
| 架构检测 | 复用 `Platform::detect()`（cfg! 编译期，本机场景始终正确） |

### 测试计划

解析函数设计为纯函数（输入字符串/路径），便于单测：

- 单测 `core/system_info.rs`：
  - `parse_productinfo`：完整字段（含 `ProductVersionInfo[1]=(SP1)`、`ProductVersionInfo[2]=(Halberd)` 拼接）、缺 ProductVersion、空内容
  - `is_kylin(path)`：参数化路径（临时目录建/不建 nkvers 文件）
  - `parse_os_release`：带引号/无引号、缺 VERSION_ID
  - `parse_sw_vers`：标准输出解析
  - `parse_ver_output`：中文/英文 Windows 输出
- 集成测试 `tests/cli_version.rs` 追加：
  - `cli --help` → stdout 含 `系统: ` 且含本机 OS 名（cfg! 动态构造，不写死平台）
  - `cli help` → stdout 含 `系统: `
  - `cli` 无参数 → stderr 含 `系统: `（保持 failure）

### 范围边界

- 不修改 `cli version` 命令输出（已有「平台」行，保持现状）
- 不新增顶层 `cli info` 命令（曾考虑的方案，用户已调整为 help 内展示）
- 不展示内核版本、主机名、shell 等信息（用户明确仅需系统名称+版本与 CPU 架构）
- 麒麟判定仅用于展示信息，不影响安装、平台枚举等既有逻辑
