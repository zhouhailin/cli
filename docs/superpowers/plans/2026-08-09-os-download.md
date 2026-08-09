# 操作系统镜像下载功能（cli os）实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 新增 `cli os` 命令面（list/info/download），基于阿里云开发者镜像 API 查询系统名、镜像版本与下载链接，并支持下载 ISO 镜像与 MD5 降级校验。

**Architecture:** core 层新增 `src/core/mirror.rs`（镜像 API 客户端：模型/解析/网络/MD5 校验，纯函数可单测），commands 层新增 `src/commands/os.rs`（薄命令分发），lib.rs 接线 `Command::Os`。下载复用现有 `core::download::download()`（.part 原子写入 + 进度 + 3 次重试）。

**Tech Stack:** Rust 2021 / clap 4 derive / serde+serde_json / ureq / md-5 0.10（新增）/ dialoguer（已有 interact 封装）/ assert_cmd（集成测试）

## Global Constraints

- 版本：v0.1.12 目标功能；发布版本号同步规则不变（CLI_VERSION tag 注入）
- 新依赖仅 `md-5 = "0.10"`（RustCrypto，与 sha2 同生态），禁止引入其他依赖
- 所有 UI 文案与错误消息使用中文；调试日志走 `debug_log!`（stderr）
- API 基址默认 `https://developer.aliyun.com/developer/api/mirror/image`，`DEVKIT_MIRROR_API` 环境变量可覆盖（测试钩子）
- 展示全部镜像不过滤架构；不做断点续传；MD5 校验降级警告策略（spec 第 3 节）
- 每个任务独立可测交付物，TDD（先写失败测试）；每任务一个 commit（feat:/docs: 前缀）
- 全量门禁：`cargo test` 全绿、`cargo clippy --all-targets -- -D warnings` 零警告、`cargo fmt --check` 通过
- serde 模型对可空字段（`deletedAt`/`gmtModified`/`md5sum` 等）使用 `Option<T>` 容错

---

### Task 1: mirror 解析层（模型 + 响应解析 + 工具函数）

**Files:**
- Modify: `Cargo.toml`（dependencies 增加 md-5）
- Create: `src/core/mirror.rs`（模型与纯函数 + 单元测试）
- Modify: `src/core/mod.rs`（注册 `pub mod mirror;`）

**Interfaces:**
- Consumes: 无（纯函数，不依赖网络）
- Produces:
  - `pub struct MirrorImage { pub id: u64, pub name: String, pub version: String, pub architecture: String, pub size: u64, pub online: u8, pub download_url: String, pub md5sum: Option<String>, pub last_update_time: Option<String> }`
  - `pub fn parse_names_response(json: &str) -> Result<Vec<String>>`
  - `pub fn parse_images_response(json: &str) -> Result<Vec<MirrorImage>>`
  - `pub fn format_size(bytes: u64) -> String`
  - `pub fn file_name_from_url(url: &str) -> Result<String>`
  - `pub fn renamed_path(dir: &Path, file_name: &str, n: usize) -> PathBuf`
  - `pub fn find_image_by_version<'a>(images: &'a [MirrorImage], version: &str) -> Option<&'a MirrorImage>`

- [ ] **Step 1: 写失败测试**

`Cargo.toml` 的 `[dependencies]` 中 `sha2 = "0.10"` 后追加：

```toml
md-5 = "0.10"
```

创建 `src/core/mirror.rs`，先只写测试（`use super::*;` 处引用尚未定义的函数，编译失败即 RED）：

