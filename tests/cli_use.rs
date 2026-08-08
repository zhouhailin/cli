use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::tempdir;

// use 命令涉及符号链接切换，Windows 上创建链接可能需要管理员权限，跳过
#[cfg(unix)]
#[test]
fn use_switches_version_and_prints_source_hint() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("java/21/bin")).unwrap();
    fs::write(
        dir.path().join("config.json"),
        r#"{"installed":{"java":["21"]},"active":{"java":"21"}}"#,
    )
    .unwrap();
    Command::cargo_bin("cli")
        .unwrap()
        .env("DEVKIT_ROOT", dir.path())
        .env("SHELL", "/bin/zsh")
        .args(["use", "java", "21"])
        .assert()
        .success()
        .stdout(predicate::str::contains("已切换到 java 21"))
        .stdout(predicate::str::contains("source /"));
}

#[test]
fn use_without_tool_prompts_in_non_tty() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("config.json"),
        r#"{"installed":{},"active":{}}"#,
    )
    .unwrap();
    Command::cargo_bin("cli")
        .unwrap()
        .env("DEVKIT_ROOT", dir.path())
        .arg("use")
        .assert()
        .failure()
        .stderr(predicate::str::contains("请指定工具名"));
}
