use assert_cmd::Command;
use predicates::prelude::*;
use std::io::{Read, Write};
use std::net::TcpListener;

/// 脚本 mock：返回指定 shell 脚本内容
fn mock_script(body: &str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let body = body.to_string();
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 4096];
        let _ = stream.read(&mut buf);
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
    let base = mock_script("#!/bin/sh\necho mock-rustup-done\n");
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
    // 完整安装标志：rustup 二进制已就位
    std::fs::create_dir_all(root.join("rustup/bin")).unwrap();
    std::fs::write(root.join("rustup/bin/rustup"), "").unwrap();
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

#[test]
fn rust_install_warns_and_continues_on_residual() {
    // 上次安装失败留下的残留（有 settings.toml 但 rustup 二进制未就位）
    let base = mock_script("#!/bin/sh\necho mock-rustup-done\n");
    let home = tempfile::tempdir().unwrap();
    let root = home.path().join("devkit");
    std::fs::create_dir_all(root.join("rustup")).unwrap();
    std::fs::write(root.join("rustup/settings.toml"), "").unwrap();
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
        .stdout(predicate::str::contains("未完成的安装残留"))
        .stdout(predicate::str::contains("mock-rustup-done"))
        .stdout(predicate::str::contains("安装完成"));
}

#[test]
fn rust_install_failure_hints_cleanup() {
    // 安装脚本失败：错误信息应包含退出码与清理指引
    let base = mock_script("#!/bin/sh\necho mock-rustup-fail\nexit 1\n");
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
        .failure()
        .stderr(predicate::str::contains("退出码 1"))
        .stderr(predicate::str::contains("rm -rf"));
}
