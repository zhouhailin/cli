# Subcommand 缩写支持设计

**日期**：2026-08-09
**状态**：已批准
**版本**：v0.1.11 目标功能

## 背景

`cli` 命令面为 `version` / `list` / `install` / `use` / `uninstall` / `update`，均为完整单词。日常使用中用户希望输入唯一前缀即可触发命令（如 `cli i` 安装、`cli l` 列出），减少击键。

## 设计

### 1. 启用 clap 原生前缀推断

`src/lib.rs` 的 `#[command]` 属性加一行：

```rust
#[command(
    name = "cli",
    version = current_version(),
    about = "跨平台开发环境一键安装工具",
    help_template = "{about-with-newline}\n版本: {version}\n\n{usage-heading} {usage}\n\n{all-args}{after-help}",
    infer_subcommands = true
)]
```

- 零新依赖（clap 4 内置）、零维护映射
- 完整命令名优先级最高，不受影响
- 参数值（工具名、版本等）不参与缩写推断

### 2. 行为预期

| 输入 | 解析结果 |
|---|---|
| `cli i` / `cli ins` / `cli install` | Install |
| `cli l` | List |
| `cli v` | Version |
| `cli up` | Update |
| `cli un` | Uninstall |
| `cli u` | 歧义（use/uninstall/update 同前缀）→ clap 报错，提示补全 |

歧义错误使用 clap 默认提示（英文），不做自定义（YAGNI）。

### 3. 测试

`src/lib.rs` 现有 `mod tests` 追加解析用例（`Cli::parse_from` 模式）：
- `cli i` → `Command::Install`
- `cli l` → `Command::List`
- `cli up` → `Command::Update`
- `cli un` → `Command::Uninstall`
- `cli u` → `ErrorKind::UnknownArgument`（歧义报错）
- 完整名 `cli use` → `Command::Use`（回归确认不受影响）

### 4. 文档

- README 命令说明补充一句"支持唯一前缀缩写（如 `cli i` 等价 `cli install`）"

### 5. 非目标

- 不做手动显式别名（`i`→install 硬映射）
- 不自定义歧义错误文案
- 不修改 help 输出与现有命令行为

## 文档

- spec：本文件
- plan：`docs/superpowers/plans/2026-08-09-subcommand-abbreviation.md`
