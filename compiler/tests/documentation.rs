//! Keep public examples on current syntax and check complete documentation snippets.
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};
use ube_compiler::{lexer::TokenKind, source::load_program, Lexer, TypeChecker};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_owned()
}

fn files(directory: &Path, extension: &str, output: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(directory).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') || matches!(name.as_ref(), "target" | "node_modules") {
            continue;
        }
        let kind = entry.file_type().unwrap();
        if kind.is_dir() {
            files(&entry.path(), extension, output);
        } else if kind.is_file()
            && entry
                .path()
                .extension()
                .is_some_and(|value| value == extension)
        {
            output.push(entry.path());
        }
    }
    output.sort();
}

fn current_syntax(source: &str, label: &str) {
    let mut lexer = Lexer::new(source);
    let mut previous = TokenKind::Eof;
    loop {
        let token = lexer.next_token();
        assert!(
            !matches!(token.kind, TokenKind::Async),
            "{label}: use Async.start, not a legacy async declaration/expression"
        );
        assert!(
            token.kind != TokenKind::Await || previous == TokenKind::Dot,
            "{label}: use task.await(), not prefix await"
        );
        assert!(
            !matches!(&token.kind, TokenKind::Identifier(name) if name == "Task"),
            "{label}: Task has been renamed to Async"
        );
        if token.kind == TokenKind::Eof {
            break;
        }
        previous = token.kind;
    }
}

fn check(path: &Path, label: &str) {
    let loaded = load_program(path).unwrap_or_else(|error| panic!("{label}: {error}"));
    let mut checker = TypeChecker::new();
    assert!(
        checker.check_program(&loaded.program),
        "{label}: {:?}",
        checker.diagnostics
    );
}

#[test]
fn every_shipped_ube_example_checks_and_uses_current_task_syntax() {
    let mut sources = Vec::new();
    files(&root().join("examples"), "ube", &mut sources);
    assert!(!sources.is_empty());
    for source in sources {
        let label = source.display().to_string();
        current_syntax(&fs::read_to_string(&source).unwrap(), &label);
        check(&source, &label);
    }
}

fn snippets(markdown: &str) -> Vec<(usize, &str)> {
    let mut blocks = Vec::new();
    let mut remaining = markdown;
    let mut offset = 0;
    while let Some(start) = remaining.find("```ube\n") {
        let body_start = start + "```ube\n".len();
        let Some(end) = remaining[body_start..].find("```") else {
            break;
        };
        blocks.push((
            offset + body_start,
            &remaining[body_start..body_start + end],
        ));
        let next = body_start + end + 3;
        offset += next;
        remaining = &remaining[next..];
    }
    blocks
}

fn declaration(source: &str) -> Option<String> {
    let mut names = source.lines().filter_map(|line| {
        let line = line
            .strip_prefix("class ")
            .or_else(|| line.strip_prefix("trait "))?;
        Some(
            line.chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect::<String>(),
        )
    });
    let name = names.next()?;
    if names.next().is_some() || name.is_empty() {
        None
    } else {
        Some(name)
    }
}

#[test]
fn markdown_ube_examples_use_current_syntax_and_complete_classes_check() {
    let mut documents = Vec::new();
    files(&root(), "md", &mut documents);
    let mut checked = 0;
    for document in documents {
        let markdown = fs::read_to_string(&document).unwrap();
        let blocks = snippets(&markdown);
        for (position, source) in &blocks {
            let line = markdown[..*position].lines().count() + 1;
            let label = format!("{}:{line}", document.display());
            current_syntax(source, &label);
            let Some(name) = declaration(source) else {
                continue;
            };
            let directory = tempfile::tempdir().unwrap();
            // Each documentation example is independent. Resolve supporting types
            // from the nearest matching declaration in the same document.
            let mut dependencies = HashMap::new();
            for (other_position, other_source) in &blocks {
                if let Some(other_name) = declaration(other_source) {
                    if other_name == name {
                        continue;
                    }
                    let distance = position.abs_diff(*other_position);
                    let item = dependencies.entry(other_name).or_insert((usize::MAX, ""));
                    if distance < item.0 {
                        *item = (distance, *other_source);
                    }
                }
            }
            for (name, (_, source)) in dependencies {
                fs::write(directory.path().join(format!("{name}.ube")), source).unwrap();
            }
            // The dependency-import example explicitly assumes this package type.
            if source.contains("import greetings.Greeting") {
                fs::create_dir(directory.path().join("greetings")).unwrap();
                fs::write(
                    directory.path().join("greetings/Greeting.ube"),
                    "class Greeting { string message() { return 'Hello' } }",
                )
                .unwrap();
            }
            let path = directory.path().join(format!("{name}.ube"));
            fs::write(&path, source).unwrap();
            check(&path, &label);
            checked += 1;
        }
    }
    assert!(
        checked > 50,
        "expected the complete documentation examples to be checked"
    );
}
