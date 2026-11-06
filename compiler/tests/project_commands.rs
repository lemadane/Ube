use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

fn cli(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ube-compiler"))
        .args(args)
        .current_dir(root)
        .output()
        .unwrap()
}
fn success(output: Output) -> String {
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}
fn failure(output: Output, expected: &str) {
    assert!(!output.status.success(), "command unexpectedly succeeded");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(expected), "expected {expected:?}: {stderr}");
}
fn package(root: &Path, name: &str, class: &str, source: &str) {
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("ube.toml"),
        format!("[package]\nname = '{name}'\nversion = '0.1.0'\nedition = '2026'\n"),
    )
    .unwrap();
    fs::write(root.join("src").join(format!("{class}.ube")), source).unwrap();
}
fn git(root: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}
fn commit(root: &Path) -> String {
    git(root, &["add", "."]);
    git(
        root,
        &[
            "-c",
            "user.name=Ube Tests",
            "-c",
            "user.email=tests@example.invalid",
            "commit",
            "-qm",
            "fixture",
        ],
    );
    git(root, &["rev-parse", "HEAD"])
}

#[test]
fn new_init_manifest_build_run_and_clean_work_without_explicit_entry() {
    let dir = tempfile::tempdir().unwrap();
    success(cli(dir.path(), &["new", "hello-world"]));
    let root = dir.path().join("hello-world");
    assert!(root.join("src/Main.ube").is_file());
    assert_eq!(
        fs::read_to_string(root.join(".gitignore")).unwrap(),
        ".build/\n"
    );
    assert!(!root.join("src/main.ube").exists());
    success(cli(&root, &["check"]));
    success(cli(&root, &["build"]));
    assert!(root.join(".build/hello-world").is_file());
    assert!(root.join(".build/hello-world.ll").is_file());
    assert!(!root.join("src/Main.ll").exists());
    assert!(success(cli(&root, &["run"])).contains("Hello from Ube!"));
    fs::create_dir_all(root.join("src/nested")).unwrap();
    success(cli(&root.join("src/nested"), &["check"]));
    success(cli(&root, &["install"]));
    let lock = fs::read(root.join("ube.lock")).unwrap();
    success(cli(&root, &["clean"]));
    assert!(!root.join(".build/hello-world").exists());
    assert!(!root.join(".build/hello-world.ll").exists());
    assert!(root.join("src/Main.ube").is_file());
    assert_eq!(fs::read(root.join("ube.lock")).unwrap(), lock);
    assert!(root.join(".build/.deps").exists());
    success(cli(&root, &["clean"]));
    failure(cli(&root, &["init"]), "already exists");
}

#[test]
fn init_preserves_existing_source_and_checks_manifest_entry_names() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    let source = "class Custom { main() {} }";
    fs::write(dir.path().join("src/Custom.ube"), source).unwrap();
    fs::write(
        dir.path().join(".gitignore"),
        "# Local rules\n.build/\nprivate.env",
    )
    .unwrap();
    success(cli(dir.path(), &["init", "--name", "existing-app"]));
    assert_eq!(
        fs::read_to_string(dir.path().join(".gitignore")).unwrap(),
        "# Local rules\n.build/\nprivate.env"
    );
    assert!(!dir.path().join("src/Main.ube").exists());
    assert_eq!(
        fs::read_to_string(dir.path().join("src/Custom.ube")).unwrap(),
        source
    );
    let manifest = fs::read_to_string(dir.path().join("ube.toml"))
        .unwrap()
        .replace("src/Main.ube", "src/Custom.ube");
    fs::write(dir.path().join("ube.toml"), manifest).unwrap();
    success(cli(dir.path(), &["check"]));
    fs::rename(
        dir.path().join("src/Custom.ube"),
        dir.path().join("src/custom.ube"),
    )
    .unwrap();
    failure(
        cli(dir.path(), &["build", "src/custom.ube"]),
        "expected 'Custom.ube'",
    );
}