```rust
use std::path::Path;

use anyhow::Result;

#[cfg(test)]
mod tests {
    use super::*;

    const IMAGE_JSON: &str = r#"{"success":true,"code":"200","message":"查询成功","data":[{"id":5709,"name":"almalinux","uuid":"x","version":"9(latest-aarch64-boot)","architecture":"","size":1458331648,"online":1,"downloadUrl":"https://mirrors.aliyun.com/almalinux/9.8/isos/aarch64/AlmaLinux-9-latest-aarch64-boot.iso","md5sum":null,"lastUpdateTime":"2026-05-28 22:40:16","deletedAt":null,"status":"ok","gmtCreate":"2026-05-28 22:40:16","gmtModified":null,"isDel":0}]}"#;

    #[test]
    fn parse_names_response_ok() {
        let names =
            parse_names_response(r#"{"success":true,"message":"查询成功","data":["almalinux","ubuntu"]}"#)
                .unwrap();
        assert_eq!(names, vec!["almalinux", "ubuntu"]);
    }

    #[test]
    fn parse_names_response_failed_flag_errors() {
        let err = parse_names_response(r#"{"success":false,"message":"服务异常","data":null}"#)
            .unwrap_err();
        assert!(err.to_string().contains("查询失败"));
    }

    #[test]
    fn parse_names_response_invalid_json_errors() {
        assert!(parse_names_response("nope").is_err());
    }

    #[test]
    fn parse_images_response_handles_null_fields() {
        let images = parse_images_response(IMAGE_JSON).unwrap();
        assert_eq!(images.len(), 1);
        let img = &images[0];
        assert_eq!(img.version, "9(latest-aarch64-boot)");
        assert_eq!(img.size, 1458331648);
        assert_eq!(
            img.download_url,
            "https://mirrors.aliyun.com/almalinux/9.8/isos/aarch64/AlmaLinux-9-latest-aarch64-boot.iso"
        );
        assert!(img.md5sum.is_none());
        assert_eq!(img.last_update_time.as_deref(), Some("2026-05-28 22:40:16"));
    }

    #[test]
    fn format_size_gb_and_mb() {
        assert_eq!(format_size(1458331648), "1.4 GB");
        assert_eq!(format_size(52 * 1024 * 1024), "52.0 MB");
    }

    #[test]
    fn file_name_from_url_last_segment() {
        assert_eq!(
            file_name_from_url(
                "https://mirrors.aliyun.com/almalinux/9.8/isos/aarch64/AlmaLinux-9-latest-aarch64-boot.iso"
            )
            .unwrap(),
            "AlmaLinux-9-latest-aarch64-boot.iso"
        );
    }

    #[test]
    fn renamed_path_appends_number_before_ext() {
        assert_eq!(
            renamed_path(Path::new("/tmp"), "a.iso", 1),
            std::path::PathBuf::from("/tmp/a.1.iso")
        );
    }

    #[test]
    fn find_image_by_version_exact_match() {
        let images = parse_images_response(IMAGE_JSON).unwrap();
        assert!(find_image_by_version(&images, "9(latest-aarch64-boot)").is_some());
        assert!(find_image_by_version(&images, "8").is_none());
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test mirror`
Expected: 编译失败（`parse_names_response` 等未定义）——RED

- [ ] **Step 3: 实现解析层**

`src/core/mod.rs` 的 `pub mod config;` 前插入 `pub mod mirror;`。

`src/core/mirror.rs` 测试代码上方写入实现：

```rust
use std::path::Path;

use anyhow::{anyhow, Result};

/// 阿里云开发者镜像 API 单条镜像记录（未知字段忽略，可空字段 Option 容错）
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MirrorImage {
    pub id: u64,
    pub name: String,
    pub version: String,
    pub architecture: String,
    pub size: u64,
    pub online: u8,
    pub download_url: String,
    pub md5sum: Option<String>,
    pub last_update_time: Option<String>,
}

/// findAllName 响应解析；success=false 或 JSON 非法报错
pub fn parse_names_response(json: &str) -> Result<Vec<String>> {
    #[derive(serde::Deserialize)]
    struct NamesResponse {
        success: bool,
        message: String,
        data: Option<Vec<String>>,
    }
    let resp: NamesResponse =
        serde_json::from_str(json).map_err(|e| anyhow!("解析系统名列表失败: {e}"))?;
    if !resp.success {
        return Err(anyhow!("查询失败: {}", resp.message));
    }
    Ok(resp.data.unwrap_or_default())
}

/// findByNameOrVersion 响应解析
pub fn parse_images_response(json: &str) -> Result<Vec<MirrorImage>> {
    #[derive(serde::Deserialize)]
    struct ImagesResponse {
        success: bool,
        message: String,
        data: Option<Vec<MirrorImage>>,
    }
    let resp: ImagesResponse =
        serde_json::from_str(json).map_err(|e| anyhow!("解析镜像列表失败: {e}"))?;
    if !resp.success {
        return Err(anyhow!("查询失败: {}", resp.message));
    }
    Ok(resp.data.unwrap_or_default())
}

/// 字节数 → "1.4 GB" / "52.0 MB"
pub fn format_size(bytes: u64) -> String {
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    if bytes as f64 >= GB {
        format!("{:.1} GB", bytes as f64 / GB)
    } else {
        format!("{:.1} MB", bytes as f64 / MB)
    }
}

/// 从下载链接提取文件名（URL 末段）
pub fn file_name_from_url(url: &str) -> Result<String> {
    url.rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .map(String::from)
        .ok_or_else(|| anyhow!("无法从下载链接提取文件名: {url}"))
}

/// 重命名候选路径：`<stem>.<n>.<ext>`（如 a.iso → a.1.iso）
pub fn renamed_path(dir: &Path, file_name: &str, n: usize) -> std::path::PathBuf {
    let path = Path::new(file_name);
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(file_name);
    if ext.is_empty() {
        dir.join(format!("{stem}.{n}"))
    } else {
        dir.join(format!("{stem}.{n}.{ext}"))
    }
}

/// 按 version 字段精确匹配镜像
pub fn find_image_by_version<'a>(
    images: &'a [MirrorImage],
    version: &str,
) -> Option<&'a MirrorImage> {
    images.iter().find(|i| i.version == version)
}

#[cfg(test)]
mod tests {
    // ... Step 1 的测试代码 ...
}
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test mirror`
Expected: 7 个测试全部 PASS

