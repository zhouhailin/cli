# cli —— 跨平台开发环境安装器 · 设计文档

**日期**: 2026-08-08 | **状态**: 已批准 | **版本**: 1.0.0

## 概述

`cli` 是一个跨平台（macOS / Linux / Windows）的开发环境一键安装 CLI 工具。它以自管理目录（`~/.devkit/`）为核心，实现 Java（多发行版多版本）、Maven、Node.js、Go、Redis（源码编译 + 服务 + 哨兵/集群）、MySQL（官方二进制 + 交互配置 + 服务）的安装、版本切换与服务管理，并支持通过 GitHub Releases 自更新。

## 架构

### 总体结构

```
cli (单一 Rust 二进制, clap 命令分发)
├── core 层（跨工具共享）
│   ├── platform    — macOS/Linux/Windows 平台抽象与检测
│   ├── download    — 下载 + SHA-256 校验 + tar.gz/zip 解压 + 原子安装
│   ├── shell       — 环境变量注入（.zshrc/.bashrc/Windows 注册表，幂等带标记）
│   ├── config      — ~/.devkit/config.json 持久化（已装版本/激活版本/镜像配置）
│   ├── service     — 服务注册统一接口（launchd / systemd / Windows sc）
│   └── interact    — dialoguer 交互选择（发行版/版本/确认）
└── tools 层（每工具一个模块，只依赖 core）
    ├── java / maven / node / go / redis / mysql
    └── self_update
```

### 目录布局

```
~/.devkit/
├── config.json                 # 激活版本、GOPROXY、镜像设置
├── java/<vendor>/<version>/    # 如 java/dragonwell/21
├── maven/<version>/            # 含自定义 settings.xml（国内源）
├── node/<version>/  go/<version>/
├── redis/<version>/            # 源码编译产物
├── mysql/<version>/            # 官方二进制 + data/ + my.cnf
├── etc/                        # redis.conf / sentinel.conf / 集群配置
└── services/                   # 生成的 launchd plist / systemd unit
```

### 命令面

```
cli install java|maven|node|go|redis|mysql   # 交互式安装（各工具自己的流程）
cli list                                      # 已安装工具与版本
cli use <tool> <version>                      # 切换激活版本
cli service start|stop|status|restart <tool> [instance]
cli redis sentinel init <端口>                # 哨兵部署
cli redis cluster create <节点:端口...>       # 集群部署
cli uninstall <tool> [version]
cli self-update                               # 手动更新（GitHub Releases）
cli version                                   # 版本信息 + 自动检查更新
```

环境变量注入：`JAVA_HOME`、`PATH` 追加 `~/.devkit/<tool>/<version>/bin`。激活版本用符号链接 `~/.devkit/current/` 统一管理，切换即换链接，不重写配置。

## 各工具设计

### Java

交互选发行版 → 选版本（8/11/21，该发行版不提供则跳过）→ 下载校验解压 → 注入 `JAVA_HOME` + PATH。

发行版与下载源：

| 发行版 | 下载源 |
|---|---|
| Temurin | Adoptium API（提供 checksum） |
| Zulu | Azul API |
| Liberica | BellSoft API |
| Dragonwell（阿里） | GitHub Releases（v8/11/21 标签） |
| Bisheng（华为） | 华为云 / GitHub Releases |
| Kona（腾讯） | GitHub Releases（TencentKona） |

统一抽象为 `Vendor { name, versions(), download_url(platform, arch, version) }`。

### Maven

从 Apache 官方目录拉版本列表 → 用户选择 → 下载解压 → 询问是否配置国内源（阿里云 `https://maven.aliyun.com/repository/public` 写进 `settings.xml`，`mirrorOf: central`）→ 注入 PATH。

### Node.js

拉 `nodejs.org/dist/index.json` → 过滤 LTS 行展示（版本号/代号/维护状态）→ 下载对应平台 tarball/zip → 解压 → `cli use node <v>` 切换（symlink 机制）。

### Go

