# cli — 跨平台开发环境一键安装工具

[![CI](https://github.com/zhouhailin/cli/actions/workflows/ci.yml/badge.svg)](https://github.com/zhouhailin/cli/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/zhouhailin/cli)](https://github.com/zhouhailin/cli/releases)

一条命令安装并管理 Java / Node.js / Go / Maven 等开发环境，支持多版本共存与一键切换，自动注入 PATH，无需手动配置环境变量。

## 功能特性

- **多工具支持**：Java（6 大发行版）、Node.js、Go、Maven
- **多版本共存**：同一工具可安装多个版本，`use` 命令一键切换
- **实时版本解析**：下载地址与最新补丁版本每次运行时从官方 API 拉取，永远装到最新
- **官方渠道直链**：华为云镜像、阿里 OSS、nodejs.org、Apache 等官方源，国内可达
- **SHA-256 完整性校验**：毕昇 JDK 自动校验，防止下载损坏或被篡改
- **下载自动重试**：网络失败自动退避重试 3 次
- **跨平台**：macOS / Linux / Windows（x86_64 / aarch64）
- **PATH 自动注入**：安装后自动写入 shell 配置文件（`.zshrc` / `.bashrc`）
- **调试日志**：`CLI_DEBUG=true` 输出完整下载安装过程

## 安装

从 [Releases 页面](https://github.com/zhouhailin/cli/releases) 下载对应平台的二进制：

| 平台 | 文件 |
|------|------|
| Linux x86_64 | `cli-linux-x64` |
| Linux aarch64 | `cli-linux-arm64` |
| macOS x86_64 | `cli-macos-x64` |
| macOS aarch64 | `cli-macos-arm64` |
| Windows x86_64 | `cli-windows-x64.exe` |

> Linux 二进制为 **musl 静态编译**，不依赖系统 glibc，可在 CentOS 7 等旧版本 Linux 上直接运行。

macOS / Linux 赋予执行权限后即可使用：

```bash
chmod +x cli-macos-arm64
sudo mv cli-macos-arm64 /usr/local/bin/cli
```

## 快速开始

```bash
# 交互式安装 Java（选择发行版与版本）
cli install java

# 交互式安装 Node.js
cli install node

# 查看已安装的工具
cli list

# 切换 Java 版本
cli use java 21
```

安装完成后重新打开终端（或 `source ~/.zshrc`），即可直接使用 `java`、`node`、`go`、`mvn` 等命令。

## 命令详解

### `cli install <tool>`

交互式安装工具，支持：`java`、`node`、`go`、`maven`。

- **java**：先选择 JDK 发行版，再选择大版本（8/11/17/21/25 视发行版而定），下载时实时解析最新补丁版本
- **node / go / maven**：从官方 API 拉取版本列表交互选择

安装流程：下载（带重试）→ SHA-256 校验（毕昇）→ 解压 → 剥离单顶层目录 → 注册配置 → 注入 PATH。

### `cli use <tool> [version]`

切换已安装版本的激活状态（不填版本则交互选择）：

```bash
cli use java 17      # 直接指定
cli use node         # 交互选择
```

### `cli list`

列出所有已安装工具与版本，标记当前激活版本。

### `cli version`

显示版本号、平台与根目录。

## 支持的 Java 发行版

| 发行版 | 下载渠道 | 支持版本 | 平台限制 |
|--------|---------|---------|---------|
| Dragonwell（阿里） | 官方 releases.json（OSS 优先） | 8 / 11 / 17 / 21 / 25 | 不支持 macOS |
| Bisheng 毕昇（华为） | 鲲鹏官网 API + 华为云镜像 | 8 / 11 / 17 / 21 | 仅 Linux |
| Temurin（Eclipse Adoptium） | Adoptium API | 8 / 11 / 21 | - |
| Zulu（Azul） | Azul API | 8 / 11 / 21 | - |
| Liberica（BellSoft） | BellSoft API | 8 / 11 / 21 | - |
| Kona（腾讯） | GitHub Releases | 8 / 11 / 17 / 21 | - |

## 环境变量

| 变量 | 说明 |
|------|------|
| `DEVKIT_ROOT` | 自定义安装根目录（默认 `~/.devkit`），适合多环境隔离或 CI 测试 |
| `CLI_DEBUG=true` | 输出调试日志到 stderr：下载地址、文件字节数、SHA-256 校验、解压与安装路径等 |

```bash
DEVKIT_ROOT=/data/devkit cli install java    # 安装到自定义目录
CLI_DEBUG=true cli install node              # 观察完整下载过程
```

## 目录结构

```
~/.devkit/
├── config.json          # 已安装工具与激活版本记录
├── cache/               # 下载的归档文件
├── <tool>/<version>/    # 各工具安装目录
└── current/<tool>/      # 指向当前激活版本的软链（PATH 注入点）
```

## 从源码构建

```bash
git clone git@github.com:zhouhailin/cli.git
cd cli
cargo build --release
./target/release/cli --help
```

## 开发与发布

- **测试**：`cargo test`（91 个单元与集成测试），`cargo clippy --all-targets -- -D warnings`，`cargo fmt --check`
- **CI**：推送 `main` 或发起 PR 自动运行 test / clippy / fmt（[ci.yml](.github/workflows/ci.yml)）
- **发布**：打 tag 自动构建 5 平台二进制并创建 GitHub Release（[release.yml](.github/workflows/release.yml)）：

```bash
git tag v0.1.0 && git push origin v0.1.0
```

发布前自动执行测试门禁，失败则不发布；Release 自动附带自上个 tag 起的变更日志。
