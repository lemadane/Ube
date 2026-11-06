//! Project manifests, dependency snapshots, and reproducible installations.
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashSet},
    fs,
    path::{Component, Path, PathBuf},
    process::Command,
};
use toml_edit::{value, ArrayOfTables, DocumentMut, InlineTable, Item, Table, Value};

type Result<T> = std::result::Result<T, String>;
pub const MANIFEST_NAMES: [&str; 3] = ["ube.toml", "ube.yaml", "ube.json"];
const BUILD_MARKER: &str = ".ube-build";
const DEPS_MARKER: &str = ".ube-dependencies";

struct InstallGuard(PathBuf);

impl Drop for InstallGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

#[derive(Clone, Debug)]
pub struct Manifest {
    pub root: PathBuf,
    pub name: String,
    pub version: String,
    pub entry: PathBuf,
    pub document: DocumentMut,
    manifest_name: String,
}

#[derive(Clone, Debug, PartialEq)]
struct Dependency {
    path: Option<String>,
    git: Option<String>,
    rev: Option<String>,
}

#[derive(Clone, Debug)]
struct Locked {
    id: String,
    spec: String,
    version: String,
    revision: String,
    checksum: String,
}

fn read_document(path: &Path) -> Result<DocumentMut> {
    fs::read_to_string(path)
        .map_err(|e| format!("{}: {e}", path.display()))?
        .parse()
        .map_err(|e| format!("{}: {e}", path.display()))
}

pub fn manifest_path(root: &Path) -> Result<PathBuf> {
    let files: Vec<_> = MANIFEST_NAMES
        .iter()
        .filter(|name| root.join(name).exists())
        .collect();
    match files.as_slice() {
        [name] => Ok(root.join(name)),
        [] => Err("No Ube manifest found; expected ube.toml, ube.yaml, or ube.json".into()),
        _ => Err(
            "Multiple Ube manifests found; keep exactly one of ube.toml, ube.yaml, or ube.json"
                .into(),
        ),
    }
}

fn read_manifest(path: &Path) -> Result<DocumentMut> {
    if path.extension().and_then(|s| s.to_str()) == Some("toml") {
        return read_document(path);
    }
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let data: serde_json::Value = if path.extension().and_then(|s| s.to_str()) == Some("json") {
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?
    } else {
        serde_yaml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?
    };
    if !data.is_object() {
        return Err(format!("{}: manifest must be an object", path.display()));
    }
    toml_edit::ser::to_document(&data).map_err(|e| format!("{}: {e}", path.display()))
}

fn serialize_manifest(document: &DocumentMut, name: &str) -> Result<String> {
    if name.ends_with(".toml") {
        return Ok(document.to_string());
    }
    let data: serde_json::Value =
        toml_edit::de::from_str(&document.to_string()).map_err(|e| e.to_string())?;
    if name.ends_with(".json") {
        serde_json::to_string_pretty(&data)
            .map(|text| text + "\n")
            .map_err(|e| e.to_string())
    } else {
        serde_yaml::to_string(&data).map_err(|e| e.to_string())
    }
}

fn format_name(format: &str) -> Result<&'static str> {
    match format {
        "toml" => Ok("ube.toml"),
        "yaml" => Ok("ube.yaml"),
        "json" => Ok("ube.json"),
        _ => Err("Manifest format must be toml, yaml, or json".into()),
    }
}

fn identifier(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        && matches!(
            crate::Lexer::new(name).next_token().kind,
            crate::lexer::token::TokenKind::Identifier(_)
        )
}

fn package_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        && name.chars().next().unwrap().is_ascii_alphanumeric()
}

fn safe_relative(path: &Path) -> bool {
    !path.as_os_str().is_empty() && path.components().all(|c| matches!(c, Component::Normal(_)))
}

