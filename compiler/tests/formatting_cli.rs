use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};

fn cli() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ube-compiler"));
    command.arg("format");
    command
}

#[test]
fn formats_files_outside_the_checkout_and_preserves_exit_codes() {
    let directory = tempfile::tempdir().unwrap();
    let source = "Console.printLine('hello')";
    let file = directory.path().join("App.ube");
    fs::write(&file, source).unwrap();
    let check = cli().current_dir(directory.path()).args(["--check", "."]).output().unwrap();
    assert_eq!(check.status.code(), Some(1), "{check:?}");
    assert_eq!(String::from_utf8_lossy(&check.stdout), "App.ube\n");
    assert_eq!(fs::read_to_string(&file).unwrap(), source);
    let write = cli().current_dir(directory.path()).args(["-w", "."]).output().unwrap();
    assert!(write.status.success(), "{write:?}");
    assert_eq!(fs::read_to_string(&file).unwrap(), format!("{source}\n"));
    assert!(cli().arg("--check").arg(&file).status().unwrap().success());
    assert_eq!(cli().arg("--invalid").output().unwrap().status.code(), Some(2));
}

#[test]
fn inherits_stdin_and_prints_subcommand_help() {
    let mut child = cli().stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(b"Console.printLine('hello')").unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"Console.printLine('hello')\n");
    let help = cli().arg("--help").output().unwrap();
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("Usage: ube format"));
}

#[test]
fn missing_node_has_an_actionable_error() {
    let directory = tempfile::tempdir().unwrap();
    let output = cli().arg("--help").env("PATH", directory.path()).output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("Node.js (version 18+ required)"));
}
