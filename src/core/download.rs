use std::io::{IsTerminal, Read, Write};
use std::path::Path;

use anyhow::{anyhow, Result};

use crate::debug_log;

pub fn http_get_string(url: &str) -> Result<String> {
    http_get_string_with_headers(url, &[])
}

/// 带自定义请求头获取文本（部分官方 API 需要 Referer/UA 校验）
pub fn http_get_string_with_headers(url: &str, headers: &[(&str, &str)]) -> Result<String> {
    debug_log!("HTTP GET {url}");
    let mut request = ureq::get(url);
    for (k, v) in headers {
        request = request.set(k, v);
    }
    let body = request.call()?.into_string()?;
    debug_log!("HTTP GET 完成: {} 字节", body.len());
    Ok(body)
}

/// 进度行渲染：有总大小显示百分比，无总大小（chunked）仅显示字节
fn format_progress(label: &str, done: u64, total: Option<u64>) -> String {
    let mb = |b: u64| b as f64 / (1024.0 * 1024.0);
    match total {
        Some(t) if t > 0 => {
            let pct = (done as f64 / t as f64 * 100.0).min(100.0) as u64;
            format!("下载 {label}: {:.1}/{:.1} MB ({pct}%)", mb(done), mb(t))
        }
        _ => format!("下载 {label}: {:.1} MB", mb(done)),
    }
}

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

pub fn verify_sha256(path: &Path, expected: &str) -> Result<()> {
    let actual = sha256_of(path)?;
    if actual != expected.to_lowercase() {
        return Err(anyhow!("SHA-256 校验失败: 期望 {expected}, 实际 {actual}"));
    }
    Ok(())
}

pub fn sha256_of(path: &Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    let mut file = std::fs::File::open(path)?;
    std::io::copy(&mut file, &mut hasher)?;
    Ok(format!("{:x}", hasher.finalize()))
}

pub fn extract_archive(archive: &Path, dest_dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dest_dir)?;
    let name = archive
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    debug_log!("解压 {name} -> {}", dest_dir.display());
    if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        let file = std::fs::File::open(archive)?;
        let gz = flate2::read::GzDecoder::new(file);
        let mut tar = tar::Archive::new(gz);
        tar.unpack(dest_dir)?;
        Ok(())
    } else if name.ends_with(".zip") {
        let file = std::fs::File::open(archive)?;
        let mut zip = zip::ZipArchive::new(file)?;
        zip.extract(dest_dir)?;
        Ok(())
    } else {
        Err(anyhow!("不支持的压缩格式: {name}"))
    }
}

// ---- 测试辅助：本地 mock HTTP 服务 ----
#[cfg(test)]
use std::net::TcpListener;

#[cfg(test)]
fn mock_server(responses: Vec<(u16, String)>) -> String {
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

#[cfg(test)]
fn make_tar_gz(path: &Path) {
    let file = std::fs::File::create(path).unwrap();
    let gz = flate2::write::GzEncoder::new(file, flate2::Compression::default());
    let mut tar = tar::Builder::new(gz);
    let mut header = tar::Header::new_gnu();
    header.set_size(4);
    header.set_mode(0o644);
    header.set_cksum();
    tar.append_data(&mut header, "hello.txt", std::io::Cursor::new(b"data"))
        .unwrap();
    tar.finish().unwrap();
}

#[cfg(test)]
fn make_zip(path: &Path) {
    let file = std::fs::File::create(path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    zip.start_file("hello.txt", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(b"data").unwrap();
    zip.finish().unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn http_get_string_returns_body() {
        let base = mock_server(vec![(200, "hello-world".to_string())]);
        assert_eq!(
            http_get_string(&format!("{base}/x")).unwrap(),
            "hello-world"
        );
    }

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

    #[test]
    fn download_writes_file_content() {
        let base = mock_server(vec![(200, "binary-data".to_string())]);
        let dir = tempdir().unwrap();
        let dest = dir.path().join("out.bin");
        download(&format!("{base}/f"), &dest, None, "test").unwrap();
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "binary-data");
    }

    #[test]
    fn download_retries_on_server_error() {
        let base = mock_server(vec![(500, "err".to_string()), (200, "ok".to_string())]);
        let dir = tempdir().unwrap();
        let dest = dir.path().join("out.bin");
        download(&format!("{base}/f"), &dest, None, "test").unwrap();
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "ok");
    }

    #[test]
    fn download_fails_on_sha_mismatch() {
        let base = mock_server(vec![(200, "data".to_string())]);
        let dir = tempdir().unwrap();
        let dest = dir.path().join("out.bin");
        let err = download(
            &format!("{base}/f"),
            &dest,
            Some("0000000000000000000000000000000000000000000000000000000000000000"),
            "test",
        )
        .unwrap_err();
        assert!(err.to_string().contains("SHA-256 校验失败"));
    }

    #[test]
    fn sha256_of_returns_expected_hash() {
        let dir = tempdir().unwrap();
        let f = dir.path().join("a.txt");
        std::fs::write(&f, b"data").unwrap();
        assert_eq!(
            sha256_of(&f).unwrap(),
            "3a6eb0790f39ac87c94f3856b2dd2c5d110e6811602261a9a923d3bb23adc8b7"
        );
    }

    #[test]
    fn verify_sha256_accepts_correct_hash() {
        let dir = tempdir().unwrap();
        let f = dir.path().join("a.txt");
        std::fs::write(&f, b"data").unwrap();
        verify_sha256(
            &f,
            "3A6EB0790F39AC87C94F3856B2DD2C5D110E6811602261A9A923D3BB23ADC8B7",
        )
        .unwrap(); // 大写也应通过
    }

    #[test]
    fn extract_archive_unpacks_tar_gz() {
        let dir = tempdir().unwrap();
        let archive = dir.path().join("x.tar.gz");
        make_tar_gz(&archive);
        let out = dir.path().join("out");
        extract_archive(&archive, &out).unwrap();
        assert_eq!(
            std::fs::read_to_string(out.join("hello.txt")).unwrap(),
            "data"
        );
    }

    #[test]
    fn extract_archive_unpacks_zip() {
        let dir = tempdir().unwrap();
        let archive = dir.path().join("x.zip");
        make_zip(&archive);
        let out = dir.path().join("out");
        extract_archive(&archive, &out).unwrap();
        assert_eq!(
            std::fs::read_to_string(out.join("hello.txt")).unwrap(),
            "data"
        );
    }

    #[test]
    fn extract_archive_rejects_unknown_format() {
        let dir = tempdir().unwrap();
        let archive = dir.path().join("x.rar");
        std::fs::write(&archive, b"x").unwrap();
        let err = extract_archive(&archive, &dir.path().join("out")).unwrap_err();
        assert!(err.to_string().contains("不支持的压缩格式"));
    }
}