- [ ] **Step 5: 提交**

```bash
git add Cargo.toml Cargo.lock src/core/mirror.rs src/core/mod.rs
git commit -m "feat: mirror 镜像 API 解析层（模型/响应解析/大小与文件名工具）"
```

---

### Task 2: mirror 网络层（api_base + fetch 函数）

**Files:**
- Modify: `src/core/mirror.rs`（追加网络函数 + mock 集成测试）

**Interfaces:**
- Consumes: `http_get_string(url: &str) -> Result<String>`（`crate::core::download`）；Task 1 的 parse 函数
- Produces:
  - `pub fn api_base() -> String`（`DEVKIT_MIRROR_API` 覆盖，默认官方基址）
  - `pub fn fetch_all_names() -> Result<Vec<String>>`
  - `pub fn fetch_images(name: &str) -> Result<Vec<MirrorImage>>`

- [ ] **Step 1: 写失败测试**

`src/core/mirror.rs` 的 `use` 区补充 `use crate::core::download::http_get_string;`，tests 模块内追加（先写测试，函数未定义 → RED）：

```rust
    use serial_test::serial;

    #[cfg(test)]
    fn mock_server(responses: Vec<(u16, String)>) -> String {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for (status, body) in responses {
                let (mut stream, _) = listener.accept().unwrap();
                let mut buf = [0u8; 4096];
                let _ = stream.read(&mut buf);
                let reason = if status == 200 { "OK" } else { "Error" };
                let resp = format!(
                    "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(resp.as_bytes());
            }
        });
        format!("http://{addr}")
    }

    #[test]
    #[serial]
    fn fetch_all_names_hits_api() {
        let base = mock_server(vec![(
            200,
            r#"{"success":true,"message":"查询成功","data":["almalinux","ubuntu"]}"#.to_string(),
        )]);
        std::env::set_var("DEVKIT_MIRROR_API", &base);
        let names = fetch_all_names().unwrap();
        std::env::remove_var("DEVKIT_MIRROR_API");
        assert_eq!(names, vec!["almalinux", "ubuntu"]);
    }

    #[test]
    #[serial]
    fn fetch_images_hits_api() {
        let base = mock_server(vec![(200, IMAGE_JSON.to_string())]);
        std::env::set_var("DEVKIT_MIRROR_API", &base);
        let images = fetch_images("almalinux").unwrap();
        std::env::remove_var("DEVKIT_MIRROR_API");
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].version, "9(latest-aarch64-boot)");
    }
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test mirror`
Expected: 编译失败（`fetch_all_names` / `fetch_images` 未定义）——RED

- [ ] **Step 3: 实现网络层**

`src/core/mirror.rs` 实现区（`find_image_by_version` 之后）追加：

```rust
/// API 基址：DEVKIT_MIRROR_API 环境变量覆盖（测试钩子），默认阿里云开发者镜像 API
pub fn api_base() -> String {
    std::env::var("DEVKIT_MIRROR_API").unwrap_or_else(|_| {
        "https://developer.aliyun.com/developer/api/mirror/image".to_string()
    })
}

pub fn fetch_all_names() -> Result<Vec<String>> {
    let body = http_get_string(&format!("{}/findAllName", api_base()))?;
    parse_names_response(&body)
}

pub fn fetch_images(name: &str) -> Result<Vec<MirrorImage>> {
    let body = http_get_string(&format!("{}/findByNameOrVersion?name={name}", api_base()))?;
    parse_images_response(&body)
}
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test mirror`
Expected: 全部 PASS（含 Task 1 的 7 个，共 9 个）

- [ ] **Step 5: 提交**

```bash
git add src/core/mirror.rs
git commit -m "feat: mirror 镜像 API 网络层（fetch_all_names/fetch_images + mock 测试）"
```

