use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};
use ube_compiler::parser::ast::Item;
use ube_compiler::{DiagnosticEmitter, IrBuilder, Lexer, Parser, TextualLlvmEmitter, TypeChecker};

fn discover(path: &Path, explicit: bool, files: &mut Vec<PathBuf>) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if metadata.file_type().is_symlink() {
        return Ok(());
    }
    if metadata.is_dir() {
        for entry in fs::read_dir(path).map_err(|e| e.to_string())? {
            discover(&entry.map_err(|e| e.to_string())?.path(), false, files)?;
        }
    } else if metadata.is_file() {
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        if (explicit && name.ends_with(".ube"))
            || name.ends_with("Tests.ube")
            || name.ends_with("Test.ube")
            || name.ends_with("_test.ube")
            || name.ends_with(".test.ube")
        {
            files.push(path.to_owned());
        }
    }
    Ok(())
}

pub fn run(args: &[String]) -> i32 {
    let mut path = None;
    let mut timeout = 10_000u64;
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--timeout" {
            i += 1;
            match args
                .get(i)
                .and_then(|s| s.parse::<u64>().ok())
                .filter(|n| *n > 0)
            {
                Some(value) => timeout = value,
                None => {
                    eprintln!("--timeout requires a positive number of milliseconds");
                    return 1;
                }
            }
        } else if !args[i].starts_with('-') && path.is_none() {
            path = Some(PathBuf::from(&args[i]));
        } else {
            eprintln!("Unknown test argument: {}", args[i]);
            return 1;
        }
        i += 1;
    }
    let mut files = Vec::new();
    if let Err(error) = discover(&path.unwrap_or_else(|| "tests".into()), true, &mut files) {
        eprintln!("Test discovery failed: {error}");
        return 1;
    }
    files.sort();
    let mut passed = 0;
    let mut failed = 0;
    for file in files {
        let source = match fs::read_to_string(&file) {
            Ok(source) => source,
            Err(error) => {
                eprintln!("{}: {error}", file.display());
                failed += 1;
                continue;
            }
        };
        let loaded = match ube_compiler::source::load_program(&file) {
            Ok(program) => program,
            Err(error) => {
                eprintln!("{error}");
                failed += 1;
                continue;
            }
        };
        let program = loaded.program;
        // A generated entry point owns test execution; never silently execute or replace an application main.
        if program.items.iter().any(|item| matches!(item, Item::Class(c) if c.name == "UbeTestRunner" || c.methods.iter().any(|m| m.name == "main"))) {
            eprintln!("{}: test files must not define main() or UbeTestRunner", file.display()); failed += 1; continue;
        }
        // Imported classes provide dependencies, but only this file owns tests.
        let mut parser = Parser::new(Lexer::new(&source));
        let local = parser.parse_program();
        for item in &local.items {
            let Item::Class(class) = item else {
                continue;
            };
            for method in class.methods.iter().filter(|m| m.name.starts_with("test")) {
                let label = format!("{}::{}.{}", file.display(), class.name, method.name);
                let hooks: Vec<_> = class
                    .methods
                    .iter()
                    .filter(|m| m.name == "beforeEach" || m.name == "afterEach")
                    .collect();
                if !class.primary_params.is_empty()
                    || !class.cases.is_empty()
                    || !method.params.is_empty()
                    || method.return_type.is_some()
                    || hooks
                        .iter()
                        .any(|m| !m.params.is_empty() || m.return_type.is_some())
                {
                    eprintln!("FAIL {label}: test classes require no constructor parameters; tests/hooks require no parameters or return type");
                    failed += 1;
                    continue;
                }
                let call = |name: &str, asynchronous: bool| {
                    format!(
                        "{} subject.{name}()",
                        if asynchronous { "await" } else { "" }
                    )
                };
                let before = hooks
                    .iter()
                    .find(|m| m.name == "beforeEach")
                    .map(|m| call(&m.name, m.is_async))
                    .unwrap_or_default();
                let after = hooks
                    .iter()
                    .find(|m| m.name == "afterEach")
                    .map(|m| call(&m.name, m.is_async))
                    .unwrap_or_default();
                let invocation = call(&method.name, method.is_async);
                let body = if after.is_empty() {
                    format!("{before}\n{invocation}")
                } else {
                    format!("try {{ {before}\n{invocation} }} finally {{ {after} }}")
                };
                let generated = format!(
                    "{}\nclass UbeTestRunner {{ main() {{ final subject = {}()\n{body}\n}} }}",
                    loaded.source, class.name
                );
                let mut parser = Parser::new(Lexer::new(&generated));
                let program = parser.parse_program();
                let mut checker = TypeChecker::new();
                checker.check_program(&program);
                if !parser.diagnostics.is_empty() || !checker.diagnostics.is_empty() {
                    DiagnosticEmitter::emit(&parser.diagnostics, &generated);
                    DiagnosticEmitter::emit(&checker.diagnostics, &generated);
                    eprintln!("FAIL {label}: compilation failed");
                    failed += 1;
                    continue;
                }
                let mut builder = IrBuilder::new();
                let ir = builder.build_program(&program);
                if !builder.diagnostics.is_empty() {
                    DiagnosticEmitter::emit(&builder.diagnostics, &generated);
                    eprintln!("FAIL {label}: lowering failed");
                    failed += 1;
                    continue;
                }
                let llvm = TextualLlvmEmitter::new().emit_program(&ir);
                if super::run_llvm_timeout(&llvm, Some(Duration::from_millis(timeout)), Some(&file), &[])
                    == 0
                {
                    println!("PASS {label}");
                    passed += 1;
                } else {
                    eprintln!("FAIL {label}");
                    failed += 1;
                }
            }
        }
    }
    println!("Tests: {passed} passed, {failed} failed");
    if passed + failed == 0 {
        eprintln!("No tests found");
        return 1;
    }
    i32::from(failed != 0)
}
