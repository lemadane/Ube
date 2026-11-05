//! Load physical source files before combining their declarations for compilation.
use crate::parser::ast::{Item, Program};
use crate::{DiagnosticEmitter, Lexer, Parser};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

/// Each physical file owns at most one class or trait. Imports are relative to
/// the importing file: `import services.Gateway` loads `services/Gateway.ube`.
/// The parser remains usable for synthetic, combined compilation units.
pub struct LoadedProgram {
    pub program: Program,
    /// Combined source preserves meaningful spans for semantic diagnostics.
    pub source: String,
}

pub fn load_program(path: &Path) -> Result<LoadedProgram, String> {
    if let Ok(root) = crate::project::find_root(path) {
        crate::project::verify_installation(&root)?;
    }
    let mut source = String::new();
    let mut visited = HashSet::new();
    load(path, &mut visited, &mut source)?;
    let mut parser = Parser::new(Lexer::new(&source));
    let program = parser.parse_program();
    Ok(LoadedProgram { program, source })
}

fn load(path: &Path, visited: &mut HashSet<PathBuf>, output: &mut String) -> Result<(), String> {
    let canonical = fs::canonicalize(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if !visited.insert(canonical.clone()) {
        return Ok(());
    }
    let source = fs::read_to_string(&canonical).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut parser = Parser::new(Lexer::new(&source));
    let program = parser.parse_program();
    let mut first = None;
    for item in &program.items {
        let (name, span) = match item {
            Item::Class(c) => (&c.name, c.span),
            Item::Trait(t) => (&t.name, t.span),
            Item::Import(_) => continue,
        };
        if path.file_stem().and_then(|s| s.to_str()) != Some(name.as_str()) {
            parser.diagnostics.push(crate::Diagnostic::error(
                format!("Source filename must match the declared type name exactly; expected '{name}.ube'"),
                Some(span),
            ));
        }
        if let Some(previous) = first {
            parser.diagnostics.push(crate::Diagnostic::error(
                format!("A source file may declare only one class or trait; move '{name}' to a separate file (this file already declares '{previous}')"),
                Some(span),
            ));
        } else {
            first = Some(name);
        }
    }
    if !parser.diagnostics.is_empty() {
        eprintln!("{}:", path.display());
        DiagnosticEmitter::emit(&parser.diagnostics, &source);
        return Err(format!("{}: source validation failed", path.display()));
    }
    for item in &program.items {
        if let Item::Import(import) = item {
            if !import.symbols.is_empty() || import.package_alias.is_some() {
                return Err(format!("{}: local file imports use 'import path.Type'; import aliases and symbol lists are not supported", path.display()));
            }
            let dependency = match crate::project::dependency_import(&canonical, &import.path)? {
                Some(path) => path,
                None => {
                    let mut dependency = canonical.parent().unwrap().to_path_buf();
                    for part in &import.path {
                        dependency.push(part);
                    }
                    dependency.set_extension("ube");
                    dependency
                }
            };
            load(&dependency, visited, output)?;
        }
    }
    output.push_str(&format!("\n// Source: {}\n", canonical.display()));
    output.push_str(&source);
    output.push('\n');
    Ok(())
}
