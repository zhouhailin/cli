# 下载进度展示实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `cli install`/`cli update` 下载时在 stderr 实时显示单行进度（仅 TTY），零新依赖。

**Architecture:** `download()` 增加 `label` 参数，手动 read/write 循环替代 `io::copy` 累计字节；`format_progress` 纯函数渲染进度行；`IsTerminal` 检测 stderr 决定是否输出；`install_archive` 内部拼 label 使 5 个工具调用点零改动。

**Tech Stack:** Rust std（`std::io::IsTerminal`，1.70+）、ureq 2、现有 anyhow。

## Global Constraints

- 进度输出到 **stderr**，绝不污染 stdout 管道
- 仅 TTY 显示；管道/CI/重定向下完全静默（保持现有行为）
- 不新增第三方依赖（indicatif 等）
- 不改变下载重试（3 次）、SHA-256 校验、原子 rename 逻辑
- 全角冒号 `：` 用于中文文案（与现有错误/提示风格一致，但进度行用半角冒号 `:` 分隔标签与数据，避免全角占用宽度——**统一采用半角冒号**，见 Task 1 测试断言）
- 每任务结束 `cargo test -q` 全量回归 + `cargo clippy --all-targets -- -D warnings` + `cargo fmt --check`

---

### Task 1: format_progress 纯函数（TDD）

**Files:**
- Modify: `src/core/download.rs`（新增私有函数 + 测试）

**Interfaces:**
- Produces: `fn format_progress(label: &str, done: u64, total: Option<u64>) -> String`（模块内私有，Task 2 同文件使用；`None` 表示无 Content-Length）

- [ ] **Step 1: 写失败测试**（`src/core/download.rs` 的 `mod tests` 内追加）

```rust
    #[test]
    fn format_progress_with_total_shows_percent() {
        let s = format_progress("java 21.0.5", 47 * 1024 * 1024, Some(100 * 1024 * 1024));
        assert_eq!(s, "下载 java 21.0.5: 47.0/100.0 MB (47%)");
    }

    #[test]
    fn format_progress_without_total_shows_bytes_only() {
        let s = format_progress("cli 自更新", 5 * 1024 * 1024, None);
        assert_eq!(s, "下载 cli 自更新: 5.0 MB");
    }
```

- [ ] **Step 2: 确认 RED**

Run: `cargo test --lib download::tests::format_progress -q`
Expected: 编译错误 E0425（`cannot find function format_progress`）

- [ ] **Step 3: 实现 format_progress**（`download()` 函数之前插入）

```rust
/// 进度行渲染：有总大小显示百分比，无总大小（chunked）仅显示字节
fn format_progress(label: &str, done: u64, total: Option<u64>) -> String {
    let mb = |b: u64| b as f64 / (1024.0 * 1024.0);
    match total {
        Some(t) if t > 0 => {
            let pct = (done as f64 / t as f64 * 100.0).min(100.0) as u64;
            format!(
                "下载 {label}: {:.1}/{:.1} MB ({pct}%)",
                mb(done),
                mb(t)
            )
        }
        _ => format!("下载 {label}: {:.1} MB", mb(done)),
    }
}
```

- [ ] **Step 4: 确认 GREEN**

Run: `cargo test --lib download::tests::format_progress -q`
Expected: 2 passed

- [ ] **Step 5: 提交**

```bash
git add src/core/download.rs
git commit -m "feat: 下载进度行渲染纯函数 format_progress（有/无总大小两种格式）"
```

---

### Task 2: download() 改造 + 调用点接线

**Files:**
- Modify: `src/core/download.rs`（`download` 函数签名与实现 + 3 处测试调用点）
- Modify: `src/core/installer.rs:53`（唯一调用点）
- Modify: `src/core/tools/self_update.rs:65`（唯一调用点）

**Interfaces:**
- Consumes: Task 1 的 `format_progress(label, done, total)`
- Produces: `pub fn download(url: &str, dest: &Path, expected_sha256: Option<&str>, label: &str) -> Result<()>`（label 为进度前缀，如 `"java 21.0.5"`）

- [ ] **Step 1: 改 download 签名与实现**

`src/core/download.rs` 中 `download` 整体替换为（注意 `use std::io::{IsTerminal, Read, Write};` 需加在文件顶部 `use std::path::Path;` 之后）：