---

### Task 3: mirror MD5 校验层

**Files:**
- Modify: `src/core/mirror.rs`（追加 MD5 函数 + 单元测试）

**Interfaces:**
- Consumes: `http_get_string`；Task 1 的 `MirrorImage`
- Produces:
  - `pub fn md5_of(path: &Path) -> Result<String>`
  - `pub fn parse_md5sums(text: &str) -> HashMap<String, String>`
  - `pub fn verify_image_md5(path: &Path, image: &MirrorImage) -> Result<()>`（无字段/拉取失败/匹配不到 → 警告降级返回 Ok；匹配不一致 → Err）

- [ ] **Step 1: 写失败测试**

`src/core/mirror.rs` tests 模块追加（`md5_of` 期望值：字符串 `"data"` 的 MD5 是 `8d777f385d3dfec8815d20f7496026dc`）：

```rust
    #[test]
    fn parse_md5sums_ok() {
        let sums = parse_md5sums("abc123  a.iso\nxyz  b.iso\n");
        assert_eq!(sums.get("a.iso"), Some(&"abc123".to_string()));
        assert_eq!(sums.get("b.iso"), Some(&"xyz".to_string()));
        assert!(sums.get("c.iso").is_none());
    }

    #[test]
    fn md5_of_matches_known_hash() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("a.txt");
        std::fs::write(&f, b"data").unwrap();
        assert_eq!(md5_of(&f).unwrap(), "8d777f385d3dfec8815d20f7496026dc");
    }

    #[test]
    fn verify_image_md5_no_field_ok() {
        let images = parse_images_response(IMAGE_JSON).unwrap(); // md5sum 为 null
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("x.iso");
        std::fs::write(&f, b"data").unwrap();
        verify_image_md5(&f, &images[0]).unwrap();
    }

    #[test]
    #[serial]
    fn verify_image_md5_unreachable_sumfile_warns_ok() {
        // 拉取失败（无服务监听）→ 降级警告不报错
        let mut img = parse_images_response(IMAGE_JSON).unwrap().remove(0);
        img.md5sum = Some("http://127.0.0.1:1/nonexistent/MD5SUMS".to_string());
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("x.iso");
        std::fs::write(&f, b"data").unwrap();
        verify_image_md5(&f, &img).unwrap();
    }

    #[test]
    #[serial]
    fn verify_image_md5_missing_entry_warns_ok() {
        // 校验文件里没有该文件记录 → 降级警告不报错
        let base = mock_server(vec![(
            200,
            "00000000000000000000000000000000  other.iso".to_string(),
        )]);
        std::env::set_var("DEVKIT_MIRROR_API", &base);
        let mut img = parse_images_response(IMAGE_JSON).unwrap().remove(0);
        img.md5sum = Some(format!("{base}/MD5SUMS"));
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("x.iso");
        std::fs::write(&f, b"data").unwrap();
        verify_image_md5(&f, &img).unwrap();
        std::env::remove_var("DEVKIT_MIRROR_API");
    }

    #[test]
    #[serial]
    fn verify_image_md5_mismatch_errors() {
        let base = mock_server(vec![(
            200,
            "00000000000000000000000000000000  x.iso".to_string(),
        )]);
        std::env::set_var("DEVKIT_MIRROR_API", &base);
        let mut img = parse_images_response(IMAGE_JSON).unwrap().remove(0);
        img.md5sum = Some(format!("{base}/MD5SUMS"));
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("x.iso");
        std::fs::write(&f, b"data").unwrap();
        let err = verify_image_md5(&f, &img).unwrap_err();
        std::env::remove_var("DEVKIT_MIRROR_API");
        assert!(err.to_string().contains("MD5 校验失败"));
    }
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test mirror`
Expected: 编译失败（`md5_of` 等未定义）——RED

- [ ] **Step 3: 实现 MD5 层**

`src/core/mirror.rs` 顶部 use 区改为：

```rust
use std::collections::HashMap;
use std::path::Path;

use anyhow::{anyhow, Result};

use crate::core::download::http_get_string;
```

实现区（`fetch_images` 之后）追加：

