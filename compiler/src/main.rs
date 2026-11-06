mod testing;
use std::env;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{self, Command};
use ube_compiler::{DiagnosticEmitter, IrBuilder, TextualLlvmEmitter, TypeChecker};

fn print_version() {
    println!(
        "Ube Language Compiler & Toolchain v{}",
        env!("CARGO_PKG_VERSION")
    );
}

fn print_usage() {
    println!(
        "Ube Language Compiler & Toolchain v{}",
        env!("CARGO_PKG_VERSION")
    );
    eprintln!("Usage: ube <command> [options]");
    eprintln!("Commands:");
    eprintln!("  cache [--bind IP:port] [--memory bytes] [--disk path] [--buffered] [--metrics port] [--token-env name]  Run a shared cache");
    eprintln!("  env [--key <current-key>] --add <key>:<value>[, ...]  Save encrypted values");
    eprintln!("  env [--key <current-key>] <file.json>  Encrypt and consume a JSON file");
    eprintln!("  init [--name <name>] [--format toml|yaml|json]  Initialize the current directory");
    eprintln!("  install [--update]                Install locked local/Git dependencies");
    eprintln!("  add <alias> --path <dir>           Add and install a local dependency");
    eprintln!("  add <alias> --git <url> [--rev <revision>]  Add a Git dependency");
    eprintln!("  remove <alias>                    Remove a dependency");
    eprintln!("  clean                             Remove Ube-managed .build outputs");
    eprintln!(
        "  new <project-name> [--format toml|yaml|json]  Create a new Ube project folder structure"
    );
    eprintln!(
        "  check [file.ube]                  Check source code for syntax and semantic errors"
    );
    eprintln!("  emit-ir <file.ube>                Emit custom Typed Ube IR representation");
    eprintln!("  emit-llvm <file.ube> [-o <out>]   Emit textual LLVM IR (.ll) file");
    eprintln!("  build [file.ube] [--release] [-o <out>] Compile source file to native executable");
    eprintln!(
        "  test [path] [--timeout <ms>]       Discover and run native tests (default: tests/)"
    );
    eprintln!("  run [file.ube] [-- args...]       Compile and execute with application arguments");
}

fn configure_runtime_linkage(cmd: &mut Command) {
    if let Ok(executable) = env::current_exe() {
        if let Some(executable_dir) = executable.parent() {
            // Cargo updates dependency archives in deps even when an older
            // standalone runtime build remains beside the compiler binary.
            // Prefer the paired development archive over that stale copy.
            let dependencies = executable_dir.join("deps");
            if dependencies.join("libube_runtime.a").exists()
                || dependencies.join("ube_runtime.lib").exists()
            {
                cmd.arg("-L").arg(dependencies);
            }
            if executable_dir.join("libube_runtime.a").exists()
                || executable_dir.join("ube_runtime.lib").exists()
            {
                cmd.arg("-L").arg(executable_dir);
            }
        }
    }
    let search_paths = vec![
        "/usr/local/lib",
        "target/release",
        "target/debug",
        "../target/release",
        "../target/debug",
        "../../target/release",
        "../../target/debug",
        ".",
    ];

    for path in search_paths {
        let p = Path::new(path);
        if p.join("libube_runtime.a").exists() || p.join("ube_runtime.lib").exists() {
            cmd.arg("-L").arg(path);
        }
    }
    cmd.arg("-L").arg("/usr/local/lib");
    cmd.arg("-Wl,-Bstatic")
        .arg("-lube_runtime")
        .arg("-Wl,-Bdynamic");
    cmd.arg("-lm");
    cmd.arg("-Wno-override-module");
}

