use std::path::Path;

use anyhow::{anyhow, Result};

pub fn http_get_string(url: &str) -> Result<String> {
    let body = ureq::get(url).call()?.into_string()?;
    Ok(body)
}

pub fn download(url: &str, dest: &Path, expected_sha256: Option<&str>) -> Result<()> {
    let part = dest.with_extension("part");
    let mut last_err: Option<String> = None;
    for attempt in 0..3 {
        match ureq::get(url).call() {
            Ok(resp) => {
                let mut reader = resp.into_reader();
                let mut file = std::fs::File::create(&part)?;
                std::io::copy(&mut reader, &mut file)?;
                drop(file);
                if let Some(expected) = expected_sha256 {
                    verify_sha256(&part, expected)?;
                }
                std::fs::rename(&part, dest)?;
                return Ok(());
            }
            Err(e) => {
                last_err = Some(e.to_string());
                let backoff = 200u64 * (1 << attempt);
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
use std::io::{Read, Write};
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
        assert_eq!(http_get_string(&format!("{base}/x")).unwrap(), "hello-world");
    }

    #[test]
    fn download_writes_file_content() {
        let base = mock_server(vec![(200, "binary-data".to_string())]);
        let dir = tempdir().unwrap();
        let dest = dir.path().join("out.bin");
        download(&format!("{base}/f"), &dest, None).unwrap();
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "binary-data");
    }

    #[test]
    fn download_retries_on_server_error() {
        let base = mock_server(vec![(500, "err".to_string()), (200, "ok".to_string())]);
        let dir = tempdir().unwrap();
        let dest = dir.path().join("out.bin");
        download(&format!("{base}/f"), &dest, None).unwrap();
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
        verify_sha256(&f, "3A6EB0790F39AC87C94F3856B2DD2C5D110E6811602261A9A923D3BB23ADC8B7")
            .unwrap(); // 大写也应通过
    }

    #[test]
    fn extract_archive_unpacks_tar_gz() {
        let dir = tempdir().unwrap();
        let archive = dir.path().join("x.tar.gz");
        make_tar_gz(&archive);
        let out = dir.path().join("out");
        extract_archive(&archive, &out).unwrap();
        assert_eq!(std::fs::read_to_string(out.join("hello.txt")).unwrap(), "data");
    }

    #[test]
    fn extract_archive_unpacks_zip() {
        let dir = tempdir().unwrap();
        let archive = dir.path().join("x.zip");
        make_zip(&archive);
        let out = dir.path().join("out");
        extract_archive(&archive, &out).unwrap();
        assert_eq!(std::fs::read_to_string(out.join("hello.txt")).unwrap(), "data");
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
