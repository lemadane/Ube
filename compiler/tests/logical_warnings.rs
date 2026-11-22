use std::{fs, process::Command};

#[test]
fn logical_warnings_do_not_fail_cli_commands() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(directory.path().join(".gitignore"), ".build/\n").unwrap();
    let entry = directory.path().join("App.ube");
    let body = "if true and true and true and true { print('ok') }";
    fs::write(&entry, format!("class App {{ start() {{ {body} }} }}")).unwrap();
    for command in ["check", "build", "run", "test"] {
        if command == "test" {
            fs::write(&entry, format!("class App {{ testLogic() {{ {body} }} }}")).unwrap();
        }
        let mut process = Command::new(env!("CARGO_BIN_EXE_ube-compiler"));
        process.current_dir(directory.path()).arg(command).arg(&entry);
        if command == "build" {
            process.arg("-o").arg(directory.path().join("app"));
        }
        let output = process.output().unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(output.status.success(), "{command}: {stderr}\n{stdout}");
        assert_eq!(stderr.matches("extract this logic into a method").count(), 1, "{command}: {stderr}");
        if command == "run" { assert_eq!(stdout.trim(), "ok"); }
    }
}