pub fn find_root(start: &Path) -> Result<PathBuf> {
    let mut dir = fs::canonicalize(start).map_err(|e| format!("{}: {e}", start.display()))?;
    if dir.is_file() {
        dir.pop();
    }
    for root in dir.ancestors() {
        if MANIFEST_NAMES.iter().any(|name| root.join(name).exists()) {
            return Ok(root.to_owned());
        }
    }
    Err("No Ube manifest (ube.toml, ube.yaml, or ube.json) found. Run 'ube init' to initialize a project.".into())
}

pub fn manifest(root: &Path) -> Result<Manifest> {
    let path = manifest_path(root)?;
    let document = read_manifest(&path)?;
    let package = document
        .get("package")
        .and_then(Item::as_table_like)
        .ok_or("Manifest requires a package table")?;
    let name = package
        .get("name")
        .and_then(Item::as_str)
        .ok_or("[package].name must be a string")?
        .to_owned();
    if !package_name(&name) {
        return Err(format!("Invalid package name '{name}'"));
    }
    let version = package
        .get("version")
        .and_then(Item::as_str)
        .ok_or("[package].version must be a string")?
        .to_owned();
    if version.is_empty() {
        return Err("[package].version must not be empty".into());
    }
    if let Some(edition) = package.get("edition") {
        if edition.as_str() != Some("2026") {
            return Err("Supported package edition is '2026'".into());
        }
    }
    let entry = match document.get("build") {
        None => "src/Main.ube",
        Some(build) => match build
            .as_table_like()
            .ok_or("[build] must be a table")?
            .get("entry")
        {
            None => "src/Main.ube",
            Some(entry) => entry.as_str().ok_or("[build].entry must be a string")?,
        },
    };
    let entry = PathBuf::from(entry);
    if !safe_relative(&entry) || entry.extension().and_then(|s| s.to_str()) != Some("ube") {
        return Err("[build].entry must be a relative .ube path inside the project".into());
    }
    let result = Manifest {
        root: root.to_owned(),
        name,
        version,
        entry,
        document,
        manifest_name: path.file_name().unwrap().to_string_lossy().into_owned(),
    };
    dependencies(&result)?;
    Ok(result)
}

fn dependencies(manifest: &Manifest) -> Result<BTreeMap<String, Dependency>> {
    let mut result = BTreeMap::new();
    let Some(item) = manifest.document.get("dependencies") else {
        return Ok(result);
    };
    let table = item
        .as_table_like()
        .ok_or("[dependencies] must be a table")?;
    for (name, item) in table.iter() {
        if !identifier(name) {
            return Err(format!(
                "Dependency alias '{name}' must be an identifier for use in imports"
            ));
        }
        let table = item.as_table_like().ok_or_else(|| format!("Dependency '{name}' requires {{ path = '...' }} or {{ git = '...', rev = '...' }}; no package registry is configured"))?;
        for (key, _) in table.iter() {
            if !["path", "git", "rev"].contains(&key) {
                return Err(format!("Unknown dependency option '{key}' for '{name}'"));
            }
        }
        let field = |key: &str| -> Result<Option<String>> {
            match table.get(key) {
                None => Ok(None),
                Some(v) => v
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .map(|s| Some(s.to_owned()))
                    .ok_or_else(|| {
                        format!("Dependency '{name}' option '{key}' must be a nonempty string")
                    }),
            }
        };
        let dep = Dependency {
            path: field("path")?,
            git: field("git")?,
            rev: field("rev")?,
        };
        if dep.path.is_some() == dep.git.is_some() || (dep.path.is_some() && dep.rev.is_some()) {
            return Err(format!("Dependency '{name}' requires exactly one of path or git; rev is only valid with git"));
        }
        result.insert(name.into(), dep);
    }
    Ok(result)
}

pub fn init(root: &Path, name: Option<&str>) -> Result<()> {
    init_with_format(root, name, "toml")
}

