# uninstall 命令设计文档

**日期**：2026-08-08
**状态**：已批准
**版本**：v0.1.3 目标功能

## 1. 背景

CLI 工具已实现 `install / list / use / version / self-update`，但缺少卸载能力。需求规格定义「卸载干净」：删除安装文件、配置注册、符号链接与 shell 环境注入，使环境恢复安装前状态。当前多版本共存（如 java 8/17/21 同时安装），卸载需按版本粒度操作。

## 2. 需求

1. **按版本卸载**：`cli uninstall java 17` 只删指定版本；`cli uninstall java` 单版本直接卸、多版本交互选择
2. **全量清理**：删版本目录 + 更新 config + 删 current 链接 + 工具无残留版本时清理 shell 注入（PATH/JAVA_HOME 行）；cache 压缩包保留（离线复用）
3. **交互与确认**：完全无参时交互选择工具和版本（复用 `select`），非 TTY 报错提示；删除前 `confirm` 确认（默认否）

## 3. 关键事实

- 安装时 PATH/JAVA_HOME 注入均指向 **current 链**（`~/.devkit/current/<tool>/bin`、`~/.devkit/current/java`），为**工具级**注入，不区分版本；`use` 切换版本时注入行不变，通过链接变化生效
- 因此 shell 清理只在**工具无残留版本**时触发；卸载非激活版本或有其他版本残留时不动 rc 文件

## 4. 设计

### 4.1 命令层

- `Uninstall { tool: Option<String>, version: Option<String> }` 子命令
- 有参分发：`cli uninstall <tool> <version>` 直接卸载；`<tool>` 不带版本时，已安装单版本直接卸、多版本 `select` 交互选择
- 完全无参：`select` 交互选择工具（列已安装工具）→ 版本；非 TTY 报 `请指定工具名，例如: cli uninstall java`
- 删除前 `confirm("确认卸载 {tool} {version}？", false)`，取消则退出不动作
- 目标版本未安装：报错 `{tool} {version} 未安装`（与 use 命令风格一致）

### 4.2 清理顺序（失败即中止，不留半状态）

```
1. confirm 确认（默认否）
2. 删版本目录  ~/.devkit/<tool>/<version>       ← 失败则报错中止，不动其他状态
3. 更新 config：installed 移除该版本；若 active 是该版本则清空 active 并 save
4. 若是激活版本：删 current/<tool> 符号链接
5. 若该工具已无残留版本：清理 shell 注入
```

### 4.3 shell 注入清理（shell.rs 扩展）

- 新增 `remove_block(rc_file, marker)`：移除 rc 文件中由 marker 包裹的整块；无块时 no-op
- 新增 `remove_tool_injections(rc_file, tool)`：读 devkit 块，过滤两条工具级行：
  - `export PATH=".../current/<tool>/bin:$PATH"`（精确匹配整行）
  - `export JAVA_HOME=".../current/java"`（行以 `export JAVA_HOME=` 开头**且值包含 `/current/java`** 才匹配，避免误删用户手动设置的其他 JAVA_HOME；仅该工具被清理时调用，当前只有 java 使用）
- 过滤后块为空 → 调 `remove_block` 移除整个 devkit 块；非空 → 剩余行写回（其他工具注入不受影响）
- 幂等：重复调用无副作用

### 4.4 边界行为

| 场景 | 行为 |
|------|------|
| 卸载非激活版本 | 只删目录 + config，不动 current 链接与 shell |
| 卸载激活版本、还有其他版本 | 删链接，提示 `已卸载激活版本，可用 cli use <tool> <version> 重新激活`；shell 注入保留，重新 use 后自动恢复有效 |
| 卸载最后一个版本 | current 链接 + shell 注入一并清理，环境恢复安装前状态 |
| 目标版本未安装 | 报错退出 |
| cache 压缩包 | 保留不动（离线复用） |

## 5. 测试计划

### 单元测试（新增约 10 个）

| 模块 | 测试 | 验证点 |
|------|------|--------|
| shell.rs | `remove_block_removes_whole_block` | 移除整块 |
| shell.rs | `remove_block_noop_when_absent` | 无块时 no-op |
| shell.rs | `remove_tool_injections_removes_tool_lines` | 移除 PATH + JAVA_HOME 行 |
| shell.rs | `remove_tool_injections_keeps_other_tools` | 其他工具行保留 |
| shell.rs | `remove_tool_injections_clears_empty_block` | 块清空后整块移除 |
| shell.rs | `remove_tool_injections_is_idempotent` | 幂等 |
| uninstall 逻辑 | `remove_non_active_version` | 只删目录 + config |
| uninstall 逻辑 | `remove_active_version_removes_link` | 激活版本删链接 |
| uninstall 逻辑 | `remove_last_version_cleans_injections` | 最后版本触发 shell 清理 |

### 集成测试（tests/cli_uninstall.rs）

- `uninstall_without_tool_reports_hint_when_non_tty`：无参非 TTY 报「请指定工具名」
- `uninstall_unknown_version_reports_error`：`cli uninstall java 99` 报「未安装」

### 测试隔离

- 目录/config/链接操作：`DEVKIT_ROOT` 指向 tempdir
- shell 清理：临时 rc 文件（复用现有 shell.rs 测试模式），不触碰真实主目录
- 删除确认：confirm 需可绕过——设计上命令层确认后调用纯逻辑函数，集成测试只验证报错路径；纯逻辑函数不经 confirm（confirm 由命令层执行）

## 6. 影响文件

| 文件 | 变更 |
|------|------|
| `src/lib.rs` | 新增 `Uninstall` 子命令 |
| `src/commands/uninstall.rs` | 新增（交互 + 编排） |
| `src/commands/mod.rs` | 注册 uninstall 模块 |
| `src/core/shell.rs` | 新增 `remove_block` + `remove_tool_injections` |
| `tests/cli_uninstall.rs` | 新增集成测试 |
| `README.md` | 命令详解新增 uninstall 段 |

## 7. 明确不做（YAGNI）

- 不自动激活同工具其他版本（提示用户 `use` 即可）
- 不删除 cache 压缩包
- 不清理服务注册（redis/mysql 服务功能未实现，无对应状态）
- 不支持 `uninstall --all` 批量卸载
