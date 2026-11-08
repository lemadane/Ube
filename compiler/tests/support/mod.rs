//! Materialize compact multi-type test fixtures as valid physical source files.
use std::{
    fs,
    path::{Path, PathBuf},
};
use ube_compiler::parser::ast::Item;
use ube_compiler::{Lexer, Parser};

pub fn write_source(path: &Path, source: &str) -> PathBuf {
    let mut parser = Parser::new(Lexer::new(source));
    let program = parser.parse_program();
    let declarations: Vec<_> = program
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Class(c) => Some((c.name.as_str(), c.span.start.index)),
            Item::Trait(t) => Some((t.name.as_str(), t.span.start.index)),
            _ => None,
        })
        .collect();
    if !parser.diagnostics.is_empty() || declarations.is_empty() {
        fs::write(path, source).unwrap();
        return path.to_owned();
    }
    let entry = program
        .items
        .iter()
        .find_map(|item| match item {
            Item::Class(c)
                if c.methods
                    .iter()
                    .any(|m| m.name == "start" || m.name.starts_with("test")) =>
            {
                Some(c.name.as_str())
            }
            _ => None,
        })
        .unwrap_or(declarations.last().unwrap().0);
    let module = format!(
        "{}_types",
        path.file_stem()
            .unwrap()
            .to_str()
            .unwrap()
            .replace(|c: char| !c.is_ascii_alphanumeric() && c != '_', "_")
    );
    let types = path.parent().unwrap().join(&module);
    fs::create_dir_all(&types).unwrap();
    for (i, (name, start)) in declarations.iter().enumerate() {
        let end = declarations
            .get(i + 1)
            .map(|(_, start)| *start)
            .unwrap_or(source.len());
        let mut text = String::new();
        for (other, _) in &declarations {
            if other != name && *other != entry {
                text.push_str(&format!(
                    "import {}{other}\n",
                    if *name == entry {
                        format!("{module}.")
                    } else {
                        String::new()
                    }
                ));
            }
        }
        if i == 0 {
            text.push_str(&source[..*start]);
        }
        text.push_str(&source[*start..end]);
        let target = if *name == entry {
            path.with_file_name(format!("{name}.ube"))
        } else {
            types.join(format!("{name}.ube"))
        };
        fs::write(target, text).unwrap();
    }
    path.with_file_name(format!("{entry}.ube"))
}
