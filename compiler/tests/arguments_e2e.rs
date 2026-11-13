//! Native argv transport and `ube run --` forwarding, preserving string boundaries.
mod support;
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
};
use ube_compiler::{Lexer, Parser, TypeChecker};
const SOURCE: &str = r#"class App {
    start() {
        final args = Console.args()
        Console.printLine(args.len())
        each (arg in args) { Console.print('<') Console.print(arg) Console.printLine('>') }
        if (args.len() > 0) {
            Console.printLine(args[0])
            Console.printLine(args.get(0))
            Console.printLine(Console.args()[0])
        }
        try { final bad = args[-1] } catch (Error err) { Console.printLine('negative rejected') }
        try { final bad = args.get(args.len()) } catch (Error err) { Console.printLine('past end rejected') }
    }
}"#;
struct Program {
    temp: tempfile::TempDir,
    entry: PathBuf,
    executable: PathBuf,
}
impl Program {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let entry = support::write_source(&temp.path().join("App.ube"), SOURCE);
        fs::write(temp.path().join(".gitignore"), ".build/\n").unwrap();
        let executable = temp.path().join("program");
        let output = Command::new(env!("CARGO_BIN_EXE_ube-compiler"))
            .arg("build")
            .arg(&entry)
            .arg("-o")
            .arg(&executable)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        Self {
            temp,
            entry,
            executable,
        }
    }
    fn run(&self, args: &[&str], cli: bool) -> Output {
        let mut command = if cli {
            let mut command = Command::new(env!("CARGO_BIN_EXE_ube-compiler"));
            command.arg("run").arg(&self.entry).arg("--");
            command
        } else {
            Command::new(&self.executable)
        };
        command
            .current_dir(self.temp.path())
            .args(args)
            .output()
            .unwrap()
    }
}
fn expected(args: &[&str]) -> String {
    let mut output = format!("{}\n", args.len());
    for arg in args {
        output.push_str(&format!("<{arg}>\n"));
    }
    if let Some(first) = args.first() {
        for _ in 0..3 {
            output.push_str(first);
            output.push('\n');
        }
    }
    output.push_str("negative rejected\npast end rejected\n");
    output
}
fn verify(output: Output, args: &[&str]) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected(args));
}
#[test]
fn native_and_cli_arguments_preserve_values_and_exclude_executable() {
    let program = Program::new();
    let cases: &[&[&str]] = &[
        &[],
        &["hello", "world"],
        &[
            "",
            "two words",
            "こんにちは",
            "--flag",
            "-o",
            "--release",
            "--",
            "'quoted'",
            "line\nbreak",
        ],
    ];
    for args in cases {
        verify(program.run(args, false), args);
        verify(program.run(args, true), args);
    }
}
#[test]
fn project_run_forwards_arguments_without_source_path() {
    let temp = tempfile::tempdir().unwrap();
    ube_compiler::project::init(temp.path(), Some("args-app")).unwrap();
    fs::write(temp.path().join("src/App.ube"), SOURCE).unwrap();
    let args = ["project", "--release", "two words"];
    let output = Command::new(env!("CARGO_BIN_EXE_ube-compiler"))
        .args(["run", "--"])
        .args(args)
        .current_dir(temp.path())
        .output()
        .unwrap();
    verify(output, &args);
}
#[cfg(unix)]
#[test]
fn native_non_utf8_arguments_use_replacement_characters() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};
    let program = Program::new();
    let output = Command::new(&program.executable)
        .arg(OsString::from_vec(vec![b'a', 0xff, b'b']))
        .output()
        .unwrap();
    verify(output, &["a�b"]);
}
#[test]
fn argument_api_is_read_only_and_typed() {
    for statement in [
        "Console.args(1)",
        "ConsoleArgs()",
        "ConsoleArgs.len()",
        "Console.args().get('bad')",
        "Console.args()[true]",
        "Console.args().add('bad')",
    ] {
        let source = format!("class App {{ start() {{ {statement} }} }}");
        let mut parser = Parser::new(Lexer::new(&source));
        let program = parser.parse_program();
        let mut checker = TypeChecker::new();
        assert!(
            !parser.diagnostics.is_empty() || !checker.check_program(&program),
            "accepted {statement}"
        );
    }
}