pub fn init_with_format(root: &Path, name: Option<&str>, format: &str) -> Result<()> {
    let manifest_name = format_name(format)?;
    let name = name
        .map(str::to_owned)
        .or_else(|| root.file_name().and_then(|s| s.to_str()).map(str::to_owned))
        .ok_or("Cannot infer package name; use --name <name>")?;
    if !package_name(&name) {
        return Err(format!("Invalid package name '{name}'"));
    }
    if MANIFEST_NAMES.iter().any(|name| root.join(name).exists()) {
        return Err(
            "A Ube manifest already exists; initialization does not overwrite projects".into(),
        );
    }
    let entry = root.join("src/Main.ube");
    // Existing source trees are preserved, including an existing Main.ube.
    let has_sources = root.join("src").exists();
    fs::create_dir_all(root).map_err(|e| e.to_string())?;
    let mut doc = DocumentMut::new();
    doc["package"]["name"] = value(&name);
    doc["package"]["version"] = value("0.1.0");
    doc["package"]["edition"] = value("2026");
    doc["build"]["entry"] = value("src/Main.ube");
    doc["dependencies"] = Item::Table(Table::new());
    if !has_sources {
        fs::create_dir_all(root.join("src")).map_err(|e| e.to_string())?;
        fs::write(
            &entry,
            "class Main {\n    main() {\n        log('Hello from Ube!')\n    }\n}\n",
        )
        .map_err(|e| e.to_string())?;
    }
    fs::create_dir_all(root.join("tests")).map_err(|e| e.to_string())?;
    ensure_gitignore(root)?;
    fs::write(
        root.join(manifest_name),
        serialize_manifest(&doc, manifest_name)?,
    )
    .map_err(|e| e.to_string())?;
    println!("Initialized Ube project '{name}'");
    if !entry.is_file() {
        println!("Set [build].entry to an existing class-named source file before building.");
    }
    Ok(())
}

pub fn new_project(path: &Path) -> Result<()> {
    new_project_with_format(path, "toml")
}

pub fn new_project_with_format(path: &Path, format: &str) -> Result<()> {
    format_name(format)?;
    if path.exists() {
        return Err(format!("{} already exists", path.display()));
    }
    init_with_format(path, None, format)?;
    fs::write(
        path.join("README.md"),
        "# Ube project\n\n```sh\nube build\nube run\nube test\n```\n",
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn ensure_gitignore(root: &Path) -> Result<()> {
    let path = root.join(".gitignore");
    let mut contents = match fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(format!("{}: {error}", path.display())),
    };
    let newline = if contents.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut changed = false;
    for rule in [".build/"] {
        if !contents.lines().any(|line| line.trim() == rule) {
            if !contents.is_empty() && !contents.ends_with('\n') {
                contents.push_str(newline);
            }
            contents.push_str(rule);
            contents.push_str(newline);
            changed = true;
        }
    }
    if changed {
        fs::write(&path, contents).map_err(|error| format!("{}: {error}", path.display()))?;
    }
    Ok(())
}

/// Builds validate existing protection rather than silently repairing it.
pub fn verify_build_gitignore(root: &Path) -> Result<()> {
    let path = root.join(".gitignore");
    let metadata = fs::symlink_metadata(&path)
        .map_err(|_| "Build requires .gitignore containing .build/".to_owned())?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err("Build requires a regular .gitignore file, not a symlink".into());
    }
    let contents = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut found = false;
    for line in contents.lines() {
        let rule = line.trim();
        if rule.starts_with('#') {
            continue;
        }
        if let Some(exception) = rule.strip_prefix('!') {
            if exception
                .trim_start()
                .split('/')
                .any(|part| part == ".build")
            {
                return Err(
                    "Build refused: .gitignore must not contain ! exceptions for .build".into(),
                );
            }
        } else if matches!(rule, ".build" | ".build/" | "/.build" | "/.build/") {
            found = true;
        }
    }
    if !found {
        return Err("Build requires .gitignore containing .build/".into());
    }
    verify_environment_ignored(root)
}

