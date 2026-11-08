//! Exercise real CLI processes with a private temp root and a deterministic clang barrier.
#![cfg(unix)]
use std::{
    env, fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

struct Fixture {
    directory: tempfile::TempDir,
    original_path: std::ffi::OsString,
    runtime: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        for name in ["bin", "work", "receipts"] {
            fs::create_dir(directory.path().join(name)).unwrap();
        }
        let wrapper = directory.path().join("bin/clang");
        fs::write(
            &wrapper,
            r#"#!/bin/sh
set -eu
printf '%s\n%s\n' "$1" "$3" > "$UBE_TEST_ROOT/receipts/$UBE_TEST_RUN_ID"
case "$UBE_TEST_MODE" in
    concurrent)
        attempts=0
        while [ ! -f "$UBE_TEST_ROOT/receipts/one" ] || [ ! -f "$UBE_TEST_ROOT/receipts/two" ]; do
            attempts=$((attempts + 1))
            if [ "$attempts" -gt 1000 ]; then exit 98; fi
            sleep 0.01
        done
        export PATH="$UBE_TEST_ORIGINAL_PATH"
        exec clang "$@"
        ;;
    compile-failure)
        printf 'partial output' > "$3"
        exit 42
        ;;
    execution-failure)
        printf '#!/ube-nonexistent-interpreter\n' > "$3"
        ;;
    nonzero)
        printf '#!/bin/sh\nexit 23\n' > "$3"
        ;;
    signal)
        printf '#!/bin/sh\nkill -TERM $$\n' > "$3"
        ;;
esac
chmod 755 "$3"
"#,
        )
        .unwrap();
        fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755)).unwrap();
        let executable = env::current_exe().unwrap();
        let runtime = executable.parent().unwrap().to_path_buf();
        assert!(runtime.join("libube_runtime.a").exists());
        Self {
            directory,
            original_path: env::var_os("PATH").unwrap(),
            runtime,
        }
    }
    fn root(&self) -> &Path {
        self.directory.path()
    }
    fn command(&self, id: &str, mode: &str) -> Command {
        let source_directory = self.root().join(id);
        fs::create_dir_all(&source_directory).unwrap();
        let source = source_directory.join("App.ube");
        fs::write(
            &source,
            format!("class App {{ start() {{ Console.printLine('run-{id}') }} }}"),
        )
        .unwrap();
        let mut paths = vec![self.root().join("bin")];
        paths.extend(env::split_paths(&self.original_path));
        let mut command = Command::new("timeout");
        command
            .arg("30s")
            .arg(env!("CARGO_BIN_EXE_ube-compiler"))
            .arg("run")
            .arg(source)
            .current_dir(&self.runtime)
            .env("PATH", env::join_paths(paths).unwrap())
            .env("TMPDIR", self.root().join("work"))
            .env("UBE_TEST_ROOT", self.root())
            .env("UBE_TEST_RUN_ID", id)
            .env("UBE_TEST_MODE", mode)
            .env("UBE_TEST_ORIGINAL_PATH", &self.original_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }
    fn paths(&self, id: &str) -> Vec<PathBuf> {
        fs::read_to_string(self.root().join("receipts").join(id))
            .unwrap()
            .lines()
            .map(PathBuf::from)
            .collect()
    }
    fn assert_clean(&self) {
        assert_eq!(
            fs::read_dir(self.root().join("work")).unwrap().count(),
            0,
            "temporary run files were left behind"
        );
    }
}

#[test]
fn concurrent_runs_use_distinct_scoped_directories_and_execute_their_own_programs() {
    let fixture = Fixture::new();
    let one = fixture.command("one", "concurrent").spawn().unwrap();
    let two = fixture.command("two", "concurrent").spawn().unwrap();
    let one = one.wait_with_output().unwrap();
    let two = two.wait_with_output().unwrap();
    for (id, output) in [("one", one), ("two", two)] {
        assert!(
            output.status.success(),
            "{id}: {:?}: {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            format!("run-{id}\n")
        );
    }
    let first = fixture.paths("one");
    let second = fixture.paths("two");
    assert_eq!(first.len(), 2);
    assert_eq!(second.len(), 2);
    assert_ne!(first[0].parent(), second[0].parent());
    for paths in [&first, &second] {
        assert_eq!(paths[0].parent(), paths[1].parent());
        let directory = paths[0].parent().unwrap();
        assert!(directory.starts_with(fixture.root().join("work")));
        assert!(
            !directory.exists(),
            "run directory survived process completion"
        );
    }
    fixture.assert_clean();
}

#[test]
fn scoped_run_files_are_removed_on_compile_execution_and_child_failures() {
    let fixture = Fixture::new();
    for (mode, code, message) in [
        (
            "compile-failure",
            1,
            "Failed to compile temporary executable",
        ),
        ("execution-failure", 1, "Error executing temporary binary"),
        ("nonzero", 23, ""),
        ("signal", 1, "Process terminated abnormally by signal"),
    ] {
        let output = fixture.command(mode, mode).output().unwrap();
        assert_eq!(
            output.status.code(),
            Some(code),
            "{mode}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stderr).contains(message));
        for path in fixture.paths(mode) {
            assert!(
                !path.exists(),
                "{mode}: artifact survived: {}",
                path.display()
            );
        }
        fixture.assert_clean();
    }
}