#[test]
fn local_dependencies_are_imported_locked_checked_and_removed() {
    let dir = tempfile::tempdir().unwrap();
    let lib = dir.path().join("library");
    package(
        &lib,
        "greetings",
        "Greeting",
        "class Greeting { int value() { return 42 } }",
    );
    success(cli(dir.path(), &["new", "app"]));
    let app = dir.path().join("app");
    fs::write(
        app.join("src/Main.ube"),
        "import greet.Greeting\nclass Main { main() { log(Greeting().value()) } }",
    )
    .unwrap();
    failure(cli(&app, &["add", "greet"]), "no package registry");
    success(cli(&app, &["add", "greet", "--path", "../library"]));
    let manifest = fs::read_to_string(app.join("ube.toml")).unwrap();
    assert!(manifest.contains("../library"));
    assert!(success(cli(&app, &["run"])).contains("42"));
    let lock = fs::read(app.join("ube.lock")).unwrap();
    success(cli(&app, &["install"]));
    assert_eq!(fs::read(app.join("ube.lock")).unwrap(), lock);
    // Failed installs preserve the prior snapshots and lock.
    fs::write(
        lib.join("src/Greeting.ube"),
        "class Greeting { int value() { return 43 } }",
    )
    .unwrap();
    failure(cli(&app, &["install"]), "differs from ube.lock");
    assert_eq!(fs::read(app.join("ube.lock")).unwrap(), lock);
    assert!(success(cli(&app, &["run"])).contains("42"));
    success(cli(&app, &["install", "--update"]));
    assert!(success(cli(&app, &["run"])).contains("43"));
    fs::write(
        app.join(".build/.deps/greet/src/Greeting.ube"),
        "class Greeting {}",
    )
    .unwrap();
    failure(cli(&app, &["check"]), "was modified");
    success(cli(&app, &["install"]));
    success(cli(&app, &["remove", "greet"]));
    assert!(!app.join(".build/.deps/greet").exists());
    assert!(!fs::read_to_string(app.join("ube.lock"))
        .unwrap()
        .contains("greet"));
    failure(cli(&app, &["remove", "greet"]), "not declared");
    assert!(lib.join("src/Greeting.ube").exists());
}

#[test]
fn transitive_dependencies_resolve_in_their_own_package_context() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a");
    let b = dir.path().join("b");
    package(
        &b,
        "b",
        "Number",
        "class Number { int value() { return 17 } }",
    );
    package(
        &a,
        "a",
        "Greeting",
        "import numbers.Number\nclass Greeting { int value() { return Number().value() } }",
    );
    let mut manifest = fs::read_to_string(a.join("ube.toml")).unwrap();
    manifest.push_str("\n[dependencies]\nnumbers = { path = '../b' }\n");
    fs::write(a.join("ube.toml"), manifest).unwrap();
    success(cli(dir.path(), &["new", "app"]));
    let app = dir.path().join("app");
    success(cli(&app, &["add", "greet", "--path", "../a"]));
    fs::write(
        app.join("src/Main.ube"),
        "import greet.Greeting\nclass Main { main() { log(Greeting().value()) } }",
    )
    .unwrap();
    assert!(success(cli(&app, &["run"])).contains("17"));
    let lock = fs::read_to_string(app.join("ube.lock")).unwrap();
    assert!(lock.contains("greet/numbers"));
    success(cli(&app, &["remove", "greet"]));
    assert!(!app.join(".build/.deps/greet").exists());
}

#[test]
fn git_dependencies_pin_commits_until_explicit_update() {
    let dir = tempfile::tempdir().unwrap();
    let lib = dir.path().join("git-library");
    package(
        &lib,
        "greetings",
        "Greeting",
        "class Greeting { int value() { return 21 } }",
    );
    git(&lib, &["init", "-q"]);
    let first = commit(&lib);
    success(cli(dir.path(), &["new", "app"]));
    let app = dir.path().join("app");
    success(cli(&app, &["add", "greet", "--git", lib.to_str().unwrap()]));
    fs::write(
        app.join("src/Main.ube"),
        "import greet.Greeting\nclass Main { main() { log(Greeting().value()) } }",
    )
    .unwrap();
    assert!(fs::read_to_string(app.join("ube.lock"))
        .unwrap()
        .contains(&first));
    fs::write(
        lib.join("src/Greeting.ube"),
        "class Greeting { int value() { return 22 } }",
    )
    .unwrap();
    let second = commit(&lib);
    success(cli(&app, &["install"]));
    assert!(success(cli(&app, &["run"])).contains("21"));
    assert!(fs::read_to_string(app.join("ube.lock"))
        .unwrap()
        .contains(&first));
    success(cli(&app, &["install", "--update"]));
    assert!(success(cli(&app, &["run"])).contains("22"));
    assert!(fs::read_to_string(app.join("ube.lock"))
        .unwrap()
        .contains(&second));
    success(cli(&app, &["remove", "greet"]));
    success(cli(
        &app,
        &[
            "add",
            "greet",
            "--git",
            lib.to_str().unwrap(),
            "--rev",
            &first,
        ],
    ));
    success(cli(&app, &["install", "--update"]));
    assert!(success(cli(&app, &["run"])).contains("21"));
}

