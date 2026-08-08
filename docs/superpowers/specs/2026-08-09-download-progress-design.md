# 下载进度展示设计

**日期**：2026-08-09
**状态**：已批准
**版本**：v0.1.10 目标功能

## 背景

`cli install` 下载大文件（JDK 约 100MB+、mvnd 等）时，`download()` 内部用 `ureq` + `io::copy` 一次性拷贝，全程无任何反馈，用户无法感知下载是否在进行、还剩多少。需要实时进度展示。

## 设计

### 1. `download()` 签名扩展

`src/core/download.rs`：

```rust
pub fn download(url: &str, dest: &Path, expected_sha256: Option<&str>, label: &str) -> Result<()>
```

- `label`：进度前缀显示的下载对象（如 `"java 21.0.5"`、`"cli 自更新"`）
- 手动 read/write 循环（64KB 缓冲）替代 `io::copy`，实时累计字节数

### 2. 进度渲染

- 输出到 **stderr** 单行刷新（`\r`），不污染 stdout 管道
- 格式（有 Content-Length）：`下载 {label}: {已}/{总} MB ({pct}%)`
- 格式（无 Content-Length，chunked）：`下载 {label}: {已} MB`
- 完成：清行 + 换行
- **仅 TTY 显示**：`std::io::IsTerminal` 检测 stderr（标准库，零新依赖）；管道/CI/重定向下自动隐藏，保持现有行为
- 重试（3 次尝试）时进度自然重置

### 3. 调用点

| 位置 | 改动 |
|---|---|
| `installer.rs` `install_archive` | 内部拼 `format!("{tool} {version}")` 传入 → java/node/go/maven/mvnd 5 个工具调用点**零改动** |
| `self_update.rs` | 传 `"cli 自更新"` |

### 4. 测试

- 抽纯函数 `format_progress(label, done, total: Option<u64>) -> String`：
  - 有总大小：`下载 java 21.0.5: 45.2/98.7 MB (45%)`
  - 无总大小：`下载 java 21.0.5: 45.2 MB`
- 现有测试全部不受影响（非 TTY 不输出；mock server 已带 Content-Length）

### 5. 非目标

- 不做速率/ETA/进度条动画（YAGNI，后续需要再加）
- 不新增第三方依赖（indicatif 等）
- 不改变下载/校验/重试逻辑本身

## 文档

- spec：本文件
- plan：`docs/superpowers/plans/2026-08-09-download-progress.md`