/// Check Git's effective rules and index, not just the text of .gitignore.
pub fn verify_environment_ignored(root: &Path) -> Result<()> {
    if !root.ancestors().any(|path| path.join(".git").exists()) {
        return Ok(());
    }
    let repository = Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .env("LC_ALL", "C")
        .current_dir(root)
        .output()
        .map_err(|_| "Could not run Git to check environment protection")?;
    if !repository.status.success() {
        if String::from_utf8_lossy(&repository.stderr).contains("not a git repository") {
            return Ok(());
        }
        return Err("Could not verify Git repository state before editing secrets".into());
    }
    if repository.stdout != b"true\n" {
        return Ok(());
    }
    let tracked = Command::new("git")
        .args(["ls-files", "--cached", "-z", "--", ".build/.conf"])
        .current_dir(root)
        .output()
        .map_err(|_| "Could not run Git to check environment protection")?;
    if !tracked.status.success() {
        return Err("Could not verify whether Git tracks the environment configuration".into());
    }
    if !tracked.stdout.is_empty() {
        return Err("Git already tracks the environment configuration. Remove it from tracking with 'git rm --cached -- .build/.conf' before editing secrets. Previously committed credentials still need rotation.".into());
    }
    let ignored = Command::new("git")
        .args([
            "check-ignore",
            "--quiet",
            "--no-index",
            "--",
            ".build/.conf",
        ])
        .current_dir(root)
        .status()
        .map_err(|_| "Could not run Git to check environment protection")?;
    match ignored.code() {
        Some(0) => Ok(()),
        Some(1) => Err("Git does not ignore the environment configuration. Add .build/ to .gitignore and remove any conflicting exceptions before saving secrets.".into()),
        _ => Err("Could not verify Git ignore rules for the environment configuration".into()),
    }
}

pub fn build_directory(root: &Path) -> Result<PathBuf> {
    let dir = root.join(".build");
    if dir
        .symlink_metadata()
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
    {
        return Err("Refusing to use a symlink as .build".into());
    }
    if dir.exists() && !dir.join(BUILD_MARKER).is_file() {
        return Err(".build already exists and is not managed by Ube; move it aside first".into());
    }
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    fs::write(dir.join(BUILD_MARKER), "Ube build outputs\n").map_err(|e| e.to_string())?;
    Ok(dir)
}

pub fn clean(root: &Path) -> Result<()> {
    let dir = root.join(".build");
    if dir
        .symlink_metadata()
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
    {
        return Err("Refusing to clean a symlink as .build".into());
    }
    if dir.exists() {
        if !dir.join(BUILD_MARKER).is_file() {
            return Err("Refusing to delete .build: it is not marked as Ube build output".into());
        }
        if dir.join("install.lock").symlink_metadata().is_ok()
            || dir.join("env.lock").symlink_metadata().is_ok()
        {
            return Err("Cannot clean while an installation or environment edit is running".into());
        }
        let mut retained = false;
        for entry in fs::read_dir(&dir).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            if entry.file_name() == ".conf" || entry.file_name() == ".deps" {
                retained = true;
                continue;
            }
            if entry.file_name() == BUILD_MARKER {
                continue;
            }
            if entry.file_type().map_err(|e| e.to_string())?.is_dir() {
                fs::remove_dir_all(entry.path()).map_err(|e| e.to_string())?;
            } else {
                fs::remove_file(entry.path()).map_err(|e| e.to_string())?;
            }
        }
        if !retained {
            fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
        }
    }
    println!("Cleaned build outputs");
    Ok(())
}

fn lock_entries(root: &Path) -> Result<BTreeMap<String, Locked>> {
    let path = root.join("ube.lock");
    if !path.exists() {
        return Ok(BTreeMap::new());
    }
    let doc = read_document(&path)?;
    if doc.get("version").and_then(Item::as_integer) != Some(1) {
        return Err("Unsupported ube.lock version".into());
    }
    let mut entries = BTreeMap::new();
    if let Some(item) = doc.get("package") {
        for table in item
            .as_array_of_tables()
            .ok_or("Invalid ube.lock packages")?
        {
            let field = |key: &str| {
                table
                    .get(key)
                    .and_then(Item::as_str)
                    .map(str::to_owned)
                    .ok_or_else(|| format!("ube.lock package missing '{key}'"))
            };
            let item = Locked {
                id: field("id")?,
                spec: field("source")?,
                version: field("version")?,
                revision: field("revision")?,
                checksum: field("checksum")?,
            };
            if !item.id.split('/').all(identifier) || item.id.is_empty() {
                return Err("Invalid package id in ube.lock".into());
            }
            if entries.insert(item.id.clone(), item).is_some() {
                return Err("Duplicate package id in ube.lock".into());
            }
        }
    }
    Ok(entries)
}

