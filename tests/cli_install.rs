use std::io::IsTerminal;

use assert_cmd::Command;

#[test]
fn install_without_tool_reports_hint_when_non_tty() {
    // 测试进程 stdin 非 TTY（CI/管道环境），若本地为 TTY 则跳过
    if std::io::stdin().is_terminal() && std::io::stderr().is_terminal() {
        return;
    }
    Command::cargo_bin("cli")
        .unwrap()
        .arg("install")
        .assert()
        .failure()
        .stderr(predicates::prelude::predicate::str::contains(
            "请指定工具名",
        ));
}
