use std::io::IsTerminal;

use assert_cmd::Command;
use predicates::prelude::PredicateBooleanExt;

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

#[test]
#[cfg(unix)]
fn install_reports_permission_hint_when_root_unwritable() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o500)).unwrap();
    Command::cargo_bin("cli")
        .unwrap()
        .env("DEVKIT_ROOT", dir.path())
        .arg("install")
        .arg("java")
        .assert()
        .failure()
        .stderr(
            predicates::prelude::predicate::str::contains("创建安装目录")
                .or(predicates::prelude::predicate::str::contains("不可写")),
        )
        .stderr(predicates::prelude::predicate::str::contains("提示"));
}