fn lock_document(entries: &BTreeMap<String, Locked>) -> String {
    let mut doc = DocumentMut::new();
    doc.decor_mut()
        .set_prefix("# Generated by Ube. Commit this file for reproducible installations.\n");
    doc["version"] = value(1);
    let mut packages = ArrayOfTables::new();
    for locked in entries.values() {
        let mut t = Table::new();
        t["id"] = value(&locked.id);
        t["source"] = value(&locked.spec);
        t["version"] = value(&locked.version);
        t["revision"] = value(&locked.revision);
        t["checksum"] = value(&locked.checksum);
        packages.push(t);
    }
    doc["package"] = Item::ArrayOfTables(packages);
    doc.to_string()
}

// Only package manifests and source files are installed. Never follow symlinks,
// copy build outputs, or execute package hooks.
fn source_files(dir: &Path, base: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        if kind.is_symlink() {
            return Err(format!(
                "Package source cannot contain symlinks: {}",
                path.display()
            ));
        }
        if kind.is_dir() {
            source_files(&path, base, files)?;
        } else if kind.is_file() && path.extension().and_then(|s| s.to_str()) == Some("ube") {
            files.push(path.strip_prefix(base).unwrap().to_owned());
        }
    }
    Ok(())
}

fn package_files(root: &Path) -> Result<Vec<PathBuf>> {
    let manifest = manifest_path(root)?;
    let mut files = vec![PathBuf::from(manifest.file_name().unwrap())];
    let src = root.join("src");
    if fs::symlink_metadata(&src)
        .map_err(|e| format!("{}: {e}", src.display()))?
        .file_type()
        .is_symlink()
        || fs::symlink_metadata(&manifest)
            .map_err(|e| e.to_string())?
            .file_type()
            .is_symlink()
    {
        return Err("Package src and manifest cannot be symlinks".into());
    }
    source_files(&src, root, &mut files)?;
    files.sort();
    Ok(files)
}

