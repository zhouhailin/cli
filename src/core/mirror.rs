use std::collections::HashMap;
use std::path::Path;

use anyhow::{anyhow, Result};

use crate::core::download::http_get_string;

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
}
