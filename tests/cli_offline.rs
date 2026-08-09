//! 离线部署模式集成测试（CLI_OFFLINE / DEVKIT_OFFLINE）
use assert_cmd::Command;
use predicates::prelude::*;

/// 构造含单顶层目录（node-v22.12.0/）的 tar.gz 字节（与 installer.rs 测试同构）
fn make_tar_gz_bytes() -> Vec<u8> {
    let mut out = Vec::new();
    {
        let gz = flate2::write::GzEncoder::new(&mut out, flate2::Compression::default());
        let mut tar = tar::Builder::new(gz);
        let mut dir_h = tar::Header::new_gnu();
        dir_h.set_entry_type(tar::EntryType::Directory);
        dir_h.set_mode(0o755);
        dir_h.set_size(0);
        dir_h.set_cksum();
        tar.append_data(&mut dir_h, "node-v22.12.0", std::io::empty())
            .unwrap();
        let mut header = tar::Header::new_gnu();
        header.set_size(4);
        header.set_mode(0o644);
        header.set_cksum();
        tar.append_data(
            &mut header,
            "node-v22.12.0/hello.txt",
            std::io::Cursor::new(b"data"),
        )
        .unwrap();
        tar.finish().unwrap();
    }
    out
}

/// 预置缓存：tar.gz + versions.json 清单（sha 为文件实际哈希）
fn seed_cache(root: &std::path::Path, tool: &str, version: &str, file: &str, body: &[u8]) {
    let cache_dir = root.join("cache");
    std::fs::create_dir_all(&cache_dir).unwrap();
    std::fs::write(cache_dir.join(file), body).unwrap();
    let sha = {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        let mut f = std::fs::File::open(cache_dir.join(file)).unwrap();
        std::io::copy(&mut f, &mut h).unwrap();
        format!("{:x}", h.finalize())
    };
    let manifest =
        serde_json::json!({ tool: [{ "version": version, "file": file, "sha256": sha }] });
    std::fs::write(cache_dir.join("versions.json"), manifest.to_string()).unwrap();
}

#[test]
fn offline_install_succeeds_from_cache() {
    let home = tempfile::tempdir().unwrap();
    let root = home.path().join("devkit");
    let body = make_tar_gz_bytes();
    seed_cache(
        &root,
        "node",
        "v22.11.0",
        "node-v22.11.0-linux-x64.tar.gz",
        &body,
    );
    Command::cargo_bin("cli")
        .unwrap()
        .env("HOME", home.path())
        .env("DEVKIT_ROOT", &root)
        .env("SHELL", "/bin/zsh")
        .env("CLI_OFFLINE", "true")
        .args(["install", "node", "v22.11.0"])
        .assert()
        .success()
        .stdout(predicate::str::contains("离线模式"));
    let config: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(root.join("config.json")).unwrap()).unwrap();
    assert_eq!(config["installed"]["node"][0], "v22.11.0");
    assert!(root.join("node/v22.11.0/hello.txt").exists());
}

#[test]
fn offline_install_reports_warmup_hint_when_cache_missing() {
    let home = tempfile::tempdir().unwrap();
    let root = home.path().join("devkit");
    Command::cargo_bin("cli")
        .unwrap()
        .env("HOME", home.path())
        .env("DEVKIT_ROOT", &root)
        .env("SHELL", "/bin/zsh")
        .env("CLI_OFFLINE", "true")
        .args(["install", "node", "v20.0.0"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("离线模式缺少 node v20.0.0 的缓存"));
}

#[test]
fn offline_install_rejects_rust() {
    let home = tempfile::tempdir().unwrap();
    let root = home.path().join("devkit");
    Command::cargo_bin("cli")
        .unwrap()
        .env("HOME", home.path())
        .env("DEVKIT_ROOT", &root)
        .env("SHELL", "/bin/zsh")
        .env("DEVKIT_OFFLINE", "true")
        .args(["install", "rust"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("离线模式不支持 rust 安装"));
}

#[test]
fn offline_install_non_tty_without_version_reports_hint() {
    let home = tempfile::tempdir().unwrap();
    let root = home.path().join("devkit");
    Command::cargo_bin("cli")
        .unwrap()
        .env("HOME", home.path())
        .env("DEVKIT_ROOT", &root)
        .env("SHELL", "/bin/zsh")
        .env("CLI_OFFLINE", "true")
        .args(["install", "node"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("请指定版本"));
}

#[test]
fn download_command_rejects_offline_mode() {
    let home = tempfile::tempdir().unwrap();
    Command::cargo_bin("cli")
        .unwrap()
        .env("HOME", home.path())
        .env("DEVKIT_ROOT", home.path().join("devkit"))
        .env("CLI_OFFLINE", "true")
        .args(["download", "node", "v22.11.0"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("离线模式无法下载"));
}

#[test]
fn download_command_non_tty_without_tool_reports_hint() {
    let home = tempfile::tempdir().unwrap();
    Command::cargo_bin("cli")
        .unwrap()
        .env("HOME", home.path())
        .env("DEVKIT_ROOT", home.path().join("devkit"))
        .args(["download"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("请指定工具名"));
}

#[test]
fn download_command_rejects_unknown_tool() {
    let home = tempfile::tempdir().unwrap();
    Command::cargo_bin("cli")
        .unwrap()
        .env("HOME", home.path())
        .env("DEVKIT_ROOT", home.path().join("devkit"))
        .args(["download", "rust"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("暂不支持下载 rust"));
}