#[test]
fn failed_adds_cycles_and_invalid_manifests_preserve_the_project() {
    let dir = tempfile::tempdir().unwrap();
    success(cli(dir.path(), &["new", "app"]));
    let app = dir.path().join("app");
    let before = fs::read(app.join("ube.toml")).unwrap();
    failure(
        cli(&app, &["add", "bad", "--path", "../missing"]),
        "missing",
    );
    failure(cli(&app, &["add", "self", "--path", "."]), "Cyclic package");
    assert_eq!(fs::read(app.join("ube.toml")).unwrap(), before);
    assert!(!app.join("ube.lock").exists());
    fs::write(
        app.join("ube.toml"),
        "[package]\nname = '../escape'\nversion = '1'\n",
    )
    .unwrap();
    failure(cli(&app, &["build"]), "Invalid package name");
    fs::write(
        app.join("ube.toml"),
        "[package]\nname = 'fine'\nversion = '1'\n[build]\nentry = '../Main.ube'\n",
    )
    .unwrap();
    failure(cli(&app, &["build"]), "relative .ube path");
    failure(cli(&app, &["install", "--oops"]), "Usage");
    failure(cli(&app, &["wat"]), "Unknown command");
}

#[test]
fn clean_refuses_unmanaged_or_symlinked_directories() {
    let dir = tempfile::tempdir().unwrap();
    success(cli(dir.path(), &["new", "app"]));
    let app = dir.path().join("app");
    fs::create_dir(app.join(".build")).unwrap();
    fs::write(app.join(".build/keep"), "user data").unwrap();
    failure(cli(&app, &["clean"]), "not marked");
    failure(cli(&app, &["build"]), "not managed");
    assert!(app.join(".build/keep").is_file());
    #[cfg(unix)]
    {
        fs::remove_dir_all(app.join(".build")).unwrap();
        std::os::unix::fs::symlink(dir.path(), app.join(".build")).unwrap();
        failure(cli(&app, &["clean"]), "symlink");
        failure(cli(&app, &["build"]), "symlink");
    }
}

#[test]
fn manifest_test_discovery_works_from_subdirectories_with_package_imports() {
    let dir = tempfile::tempdir().unwrap();
    package(
        &dir.path().join("library"),
        "greetings",
        "Greeting",
        "class Greeting { int value() { return 42 } }",
    );
    success(cli(dir.path(), &["new", "app"]));
    let app = dir.path().join("app");
    success(cli(&app, &["add", "greet", "--path", "../library"]));
    fs::write(app.join("tests/GreetingTests.ube"), "import greet.Greeting\nclass GreetingTests { testValue() { Testicle.equal(42, Greeting().value()) } }").unwrap();
    assert!(
        success(cli(&app.join("src"), &["test", "--timeout", "5000"])).contains("Tests: 1 passed")
    );
}

#[test]
fn install_rejects_source_symlinks_and_serializes_installations() {
    let dir = tempfile::tempdir().unwrap();
    success(cli(dir.path(), &["new", "app"]));
    let app = dir.path().join("app");
    success(cli(&app, &["install"]));
    let lock = fs::read(app.join("ube.lock")).unwrap();
    fs::write(app.join(".build/install.lock"), "another installer").unwrap();
    failure(cli(&app, &["install"]), "Cannot acquire");
    assert!(app.join(".build/install.lock").is_file());
    fs::remove_file(app.join(".build/install.lock")).unwrap();
    success(cli(&app, &["install"]));
    assert!(!app.join(".build/install.lock").exists());
    assert_eq!(fs::read(app.join("ube.lock")).unwrap(), lock);
    #[cfg(unix)]
    {
        let lib = dir.path().join("library");
        package(&lib, "greetings", "Greeting", "class Greeting {}");
        std::os::unix::fs::symlink(lib.join("src/Greeting.ube"), lib.join("src/Alias.ube"))
            .unwrap();
        let manifest = fs::read(app.join("ube.toml")).unwrap();
        failure(
            cli(&app, &["add", "greet", "--path", "../library"]),
            "symlinks",
        );
        assert_eq!(fs::read(app.join("ube.toml")).unwrap(), manifest);
        assert_eq!(fs::read(app.join("ube.lock")).unwrap(), lock);
    }
}

