# 离线部署模式与 os 无参交互设计

> 状态: 已确认（2026-08-09）
> 决策记录: CLI_OFFLINE/DEVKIT_OFFLINE 离线开关；本地版本清单 versions.json；新增 cli download 预热命令；在线缓存命中复用；os info/download 无 name 交互选系统

## 目标

1. **离线部署**：设置 `CLI_OFFLINE=true` 或 `DEVKIT_OFFLINE=true` 时，cli 不连接互联网，仅使用本地缓存文件安装部署（便于内网/离线环境）。
2. **os 交互增强**：`cli os info` / `cli os download` 的 name 改为可选，无 name 时交互展示系统名列表（即 `cli os list` 内容）供选择。

## 背景

- 现有安装流程（installer.rs `install_archive`）下载压缩包后**保留在 `<cache_dir>/`**（默认 `<root>/cache`，`DEVKIT_CACHE_DIR` 可覆盖指向外部共享目录），已有缓存基础。
- 各工具版本列表均来自网络 API（go.dev / nodejs index / apache 目录 / 毕昇 API），下载 URL 与 sha256 获取逻辑已按工具独立封装（`fetch_lts_list`/`resolve_url` 等），可复用。
- rust 为 `curl|sh` 脚本安装（无压缩包缓存），`os download` 为 ISO 下载（非 devkit 工具），两者无法离线，离线时明确报错。

## 架构

### 1. 离线开关（core/offline.rs，新增）

```rust
pub fn is_offline() -> bool
```

- `CLI_OFFLINE` 或 `DEVKIT_OFFLINE` 任一环境变量非空且非 `"0"`/`"false"`（大小写不敏感）即视为离线。
- 离线模式生效时打印提示：`离线模式: 仅使用本地缓存，不访问网络`。
- 依赖网络的命令（`cli download`、rust/os install）直接报错。

### 2. 版本清单（core/cache.rs，新增）

清单文件：`<cache_dir>/versions.json`（随缓存目录走；`DEVKIT_CACHE_DIR` 指向外部目录时清单也在该目录）。

```json
{
  "node": [
    { "version": "22.11.0", "file": "node-v22.11.0-linux-x64.tar.gz", "sha256": "3a6e..." }
  ]
}
```

- `file`：缓存压缩包文件名（与 URL 文件名一致，同 install 现有命名规则）。
- `sha256`：**下载后计算的文件实际哈希**，离线时校验缓存文件完整性（不依赖网络）。
- 更新时机：`cli download` 下载成功后、在线 `install` 下载成功后自动写入/更新（按 tool+version 幂等去重）。
- 提供 API：`load`/`save`/`find(tool, version)`/`add(tool, version, file, sha256)`。
- 清单写入失败：警告不阻断安装（与 rc/config 写入失败策略一致）。

### 3. installer.rs 重构：缓存解析与安装分离

`install_archive` 拆分为「解析压缩包路径」与「解压安装」两段：

```
resolve_archive_path(ctx, tool, version, url, sha) -> PathBuf
```

| 场景 | 行为 |
|------|------|
| 在线 + 缓存命中 | 缓存有同名文件 → 按序校验（官方 sha 有则用官方 → 否则清单 sha）→ 匹配则复用不下载；不匹配重新下载 |
| 在线 + 无缓存 | 正常下载 → 计算哈希 → 更新清单（现有流程加一步） |
| 离线 + 清单有记录 + 文件存在 | 用清单 sha256 校验文件 → 直接解压安装 |
| 离线 + 无记录/文件缺失 | 报错：`离线模式缺少 {tool} {version} 的缓存，请先在联网机器执行 cli download {tool} {version} 预热` |

解压 → 剥离单顶层目录 → 注册激活 → current 链接 → PATH 注入：全部复用现有逻辑。

### 4. install 命令入口（install.rs）

- 离线 + rust/os：报错 `离线模式不支持 {工具} 安装，仅支持 java/node/go/maven/mvnd`。
- 离线 + 压缩包类：版本来源为**清单列表**（交互 select 或显式参数查清单），不调用任何网络 API。
- 在线：现有流程不变（新增清单更新一步）。

