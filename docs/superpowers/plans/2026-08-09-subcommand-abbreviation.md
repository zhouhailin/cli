# Subcommand 缩写支持实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `cli` 子命令支持 clap 原生唯一前缀推断（`cli i` ≡ `cli install`），零新依赖。

**Architecture:** `#[command(infer_subcommands = true)]` 一行启用 clap 4 内置前缀匹配；完整命令名优先级最高；歧义前缀（`cli u`）由 clap 默认报错。测试用现有 `Cli::try_parse_from` 模式。

**Tech Stack:** clap 4 derive（已依赖）、Rust 单元测试。

## Global Constraints

- 只加 `infer_subcommands = true`，不引入任何新依赖
- 不自定义歧义错误文案（clap 默认英文提示）
- 不改动 help 输出、命令行为、内部模块结构
- 参数值（工具名/版本）不参与缩写
- 每任务结束：`cargo test -q` 全量回归 + `cargo clippy --all-targets -- -D warnings` + `cargo fmt --check`

---

### Task 1: 启用前缀推断 + 解析测试

**Files:**
- Modify: `src/lib.rs`（`#[command]` 属性 + tests 模块）

**Interfaces:**
- Produces: `infer_subcommands = true`；测试验证 `Command::Install/List/Update/Uninstall/Use` 变体

- [ ] **Step 1: 写失败测试**（`src/lib.rs` `mod tests` 内，`update_command_parses` 之后追加）

```rust
    #[test]
    fn prefix_abbreviations_parse() {
        // 唯一前缀推断：cli i / cli l / cli v / cli up / cli un
        assert!(matches!(
            Cli::try_parse_from(["cli", "i"]).unwrap().command,
            Command::Install
        ));
        assert!(matches!(
            Cli::try_parse_from(["cli", "ins"]).unwrap().command,
            Command::Install
        ));
        assert!(matches!(
            Cli::try_parse_from(["cli", "l"]).unwrap().command,
            Command::List
        ));
        assert!(matches!(
            Cli::try_parse_from(["cli", "v"]).unwrap().command,
            Command::Version
        ));
        assert!(matches!(
            Cli::try_parse_from(["cli", "up"]).unwrap().command,
            Command::Update
        ));
        assert!(matches!(
            Cli::try_parse_from(["cli", "un"]).unwrap().command,
            Command::Uninstall
        ));
    }

    #[test]
    fn ambiguous_prefix_rejected() {
        // u 同时是 use/uninstall/update 的前缀 → 歧义报错
        assert!(Cli::try_parse_from(["cli", "u"]).is_err());
    }

    #[test]
    fn full_command_names_still_parse() {
        // 完整命令名回归确认
        assert!(matches!(
            Cli::try_parse_from(["cli", "use"]).unwrap().command,
            Command::Use
        ));
    }
```

- [ ] **Step 2: 确认 RED**

Run: `cargo test --lib prefix_abbreviations_parse -q`
Expected: FAIL（`i` 被当作未知子命令，try_parse 返回 Err）

- [ ] **Step 3: 实现**

`src/lib.rs` `#[command(...)]` 属性内 `help_template` 行之后加：

```rust
    help_template = "{about-with-newline}\n版本: {version}\n\n{usage-heading} {usage}\n\n{all-args}{after-help}",
    infer_subcommands = true
```

- [ ] **Step 4: 确认 GREEN + 全量回归**

Run:
```bash
cargo test -q 2>&1 | grep "test result" | awk -F'[ ;]' '{s+=$4} END {print "TOTAL:", s}'
cargo clippy --all-targets -- -D warnings 2>&1 | tail -1
cargo fmt --check
```
Expected: TOTAL 136（133 + 3 个新测试）、clippy 无警告、fmt 干净

- [ ] **Step 5: 提交**

```bash
git add src/lib.rs
git commit -m "feat: 子命令支持唯一前缀缩写（infer_subcommands）"
```

---

### Task 2: README 文档 + 端到端验证

**Files:**
- Modify: `README.md`（命令详解节开头加一句）

- [ ] **Step 1: README 加说明**

`README.md` L62 `## 命令详解` 标题下（`### cli install [tool]` 之前）插入：

```markdown
所有子命令支持唯一前缀缩写（如 `cli i` 等价 `cli install`，`cli up` 等价 `cli update`）；前缀有歧义时（如 `cli u`）会提示补全命令名。

```

- [ ] **Step 2: 端到端验证**

Run:
```bash
cd /opt/work/demo/cli && cargo build -q
./target/debug/cli i --help | head -1
./target/debug/cli l
./target/debug/cli v
./target/debug/cli u 2>&1 | head -1   # 期望：歧义报错
```
Expected: 前三个正常执行（i 显示 install 帮助、l 列出、v 显示版本），`cli u` 报歧义错误

- [ ] **Step 3: 提交**

```bash
git add README.md
git commit -m "docs: README 补充子命令前缀缩写说明"
```

---

## 收尾

全部任务完成后使用 superpowers:finishing-a-development-branch 技能收尾（全量测试 → 环境检测 → 呈现选项）。

## Assumptions

- v0.1.11 目标功能
- 歧义错误保持 clap 默认英文提示（spec 明确非目标）
- `cli v`（Version）与 `cli l`（List）前缀互不冲突；`use/uninstall/update` 共享 `u` 前缀属预期行为