#[test]
fn build_requires_valid_gitignore_for_manifest_and_explicit_sources() {
    let dir = tempfile::tempdir().unwrap();
    success(cli(dir.path(), &["new", "app"]));
    let root = dir.path().join("app");
    let ignore = root.join(".gitignore");
    fs::remove_file(&ignore).unwrap();
    failure(cli(&root, &["build"]), "Build requires .gitignore");
    assert!(!ignore.exists());
    for rules in [
        "",
        "# .build/\n",
        "private.env\n",
        ".build/\n!.build/\n",
        ".build/\n! .build/\n",
        "!.build/\n.build/\n",
        ".build/\n!/.build/.conf\n",
        ".build/\n!**/.build/**\n",
    ] {
        fs::write(&ignore, rules).unwrap();
        failure(
            cli(&root, &["build"]),
            if rules.contains('!') {
                "! exceptions"
            } else {
                "Build requires .gitignore"
            },
        );
        failure(
            cli(&root.join("src"), &["build", "Main.ube"]),
            if rules.contains('!') {
                "! exceptions"
            } else {
                "Build requires .gitignore"
            },
        );
        assert_eq!(fs::read_to_string(&ignore).unwrap(), rules);
    }
    for rules in [
        ".build\n",
        "/.build/\n",
        "# !.build/\r\n.build/\r\n!public/\r\n",
    ] {
        fs::write(&ignore, rules).unwrap();
        success(cli(&root, &["build"]));
        assert_eq!(fs::read_to_string(&ignore).unwrap(), rules);
    }
    assert!(!root.join("src/.gitignore").exists());
    git(&root, &["init", "-q"]);
    fs::write(&ignore, ".build/\n!*/\n").unwrap();
    failure(cli(&root, &["build"]), "does not ignore");
    fs::write(&ignore, ".build/\n").unwrap();
    success(cli(&root, &["build"]));
    fs::write(root.join(".build/.conf"), "fake-ciphertext").unwrap();
    git(&root, &["add", "--all"]);
    assert!(git(&root, &["ls-files", "--", ".build/.conf"]).is_empty());

    let standalone = dir.path().join("standalone");
    fs::create_dir(&standalone).unwrap();
    fs::write(standalone.join("Main.ube"), "class Main { main() {} }").unwrap();
    failure(
        cli(&standalone, &["build", "Main.ube"]),
        "Build requires .gitignore",
    );
    fs::write(standalone.join(".gitignore"), ".build/\n").unwrap();
    success(cli(&standalone, &["build", "Main.ube"]));
}

fn read_manifest_data(path: &Path) -> serde_json::Value {
    let text = fs::read_to_string(path).unwrap();
    match path.extension().unwrap().to_str().unwrap() {
        "json" => serde_json::from_str(&text).unwrap(),
        "yaml" => serde_yaml::from_str(&text).unwrap(),
        _ => toml_edit::de::from_str(&text).unwrap(),
    }
}

#[test]
fn manifest_formats_support_scaffolding_mixed_dependencies_and_builds() {
    let dir = tempfile::tempdir().unwrap();
    for (format, dependency_format) in [("toml", "yaml"), ("yaml", "json"), ("json", "toml")] {
        let name = format!("app-{format}");
        success(cli(dir.path(), &["new", &name, "--format", format]));
        let app = dir.path().join(&name);
        let filename = format!("ube.{format}");
        assert!(app.join(&filename).is_file());
        assert_eq!(
            fs::read_to_string(app.join(".gitignore")).unwrap(),
            ".build/\n"
        );
        let library = dir.path().join(format!("lib-{format}"));
        fs::create_dir(&library).unwrap();
        success(cli(
            &library,
            &["init", "--format", dependency_format, "--name", "greetings"],
        ));
        fs::remove_file(library.join("src/Main.ube")).unwrap();
        fs::write(
            library.join("src/Greeting.ube"),
            "class Greeting { int value() { return 42 } }",
        )
        .unwrap();
        let library_path = library.to_str().unwrap();
        success(cli(
            &app.join("src"),
            &["add", "greet", "--path", library_path],
        ));
        let data = read_manifest_data(&app.join(&filename));
        assert!(data["dependencies"]["greet"]["path"].is_string());
        for other in ["toml", "yaml", "json"] {
            assert_eq!(app.join(format!("ube.{other}")).exists(), other == format);
        }
        assert!(app
            .join(format!(".build/.deps/greet/ube.{dependency_format}"))
            .is_file());
        fs::write(
            app.join("src/Main.ube"),
            "import greet.Greeting\nclass Main { main() { print(Greeting().value()) } }",
        )
        .unwrap();
        success(cli(&app.join("src"), &["check"]));
        success(cli(&app, &["build"]));
        assert!(success(cli(&app, &["run"])).contains("42"));
        let lock = fs::read(app.join("ube.lock")).unwrap();
        success(cli(&app, &["install"]));
        assert_eq!(fs::read(app.join("ube.lock")).unwrap(), lock);
        success(cli(&app, &["remove", "greet"]));
        assert!(read_manifest_data(&app.join(&filename))["dependencies"]
            .as_object()
            .unwrap()
            .is_empty());
        assert!(!app.join(".build/.deps/greet").exists());
        failure(
            cli(&app, &["init", "--format", dependency_format]),
            "already exists",
        );
    }
}

