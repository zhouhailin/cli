# CLI UX 增强与 mvnd 工具链设计

**日期**：2026-08-09
**状态**：已批准
**版本**：v0.1.7 目标功能

## 背景

CLI 现有 6 项体验问题与新增需求，一次性整合设计：

1. `cli` 无参数运行时帮助信息不含版本号
2. 安装/切换工具后只有"新终端或 source 后生效"文字，不展示具体生效命令
3. `cli use` 必须显式传工具名，无法像 `install` 一样无参数交互选择
4. Linux 下 `cli update` 后二进制仍可能不可执行（v0.1.5 修复未覆盖"原二进制无执行位"边界），且打包资产未设置执行位
5. 新增 mvnd（Maven Daemon）工具链
6. Go 下载源切换为阿里云镜像（`https://mirrors.aliyun.com/golang/`）

## 目标

- 无参数运行 `cli` 即见版本号
- 安装/use 后展示可执行的具体生效命令（`source <rc 文件>`）
- `cli use` 无参数时交互选择已安装工具与版本
- `cli update` 无条件保证替换后二进制可执行；打包产物带执行位
- `cli install mvnd` 可安装 Maven Daemon（稳定版列表 + sha256 校验 + Java 依赖提示）
- Go 下载走阿里云镜像（国内加速），版本列表仍走 go.dev 官方 JSON

## 1. 无参数运行显示版本

**文件**：`src/lib.rs`

clap 定义增加自定义 `help_template`，在 about 之后插入版本行：

```rust
#[command(
    name = "cli",
    version = current_version(),
    about = "跨平台开发环境一键安装工具",
    help_template = "{about-with-newline}\n版本: {version}\n\n{usage-heading} {usage}\n\n{all-args}{after-help}"
)]
```

- `cli`（无参数）打印帮助时即显示 `版本: 0.1.x`（clap 无子命令时自动打印 help 退出）
- `cli -V/--version` 行为不变
- main.rs 无需改动

## 2. 生效命令提示

**文件**：`src/core/shell.rs`、`src/commands/use_cmd.rs`、`src/core/installer.rs`、`src/core/tools/maven.rs`、`src/core/tools/go.rs`

shell.rs 新增：

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

调用点（统一替换/新增）：

- `use_cmd.rs`：现有固定文案 `提示: 新终端或 source 当前 shell 配置文件后生效` 替换为 `print_activation_hint()`
- `installer.rs` `install_archive` 的 inject 分支：注入 PATH 后调用
- `maven.rs`、`go.rs`：手动注入 PATH 后调用

## 3. cli use 无参数交互

**文件**：`src/lib.rs`、`src/commands/use_cmd.rs`

- `Use { tool: Option<String>, version: Option<String> }`
- `use_cmd::run`：`tool` 为 None 时从 `config.installed` 收集已安装工具名列表：
  - 无任何已安装工具 → 错误提示"尚未安装任何工具，请先执行 cli install"
  - 非 TTY → 与 install 一致提示"请指定工具名，例如: cli use java"（不抛交互错误）
  - TTY → `select("请选择要切换的工具", labels)` 后继续走现有版本选择逻辑
- `tool` 显式传入时行为不变（含版本校验、未安装提示）

## 4. update 权限双重保障

### 4.1 replace_binary 防御重写

**文件**：`src/core/tools/self_update.rs`

```rust
pub fn replace_binary(staging: &Path, exe: &Path) -> Result<()> {
    // 目标权限：优先继承原 exe 权限；读取失败或缺少执行位时用 0755 兜底
    let mode = match std::fs::metadata(exe) {
        Ok(m) => {
            let bits = m.permissions().mode() & 0o777;
            if bits & 0o111 == 0 { 0o755 } else { bits }
        }
        Err(_) => 0o755,
    };
    std::fs::set_permissions(staging, std::fs::Permissions::from_mode(mode))?;
    std::fs::rename(staging, exe)?;
    // 替换后自检：仍无执行位则补（防御边界场景）
    let after = std::fs::metadata(exe)?.permissions().mode() & 0o777;
    if after & 0o111 == 0 {
        std::fs::set_permissions(exe, std::fs::Permissions::from_mode(after | 0o111))?;
    }
    debug_log!("更新后二进制权限: {:o}", mode);
    Ok(())
}
```

语义变化：

- 原 exe 权限含执行位（0755/0700 等）→ 完整保留（现有 2 个测试保持通过）
- 原 exe 无执行位（0644）→ 0755 兜底（v0.1.5 修复的盲区，本次关键修复）
- 替换后自检兜底，保证任何情况下更新后必然可执行

**新增测试（RED 先写）**：

- `replace_binary_ensures_execute_when_original_missing_exec`：原 exe 0644 + staging 0644 → 替换后 mode & 0o111 != 0

### 4.2 release.yml 打包设置执行位

**文件**：`.github/workflows/release.yml`

"重命名为带平台后缀"步骤之后、Alpine 冒烟验证之前，增加：

```yaml
- name: 设置可执行权限
  if: matrix.strip   # 非 Windows 矩阵（strip: false 为 Windows）
  run: chmod +x ${{ matrix.asset }}
```

**边界说明**：裸二进制资产经 HTTP 下载（curl/wget）后权限由客户端 umask 决定（默认 0644），服务端 chmod 不随下载传递。`cli update` 命令内替换已由 4.1 全链路保障。tar.gz 资产或安装脚本（下载即用）本次不做（YAGNI）。

