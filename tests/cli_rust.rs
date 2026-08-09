use assert_cmd::Command;
use predicates::prelude::*;
use std::io::{Read, Write};
use std::net::TcpListener;

/// 脚本 mock：任何请求返回假 rustup-init 脚本（输出标记行）
fn mock_script() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 4096];
        let _ = stream.read(&mut buf);
        let body = "#!/bin/sh\necho mock-rustup-done\n";
        let resp = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(resp.as_bytes());
    });
    format!("http://{addr}/rustup-init.sh")
}

#[test]
fn rust_install_non_tty_defaults_official_source() {
    let base = mock_script();
    let home = tempfile::tempdir().unwrap();
    let root = home.path().join("devkit");
    Command::cargo_bin("cli")
        .unwrap()
        .env("HOME", home.path())
        .env("DEVKIT_ROOT", &root)
        .env("SHELL", "/bin/zsh")
        .env("NO_PROXY", "127.0.0.1")
        .env("DEVKIT_RUSTUP_SCRIPT", &base)
        .args(["install", "rust"])
        .assert()
        .success()
        .stdout(predicate::str::contains("mock-rustup-done"))
        .stdout(predicate::str::contains("安装完成"));
    let rc = std::fs::read_to_string(home.path().join(".zshrc")).unwrap();
    assert!(rc.contains(&format!(
        "export RUSTUP_HOME=\"{}\"",
        root.join("rustup").display()
    )));
    assert!(rc.contains(&format!(
        "export CARGO_HOME=\"{}\"",
        root.join("cargo").display()
    )));
    assert!(rc.contains(&format!(
        "export PATH=\"{}/bin:$PATH\"",
        root.join("cargo").display()
    )));
    // 官方源不注入镜像变量
    assert!(!rc.contains("RUSTUP_UPDATE_ROOT"));
}

#[test]
fn rust_install_detects_existing_installation() {
    let home = tempfile::tempdir().unwrap();
    let root = home.path().join("devkit");
    std::fs::create_dir_all(root.join("rustup")).unwrap();
    Command::cargo_bin("cli")
        .unwrap()
        .env("HOME", home.path())
        .env("DEVKIT_ROOT", &root)
        .env("SHELL", "/bin/zsh")
        .args(["install", "rust"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("已安装"));
}