#[test]
fn conflicting_or_invalid_manifests_fail_without_modification() {
    let dir = tempfile::tempdir().unwrap();
    success(cli(dir.path(), &["new", "app"]));
    let app = dir.path().join("app");
    let original = fs::read(app.join("ube.toml")).unwrap();
    for extra in ["ube.yaml", "ube.json"] {
        fs::write(app.join(extra), "{}").unwrap();
        for args in [
            &["check"][..],
            &["check", "src/Main.ube"],
            &["build"],
            &["test"],
            &["install"],
            &["clean"],
            &["env", "--add", "X:fake"],
        ] {
            failure(cli(&app, args), "Multiple Ube manifests");
        }
        assert_eq!(fs::read(app.join("ube.toml")).unwrap(), original);
        fs::remove_file(app.join(extra)).unwrap();
    }
    fs::remove_file(app.join("ube.toml")).unwrap();
    for (filename, invalid) in [
        ("ube.yaml", "package:\n  name: app\n  version: 1\n"),
        ("ube.json", r#"{"package":{"name":"app","version":1}}"#),
        ("ube.yaml", "package: ["),
        ("ube.json", "{broken"),
        ("ube.json", "[]"),
    ] {
        fs::write(app.join(filename), invalid).unwrap();
        let output = cli(&app, &["check"]);
        assert!(!output.status.success());
        assert_eq!(fs::read_to_string(app.join(filename)).unwrap(), invalid);
        fs::remove_file(app.join(filename)).unwrap();
    }
    failure(
        cli(dir.path(), &["new", "invalid", "--format", "xml"]),
        "format must be",
    );
    assert!(!dir.path().join("invalid").exists());
}

#[test]
fn git_snapshots_preserve_yaml_dependency_and_json_project_manifests() {
    let dir = tempfile::tempdir().unwrap();
    success(cli(dir.path(), &["new", "library", "--format", "yaml"]));
    let library = dir.path().join("library");
    fs::write(
        library.join("src/Greeting.ube"),
        "class Greeting { int value() { return 42 } }",
    )
    .unwrap();
    git(&library, &["init", "-q"]);
    let revision = commit(&library);
    success(cli(dir.path(), &["new", "app", "--format", "json"]));
    let app = dir.path().join("app");
    success(cli(
        &app,
        &[
            "add",
            "greet",
            "--git",
            library.to_str().unwrap(),
            "--rev",
            &revision,
        ],
    ));
    fs::write(
        app.join("src/Main.ube"),
        "import greet.Greeting\nclass Main { main() { print(Greeting().value()) } }",
    )
    .unwrap();
    success(cli(&app, &["check"]));
    assert_eq!(
        read_manifest_data(&app.join("ube.json"))["dependencies"]["greet"]["rev"],
        revision
    );
    assert!(app.join(".build/.deps/greet/ube.yaml").is_file());
    let lock = fs::read(app.join("ube.lock")).unwrap();
    success(cli(&app, &["install"]));
    assert_eq!(fs::read(app.join("ube.lock")).unwrap(), lock);
    assert!(!app.join("ube.toml").exists());
    assert!(!app.join("ube.yaml").exists());
    success(cli(&app, &["remove", "greet"]));
    assert!(read_manifest_data(&app.join("ube.json"))["dependencies"]
        .as_object()
        .unwrap()
        .is_empty());
    failure(
        cli(
            dir.path(),
            &[
                "new",
                "duplicate-flags",
                "--format",
                "yaml",
                "--format",
                "json",
            ],
        ),
        "Usage:",
    );
    assert!(!dir.path().join("duplicate-flags").exists());
}
