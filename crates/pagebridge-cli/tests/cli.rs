//! Smoke tests for the pagebridge CLI.

use assert_cmd::Command;

#[test]
fn cli_prints_version() {
    let mut cmd = Command::cargo_bin("pagebridge").unwrap();
    cmd.arg("--version").assert().success();
}

#[test]
fn cli_help_lists_subcommands() {
    let mut cmd = Command::cargo_bin("pagebridge").unwrap();
    let out = cmd.arg("--help").assert().success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    for sub in ["init", "ingest", "ask", "list", "stats", "health"] {
        assert!(stdout.contains(sub), "help missing {sub}");
    }
}
