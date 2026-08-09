# 操作系统镜像下载功能（cli os）设计

**日期**：2026-08-09
**状态**：已批准
**版本**：v0.1.12 目标功能

## 背景

`cli` 目前只覆盖开发工具链（java/node/go/maven/mvnd）的安装。用户希望扩展系统级能力：基于阿里云开发者镜像 API 查询并下载操作系统 ISO 镜像。

数据源为阿里云开发者镜像 API（实测返回格式）：

- 系统名列表：`GET https://developer.aliyun.com/developer/api/mirror/image/findAllName`
  - `data: ["almalinux","anolis","archlinux","centos","centos-arch","deepin","openSUSE","rockylinux","ubuntu"]`
- 版本/下载链接：`GET https://developer.aliyun.com/developer/api/mirror/image/findByNameOrVersion?name=almalinux`
  - 每条记录含 `version`（如 `9(latest-aarch64-boot)`，架构编码在括号内）、`size`（字节）、`downloadUrl`（ISO 直链）、`md5sum`、`online`、`lastUpdateTime`

关键事实：镜像均为 ISO 文件（1.2GB ~ 12GB+），`md5sum` 字段是指向 MD5SUMS 文件的 URL 而非直接哈希值（实测 almalinux 该字段指向 debian-cd 的 MD5SUMS，属 API 脏数据），`deletedAt`/`gmtModified` 常为 null。

## 设计

### 1. 命令面与架构

```
cli os list                        # 列出阿里云镜像支持的所有系统名
cli os info <名称>                 # 列出该系统全部镜像（版本/大小/时间/链接）
cli os download <名称> [--version <版本>] [-o <目录>]
```

- 顶层 `Command` 新增 `Os { subcommand: OsCommand }`，`OsCommand::{List, Info { name }, Download { name, version, output_dir }}`
- 缩写兼容性（infer_subcommands 生效下核对）：顶层 `o` 唯一指向 os；os 内部 `l`/`i`/`d` 分别唯一指向 list/info/download，互不影响
- 代码组织（方案 A）：
  - 新增 `src/core/mirror.rs`：镜像 API 客户端。模型 `MirrorImage`、`fetch_all_names()`、`fetch_images(name)`、响应/MD5SUMS 解析、大小格式化、文件名提取。纯数据与解析逻辑，便于单元测试（与 `core/tools/go.rs` 的 parse 函数模式一致）
  - 新增 `src/commands/os.rs`：命令分发 + 三子命令实现（薄命令层）
  - `src/lib.rs` run() 增加分发；`src/core/mod.rs`、`src/commands/mod.rs` 注册新模块
- API 基址：常量 + `DEVKIT_MIRROR_API` 环境变量覆盖（测试钩子，模式同 `CLI_DEBUG`）
- 新依赖：`md-5 = "0.10"`（RustCrypto，与 sha2 同生态）

### 2. 各子命令行为

**`os list`**：调用 findAllName → 逐行打印系统名；空列表给出中文提示；`success=false` 或网络错误 → 明确中文报错。

**`os info <名称>`**：调用 findByNameOrVersion → 表格展示全部镜像（不过滤架构，保持 API 返回顺序）：

```
AlmaLinux 共 N 个镜像:
 #  版本                    大小        更新时间           链接
 1  9(latest-aarch64-boot)  1.4 GB     2026-05-28 22:40   https://...
```

- 大小格式化为 GB/MB（`format_size` 纯函数）

**`os download <名称>`**：
1. 拉取镜像列表并展示全部条目
2. 选择：`--version` 精确匹配 version 字段（匹配失败报错并列出可用值）；未提供且 TTY → `interact::select` 交互选择（标签「版本 + 大小」）；未提供且非 TTY → 报错提示加 `--version`
3. TTY 下 `confirm` 确认后下载；非 TTY 跳过确认直接下载（脚本显式传参即表达意图）
4. 复用 `core::download::download()`（.part 原子写入 + 进度 + 3 次重试）；目标文件名取 downloadUrl 末段；保存到 `-o` 指定目录（默认当前目录，目录不存在则 create_dir_all）
5. 已存在处理：TTY → 覆盖/跳过/重命名三选（dialoguer），重命名为 `<原名>.1.iso` 递增后缀；非 TTY → 跳过并提示
6. 下载完成后 MD5 校验（见下节）

### 3. MD5 校验（降级警告策略）

- 下载完成后，若 `md5sum` 字段非空 → 拉取该 URL → 按 MD5SUMS 格式（`hash  filename` 行）解析 → 用下载文件名精确匹配
- 拉取失败 / 匹配不到 → **警告不阻断**（API 脏数据常态）
- 匹配到哈希且校验不一致 → 报错并提示重跑，不自动重下（ISO 体积过大）
- serde 模型：`deletedAt`/`gmtModified` 等可空字段用 `Option<T>` 容错

### 4. 错误处理

- API `success=false` / HTTP 失败 / 空列表 → 中文错误
- 下载失败透传 anyhow 错误（复用 download() 的 3 次重试）

### 5. 测试

- 单元（mirror.rs）：findAllName 响应解析、findByNameOrVersion 响应解析（含 null 字段）、MD5SUMS 解析、`format_size`、文件名提取、`--version` 精确匹配
- 集成（mirror.rs 内 mock server，复用 download.rs 的 TcpListener 模式）：mock 两个 API 端点 + MD5SUMS，覆盖完整下载流程（小文件）
- CLI 级：新增 `tests/cli_os.rs`，设置 `DEVKIT_MIRROR_API` 指向本地 mock，断言 `os list` 输出

### 6. 文档

- README 命令说明补充 `os` 子命令段落

### 7. 非目标

- 不做架构预过滤（默认展示全部架构，用户已确认）
- 不做断点续传（下载中断需重跑，复用现有 .part 重试机制）
- 不接入 `cli install` 工具列表（os 非开发工具，独立命令面）
- 不做系统名模糊匹配/自动补全（需精确名称）
- 不实现镜像翻页/搜索（API 无此能力）

## 文档

- spec：本文件
- plan：`docs/superpowers/plans/2026-08-09-os-download.md`