```rust
/// 计算文件 MD5（小写十六进制）
pub fn md5_of(path: &Path) -> Result<String> {
    use md5::{Digest, Md5};
    let mut hasher = Md5::new();
    let mut file = std::fs::File::open(path)?;
    std::io::copy(&mut file, &mut hasher)?;
    Ok(format!("{:x}", hasher.finalize()))
}

/// 解析 MD5SUMS 文本（"<hash>  <filename>" 每行）→ 文件名 → 哈希
pub fn parse_md5sums(text: &str) -> HashMap<String, String> {
    text.lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let hash = parts.next()?.to_lowercase();
            let file = parts.next()?.to_string();
            Some((file, hash))
        })
        .collect()
}

/// 按镜像记录做 MD5 校验（降级警告策略）：
/// 无 md5sum 字段 / 拉取校验文件失败 / 文件中无该文件记录 → 警告不阻断；
/// 匹配到哈希且不一致 → 报错
pub fn verify_image_md5(path: &Path, image: &MirrorImage) -> Result<()> {
    let Some(url) = image.md5sum.as_deref().filter(|s| !s.is_empty()) else {
        return Ok(());
    };
    let text = match http_get_string(url) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("警告: 拉取校验文件失败，跳过 MD5 校验: {e}");
            return Ok(());
        }
    };
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_string();
    let sums = parse_md5sums(&text);
    let Some(expected) = sums.get(&file_name) else {
        eprintln!("警告: 校验文件中未找到 {file_name} 的记录，跳过 MD5 校验");
        return Ok(());
    };
    let actual = md5_of(path)?;
    if actual != *expected {
        return Err(anyhow!(
            "MD5 校验失败: 期望 {expected}, 实际 {actual}（可重跑下载）"
        ));
    }
    println!("MD5 校验通过: {file_name}");
    Ok(())
}
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test mirror`
Expected: 全部 PASS（共 14 个）。若 `verify_image_md5_no_field_ok` 依赖 `tempfile`，确认其为 dev-dependency 已存在

- [ ] **Step 5: 提交**

```bash
git add src/core/mirror.rs
git commit -m "feat: mirror 镜像 MD5 校验（MD5SUMS 解析/降级警告策略）"
```

---

### Task 4: 命令面接线 + os list / os info

**Files:**
- Modify: `src/lib.rs`（`OsCommand` 枚举、`Command::Os` 变体、run() 分发、解析测试）
- Create: `src/commands/os.rs`（run/run_list/run_info）
- Modify: `src/commands/mod.rs`（注册 `pub mod os;`）
- Create: `tests/cli_os.rs`（os list / os info 集成测试）

**Interfaces:**
- Consumes: `mirror::fetch_all_names`、`mirror::fetch_images`、`mirror::format_size`
- Produces:
  - `pub fn run(cmd: OsCommand) -> Result<()>`（commands/os.rs，lib.rs run() 调用）

- [ ] **Step 1: 写失败测试**

`src/lib.rs` tests 模块追加：

```rust
    #[test]
    fn os_subcommands_parse() {
        assert!(matches!(
            Cli::try_parse_from(["cli", "os", "list"]).unwrap().command,
            Command::Os {
                subcommand: OsCommand::List
            }
        ));
        assert!(matches!(
            Cli::try_parse_from(["cli", "os", "info", "almalinux"]).unwrap().command,
            Command::Os {
                subcommand: OsCommand::Info { .. }
            }
        ));
        // 前缀缩写：o → os，l → list，i → info，d → download
        assert!(matches!(
            Cli::try_parse_from(["cli", "o", "l"]).unwrap().command,
            Command::Os {
                subcommand: OsCommand::List
            }
        ));
        assert!(matches!(
            Cli::try_parse_from(["cli", "o", "d", "ubuntu", "--version", "x", "-o", "/tmp"])
                .unwrap()
                .command,
            Command::Os {
                subcommand: OsCommand::Download { .. }
            }
        ));
    }
```

创建 `tests/cli_os.rs`：