fn checksum(root: &Path) -> Result<String> {
    let mut hash = Sha256::new();
    for file in package_files(root)? {
        let name = file.to_string_lossy().replace('\\', "/");
        let bytes = fs::read(root.join(&file)).map_err(|e| e.to_string())?;
        hash.update((name.len() as u64).to_le_bytes());
        hash.update(name.as_bytes());
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(bytes);
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn copy_package(from: &Path, to: &Path) -> Result<()> {
    fs::create_dir_all(to.join("src")).map_err(|e| e.to_string())?;
    for file in package_files(from)? {
        let target = to.join(&file);
        fs::create_dir_all(target.parent().unwrap()).map_err(|e| e.to_string())?;
        fs::copy(from.join(file), target).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn git(args: &[&str], cwd: &Path) -> Result<String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|e| format!("Could not run git: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "git failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

fn spec(dep: &Dependency) -> String {
    if let Some(path) = &dep.path {
        format!("path:{path}")
    } else {
        format!(
            "git:{}#{}",
            dep.git.as_ref().unwrap(),
            dep.rev.as_deref().unwrap_or("HEAD")
        )
    }
}

struct Installer<'a> {
    old: &'a BTreeMap<String, Locked>,
    resolved: BTreeMap<String, Locked>,
    active: HashSet<String>,
    update: bool,
}

impl Installer<'_> {
    fn resolve(&mut self, parent: &Manifest, target: &Path, prefix: &str) -> Result<()> {
        for (name, dep) in dependencies(parent)? {
            let id = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            let expected = self.old.get(&id).filter(|v| v.spec == spec(&dep));
            let destination = target.join(&name);
            let checkout = tempfile::tempdir().map_err(|e| e.to_string())?;
            let (source, revision, identity) = if let Some(path) = &dep.path {
                let path = fs::canonicalize(parent.root.join(path))
                    .map_err(|e| format!("Dependency '{id}': {e}"))?;
                let identity = format!("path:{}", path.display());
                (path, String::new(), identity)
            } else {
                let url = dep.git.as_ref().unwrap();
                if url.starts_with('-') {
                    return Err("Git URLs cannot begin with '-'".into());
                }
                let directory = checkout.path().join("repo");
                git(
                    &[
                        "clone",
                        "--quiet",
                        "--no-checkout",
                        "--",
                        url,
                        directory.to_str().ok_or("Non-UTF8 checkout path")?,
                    ],
                    &parent.root,
                )?;
                let rev = if self.update {
                    dep.rev.as_deref().unwrap_or("HEAD")
                } else {
                    expected
                        .map(|v| v.revision.as_str())
                        .unwrap_or(dep.rev.as_deref().unwrap_or("HEAD"))
                };
                if rev.starts_with('-') {
                    return Err("Git revisions cannot begin with '-'".into());
                }
                let revision = git(
                    &["rev-parse", "--verify", &format!("{rev}^{{commit}}")],
                    &directory,
                )?;
                git(
                    &[
                        "-c",
                        "advice.detachedHead=false",
                        "checkout",
                        "--quiet",
                        "--detach",
                        &revision,
                    ],
                    &directory,
                )?;
                let identity = format!("git:{url}#{revision}");
                (directory, revision, identity)
            };
            if !self.active.insert(identity.clone()) {
                return Err(format!("Cyclic package dependency at '{id}'"));
            }
            let package = manifest(&source)?;
            let digest = checksum(&source)?;
            if !self.update {
                if let Some(expected) = expected {
                    if expected.checksum != digest || expected.version != package.version {
                        return Err(format!("Dependency '{id}' differs from ube.lock. Use 'ube install --update' to accept updated sources."));
                    }
                }
            }
            copy_package(&source, &destination)?;
            self.resolve(&package, &destination.join(".build/.deps"), &id)?;
            self.active.remove(&identity);
            self.resolved.insert(
                id.clone(),
                Locked {
                    id,
                    spec: spec(&dep),
                    version: package.version,
                    revision,
                    checksum: digest,
                },
            );
        }
        Ok(())
    }
}

fn install_manifest(project: &Manifest, update: bool) -> Result<()> {
    let old = lock_entries(&project.root)?;
    let cache = build_directory(&project.root)?;
    let guard_path = cache.join("install.lock");
    fs::OpenOptions::new().write(true).create_new(true).open(&guard_path)
        .map_err(|e| format!("Cannot acquire {}: {e}. If no installation is running, remove this stale lock and retry.", guard_path.display()))?;
    let _guard = InstallGuard(guard_path);
    let deps = cache.join(".deps");
    if deps
        .symlink_metadata()
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
    {
        return Err(".build/.deps cannot be a symlink".into());
    }
    if deps.exists() && !deps.join(DEPS_MARKER).is_file() {
        return Err(".build/.deps already exists and is not managed by Ube".into());
    }
    let stage = tempfile::Builder::new()
        .prefix("install-")
        .tempdir_in(&cache)
        .map_err(|e| e.to_string())?;
    let staged = stage.path().join("deps");
    fs::create_dir(&staged).map_err(|e| e.to_string())?;
    let mut installer = Installer {
        old: &old,
        resolved: BTreeMap::new(),
        active: HashSet::new(),
        update,
    };
    installer.active.insert(format!(
        "path:{}",
        fs::canonicalize(&project.root)
            .map_err(|e| e.to_string())?
            .display()
    ));
    installer.resolve(project, &staged, "")?;
    fs::write(staged.join(DEPS_MARKER), "Ube installed dependencies\n")
        .map_err(|e| e.to_string())?;
    let lock = lock_document(&installer.resolved);
    fs::write(stage.path().join("ube.lock"), lock).map_err(|e| e.to_string())?;
    fs::write(
        stage.path().join(&project.manifest_name),
        serialize_manifest(&project.document, &project.manifest_name)?,
    )
    .map_err(|e| e.to_string())?;
    let previous_lock = if project.root.join("ube.lock").exists() {
        Some(fs::read(project.root.join("ube.lock")).map_err(|e| e.to_string())?)
    } else {
        None
    };
    let backup = stage.path().join("previous");
    if deps.exists() {
        fs::rename(&deps, &backup).map_err(|e| e.to_string())?;
    }
    if let Err(e) = fs::rename(&staged, &deps) {
        if backup.exists() {
            let _ = fs::rename(&backup, &deps);
        }
        return Err(e.to_string());
    }
    if let Err(e) = fs::rename(stage.path().join("ube.lock"), project.root.join("ube.lock")) {
        let _ = fs::remove_dir_all(&deps);
        if backup.exists() {
            let _ = fs::rename(&backup, &deps);
        }
        return Err(e.to_string());
    }
    if let Err(e) = fs::rename(
        stage.path().join(&project.manifest_name),
        project.root.join(&project.manifest_name),
    ) {
        if let Some(bytes) = previous_lock {
            let _ = fs::write(project.root.join("ube.lock"), bytes);
        } else {
            let _ = fs::remove_file(project.root.join("ube.lock"));
        }
        let _ = fs::remove_dir_all(&deps);
        if backup.exists() {
            let _ = fs::rename(&backup, &deps);
        }
        return Err(e.to_string());
    }
    println!("Installed {} package(s)", installer.resolved.len());
    Ok(())
}

pub fn install(root: &Path, update: bool) -> Result<()> {
    install_manifest(&manifest(root)?, update)
}

pub fn add(root: &Path, args: &[String]) -> Result<()> {
    let name = args
        .first()
        .filter(|s| identifier(s))
        .ok_or("Usage: ube add <alias> --path <directory> | --git <url> [--rev <revision>]")?;
    let mut dep = Dependency {
        path: None,
        git: None,
        rev: None,
    };
    let mut i = 1;
    while i < args.len() {
        let option = args[i].as_str();
        i += 1;
        let value = args
            .get(i)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| format!("{option} requires a value"))?
            .clone();
        let slot = match option {
            "--path" => &mut dep.path,
            "--git" => &mut dep.git,
            "--rev" => &mut dep.rev,
            _ => return Err(format!("Unknown add option '{option}'")),
        };
        if slot.is_some() {
            return Err(format!("Duplicate option '{option}'"));
        }
        *slot = Some(value);
        i += 1;
    }
    if dep.path.is_some() == dep.git.is_some() || (dep.path.is_some() && dep.rev.is_some()) {
        return Err(
            "Specify --path or --git (with optional --rev); no package registry is configured"
                .into(),
        );
    }
    let mut project = manifest(root)?;
    if dependencies(&project)?.contains_key(name) {
        return Err(format!(
            "Dependency '{name}' already exists; remove it before adding a replacement"
        ));
    }
    // CLI paths are relative to the invoking directory, manifest paths to its root.
    if let Some(path) = &dep.path {
        let absolute = fs::canonicalize(path).map_err(|e| format!("{path}: {e}"))?;
        let base = fs::canonicalize(root).map_err(|e| e.to_string())?;
        let left: Vec<_> = base.components().collect();
        let right: Vec<_> = absolute.components().collect();
        let shared = left.iter().zip(&right).take_while(|(a, b)| a == b).count();
        let mut relative = PathBuf::new();
        for _ in shared..left.len() {
            relative.push("..");
        }
        for part in &right[shared..] {
            relative.push(part);
        }
        if relative.as_os_str().is_empty() {
            relative.push(".");
        }
        dep.path = Some(
            relative
                .to_str()
                .ok_or("Non-UTF8 dependency path")?
                .to_owned(),
        );
    }
    if let Some(url) = &dep.git {
        if Path::new(url).exists() {
            dep.git = Some(
                fs::canonicalize(url)
                    .map_err(|e| e.to_string())?
                    .to_str()
                    .ok_or("Non-UTF8 Git path")?
                    .to_owned(),
            );
        }
    }
    let mut table = InlineTable::new();
    if let Some(path) = dep.path {
        table.insert("path", Value::from(path));
    }
    if let Some(url) = dep.git {
        table.insert("git", Value::from(url));
    }
    if let Some(rev) = dep.rev {
        table.insert("rev", Value::from(rev));
    }
    if project.document.get("dependencies").is_none() {
        project.document["dependencies"] = Item::Table(Table::new());
    }
    project.document["dependencies"][name] = value(table);
    install_manifest(&project, false)?;
    println!("Added '{name}'");
    Ok(())
}

pub fn remove(root: &Path, name: &str) -> Result<()> {
    let mut project = manifest(root)?;
    if !dependencies(&project)?.contains_key(name) {
        return Err(format!("Dependency '{name}' is not declared"));
    }
    project.document["dependencies"]
        .as_table_like_mut()
        .unwrap()
        .remove(name);
    install_manifest(&project, false)?;
    println!("Removed '{name}'");
    Ok(())
}

fn dependency_dir(root: &Path, id: &str) -> PathBuf {
    let mut path = root.to_owned();
    for part in id.split('/') {
        path = path.join(".build/.deps").join(part);
    }
    path
}

/// Check that installed snapshots still match the lock before compiling.
pub fn verify_installation(root: &Path) -> Result<()> {
    let project = manifest(root)?;
    let locked = lock_entries(root)?;
    let mut expected = BTreeMap::new();
    fn visit(
        project: &Manifest,
        prefix: &str,
        root: &Path,
        locked: &BTreeMap<String, Locked>,
        expected: &mut BTreeMap<String, String>,
    ) -> Result<()> {
        for (name, dep) in dependencies(project)? {
            let id = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            let item = locked
                .get(&id)
                .filter(|v| v.spec == spec(&dep))
                .ok_or_else(|| format!("Dependency '{id}' is not locked; run 'ube install'"))?;
            let path = dependency_dir(root, &id);
            let digest = checksum(&path).map_err(|e| {
                format!("Dependency '{id}' is not installed; run 'ube install': {e}")
            })?;
            if digest != item.checksum {
                return Err(format!(
                    "Installed dependency '{id}' was modified; run 'ube install'"
                ));
            }
            expected.insert(id.clone(), item.spec.clone());
            visit(&manifest(&path)?, &id, root, locked, expected)?;
        }
        Ok(())
    }
    visit(&project, "", root, &locked, &mut expected)?;
    if expected.len() != locked.len() {
        return Err("ube.lock contains stale dependencies; run 'ube install'".into());
    }
    Ok(())
}

/// Resolve an import prefixed with a dependency alias in the nearest package.
pub fn dependency_import(file: &Path, parts: &[String]) -> Result<Option<PathBuf>> {
    let Ok(root) = find_root(file) else {
        return Ok(None);
    };
    let project = manifest(&root)?;
    let Some(alias) = parts.first() else {
        return Ok(None);
    };
    if !dependencies(&project)?.contains_key(alias) {
        return Ok(None);
    }
    if parts.len() < 2 {
        return Err(format!("Import '{alias}' must include a type name"));
    }
    let mut path = root.join(".build/.deps").join(alias).join("src");
    for part in &parts[1..] {
        path.push(part);
    }
    path.set_extension("ube");
    if !path.is_file() {
        return Err(format!(
            "{}: dependency import not found; run 'ube install'",
            path.display()
        ));
    }
    Ok(Some(path))
}
