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
