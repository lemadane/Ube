use std::{fs, process::Command};
use ube_compiler::{source::load_program, TypeChecker};

#[test]
fn rejects_multiple_declarations_in_every_combination() {
    for source in [
        "class First {} class Second {}",
        "trait First {} trait Second {}",
        "class First {} trait Second {}",
        "trait First {} class Second {}",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Application.ube");
        fs::write(&path, source).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_ube-compiler"))
            .arg("check")
            .arg(&path)
            .output()
            .unwrap();
        assert!(!output.status.success());
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains("only one class or trait"), "{error}");
        assert!(error.contains("Second"), "{error}");
    }
}

#[test]
fn imports_load_once_and_allow_cycles_with_one_declaration_per_file() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("services")).unwrap();
    fs::write(dir.path().join("Application.ube"), "import services.Greeting\nimport services.Welcome\nclass Application { main() { final welcome = Welcome() log(welcome.greet()) } }").unwrap();
    fs::write(
        dir.path().join("services/Greeting.ube"),
        "import Welcome\ntrait Greeting { string greet() }",
    )
    .unwrap();
    fs::write(
        dir.path().join("services/Welcome.ube"),
        "import Greeting\nclass Welcome { string greet() { return 'hello' } }",
    )
    .unwrap();
    let loaded = load_program(&dir.path().join("Application.ube")).unwrap();
    assert_eq!(
        loaded
            .program
            .items
            .iter()
            .filter(|i| !matches!(i, ube_compiler::parser::ast::Item::Import(_)))
            .count(),
        3
    );
    let mut checker = TypeChecker::new();
    assert!(
        checker.check_program(&loaded.program),
        "{:?}",
        checker.diagnostics
    );
}

#[test]
fn validates_imported_files_and_reports_missing_dependencies() {
    let dir = tempfile::tempdir().unwrap();
    let main = dir.path().join("Application.ube");
    fs::write(&main, "import Helper\nclass Application {}").unwrap();
    assert!(load_program(&main).err().unwrap().contains("Helper.ube"));
    fs::write(
        dir.path().join("Helper.ube"),
        "class Helper {} trait Extra {}",
    )
    .unwrap();
    assert!(load_program(&main)
        .err()
        .unwrap()
        .contains("source validation failed"));
}

#[test]
fn test_runner_loads_dependencies_without_discovering_their_tests() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("Helper.ube"),
        "class Helper { testUnused() { Testicle.fail('must not run') } int value() { return 42 } }",
    )
    .unwrap();
    let path = dir.path().join("Checks.ube");
    fs::write(
        &path,
        "import Helper\nclass Checks { testValue() { Testicle.equal(42, Helper().value()) } }",
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ube-compiler"))
        .arg("test")
        .arg(path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("Tests: 1 passed, 0 failed"));
}

#[test]
fn filenames_must_match_class_and_trait_names_exactly() {
    for declaration in ["class Expected {}", "trait Expected {}"] {
        let dir = tempfile::tempdir().unwrap();
        for filename in ["Wrong.ube", "expected.ube"] {
            let path = dir.path().join(filename);
            fs::write(&path, declaration).unwrap();
            assert!(load_program(&path).is_err());
        }
        let path = dir.path().join("Expected.ube");
        fs::write(&path, declaration).unwrap();
        assert!(load_program(&path).is_ok());
    }
}