### 5. cli download 命令（新增）

`lib.rs` Command 枚举增加 `Download { tool: Option<String>, version: Option<String> }`，`commands/download.rs` 薄分发，仅支持压缩包类 5 个工具（java/node/go/maven/mvnd）。

| 场景 | 行为 |
|------|------|
| `cli download` | 交互：选工具 → 选版本（复用各工具现有列表逻辑） |
| `cli download node 22.11.0` | 直接下载该版本 |
| `cli download node` | 交互选版本 |
| 离线模式 | 报错：`离线模式无法下载，仅支持本地缓存安装` |

下载流程：复用各工具版本列表与 URL/sha 获取逻辑 → 下载到 `<cache_dir>/<文件名>` → 计算 sha256 → 更新清单 → 打印缓存就绪信息（**不安装、不注册 config**）。已有同名缓存文件：重新下载覆盖（预热场景需要最新文件；与 install 的命中复用行为不同）。

### 6. os 无参交互增强（os.rs + lib.rs）

- `OsCommand::Info` / `OsCommand::Download` 的 `name` 改为 `Option<String>`。
- 无 name 时：调用 `mirror::fetch_all_names()`（即 `os list` 内容）→ 交互 select 选择系统 → 继续现有流程。
- 非 TTY + 无 name：报错提示指定 name（与 install/use 的非 TTY 行为一致）。
- `cli os list` 本身不变。

## 数据流

```
在线预热（联网机）:
  cli download node 22.11.0
    → fetch 版本/URL → 下载到 cache_dir → 算 sha256 → 更新 versions.json
  （或在线 install，下载成功同样更新清单）
  → 拷贝 cache_dir（含 versions.json）到离线机

离线安装（离线机）:
  CLI_OFFLINE=true cli install node 22.11.0
    → 读 versions.json → 命中记录 → 校验缓存文件 sha256 → 解压安装 → 注册激活 → 注入 PATH
```

## 错误处理

| 场景 | 行为 |
|------|------|
| 离线 + rust/os install | 报错：`离线模式不支持 {工具} 安装，仅支持 java/node/go/maven/mvnd` |
| 离线 + 缺清单/缺文件 | 报错含预热指引（见上） |
| 离线 + 清单 sha 校验失败 | 报错：`缓存文件损坏或不完整（sha256 不匹配），请重新预热` |
| 在线命中但 sha 不匹配 | 自动重新下载（不报错） |
| 清单写入失败 | 警告不阻断安装 |
| 离线 + cli download | 报错：`离线模式无法下载，仅支持本地缓存安装` |

## 测试

### 单元测试

- offline.rs：`is_offline` 各取值（`true`/`1`/空/`0`/`false` 大小写）。
- cache.rs：清单 load/save 往返、add 幂等去重、find 命中/未命中、文件缺失时 load 返回空清单。
- installer.rs：在线命中复用（预置缓存 + mock 服务器不应被调用）；sha 不匹配重新下载。

### 集成测试

- tests/cli_offline.rs（新增）：
  1. 离线安装全流程：预置缓存文件 + 清单，`CLI_OFFLINE=true` install node → 成功注册激活。
  2. 离线缺文件：报错含预热指引。
  3. 离线 rust：报错「离线模式不支持」。
  4. `cli download`：mock 服务器下载 + 清单更新 + 不安装（config 无记录）。
- tests/cli_os.rs（扩展）：`os info`/`os download` 无 name 交互选系统（mock API 返回系统列表）。

## 范围边界（YAGNI）

- 离线模式仅覆盖压缩包类工具（java/node/go/maven/mvnd）；rust、os download、cli update 离线时报错。
- 不做 crates.io/镜像清单的离线化。
- 不做缓存清理（LRU 等）。
- `cli download` 不做批量/全量下载（每次一个工具一个版本）。
- 离线版本列表来源仅 versions.json，不扫描缓存目录文件名。

## 文档

- README：环境变量表增加 CLI_OFFLINE/DEVKIT_OFFLINE；新增 cli download 命令说明与离线部署流程示例。