fn parse_env_entries(args: &[String]) -> Result<Vec<(String, String)>, String> {
    let invalid = || {
        "Expected comma-separated <key>:<value> entries; quote values containing spaces".to_owned()
    };
    let mut entries = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let (name, initial) = args[i].split_once(':').ok_or_else(invalid)?;
        i += 1;
        let value = if initial.is_empty() && i < args.len() && args[i] != "," {
            let value = args[i].as_str();
            i += 1;
            value
        } else {
            initial
        };
        let (value, separated) = match value.strip_suffix(',') {
            Some(value) => (value, true),
            None => (value, false),
        };
        entries.push((name.to_owned(), value.to_owned()));
        if separated {
            if i == args.len() {
                return Err(invalid());
            }
        } else if i < args.len() {
            if args[i] != "," {
                return Err(invalid());
            }
            i += 1;
            if i == args.len() {
                return Err(invalid());
            }
        }
    }
    if entries.is_empty() {
        return Err(invalid());
    }
    Ok(entries)
}

fn project_command(command: &str, args: &[String]) -> Result<(), String> {
    use ube_compiler::project;
    let cwd = env::current_dir().map_err(|e| e.to_string())?;
    if let Ok(root) = project::find_root(&cwd) {
        project::manifest_path(&root)?;
    }
    match command {
        "new" => {
            let (mut path, mut format) = (None, None);
            let mut i = 0;
            while i < args.len() {
                match args[i].as_str() {
                    "--format" if i + 1 < args.len() && format.is_none() => {
                        i += 1;
                        format = Some(args[i].as_str());
                    }
                    arg if !arg.starts_with('-') && path.is_none() => path = Some(arg),
                    _ => {
                        return Err("Usage: ube new <project-name> [--format toml|yaml|json]".into())
                    }
                }
                i += 1;
            }
            project::new_project_with_format(
                Path::new(path.ok_or("Project name required")?),
                format.unwrap_or("toml"),
            )
        }
        "init" => {
            let (mut name, mut format) = (None, None);
            let mut i = 0;
            while i < args.len() {
                match args[i].as_str() {
                    "--name" if i + 1 < args.len() && name.is_none() => {
                        i += 1;
                        name = Some(args[i].as_str());
                    }
                    "--format" if i + 1 < args.len() && format.is_none() => {
                        i += 1;
                        format = Some(args[i].as_str());
                    }
                    _ => {
                        return Err(
                            "Usage: ube init [--name <name>] [--format toml|yaml|json]".into()
                        )
                    }
                }
                i += 1;
            }
            project::init_with_format(&cwd, name, format.unwrap_or("toml"))
        }
        "install" => {
            let update = match args {
                [] => false,
                [flag] if flag == "--update" => true,
                _ => return Err("Usage: ube install [--update]".into()),
            };
            project::install(&project::find_root(&cwd)?, update)
        }
        "env" => {
            let (current_key, entry_args, input) = match args {
                [flag, rest @ ..] if flag == "--add" => (None, rest, None),
                [flag, key, add, rest @ ..] if flag == "--key" && add == "--add" => {
                    (Some(key.as_str()), rest, None)
                }
                [file] if !file.starts_with('-') => (None, &[][..], Some(Path::new(file))),
                [flag, key, file] if flag == "--key" && !file.starts_with('-') => {
                    (Some(key.as_str()), &[][..], Some(Path::new(file)))
                }
                _ => return Err(
                    "Usage: ube env [--key <current-key>] <file.json> | --add <key>:<value>[, ...]"
                        .into(),
                ),
            };
            let root = project::find_root(&cwd)?;
            project::verify_environment_ignored(&root)?;
            eprintln!("WARNING: Encryption keys and values supplied on the command line may appear in shell history and process listings.");
            let (key, imported) = match input {
                Some(path) => {
                    let mut result = ube_runtime::secrets::import_file(&root, path, current_key)?;
                    (std::mem::take(&mut result.key), Some(result))
                }
                None => {
                    let entries = parse_env_entries(entry_args)?;
                    (
                        ube_runtime::secrets::add(&root, &entries, current_key)?,
                        None,
                    )
                }
            };

            eprintln!("WARNING: Keep the encryption key private. Never commit it or expose it in chats, screenshots, terminal recordings, or logs. Keep a secure backup in a password manager or secret manager.");
            {
                let mut output = std::io::stdout().lock();
                writeln!(output, "Encryption key: {}", key.as_str())
                    .and_then(|_| output.flush())
                    .map_err(|_| "Could not print the encryption key. Any input JSON file was preserved; retry the import to generate a new key.")?;
            }
            println!("Encrypted environment values saved");
            eprintln!("Save this new key now. It replaces the previous key for the current configuration. Ube has not saved it.");
            match imported.as_ref().map(|result| result.consume()) {
                Some(true) => println!("Plaintext JSON file consumed"),
                Some(false) => return Err("Encrypted values were saved and the new key was printed, but the plaintext JSON changed or could not be removed. Remove it manually after checking its contents.".into()),
                None => (),
            }
            Ok(())
        }
        "add" => project::add(&project::find_root(&cwd)?, args),
        "remove" if args.len() == 1 => project::remove(&project::find_root(&cwd)?, &args[0]),
        "clean" if args.is_empty() => project::clean(&project::find_root(&cwd)?),
        _ => Err(format!(
            "Invalid arguments for '{command}'; see 'ube --help'"
        )),
    }
}