```rust
use assert_cmd::Command;
use predicates::prelude::*;
use std::io::{Read, Write};
use std::net::TcpListener;

/// 单请求 mock：返回 findAllName / findByNameOrVersion 响应
fn mock_api(names_body: &str, images_body: &str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let names_body = names_body.to_string();
    let images_body = images_body.to_string();
    std::thread::spawn(move || {
        for _ in 0..2 {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf);
            let req = String::from_utf8_lossy(&buf);
            let body = if req.contains("findAllName") {
                &names_body
            } else {
                &images_body
            };
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(resp.as_bytes());
        }
    });
    format!("http://{addr}")
}

const IMAGES_JSON: &str = r#"{"success":true,"code":"200","message":"查询成功","data":[{"id":1,"name":"almalinux","version":"9(latest-aarch64-boot)","architecture":"","size":1458331648,"online":1,"downloadUrl":"https://mirrors.aliyun.com/almalinux/9.8/isos/aarch64/AlmaLinux-9-latest-aarch64-boot.iso","md5sum":null,"lastUpdateTime":"2026-05-28 22:40:16","deletedAt":null,"status":"ok","gmtCreate":null,"gmtModified":null,"isDel":0}]}"#;

#[test]
fn os_list_shows_mirror_names() {
    let base = mock_api(
        r#"{"success":true,"message":"查询成功","data":["almalinux","ubuntu"]}"#,
        IMAGES_JSON,
    );
    Command::cargo_bin("cli")
        .unwrap()
        .env("DEVKIT_MIRROR_API", &base)
        .args(["os", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("almalinux"))
        .stdout(predicate::str::contains("ubuntu"));
}

#[test]
fn os_info_shows_images_table() {
    let base = mock_api(
        r#"{"success":true,"message":"查询成功","data":["almalinux"]}"#,
        IMAGES_JSON,
    );
    Command::cargo_bin("cli")
        .unwrap()
        .env("DEVKIT_MIRROR_API", &base)
        .args(["os", "info", "almalinux"])
        .assert()
        .success()
        .stdout(predicate::str::contains("共 1 个镜像"))
        .stdout(predicate::str::contains("9(latest-aarch64-boot)"))
        .stdout(predicate::str::contains("1.4 GB"))
        .stdout(predicate::str::contains("AlmaLinux-9-latest-aarch64-boot.iso"));
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test os_subcommands_parse && cargo test --test cli_os`
Expected: 编译失败（`OsCommand` 未定义、`commands::os` 不存在）——RED

- [ ] **Step 3: 实现命令面**

`src/lib.rs` 的 `Command` 枚举中 `Update` 变体后追加：

```rust
    /// 操作系统镜像查询与下载（阿里云镜像）
    Os {
        #[command(subcommand)]
        subcommand: OsCommand,
    },
```

`Command` 枚举之后追加（与 `Command` 同文件同层）：

```rust
#[derive(Subcommand)]
pub enum OsCommand {
    /// 列出阿里云镜像支持的系统名
    List,
    /// 查询系统全部镜像（版本/大小/链接）
    Info {
        /// 系统名（如 almalinux、ubuntu）
        name: String,
    },
    /// 下载系统 ISO 镜像
    Download {
        /// 系统名（如 almalinux、ubuntu）
        name: String,
        /// 精确指定镜像版本（version 字段）；不填则交互选择
        #[arg(long)]
        version: Option<String>,
        /// 下载保存目录（默认当前目录）
        #[arg(short, long, default_value = ".")]
        output_dir: String,
    },
}
```

`src/lib.rs` 的 run() match 追加分支：

```rust
        Command::Os { subcommand } => commands::os::run(subcommand),
```

创建 `src/commands/os.rs`：

```rust
use anyhow::{anyhow, Result};

use crate::core::mirror;
use crate::OsCommand;

pub fn run(cmd: OsCommand) -> Result<()> {
    match cmd {
        OsCommand::List => run_list(),
        OsCommand::Info { name } => run_info(&name),
        OsCommand::Download {
            name,
            version,
            output_dir,
        } => run_download(&name, version.as_deref(), &output_dir),
    }
}

pub fn run_list() -> Result<()> {
    let names = mirror::fetch_all_names()?;
    if names.is_empty() {
        println!("暂无可用系统镜像");
        return Ok(());
    }
    for n in names {
        println!("{n}");
    }
    Ok(())
}

pub fn run_info(name: &str) -> Result<()> {
    let images = mirror::fetch_images(name)?;
    if images.is_empty() {
        println!("系统 {name} 暂无可用镜像");
        return Ok(());
    }
    println!("{name} 共 {} 个镜像:", images.len());
    for (i, img) in images.iter().enumerate() {
        println!(
            " {}  {:<28} {:<10} {}  {}",
            i + 1,
            img.version,
            mirror::format_size(img.size),
            img.last_update_time.as_deref().unwrap_or("-"),
            img.download_url
        );
    }
    Ok(())
}

pub fn run_download(
    name: &str,
    version: Option<&str>,
    output_dir: &str,
) -> Result<()> {
    // Task 5 实现
    Err(anyhow!("download 尚未实现"))
}
```