```rust
pub fn download(url: &str, dest: &Path, expected_sha256: Option<&str>, label: &str) -> Result<()> {
    let part = dest.with_extension("part");
    debug_log!("开始下载 {url} -> {}", dest.display());
    // 仅 TTY 显示进度；管道/CI/重定向静默
    let show_progress = std::io::stderr().is_terminal();
    let mut last_err: Option<String> = None;
    for attempt in 0..3 {
        match ureq::get(url).call() {
            Ok(resp) => {
                let total = resp
                    .header("Content-Length")
                    .and_then(|v| v.parse::<u64>().ok());
                let mut reader = resp.into_reader();
                let mut file = std::fs::File::create(&part)?;
                let mut buf = [0u8; 64 * 1024];
                let mut done: u64 = 0;
                loop {
                    let n = reader.read(&mut buf)?;
                    if n == 0 {
                        break;
                    }
                    file.write_all(&buf[..n])?;
                    done += n as u64;
                    if show_progress {
                        eprint!("\r{}", format_progress(label, done, total));
                    }
                }
                drop(file);
                if show_progress {
                    eprint!("\r\x1b[K"); // 清行，避免残留半行
                }
                debug_log!("下载完成: {} 字节（第 {}/3 次尝试）", done, attempt + 1);
                if let Some(expected) = expected_sha256 {
                    debug_log!("校验 SHA-256: 期望 {expected}");
                    verify_sha256(&part, expected)?;
                    debug_log!("SHA-256 校验通过");
                }
                std::fs::rename(&part, dest)?;
                return Ok(());
            }
            Err(e) => {
                last_err = Some(e.to_string());
                let backoff = 200u64 * (1 << attempt);
                debug_log!("下载失败(尝试 {}/3): {e}，{backoff}ms 后重试", attempt + 1);
                std::thread::sleep(std::time::Duration::from_millis(backoff));
            }
        }
    }
    Err(anyhow!("下载失败: {}", last_err.unwrap_or_default()))
}
```

- [ ] **Step 2: 修 3 处测试调用点**（`download` 测试内，全部加 `"test"` 标签）

`src/core/download.rs` `mod tests` 中：
- `download_writes_file_content`：`download(&format!("{base}/f"), &dest, None)` → `download(&format!("{base}/f"), &dest, None, "test")`
- `download_retries_on_server_error`：同上
- `download_fails_on_sha_mismatch`：`download(&format!("{base}/f"), &dest, Some("0000..."))` → 末尾加 `, "test"`

- [ ] **Step 3: 确认 RED（编译断）**

Run: `cargo test -q 2>&1 | head -5`
Expected: 编译错误 E0061（`this function takes 4 arguments but 3 were supplied`，来自 installer.rs / self_update.rs）

- [ ] **Step 4: 修 2 个生产调用点**

`src/core/installer.rs:53`：
```rust
    download(url, &archive_path, sha256, &format!("{tool} {version}"))?;
```

`src/core/tools/self_update.rs:65`：
```rust
    download(&url, &staging, None, "cli 自更新")?;
```

- [ ] **Step 5: 确认 GREEN + 全量回归**

Run:
```bash
cargo test -q 2>&1 | grep "test result" | awk -F'[ ;]' '{s+=$4} END {print "TOTAL:", s}'
cargo clippy --all-targets -- -D warnings 2>&1 | tail -1
cargo fmt --check
```
Expected: TOTAL 133（131 + Task 1 的 2 个）、clippy 无警告、fmt 干净

- [ ] **Step 6: 提交**

```bash
git add src/core/download.rs src/core/installer.rs src/core/tools/self_update.rs
git commit -m "feat: 下载过程实时显示进度（stderr 单行刷新，仅 TTY；install_archive 自动拼标签）"
```

---

### Task 3: 端到端验证

**Files:**
- 无代码改动（验证不通过则回 Task 2 修）

- [ ] **Step 1: 伪 TTY 验证真实下载进度**

Run（mvnd 约 30MB，足够观察进度；临时 root 避免污染）:
```bash
cd /opt/work/demo/cli && cargo build -q && rm -rf /tmp/devkit-progress-e2e && mkdir -p /tmp/devkit-progress-e2e
printf '\n\n' | script -q /dev/null env DEVKIT_ROOT=/tmp/devkit-progress-e2e SHELL=/bin/zsh HOME=/tmp/devkit-progress-e2e ./target/debug/cli install mvnd 2>&1 | tr '\r' '\n' | grep -E "下载|安装完成" | head -6
```
Expected: 输出含 `下载 mvnd 1.0.6: x.x/y.y MB (z%)`（进度行）+ `mvnd 1.0.6 安装完成`

- [ ] **Step 2: 验证管道场景无进度输出**

Run:
```bash
cd /opt/work/demo/cli && DEVKIT_ROOT=/tmp/devkit-progress-e2e ./target/debug/cli list 2>&1 | grep -c "下载" || echo "管道场景无进度输出 ✓"
```
Expected: 无 `下载` 字样（0 或 grep 无匹配）

- [ ] **Step 3: 提交（如有修复）**

若 Step 1/2 发现缺陷：修复后 `cargo test -q` 全量回归再提交；否则跳过。

---

## 收尾

全部任务完成后使用 superpowers:finishing-a-development-branch 技能收尾（全量测试 → 环境检测 → 呈现选项）。

## Assumptions

- v0.1.10 目标功能；Kona 修复（7e9314e）已单独提交，不在此 plan 内
- 进度行统一半角冒号 `:`（数据展示），与全角冒号 `：` 的中文提示文案互不影响
- `IsTerminal` 仅检测 stderr；测试环境（管道）自然静默，现有 131 测试不受影响