### 4.3 根因待确认

用户报告"新版（v0.1.5/v0.1.6）实测 cli update 后仍丢权限"。4.1 的防御重写覆盖已知盲区（原 exe 无执行位）。发布 v0.1.7 后需服务器实测确认：若仍复现，按系统化调试重新调查（CLI_DEBUG 日志辅助）。

## 5. mvnd 工具链

**文件**：`src/core/tools/mvnd.rs`（新建）、`src/core/versions.rs`、`src/core/tools/maven.rs`、`src/core/tools/mod.rs`、`src/commands/install.rs`、`README.md`

### 5.1 通用版本目录解析提取

maven.rs 的 `parse_maven_versions`（纯数字点分过滤 + 降序）提取为 `core/versions.rs` 的通用函数：

```rust
/// 从目录页 HTML 提取纯数字点分版本目录名（如 3.9.9、1.0.6），降序
pub fn parse_version_dirs(html: &str) -> Result<Vec<String>>
```

- maven.rs 改为调用（现有测试保持通过，行为不变）
- mvnd 复用：自动过滤 `2.0.0-rc-3`、`1.0-m6` 等非纯数字（仅稳定版）

### 5.2 mvnd 模块

- 版本源：`https://mirrors.aliyun.com/apache/maven/mvnd/` 目录页 → `parse_version_dirs`（镜像未同步 .sha256，校验文件仍从 archive 获取）
- URL（平台映射，mvnd 用 amd64/aarch64 命名）：

```rust
pub fn resolve_url(version: &str, platform: &Platform) -> String {
    let os = match platform.os { Os::MacOs => "darwin", Os::Linux => "linux", Os::Windows => "windows" };
    let arch = match platform.arch { Arch::X86_64 => "amd64", Arch::Aarch64 => "aarch64" };
    format!("https://mirrors.aliyun.com/apache/maven/mvnd/{version}/maven-mvnd-{version}-{os}-{arch}.tar.gz")
}
```

- sha256 校验：`GET {url}.sha256` 提取哈希（去除换行）后传给 `install_archive(sha256)`
- `install(version_hint)`：版本 select（"Maven Daemon (mvnd) x.x.x"）→ confirm → sha256 → `install_archive(url, Some(sha), "mvnd", version, ctx, true)` → 检查 `config.active` 无 java 时提示 "mvnd 依赖 Java，请先执行 cli install java"
- 解压目录：`maven-mvnd-<ver>-<os>-<arch>/` 单顶层目录，`flatten_single_top_dir` 自动剥离 → `bin/mvnd`
- Windows 资产同样为 tar.gz（archive 提供 windows-amd64.tar.gz），extract 支持性实现时确认，若不支持则 Windows 平台报错提示（mvnd 主场景为 Linux/macOS 开发机）

### 5.3 接入点

- `tools/mod.rs`：`pub mod mvnd;`
- `install.rs`：TOOL_CHOICES 加 `("Maven Daemon (mvnd)", "mvnd")`，match 加 `"mvnd" => mvnd::install(None)`
- uninstall/list 已 config 驱动，无需改动
- README 工具列表同步

**测试**：

- `parse_version_dirs` 过滤 rc/beta 与降序（含 `2.0.0-rc-3` 剔除断言）
- `resolve_url` 各平台映射（darwin-aarch64 / linux-amd64 / windows-amd64）
- sha256 提取（含换行剥离）
- maven 现有 `parse_maven_versions` 测试改调通用函数后保持通过

## 6. Go 下载源换阿里云

**文件**：`src/core/tools/go.rs`

- `resolve_url` 改为 `https://mirrors.aliyun.com/golang/go{version}.{os}-{arch}.tar.gz`
- `fetch_versions` 保持 `https://go.dev/dl/?mode=json`（镜像无列表 API，官方 JSON 稳定可靠）
- 不做官方 fallback（YAGNI）；阿里云缺文件时下载报错，错误信息含完整 URL 便于排查
- 测试：`resolve_url_macos_arm64` 期望值更新为阿里云 URL

## 测试策略

| 需求 | 测试类型 | 断言 |
|------|---------|------|
| 1 版本显示 | 集成 | `cli` 无参数输出含 `版本: ` 与当前版本号 |
| 2 生效提示 | 单元 + 集成 | `print_activation_hint` 输出含 `source <rc>`；use/install 输出含具体命令 |
| 3 use 交互 | 集成 | 非 TTY `cli use` 输出"请指定工具名"；显式 tool 行为不变 |
| 4 权限 | 单元 | 0644 兜底 0755（RED 先写）；0755/0700 保留（现有） |
| 5 mvnd | 单元 | 解析/URL/sha256 |
| 6 go 镜像 | 单元 | resolve_url 阿里云 |

回归：全量测试 + clippy + fmt；release.yml 变更在发布流水线验证。

## 版本发布

全部完成后发 v0.1.7（流程同 v0.1.6：push main → CI → tag → Release → 端到端验证）。发布后服务器实测：`cli update` 权限保留、`cli install mvnd`、`cli install go` 走阿里云。

## 假设与边界

- mvnd 稳定版判定：纯数字点分目录名（1.0.x）；2.0.0-rc 等不列入
- mvnd 安装不捆绑 Java（依赖提示为主）
- curl 裸二进制下载权限问题不在本次范围（4.2 边界说明）
- use 无参数交互仅列已安装工具（非全部支持工具）