`src/commands/mod.rs` 追加 `pub mod os;`。

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test os_subcommands_parse && cargo test --test cli_os`
Expected: 全部 PASS

- [ ] **Step 5: 提交**

```bash
git add src/lib.rs src/commands/os.rs src/commands/mod.rs tests/cli_os.rs
git commit -m "feat: os 子命令 list/info（命令面接线 + 集成测试）"
```

---

### Task 5: os download 实现

**Files:**
- Modify: `src/commands/os.rs`（实现 `run_download`）
- Modify: `tests/cli_os.rs`（追加 download 集成测试，mock 增加 ISO 下载端点）

**Interfaces:**
- Consumes: `mirror::fetch_images` / `mirror::find_image_by_version` / `mirror::format_size` / `mirror::file_name_from_url` / `mirror::renamed_path` / `mirror::verify_image_md5`；`interact::{is_interactive, select, confirm}`；`download::download`
- Produces: `run_download` 完整实现（无新公开接口）

- [ ] **Step 1: 写失败测试**

`tests/cli_os.rs` 追加 mock 辅助（双请求：API JSON + ISO 二进制，ISO URL 动态指向 mock 自身）与测试：

```rust
/// 双请求 mock：findByNameOrVersion → 镜像 JSON（downloadUrl 指向自身）；ISO 路径 → 二进制
fn mock_api_with_iso() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for _ in 0..2 {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf);
            let req = String::from_utf8_lossy(&buf);
            let (status, body) = if req.contains("findByNameOrVersion") {
                (
                    200u16,
                    format!(
                        r#"{{"success":true,"message":"查询成功","data":[{{"id":1,"name":"almalinux","version":"9(latest-aarch64-boot)","architecture":"","size":8,"online":1,"downloadUrl":"http://{addr}/AlmaLinux-9-latest-aarch64-boot.iso","md5sum":null,"lastUpdateTime":"2026-05-28 22:40:16","deletedAt":null,"status":"ok","gmtCreate":null,"gmtModified":null,"isDel":0}}]}}"#
                    ),
                )
            } else {
                (200u16, "iso-data".to_string())
            };
            let resp = format!(
                "HTTP/1.1 {status} OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(resp.as_bytes());
        }
    });
    format!("http://{addr}")
}

