use std::io::IsTerminal;

use assert_cmd::Command;

#[test]
fn uninstall_without_tool_reports_hint_when_non_tty() {
    // 测试进程 stdin 非 TTY（CI/管道环境），若本地为 TTY 则跳过
    if std::io::stdin().is_terminal() && std::io::stderr().is_terminal() {
        return;
    }
    Command::cargo_bin("cli")
        .unwrap()
        .arg("uninstall")
        .assert()
        .failure()
        .stderr(predicates::prelude::predicate::str::contains(
            "请指定工具名",
        ));
}

#[test]
fn uninstall_unknown_tool_reports_not_installed() {
    // DEVKIT_ROOT 指向空目录，避免读真实 ~/.devkit 配置
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_str().unwrap().to_string();
    Command::cargo_bin("cli")
        .unwrap()
        .env("DEVKIT_ROOT", root)
        .args(["uninstall", "java", "99"])
        .assert()
        .failure()
        .stderr(predicates::prelude::predicate::str::contains("尚未安装"));
}
