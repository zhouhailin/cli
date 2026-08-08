use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::tempdir;

#[test]
fn list_shows_installed_tools_with_active_mark() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("config.json"),
        r#"{"installed":{"java":["21"],"node":["22.11.0"]},"active":{"java":"21"}}"#,
    )
    .unwrap();
    Command::cargo_bin("cli")
        .unwrap()
        .env("DEVKIT_ROOT", dir.path())
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("java: 21 [当前激活]"))
        .stdout(predicate::str::contains("node: 22.11.0"));
}

#[test]
fn list_empty_state_prints_install_hint() {
    let dir = tempdir().unwrap();
    Command::cargo_bin("cli")
        .unwrap()
        .env("DEVKIT_ROOT", dir.path())
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("尚未安装任何工具"));
}