#[test]
fn os_download_with_version_downloads_file() {
    let base = mock_api_with_iso();
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("dl");
    Command::cargo_bin("cli")
        .unwrap()
        .env("DEVKIT_MIRROR_API", &base)
        .args([
            "os", "download", "almalinux", "--version", "9(latest-aarch64-boot)", "-o",
            out.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("下载完成"));
    assert_eq!(
        std::fs::read_to_string(out.join("AlmaLinux-9-latest-aarch64-boot.iso")).unwrap(),
        "iso-data"
    );
}

#[test]
fn os_download_skips_existing_file_non_tty() {
    let base = mock_api_with_iso();
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("AlmaLinux-9-latest-aarch64-boot.iso");
    std::fs::write(&dest, b"old").unwrap();
    Command::cargo_bin("cli")
        .unwrap()
        .env("DEVKIT_MIRROR_API", &base)
        .args([
            "os", "download", "almalinux", "--version", "9(latest-aarch64-boot)", "-o",
            dir.path().to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("已存在，跳过"));
    assert_eq!(std::fs::read_to_string(&dest).unwrap(), "old");
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test --test cli_os os_download`
Expected: FAIL（`run_download` 返回 "download 尚未实现"，命令 exit 非 0）——RED

- [ ] **Step 3: 实现 run_download**

`src/commands/os.rs` 中占位实现替换为：

```rust
use std::path::Path;

use anyhow::{anyhow, Result};

use crate::core::download;
use crate::core::interact::{confirm, is_interactive, select};
use crate::core::mirror;
use crate::OsCommand;

pub fn run_download(
    name: &str,
    version: Option<&str>,
    output_dir: &str,
) -> Result<()> {
    let images = mirror::fetch_images(name)?;
    if images.is_empty() {
        return Err(anyhow!("系统 {name} 暂无可用镜像"));
    }
    println!("{name} 共 {} 个镜像:", images.len());
    for (i, img) in images.iter().enumerate() {
        println!(
            " {}  {:<28} {:<10} {}",
            i + 1,
            img.version,
            mirror::format_size(img.size),
            img.download_url
        );
    }
    let selected = match version {
        Some(v) => mirror::find_image_by_version(&images, v).ok_or_else(|| {
            let avail: Vec<&str> = images.iter().map(|i| i.version.as_str()).collect();
            anyhow!("未找到版本 {v}，可用版本: {}", avail.join("、"))
        })?,
        None => {
            if !is_interactive() {
                return Err(anyhow!(
                    "非终端环境请通过 --version 指定镜像版本，例如: cli os download {name} --version <版本>"
                ));
            }
            let labels: Vec<String> = images
                .iter()
                .map(|i| format!("{}  ({})", i.version, mirror::format_size(i.size)))
                .collect();
            let idx = select(&format!("请选择要下载的 {name} 镜像"), &labels)?;
            &images[idx]
        }
    };
    let dir = Path::new(output_dir);
    std::fs::create_dir_all(dir)?;
    let file_name = mirror::file_name_from_url(&selected.download_url)?;
    let mut dest = dir.join(&file_name);
    if dest.exists() {
        if !is_interactive() {
            println!("目标文件已存在，跳过: {}", dest.display());
            return Ok(());
        }
        let idx = select("文件已存在，请选择处理方式", &["覆盖下载", "跳过", "重命名"])?;
        match idx {
            0 => {}
            1 => {
                println!("已跳过");
                return Ok(());
            }
            _ => {
                let mut n = 1;
                while mirror::renamed_path(dir, &file_name, n).exists() {
                    n += 1;
                }
                dest = mirror::renamed_path(dir, &file_name, n);
            }
        }
    }
    println!(
        "准备下载 {name} {} → {}",
        selected.version,
        dest.display()
    );
    println!(
        "大小: {} | 链接: {}",
        mirror::format_size(selected.size),
        selected.download_url
    );
    if is_interactive() && !confirm("确认开始下载？", true)? {
        println!("已取消");
        return Ok(());
    }
    download::download(
        &selected.download_url,
        &dest,
        None,
        &format!("{name} {}", selected.version),
    )?;
    mirror::verify_image_md5(&dest, selected)?;
    println!("下载完成: {}", dest.display());
    Ok(())
}
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test --test cli_os`
Expected: 4 个测试全部 PASS

- [ ] **Step 5: 提交**

```bash
git add src/commands/os.rs tests/cli_os.rs
git commit -m "feat: os download 镜像下载（选择/确认/冲突处理/MD5 校验）"
```

---

### Task 6: README 更新与全量验证

**Files:**
- Modify: `README.md`

- [ ] **Step 1: README 补充 os 命令说明**

README 的 `### cli list` 小节之前插入：

```markdown
### `cli os <subcommand>`

查询并下载阿里云开发者镜像的操作系统 ISO（almalinux / ubuntu / centos / rockylinux / anolis / deepin / archlinux / openSUSE / centos-arch）：

- `cli os list`：列出所有可用系统名
- `cli os info <系统名>`：列出该系统全部镜像（版本 / 大小 / 更新时间 / 下载链接）
- `cli os download <系统名> [--version <版本>] [-o <目录>]`：下载 ISO 到指定目录（默认当前目录）；不填 `--version` 时交互选择，非终端环境必须显式指定；文件已存在时交互选择覆盖 / 跳过 / 重命名（非终端自动跳过）
- 下载完成后尝试按镜像记录中的 MD5SUMS 做校验；校验文件拉取失败或未收录时仅警告（API 数据不完整时降级）
```

README 的「环境变量」表格末尾追加一行：

```markdown
| `DEVKIT_MIRROR_API` | 阿里云镜像 API 基址覆盖（默认 `https://developer.aliyun.com/developer/api/mirror/image`），测试用 |
```

README 的「功能特性」列表追加一条：

```markdown
- **系统镜像下载**：查询并下载阿里云镜像的 Linux ISO（9 个系统，交互选择版本）
```

- [ ] **Step 2: 全量门禁验证**

Run: `cargo test` 与 `cargo clippy --all-targets -- -D warnings` 与 `cargo fmt --check`
Expected: 全部 PASS / 零警告 / 无格式差异（若有格式差异先 `cargo fmt` 再重跑）

- [ ] **Step 3: 手动冒烟（可选，网络可用时）**

```bash
cargo run -- os list
cargo run -- os info almalinux
```

Expected: 输出真实系统名与镜像列表

- [ ] **Step 4: 提交**

```bash
git add README.md
git commit -m "docs: README 补充 os 子命令与 DEVKIT_MIRROR_API 说明"
```

---

## 验证清单（全部任务完成后）

- [ ] `cargo test` 全绿（含新增 mirror 单元测试与 tests/cli_os.rs 集成测试）
- [ ] `cargo clippy --all-targets -- -D warnings` 零警告
- [ ] `cargo fmt --check` 通过
- [ ] `cli os --help` 显示三个子命令；`cli o l` / `cli os i almalinux` / `cli os d ubuntu` 缩写可用
- [ ] README 已同步（命令详解 / 环境变量 / 功能特性）
- [ ] 提交历史：6 个 commit（Task 1-6），每个独立可验证
