use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn version_subcommand_prints_version() {
    Command::cargo_bin("cli")
        .unwrap()
        .arg("version")
        .assert()
        .success()
        .stdout(predicate::str::contains("cli 0.1.0"));
}

#[test]
fn version_flag_prints_version() {
    Command::cargo_bin("cli")
        .unwrap()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("0.1.0"));
}

#[test]
fn no_args_prints_version_in_help() {
    Command::cargo_bin("cli")
        .unwrap()
        .assert()
        .failure()
        .stderr(predicate::str::contains("版本: 0.1.0"));
}

#[test]
fn help_flag_shows_system_line() {
    Command::cargo_bin("cli")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("系统: "))
        .stdout(predicate::str::contains("x86_64").or(predicate::str::contains("aarch64")));
}

#[test]
fn help_subcommand_shows_system_line() {
    Command::cargo_bin("cli")
        .unwrap()
        .arg("help")
        .assert()
        .success()
        .stdout(predicate::str::contains("系统: "));
}

#[test]
fn no_args_shows_system_line() {
    Command::cargo_bin("cli")
        .unwrap()
        .assert()
        .failure()
        .stderr(predicate::str::contains("系统: "));
}

#[test]
fn version_shows_platform_and_root() {
    let dir = tempfile::tempdir().unwrap();
    // 平台字符串与 Platform::detect 一致，按编译目标动态构造，避免绑定具体平台
    let os = if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "windows"
    };
    let arch = if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else {
        "aarch64"
    };
    Command::cargo_bin("cli")
        .unwrap()
        .env("DEVKIT_ROOT", dir.path())
        .arg("version")
        .assert()
        .success()
        .stdout(predicate::str::contains(format!("平台: {os} ({arch})")))
        .stdout(predicate::str::contains(dir.path().to_str().unwrap()));
}