// Return before main calls process::exit so TempDir's destructor runs on every
// normal success/error path, including linking and execution failures.
fn run_llvm(llvm_ir: &str, source: &Path, arguments: &[String]) -> i32 {
    run_llvm_timeout(llvm_ir, None, Some(source), arguments)
}

fn run_llvm_timeout(
    llvm_ir: &str,
    timeout: Option<std::time::Duration>,
    source: Option<&Path>,
    arguments: &[String],
) -> i32 {
    let directory = match tempfile::Builder::new().prefix("ube-run-").tempdir() {
        Ok(directory) => directory,
        Err(error) => {
            eprintln!("Error creating temporary run directory: {}", error);
            return 1;
        }
    };
    let ll_path = directory.path().join("app.ll");
    let bin_path = directory
        .path()
        .join(format!("app{}", env::consts::EXE_SUFFIX));
    if let Err(error) = fs::write(&ll_path, llvm_ir) {
        eprintln!("Error writing temporary IR: {}", error);
        return 1;
    }
    let mut clang_cmd = Command::new("clang");
    clang_cmd.arg(&ll_path).arg("-o").arg(&bin_path);
    configure_runtime_linkage(&mut clang_cmd);
    match clang_cmd.status() {
        Ok(status) if status.success() => {}
        _ => {
            eprintln!("Failed to compile temporary executable with clang.");
            return 1;
        }
    }
    let mut program = Command::new(&bin_path);
    program.args(arguments);
    if env::var_os("UBE_ENV_FILE").is_none() {
        if let Some(source) = source {
            let root = ube_compiler::project::find_root(source)
                .unwrap_or_else(|_| source.parent().unwrap_or_else(|| Path::new(".")).to_owned());
            program.env("UBE_ENV_PROJECT_ROOT", root);
        }
    }
    let status = if let Some(limit) = timeout {
        match program.spawn() {
            Ok(mut child) => {
                let started = std::time::Instant::now();
                loop {
                    match child.try_wait() {
                        Ok(Some(status)) => break Ok(status),
                        Ok(None) if started.elapsed() < limit => {
                            std::thread::sleep(std::time::Duration::from_millis(5))
                        }
                        Ok(None) => {
                            let _ = child.kill();
                            let _ = child.wait();
                            eprintln!("Test timed out after {} ms", limit.as_millis());
                            return 1;
                        }
                        Err(error) => {
                            let _ = child.kill();
                            let _ = child.wait();
                            break Err(error);
                        }
                    }
                }
            }
            Err(error) => Err(error),
        }
    } else {
        program.status()
    };
    match status {
        Ok(status) => match status.code() {
            Some(code) => code,
            None => {
                eprintln!("Process terminated abnormally by signal.");
                1
            }
        },
        Err(error) => {
            eprintln!("Error executing temporary binary: {}", error);
            1
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        print_usage();
        process::exit(1);
    }

    let command = &args[1];

    if command == "--version" || command == "-V" || command == "-v" || command == "version" {
        print_version();
        return;
    }

    if command == "--help" || command == "-h" || command == "help" {
        print_usage();
        return;
    }
    if command == "cache" {
        if let Err(error) = ube_runtime::cache::command(&args[2..]) {
            eprintln!("Error: {error}"); process::exit(1);
        }
        return;
    }
    if ["new", "init", "install", "add", "remove", "clean", "env"].contains(&command.as_str()) {
        if let Err(error) = project_command(command, &args[2..]) {
            eprintln!("Error: {error}");
            process::exit(1);
        }
        return;
    }
    if command == "test" {
        let mut test_args = args[2..].to_vec();
        let cwd = env::current_dir().unwrap_or_else(|_| ".".into());
        if let Ok(root) = ube_compiler::project::find_root(&cwd) {
            if let Err(error) = ube_compiler::project::verify_installation(&root) {
                eprintln!("Error: {error}");
                process::exit(1);
            }
            // Explicit paths remain relative to the calling directory.
            if !test_args.iter().enumerate().any(|(i, arg)| {
                !arg.starts_with('-') && (i == 0 || test_args[i - 1] != "--timeout")
            }) {
                test_args.insert(0, root.join("tests").to_string_lossy().into_owned());
            }
        }
        process::exit(testing::run(&test_args));
    }
    if !["check", "emit-ir", "emit-llvm", "build", "run"].contains(&command.as_str()) {
        eprintln!("Unknown command '{command}'");
        print_usage();
        process::exit(1);
    }

    let mut file_path = String::new();
    let mut out_binary = String::new();
    let mut is_release = false;
    let mut program_args = Vec::new();

    let mut i = 2;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--" && command == "run" {
            program_args.extend_from_slice(&args[i + 1..]);
            break;
        } else if arg == "--release" {
            is_release = true;
        } else if arg == "-o" && i + 1 < args.len() {
            i += 1;
            out_binary = args[i].clone();
        } else if !arg.starts_with('-') && file_path.is_empty() {
            file_path = arg.clone();
        } else {
            eprintln!("Unexpected argument '{arg}'");
            process::exit(1);
        }
        i += 1;
    }

    let mut project_output = None;
    if file_path.is_empty() {
        let project = (|| -> Result<_, String> {
            let root =
                ube_compiler::project::find_root(&env::current_dir().map_err(|e| e.to_string())?)?;
            let manifest = ube_compiler::project::manifest(&root)?;
            ube_compiler::project::verify_installation(&root)?;
            file_path = root.join(&manifest.entry).to_string_lossy().into_owned();
            if command == "build" {
                let output = ube_compiler::project::build_directory(&root)?;
                project_output = Some(output.join(&manifest.name));
            }
            Ok(())
        })();
        if let Err(error) = project {
            eprintln!("Error: {error}");
            process::exit(1);
        }
    }
    if command == "build" {
        let source = Path::new(&file_path);
        let root = ube_compiler::project::find_root(source).unwrap_or_else(|_| {
            source
                .parent()
                .filter(|path| !path.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new("."))
                .to_owned()
        });
        if let Err(error) = ube_compiler::project::verify_build_gitignore(&root) {
            eprintln!("Error: {error}");
            process::exit(1);
        }
    }
    if out_binary.is_empty() {
        out_binary = project_output
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| {
                file_path
                    .trim_end_matches(".ube")
                    .trim_end_matches(".tl")
                    .to_string()
            });
    }

    let loaded = match ube_compiler::source::load_program(Path::new(&file_path)) {
        Ok(program) => program,
        Err(error) => {
            eprintln!("{error}");
            process::exit(1);
        }
    };

    let program = loaded.program;
    let source_code = loaded.source;
    let mut checker = TypeChecker::new();
    checker.check_program(&program);

    if !checker.diagnostics.is_empty() {
        DiagnosticEmitter::emit(&checker.diagnostics, &source_code);
        process::exit(1);
    }

    match command.as_str() {
        "check" => {
            println!("Check successful! 0 errors.");
        }
        "emit-ir" => {
            let mut builder = IrBuilder::new();
            let ir_program = builder.build_program(&program);

            if !builder.diagnostics.is_empty() {
                DiagnosticEmitter::emit(&builder.diagnostics, &source_code);
                process::exit(1);
            }
            println!("{:#?}", ir_program);
        }
        "emit-llvm" => {
            let mut builder = IrBuilder::new();
            let ir_program = builder.build_program(&program);

            if !builder.diagnostics.is_empty() {
                DiagnosticEmitter::emit(&builder.diagnostics, &source_code);
                process::exit(1);
            }

            let mut emitter = TextualLlvmEmitter::new();
            let llvm_ir = emitter.emit_program(&ir_program);

            if !out_binary.is_empty()
                && out_binary != file_path.trim_end_matches(".ube").trim_end_matches(".tl")
            {
                if let Err(e) = fs::write(&out_binary, &llvm_ir) {
                    eprintln!("Error writing LLVM IR file '{}': {}", out_binary, e);
                    process::exit(1);
                }
                println!("LLVM IR successfully written to '{}'", out_binary);
            } else {
                println!("{}", llvm_ir);
            }
        }
        "build" => {
            let mut builder = IrBuilder::new();
            let ir_program = builder.build_program(&program);

            if !builder.diagnostics.is_empty() {
                DiagnosticEmitter::emit(&builder.diagnostics, &source_code);
                process::exit(1);
            }

            let mut emitter = TextualLlvmEmitter::new();
            let llvm_ir = emitter.emit_program(&ir_program);

            let base_name = file_path.trim_end_matches(".ube").trim_end_matches(".tl");
            let ll_path = project_output
                .as_ref()
                .map(|p| p.with_extension("ll").to_string_lossy().into_owned())
                .unwrap_or_else(|| format!("{}.ll", base_name));

            if let Err(e) = fs::write(&ll_path, &llvm_ir) {
                eprintln!("Error writing temporary LLVM IR file '{}': {}", ll_path, e);
                process::exit(1);
            }

            println!("LLVM IR generated at '{}'", ll_path);

            let mut clang_cmd = Command::new("clang");
            clang_cmd.arg(&ll_path).arg("-o").arg(&out_binary);
            configure_runtime_linkage(&mut clang_cmd);

            if is_release {
                clang_cmd.arg("-O3").arg("-flto");
                println!("Building in release mode (-O3, -flto)...");
            }

            let status = clang_cmd.status();

            match status {
                Ok(s) if s.success() => {
                    println!("Successfully built native binary: './{}'", out_binary);
                }
                _ => {
                    eprintln!("Error: Linking with clang failed.");
                    process::exit(1);
                }
            }
        }
        "run" => {
            let mut builder = IrBuilder::new();
            let ir_program = builder.build_program(&program);

            if !builder.diagnostics.is_empty() {
                DiagnosticEmitter::emit(&builder.diagnostics, &source_code);
                process::exit(1);
            }

            let mut emitter = TextualLlvmEmitter::new();
            let llvm_ir = emitter.emit_program(&ir_program);

            let code = run_llvm(&llvm_ir, Path::new(&file_path), &program_args);
            process::exit(code);
        }
        other => {
            eprintln!("Unknown command '{}'", other);
            print_usage();
            process::exit(1);
        }
    }
}
