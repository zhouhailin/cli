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
