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