拉 `go.dev/dl/?mode=json` 稳定版列表 → 选择 → 下载解压 → 询问 GOPROXY 配置（默认 `https://goproxy.cn,direct`，`go env -w` 写入）→ PATH 注入。

### Redis（仅 macOS/Linux）

拉版本列表 → 选择 → 下载源码 → 本地 `make -j` 编译安装到自管理目录 → 生成 `redis.conf`（端口/持久化/日志路径）→ 询问注册系统服务。

- **哨兵**: `cli redis sentinel init` 生成 `sentinel.conf` + 哨兵服务实例
- **集群**: `cli redis cluster create` 输入节点列表，生成 N 份节点配置（cluster-enabled yes）+ 用 `redis-cli --cluster create` 自动建集群（无需 Ruby 依赖）

### MySQL（仅 8.x）

列出 8.x 官方版本 → 选择 → 交互输入端口 / root 密码 / 数据目录 / 字符集（默认 utf8mb4）→ 下载官方二进制包 → `mysqld --initialize-insecure` 初始化 → 生成 `my.cnf` → 注册服务。

### 服务管理（统一接口）

| 平台 | 方式 |
|---|---|
| macOS | launchd `~/Library/LaunchAgents/com.devkit.<tool>.<instance>.plist`，`launchctl bootstrap` 注册，免 sudo |
| Linux | systemd user unit（`~/.config/systemd/user/`）+ `systemctl --user enable --now`，提示 `loginctl enable-linger` 开机自启 |
| Windows | MySQL 用 `sc.exe` 注册原生服务（Redis 不支持 Windows） |

服务实例命名空间：`cli service start redis main`、`cli service start redis sentinel-26379`、`cli service start mysql main`。

### 自更新（GitHub Releases）

- `cli version` 启动时后台检查 GitHub API 最新 release tag，有新版提示
- `cli self-update` 手动触发：拉取目标平台二进制 → 校验 → 原子替换自身（macOS/Linux 替换 `$argv[0]` 解析路径；Windows 先改名旧文件再替换绕过占用锁）

## 错误处理与安全

- 下载优先 SHA-256 校验（Adoptium/Azul/BellSoft/Node/go.dev 均提供 checksum）；Redis 源码无官方 checksum，至少校验解压产物关键二进制存在
- 幂等：已安装版本检测 → 重复安装提示"已存在，重新安装/跳过"；下载用 `.part` 临时文件 + 原子重命名，中断可重试
- 网络容错：失败自动重试 3 次（指数退避）；提供 `--mirror` 全局参数
- symlink 原子替换（先临时链接再 rename），失败自动回滚
- 服务操作前检测端口占用；Linux 检查 systemd 可用性并给出降级提示
- MySQL root 密码交互输入不回显；只存本地 `my.cnf`（权限 600），不入 config.json
- 平台不支持的操作用中文给出明确错误信息

## 测试策略

| 层 | 内容 |
|---|---|
| 单元测试 | 版本解析/比较、配置读写、路径布局、环境注入脚本生成、服务 unit 文件内容快照 |
| 集成测试 | mock HTTP server 模拟各发行版 API 与下载；解压/校验/原子安装流程；symlink 切换与回滚 |
| 服务测试 | 只断言生成的 plist/unit 内容，不真实注册 |
| 端到端 | CI 三平台构建 + 冒烟测试（`cli list`/`cli version`） |

## 开发阶段

- **P0 骨架**: 项目脚手架、core 层（platform/download/shell/config）、`cli list/version` 可用
- **P1 语言工具**: java → node → go → maven（依赖 core 成熟逐个落地）
- **P2 服务型工具**: mysql → redis（含哨兵/集群）→ service 子命令
- **P3 收尾**: self-update、GitHub Actions 三平台发布流水线、README（中文）

## 工程流程

1. 本设计已通过用户批准（2026-08-08）
2. Speckit 已初始化（`.specify/`，lingma 集成），宪法已批准（中文 v1.0.0）
3. 下一步：writing-plans 生成实施计划 → TDD 逐模块开发 → 分阶段验证
