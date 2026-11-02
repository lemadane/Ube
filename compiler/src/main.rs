use std::env;
use std::fs;
use std::path::Path;
use std::process::{self, Command};
use ube_compiler::{DiagnosticEmitter, IrBuilder, Lexer, Parser, TextualLlvmEmitter, TypeChecker};

fn print_version() {
    println!("Ube Language Compiler & Toolchain v{}", env!("CARGO_PKG_VERSION"));
}

fn print_usage() {
    println!("Ube Language Compiler & Toolchain v{}", env!("CARGO_PKG_VERSION"));
    eprintln!("Usage: ube <command> [options]");
    eprintln!("Commands:");
    eprintln!("  new <project-name>                  Create a new Ube project folder structure");
    eprintln!("  check <file.ube>                  Check source code for syntax and semantic errors");
    eprintln!("  emit-ir <file.ube>                Emit custom Typed Ube IR representation");
    eprintln!("  emit-llvm <file.ube> [-o <out>]   Emit textual LLVM IR (.ll) file");
    eprintln!("  build <file.ube> [--release] [-o <out>] Compile source file to native executable");
    eprintln!("  run <file.ube>                    Compile and execute source file immediately");
}

fn configure_runtime_linkage(cmd: &mut Command) {
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
    if let Ok(executable) = env::current_exe() {
        if let Some(executable_dir) = executable.parent() {
            if executable_dir.join("libube_runtime.a").exists()
                || executable_dir.join("ube_runtime.lib").exists()
            {
                cmd.arg("-L").arg(executable_dir);
            }
        }
    }
    cmd.arg("-L").arg("/usr/local/lib");
    cmd.arg("-Wl,-Bstatic").arg("-lube_runtime").arg("-Wl,-Bdynamic");
    cmd.arg("-lm");
    cmd.arg("-Wno-override-module");
}

fn handle_new_project(project_name: &str) {
    let proj_dir = Path::new(project_name);
    if proj_dir.exists() {
        eprintln!("Error: Directory '{}' already exists.", project_name);
        process::exit(1);
    }

    let src_dir = proj_dir.join("src");
    if let Err(e) = fs::create_dir_all(&src_dir) {
        eprintln!("Error creating project directory: {}", e);
        process::exit(1);
    }

    // 1. Create ube.toml
    let toml_content = format!(
        r#"[package]
name = "{}"
version = "{}"
edition = "2026"

[dependencies]
"#,
        project_name,
        env!("CARGO_PKG_VERSION")
    );
    let _ = fs::write(proj_dir.join("ube.toml"), toml_content);

    // 2. Create README.md
    let readme_content = format!(
        "# {}\n\nA new project written in the Ube programming language.\n\n## Running the Project\n\n```bash\nube run src/main.ube\n```\n",
        project_name
    );
    let _ = fs::write(proj_dir.join("README.md"), readme_content);

    // 3. Create src/main.ube
    let main_ube_content = r#"class Application {
    main() {
        print(`Hello from Ube!`)
    }
}
"#;
    let _ = fs::write(src_dir.join("main.ube"), main_ube_content);

    println!("Created new Ube project '{}'!", project_name);
    println!("To get started:");
    println!("  cd {}", project_name);
    println!("  ube run src/main.ube");
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

    if command == "new" {
        if args.len() < 3 {
            eprintln!("Error: Please specify a project name.");
            eprintln!("Usage: ube new <project-name>");
            process::exit(1);
        }
        handle_new_project(&args[2]);
        return;
    }

    if args.len() < 3 {
        print_usage();
        process::exit(1);
    }

    let mut file_path = String::new();
    let mut out_binary = String::new();
    let mut is_release = false;

    let mut i = 2;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--release" {
            is_release = true;
        } else if arg == "-o" && i + 1 < args.len() {
            i += 1;
            out_binary = args[i].clone();
        } else if !arg.starts_with('-') && file_path.is_empty() {
            file_path = arg.clone();
        }
        i += 1;
    }

    if file_path.is_empty() {
        eprintln!("Error: No source file specified.");
        print_usage();
        process::exit(1);
    }

    if out_binary.is_empty() {
        let base = file_path.trim_end_matches(".ube").trim_end_matches(".tl");
        out_binary = base.to_string();
    }

    let source_code = match fs::read_to_string(&file_path) {
        Ok(code) => code,
        Err(err) => {
            eprintln!("Error reading source file '{}': {}", file_path, err);
            process::exit(1);
        }
    };

    let lexer = Lexer::new(&source_code);
    let mut parser = Parser::new(lexer);
    let program = parser.parse_program();

    if !parser.diagnostics.is_empty() {
        DiagnosticEmitter::emit(&parser.diagnostics, &source_code);
        process::exit(1);
    }

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

            if !out_binary.is_empty() && out_binary != file_path.trim_end_matches(".ube").trim_end_matches(".tl") {
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
            let ll_path = format!("{}.ll", base_name);

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

            let tmp_dir = env::temp_dir();
            let ll_path = tmp_dir.join("ube_temp_app.ll");
            let bin_path = tmp_dir.join("ube_temp_app");

            if let Err(e) = fs::write(&ll_path, &llvm_ir) {
                eprintln!("Error writing temporary IR: {}", e);
                process::exit(1);
            }

            let mut clang_cmd = Command::new("clang");
            clang_cmd.arg(&ll_path).arg("-o").arg(&bin_path);
            configure_runtime_linkage(&mut clang_cmd);

            let compile_status = clang_cmd.status();

            match compile_status {
                Ok(s) if s.success() => {
                    let run_status = Command::new(&bin_path).status();
                    let _ = fs::remove_file(&ll_path);
                    let _ = fs::remove_file(&bin_path);
                    match run_status {
                        Ok(exit_status) => {
                            if let Some(code) = exit_status.code() {
                                process::exit(code);
                            } else {
                                eprintln!("Process terminated abnormally by signal.");
                                process::exit(1);
                            }
                        }
                        Err(e) => {
                            eprintln!("Error executing temporary binary: {}", e);
                            process::exit(1);
                        }
                    }
                }
                _ => {
                    eprintln!("Failed to compile temporary executable with clang.");
                    let _ = fs::remove_file(&ll_path);
                    process::exit(1);
                }
            }
        }
        other => {
            eprintln!("Unknown command '{}'", other);
            print_usage();
            process::exit(1);
        }
    }
}
