use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

struct Fixture {
    _temp: tempfile::TempDir,
    root: PathBuf,
    keys: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("app");
        let keys = temp.path().join("private-keys");
        fs::create_dir(&root).unwrap();
        let fixture = Self {
            _temp: temp,
            root,
            keys,
        };
        success(fixture.cli(&["init", "--name", "env-app"]));
        fixture
    }
    fn command(&self, executable: impl AsRef<Path>, cwd: &Path) -> Command {
        let mut command = Command::new(executable.as_ref());
        command
            .current_dir(cwd)
            .env("UBE_ENV_KEY_DIR", &self.keys)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env_remove("UBE_ENV_KEY_FILE")
            .env_remove("UBE_ENV_FILE")
            .env_remove("UBE_ENV_PROJECT_ROOT")
            .env_remove("UBE_SECRET_TEST_PASSWORD")
            .env_remove("UBE_SECRET_EMPTY")
            .env_remove("UBE_SECRET_MISSING");
        for name in [
            "DB_PASSWORD",
            "okta_username",
            "okta_secret",
            "ENDPOINT",
            "ADDED",
            "PASSWORD",
            "SECOND",
        ] {
            command.env_remove(name);
        }
        if self.key().is_file() {
            command.env("UBE_ENV_KEY_FILE", self.key());
        }
        command
    }
    fn cli(&self, args: &[&str]) -> Output {
        let output = self
            .command(env!("CARGO_BIN_EXE_ube-compiler"), &self.root)
            .args(args)
            .output()
            .unwrap();
        if output.status.success() {
            if let Some(key) = String::from_utf8_lossy(&output.stdout)
                .lines()
                .find_map(|line| line.strip_prefix("Encryption key: "))
            {
                // Simulate the user saving the printed key outside the project.
                fs::create_dir_all(&self.keys).unwrap();
                let mut options = fs::OpenOptions::new();
                options.write(true).create(true).truncate(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    options.mode(0o600);
                }
                use std::io::Write;
                options
                    .open(self.key())
                    .unwrap()
                    .write_all(key.as_bytes())
                    .unwrap();
            }
        }
        output
    }
    fn source(&self, text: &str) {
        fs::write(self.root.join("src/Main.ube"), text).unwrap();
    }
    fn key(&self) -> PathBuf {
        self.keys.join("current.key")
    }
}
fn success(output: Output) -> String {
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}
fn failure(output: Output, message: &str) -> String {
    assert!(!output.status.success(), "Unexpected success");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(text.contains(message), "Expected {message:?}: {text}");
    text
}
fn populate(fixture: &Fixture) {
    success(fixture.cli(&[
        "env",
        "--add",
        "UBE_SECRET_TEST_PASSWORD:sentinel-secret-π-73482",
    ]));
    success(fixture.cli(&["env", "--add", "UBE_SECRET_EMPTY:"]));
}

#[test]
fn encrypts_added_values_and_decrypts_only_on_runtime_access() {
    let fixture = Fixture::new();
    let message = success(fixture.cli(&[
        "env",
        "--add",
        "UBE_SECRET_TEST_PASSWORD:sentinel-secret-π-73482",
    ]));
    assert!(message.contains("WARNING: Keep the encryption key private."));
    assert!(message.contains("shell history and process listings"));
    assert!(!message.contains("sentinel-secret"));
    success(fixture.cli(&["env", "--add", "UBE_SECRET_EMPTY:"]));
    let bytes = fs::read(fixture.root.join(".build/.conf")).unwrap();
    assert!(!bytes
        .windows(b"sentinel-secret".len())
        .any(|s| s == b"sentinel-secret"));
    assert!(!bytes
        .windows(b"UBE_SECRET_TEST_PASSWORD".len())
        .any(|s| s == b"UBE_SECRET_TEST_PASSWORD"));
    assert!(!fixture.key().starts_with(&fixture.root));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(fixture.key()).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    fixture.source("class Main { main() { print(Env.require('UBE_SECRET_TEST_PASSWORD')) print(Env.has('UBE_SECRET_EMPTY')) print(Env.require('UBE_SECRET_EMPTY')) print(Env.has('UBE_SECRET_MISSING')) try { Env.require('UBE_SECRET_MISSING') } catch (Error error) { print('missing') } } }");
    assert!(success(fixture.cli(&["run"]))
        .contains("sentinel-secret-π-73482\ntrue\n\nfalse\nmissing\n"));
    success(fixture.cli(&["build"]));
    assert!(!fs::read_to_string(fixture.root.join(".build/env-app.ll"))
        .unwrap()
        .contains("sentinel-secret"));
    let output = fixture
        .command(fixture.root.join(".build/env-app"), fixture._temp.path())
        .output()
        .unwrap();
    assert!(success(output).contains("sentinel-secret-π-73482"));
    let vault = fs::read(fixture.root.join(".build/.conf")).unwrap();
    success(fixture.cli(&["clean"]));
    assert!(!fixture.root.join(".build/env-app").exists());
    assert_eq!(fs::read(fixture.root.join(".build/.conf")).unwrap(), vault);
    success(fixture.cli(&["install"]));
    assert_eq!(fs::read(fixture.root.join(".build/.conf")).unwrap(), vault);
    assert!(!fixture.root.join(".ube").exists());
}

#[test]
fn updates_rotate_the_key_and_preserve_other_values() {
    let fixture = Fixture::new();
    populate(&fixture);
    let before = fs::read(fixture.root.join(".build/.conf")).unwrap();
    let key = fs::read(fixture.key()).unwrap();
    success(fixture.cli(&[
        "env",
        "--add",
        "UBE_SECRET_TEST_PASSWORD:replacement:with spaces:and colons",
    ]));
    let after = fs::read(fixture.root.join(".build/.conf")).unwrap();
    assert_ne!(&before[8..24], &after[8..24]);
    assert_ne!(&before[24..36], &after[24..36]);
    assert_ne!(fs::read(fixture.key()).unwrap(), key);
    let latest = fs::read(fixture.key()).unwrap();
    fs::write(fixture.key(), &key).unwrap();
    let failed = fixture.cli(&["env", "--add", "SHOULD_NOT_SAVE:value"]);
    failure(failed, "authentication failed");
    assert_eq!(fs::read(fixture.root.join(".build/.conf")).unwrap(), after);
    fs::write(fixture.key(), latest).unwrap();
    assert_eq!(fs::read_dir(&fixture.keys).unwrap().count(), 1);
    fixture.source("class Main { main() { print(Env.require('UBE_SECRET_TEST_PASSWORD')) print(Env.has('UBE_SECRET_EMPTY')) } }");
    assert!(success(fixture.cli(&["run"])).contains("replacement:with spaces:and colons\ntrue\n"));
    assert!(!fixture.root.join(".build/env.lock").exists());
}

#[test]
fn tampering_wrong_keys_missing_keys_and_permissions_fail_closed() {
    let fixture = Fixture::new();
    populate(&fixture);
    fixture.source("class Main { main() { try { Env.require('UBE_SECRET_TEST_PASSWORD') print('wrong') } catch (Error error) { print('blocked') } } }");
    let key = fixture.key();
    let original = fs::read(&key).unwrap();
    let vault = fixture.root.join(".build/.conf");
    let original_vault = fs::read(&vault).unwrap();
    fs::write(&key, [7u8; 32]).unwrap();
    failure(
        fixture.cli(&["env", "--add", "NEW_VALUE:never-save"]),
        "authentication failed",
    );
    assert_eq!(fs::read(&vault).unwrap(), original_vault);
    assert!(success(fixture.cli(&["run"])).contains("blocked"));
    fs::write(&key, original).unwrap();
    let mut corrupted = original_vault.clone();
    *corrupted.last_mut().unwrap() ^= 1;
    fs::write(&vault, &corrupted).unwrap();
    failure(
        fixture.cli(&["env", "--add", "NEW_VALUE:never-save"]),
        "authentication failed",
    );
    assert_eq!(fs::read(&vault).unwrap(), corrupted);
    assert!(success(fixture.cli(&["run"])).contains("blocked"));
    fs::write(&vault, &original_vault).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&key, fs::Permissions::from_mode(0o644)).unwrap();
        failure(
            fixture.cli(&["env", "--add", "NEW_VALUE:never-save"]),
            "permissions",
        );
        assert!(success(fixture.cli(&["run"])).contains("blocked"));
        fs::set_permissions(&key, fs::Permissions::from_mode(0o600)).unwrap();
    }
    fs::remove_file(key).unwrap();
    failure(
        fixture.cli(&["env", "--add", "NEW_VALUE:never-save"]),
        "key required",
    );
    assert_eq!(fs::read(&vault).unwrap(), original_vault);
    success(fixture.cli(&["build"]));
    assert!(success(fixture.cli(&["run"])).contains("blocked"));
    fixture.source("class Main { main() { print('no secret access') } }");
    assert!(success(fixture.cli(&["run"])).contains("no secret access"));
}

#[test]
fn process_environment_overrides_vault_and_explicit_paths_work() {
    let fixture = Fixture::new();
    populate(&fixture);
    fixture.source("class Main { main() { print(Env.require('UBE_SECRET_TEST_PASSWORD')) } }");
    let out = fixture
        .command(env!("CARGO_BIN_EXE_ube-compiler"), &fixture.root)
        .env("UBE_SECRET_TEST_PASSWORD", "override-value")
        .args(["run"])
        .output()
        .unwrap();
    assert!(success(out).contains("override-value"));
    let out = fixture
        .command(env!("CARGO_BIN_EXE_ube-compiler"), fixture._temp.path())
        .arg("run")
        .arg(fixture.root.join("src/Main.ube"))
        .output()
        .unwrap();
    assert!(success(out).contains("sentinel-secret-π-73482"));
    success(fixture.cli(&["build"]));
    let out = fixture
        .command(fixture.root.join(".build/env-app"), fixture._temp.path())
        .env("UBE_ENV_FILE", fixture.root.join(".build/.conf"))
        .env("UBE_ENV_KEY_FILE", fixture.key())
        .output()
        .unwrap();
    assert!(success(out).contains("sentinel-secret-π-73482"));
}

#[test]
fn invalid_arguments_names_locks_and_symlinks_preserve_existing_values() {
    let fixture = Fixture::new();
    populate(&fixture);
    let vault = fixture.root.join(".build/.conf");
    let original = fs::read(&vault).unwrap();
    for args in [
        vec!["env"],
        vec!["env", "file.json"],
        vec!["env", "--add"],
        vec!["env", "--add", "NO_SEPARATOR"],
        vec!["env", "--add", ":never-echo"],
        vec!["env", "--add", "BAD-NAME:never-echo"],
        vec!["env", "--add", "UBE_ENV_FILE:never-echo"],
        vec!["env", "--add", "GOOD:never-echo", "--keep"],
    ] {
        let out = fixture.cli(&args);
        assert!(!out.status.success());
        assert!(!String::from_utf8_lossy(&out.stderr).contains("never-echo"));
        assert_eq!(fs::read(&vault).unwrap(), original);
    }
    let lock = fixture.root.join(".build/env.lock");
    fs::write(&lock, "busy").unwrap();
    failure(fixture.cli(&["env", "--add", "NEW:never-save"]), "lock");
    failure(fixture.cli(&["clean"]), "environment edit");
    assert!(lock.exists());
    fs::remove_file(lock).unwrap();
    #[cfg(unix)]
    {
        fs::rename(&vault, fixture.root.join("original-vault")).unwrap();
        std::os::unix::fs::symlink(fixture.root.join("original-vault"), &vault).unwrap();
        failure(
            fixture.cli(&["env", "--add", "NEW:never-save"]),
            "regular file",
        );
        assert_eq!(
            fs::read(fixture.root.join("original-vault")).unwrap(),
            original
        );
    }
}

#[test]
fn external_keys_must_be_private_and_outside_the_project() {
    let fixture = Fixture::new();
    populate(&fixture);
    let old_key = fs::read(fixture.key()).unwrap();
    let inside = fixture.root.join("key");
    fs::write(&inside, &old_key).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&inside, fs::Permissions::from_mode(0o600)).unwrap();
    }
    let out = fixture
        .command(env!("CARGO_BIN_EXE_ube-compiler"), &fixture.root)
        .env("UBE_ENV_KEY_FILE", &inside)
        .args(["env", "--add", "PASSWORD:test-value"])
        .output()
        .unwrap();
    failure(out, "outside the project");
    let outside = fixture._temp.path().join("key");
    fs::rename(inside, &outside).unwrap();
    let out = fixture
        .command(env!("CARGO_BIN_EXE_ube-compiler"), &fixture.root)
        .env("UBE_ENV_KEY_FILE", &outside)
        .args(["env", "--add", "PASSWORD:test-value"])
        .output()
        .unwrap();
    let text = success(out);
    assert_eq!(fs::read(&outside).unwrap(), old_key); // Ube never rewrites key storage.
    let latest = text
        .lines()
        .find_map(|line| line.strip_prefix("Encryption key: "))
        .unwrap();
    fs::write(&outside, latest).unwrap(); // The user supplies the newly printed key.
    fixture.source("class Main { main() { print(Env.require('PASSWORD')) } }");
    let out = fixture
        .command(env!("CARGO_BIN_EXE_ube-compiler"), &fixture.root)
        .env("UBE_ENV_KEY_FILE", outside)
        .args(["run"])
        .output()
        .unwrap();
    assert!(success(out).contains("test-value"));
}

#[test]
fn env_api_is_typed_and_works_in_native_tests_and_async_tasks() {
    let fixture = Fixture::new();
    populate(&fixture);
    for source in [
        "class Main { main() { Env() } }",
        "class Main { main() { Env.get(1) } }",
        "class Main { main() { Env.require(null) } }",
        "class Main { main() { Env.unknown('X') } }",
        "class Main { main() { env.require('X') } }",
    ] {
        fixture.source(source);
        assert!(
            !fixture.cli(&["check"]).status.success(),
            "accepted {source}"
        );
    }
    fixture.source("class Main { main() { string? value = Env.get('UBE_SECRET_TEST_PASSWORD') if (value) { print(value) } } }");
    assert!(success(fixture.cli(&["run"])).contains("sentinel-secret-π-73482"));
    fs::write(fixture.root.join("tests/EnvTests.ube"), "class EnvTests { testVault() { Testicle.isTrue(Env.has('UBE_SECRET_TEST_PASSWORD')) Testicle.isFalse(Env.has('UBE_SECRET_MISSING')) } async testAsync() { final task = async (Env.has('UBE_SECRET_TEST_PASSWORD')) Testicle.isTrue(await task) } }").unwrap();
    assert!(success(fixture.cli(&["test"])).contains("Tests: 2 passed"));
}

#[test]
fn comma_separated_batch_with_spaces_is_saved_atomically() {
    let fixture = Fixture::new();
    populate(&fixture);
    success(fixture.cli(&[
        "env",
        "--add",
        "DB_PASSWORD:your-password,",
        "okta_username:username_value,",
        "okta_secret:",
        "secrete_value",
    ]));
    fixture.source("class Main { main() { print(Env.require('DB_PASSWORD')) print(Env.require('okta_username')) print(Env.require('okta_secret')) print(Env.has('UBE_SECRET_EMPTY')) } }");
    assert!(success(fixture.cli(&["run"]))
        .contains("your-password\nusername_value\nsecrete_value\ntrue\n"));
    let vault = fixture.root.join(".build/.conf");
    let before = fs::read(&vault).unwrap();
    failure(
        fixture.cli(&[
            "env",
            "--add",
            "DB_PASSWORD:changed,",
            "BAD-NAME:never-echo",
        ]),
        "identifiers",
    );
    assert_eq!(fs::read(&vault).unwrap(), before);
    success(fixture.cli(&[
        "env",
        "--add",
        "DB_PASSWORD:",
        "new value:with:colons",
        ",",
        "okta_username:next",
    ]));
    assert!(success(fixture.cli(&["run"]))
        .contains("new value:with:colons\nnext\nsecrete_value\ntrue\n"));
}

#[test]
fn generated_key_is_printed_only_after_success_and_never_saved_by_ube() {
    let fixture = Fixture::new();
    let output = fixture
        .command(env!("CARGO_BIN_EXE_ube-compiler"), &fixture.root)
        .args(["env", "--add", "PASSWORD:first-value"])
        .output()
        .unwrap();
    let text = success(output);
    let key = text
        .lines()
        .find_map(|line| line.strip_prefix("Encryption key: "))
        .unwrap();
    assert_eq!(key.len(), 64);
    assert!(!fixture.keys.exists());
    let vault = fs::read(fixture.root.join(".build/.conf")).unwrap();
    assert!(!vault
        .windows(key.len())
        .any(|bytes| bytes == key.as_bytes()));
    let output = fixture
        .command(env!("CARGO_BIN_EXE_ube-compiler"), &fixture.root)
        .args(["env", "--add", "PASSWORD:second-value"])
        .output()
        .unwrap();
    let text = failure(output, "Current decryption key required");
    assert!(!text.contains("Encryption key: "));
    assert_eq!(fs::read(fixture.root.join(".build/.conf")).unwrap(), vault);
}

#[test]
fn key_argument_updates_without_key_file_and_rotates_after_success() {
    let fixture = Fixture::new();
    populate(&fixture);
    let current = fs::read_to_string(fixture.key()).unwrap();
    let before = fs::read(fixture.root.join(".build/.conf")).unwrap();
    let output = fixture
        .command(env!("CARGO_BIN_EXE_ube-compiler"), &fixture.root)
        .env_remove("UBE_ENV_KEY_FILE")
        .args([
            "env",
            "--key",
            &current,
            "--add",
            "ADDED:new-value,",
            "SECOND:",
            "second-value",
        ])
        .output()
        .unwrap();
    let text = success(output);
    assert!(!text.contains(&current));
    let latest = text
        .lines()
        .find_map(|line| line.strip_prefix("Encryption key: "))
        .unwrap();
    assert_ne!(latest, current);
    let after = fs::read(fixture.root.join(".build/.conf")).unwrap();
    assert_ne!(after, before);
    for invalid in [current.as_str(), "not-a-valid-key"] {
        let output = fixture
            .command(env!("CARGO_BIN_EXE_ube-compiler"), &fixture.root)
            .env_remove("UBE_ENV_KEY_FILE")
            .args(["env", "--key", invalid, "--add", "ADDED:bad-value"])
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(!String::from_utf8_lossy(&output.stdout).contains("Encryption key: "));
        assert!(!String::from_utf8_lossy(&output.stderr).contains(invalid));
        assert_eq!(fs::read(fixture.root.join(".build/.conf")).unwrap(), after);
    }
    fs::write(fixture.key(), latest).unwrap();
    fixture.source("class Main { main() { print(Env.require('ADDED')) print(Env.require('SECOND')) print(Env.has('UBE_SECRET_EMPTY')) } }");
    assert!(success(fixture.cli(&["run"])).contains("new-value\nsecond-value\ntrue\n"));
}

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "Git fixture failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn new_and_init_keep_encrypted_values_and_keys_out_of_git_staging() {
    for initialize in [false, true] {
        let fixture = Fixture::new();
        fs::remove_file(fixture.root.join("ube.toml")).unwrap();
        fs::remove_file(fixture.root.join(".gitignore")).unwrap();
        if initialize {
            fs::write(
                fixture.root.join(".gitignore"),
                "# Custom rule\nprivate.txt",
            )
            .unwrap();
            success(fixture.cli(&["init", "--name", "env-app"]));
        } else {
            // Exercise new against a genuinely new directory, then reuse the fixture.
            fs::remove_dir_all(&fixture.root).unwrap();
            let output = fixture
                .command(env!("CARGO_BIN_EXE_ube-compiler"), fixture._temp.path())
                .args(["new", "app"])
                .output()
                .unwrap();
            success(output);
        }
        git(&fixture.root, &["init", "-q"]);
        populate(&fixture);
        let key = fs::read_to_string(fixture.key()).unwrap();
        fixture.source(
            "class Main { main() { final password = Env.require('UBE_SECRET_TEST_PASSWORD') } }",
        );
        success(fixture.cli(&["build"]));
        git(&fixture.root, &["add", "--all"]);
        assert!(git(&fixture.root, &["ls-files", "--", ".build"]).is_empty());
        assert!(!git(&fixture.root, &["check-ignore", "--", ".build/.conf"]).is_empty());
        let staged = git(
            &fixture.root,
            &["diff", "--cached", "--no-ext-diff", "--no-textconv"],
        );
        assert!(!staged.contains("sentinel-secret"));
        assert!(!staged.contains(&key));
        let ignore = fs::read_to_string(fixture.root.join(".gitignore")).unwrap();
        assert!(ignore.lines().any(|line| line == ".build/"));
        if initialize {
            assert!(ignore.starts_with("# Custom rule\nprivate.txt\n"));
        }
    }
}

#[test]
fn missing_or_negated_ignore_rules_block_edits_without_changing_existing_secrets() {
    let fixture = Fixture::new();
    git(&fixture.root, &["init", "-q"]);
    populate(&fixture);
    let before = fs::read(fixture.root.join(".build/.conf")).unwrap();
    let key = fs::read(fixture.key()).unwrap();
    for ignore in [
        "",
        ".build/\n!.build/\n",
        ".build/\n!.build/\n.build/*\n!.build/.conf\n",
    ] {
        fs::write(fixture.root.join(".gitignore"), ignore).unwrap();
        let text = failure(
            fixture.cli(&["env", "--add", "PASSWORD:do-not-echo-this"]),
            "does not ignore",
        );
        assert!(!text.contains("do-not-echo-this"));
        assert!(!text.contains("Encryption key: "));
        assert_eq!(fs::read(fixture.root.join(".build/.conf")).unwrap(), before);
        assert_eq!(fs::read(fixture.key()).unwrap(), key);
    }
    fs::remove_file(fixture.root.join(".gitignore")).unwrap();
    failure(
        fixture.cli(&["env", "--add", "PASSWORD:do-not-save"]),
        "does not ignore",
    );
    assert_eq!(fs::read(fixture.root.join(".build/.conf")).unwrap(), before);
}

#[test]
fn already_tracked_configuration_blocks_edits_until_explicitly_untracked() {
    let fixture = Fixture::new();
    git(&fixture.root, &["init", "-q"]);
    populate(&fixture);
    let before = fs::read(fixture.root.join(".build/.conf")).unwrap();
    git(&fixture.root, &["add", "-f", "--", ".build/.conf"]);
    let text = failure(
        fixture.cli(&["env", "--add", "PASSWORD:do-not-save"]),
        "already tracks",
    );
    assert!(!text.contains("Encryption key: "));
    assert_eq!(fs::read(fixture.root.join(".build/.conf")).unwrap(), before);
    git(&fixture.root, &["rm", "--cached", "--", ".build/.conf"]);
    success(fixture.cli(&["env", "--add", "PASSWORD:now-ignored"]));
    git(&fixture.root, &["add", "--all"]);
    assert!(git(&fixture.root, &["ls-files", "--", ".build"]).is_empty());
}

#[test]
fn git_ignore_protection_works_in_nested_projects_and_worktrees() {
    let fixture = Fixture::new();
    git(fixture._temp.path(), &["init", "-q"]);
    fs::write(fixture._temp.path().join(".gitignore"), "private-keys/\n").unwrap();
    populate(&fixture);
    git(fixture._temp.path(), &["add", "--all"]);
    assert!(git(fixture._temp.path(), &["ls-files", "--", "app/.build"]).is_empty());
    assert!(git(fixture._temp.path(), &["ls-files", "--", "private-keys"]).is_empty());
    git(
        fixture._temp.path(),
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
    let worktree = fixture._temp.path().join("linked-worktree");
    git(
        fixture._temp.path(),
        &[
            "worktree",
            "add",
            "--quiet",
            "--detach",
            worktree.to_str().unwrap(),
        ],
    );
    let app = worktree.join("app");
    let output = fixture
        .command(env!("CARGO_BIN_EXE_ube-compiler"), &app)
        .args(["env", "--add", "PASSWORD:worktree-value"])
        .output()
        .unwrap();
    success(output);
    git(&worktree, &["add", "--all"]);
    assert!(git(&worktree, &["ls-files", "--", "app/.build"]).is_empty());
    fs::write(app.join(".gitignore"), "").unwrap();
    let output = fixture
        .command(env!("CARGO_BIN_EXE_ube-compiler"), &app)
        .args(["env", "--add", "PASSWORD:do-not-save"])
        .output()
        .unwrap();
    failure(output, "does not ignore");
}

#[test]
fn init_handles_empty_existing_and_crlf_ignore_files_without_duplicate_rules() {
    for initial in [
        "",
        "# Custom rules\nprivate.env",
        ".build/\n",
        "# Windows\r\n.build/\r\nprivate.env\r\n",
    ] {
        let fixture = Fixture::new();
        fs::remove_file(fixture.root.join("ube.toml")).unwrap();
        fs::write(fixture.root.join(".gitignore"), initial).unwrap();
        git(&fixture.root, &["init", "-q"]);
        success(fixture.cli(&["init", "--name", "existing-app"]));
        let actual = fs::read_to_string(fixture.root.join(".gitignore")).unwrap();
        assert!(actual.starts_with(initial));
        assert_eq!(actual.lines().filter(|line| *line == ".build/").count(), 1);
        if initial.contains("\r\n") {
            assert_eq!(actual, initial);
        }
        populate(&fixture);
        git(&fixture.root, &["add", "--all"]);
        assert!(git(&fixture.root, &["ls-files", "--", ".build"]).is_empty());
    }
}

#[test]
fn unsafe_storage_and_invalid_git_state_never_publish_keys_or_modify_configuration() {
    let fixture = Fixture::new();
    fs::create_dir(fixture.root.join(".build")).unwrap();
    fs::write(fixture.root.join(".build/user-file"), "preserve me").unwrap();
    let text = failure(
        fixture.cli(&["env", "--add", "PASSWORD:do-not-save"]),
        "not managed",
    );
    assert!(!text.contains("Encryption key: "));
    assert!(!fixture.key().exists());
    assert_eq!(
        fs::read_to_string(fixture.root.join(".build/user-file")).unwrap(),
        "preserve me"
    );
    fs::remove_dir_all(fixture.root.join(".build")).unwrap();
    #[cfg(unix)]
    {
        let outside = fixture._temp.path().join("outside");
        fs::create_dir(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, fixture.root.join(".build")).unwrap();
        failure(
            fixture.cli(&["env", "--add", "PASSWORD:do-not-save"]),
            "symlink",
        );
        assert_eq!(fs::read_dir(outside).unwrap().count(), 0);
        fs::remove_file(fixture.root.join(".build")).unwrap();
    }
    git(&fixture.root, &["init", "-q"]);
    populate(&fixture);
    let before = fs::read(fixture.root.join(".build/.conf")).unwrap();
    fs::write(fixture.root.join(".git/config"), "[invalid config").unwrap();
    let text = failure(
        fixture.cli(&["env", "--add", "PASSWORD:do-not-save"]),
        "repository state",
    );
    assert!(!text.contains("Encryption key: "));
    assert_eq!(fs::read(fixture.root.join(".build/.conf")).unwrap(), before);
}

/// Black-box workflow through the real CLI, Git index, and native executable.
#[test]
fn complete_environment_lifecycle_end_to_end() {
    for initialize in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let fixture = Fixture {
            root: temp.path().join("app"),
            keys: temp.path().join("private-keys"),
            _temp: temp,
        };
        if initialize {
            fs::create_dir(&fixture.root).unwrap();
            fs::write(
                fixture.root.join(".gitignore"),
                "# Existing rules\nprivate.txt",
            )
            .unwrap();
            success(fixture.cli(&["init", "--name", "app"]));
        } else {
            success(
                fixture
                    .command(env!("CARGO_BIN_EXE_ube-compiler"), fixture._temp.path())
                    .args(["new", "app"])
                    .output()
                    .unwrap(),
            );
        }
        assert!(fs::read_to_string(fixture.root.join(".gitignore"))
            .unwrap()
            .lines()
            .any(|line| line == ".build/"));
        git(&fixture.root, &["init", "-q"]);

        let first = success(
            fixture
                .command(env!("CARGO_BIN_EXE_ube-compiler"), &fixture.root)
                .args([
                    "env",
                    "--add",
                    "DB_PASSWORD:first-db,",
                    "okta_username:fixture-user,",
                    "okta_secret:",
                    "fixture-secret",
                ])
                .output()
                .unwrap(),
        );
        let old_key = first
            .lines()
            .find_map(|line| line.strip_prefix("Encryption key: "))
            .unwrap()
            .to_owned();
        assert_eq!(old_key.len(), 64);
        assert!(
            !fixture.keys.exists(),
            "Ube must not save the generated key"
        );
        assert!(!first.contains("first-db"));

        let saved = success(
            fixture
                .command(env!("CARGO_BIN_EXE_ube-compiler"), &fixture.root)
                .args([
                    "env",
                    "--key",
                    &old_key,
                    "--add",
                    "DB_PASSWORD:updated-db,",
                    "ENDPOINT:",
                    "https://fixture.invalid:8443/api",
                ])
                .output()
                .unwrap(),
        );
        let latest_key = saved
            .lines()
            .find_map(|line| line.strip_prefix("Encryption key: "))
            .unwrap()
            .to_owned();
        assert_ne!(latest_key, old_key);
        let vault = fixture.root.join(".build/.conf");
        let ciphertext = fs::read(&vault).unwrap();
        for secret in [
            "first-db",
            "updated-db",
            "fixture-user",
            "fixture-secret",
            old_key.as_str(),
            latest_key.as_str(),
        ] {
            assert!(!ciphertext
                .windows(secret.len())
                .any(|bytes| bytes == secret.as_bytes()));
        }
        failure(
            fixture
                .command(env!("CARGO_BIN_EXE_ube-compiler"), &fixture.root)
                .args(["env", "--key", &old_key, "--add", "DB_PASSWORD:stale"])
                .output()
                .unwrap(),
            "authentication failed",
        );
        assert_eq!(fs::read(&vault).unwrap(), ciphertext);

        // Simulate provisioning the latest printed key outside the deployed project.
        fs::create_dir(&fixture.keys).unwrap();
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        use std::io::Write;
        options
            .open(fixture.key())
            .unwrap()
            .write_all(latest_key.as_bytes())
            .unwrap();
        fixture.source("class Main { main() { print(Env.require('DB_PASSWORD')) print(Env.require('okta_username')) print(Env.require('okta_secret')) print(Env.require('ENDPOINT')) } }");
        let expected =
            "updated-db\nfixture-user\nfixture-secret\nhttps://fixture.invalid:8443/api\n";
        success(fixture.cli(&["build"]));
        assert_eq!(success(fixture.cli(&["run"])), expected);
        let binary = fixture.root.join(".build/app");
        assert_eq!(
            success(
                fixture
                    .command(&binary, fixture._temp.path())
                    .output()
                    .unwrap()
            ),
            expected
        );
        // A deployed native process must also reject the superseded key.
        fs::write(fixture.key(), &old_key).unwrap();
        failure(
            fixture
                .command(&binary, fixture._temp.path())
                .output()
                .unwrap(),
            "authentication failed",
        );
        fs::write(fixture.key(), &latest_key).unwrap();
        for artifact in [
            fixture.root.join("src/Main.ube"),
            fixture.root.join(".build/app.ll"),
            binary.clone(),
        ] {
            let bytes = fs::read(artifact).unwrap();
            for secret in ["updated-db", "fixture-secret", latest_key.as_str()] {
                assert!(!bytes
                    .windows(secret.len())
                    .any(|window| window == secret.as_bytes()));
            }
        }

        git(&fixture.root, &["add", "--all"]);
        assert!(git(&fixture.root, &["ls-files", "--", ".build"]).is_empty());
        let staged = git(
            &fixture.root,
            &["diff", "--cached", "--no-ext-diff", "--no-textconv"],
        );
        for secret in [
            "first-db",
            "updated-db",
            "fixture-user",
            "fixture-secret",
            old_key.as_str(),
            latest_key.as_str(),
        ] {
            assert!(!staged.contains(secret));
        }
        git(&fixture.root, &["add", "-f", "--", ".build/.conf"]);
        failure(
            fixture.cli(&["env", "--key", &latest_key, "--add", "DB_PASSWORD:tracked"]),
            "already tracks",
        );
        assert_eq!(fs::read(&vault).unwrap(), ciphertext);
        git(&fixture.root, &["rm", "--cached", "--", ".build/.conf"]);
        success(fixture.cli(&["clean"]));
        assert!(!binary.exists());
        assert_eq!(fs::read(&vault).unwrap(), ciphertext);
        success(fixture.cli(&["build"]));
        assert_eq!(
            success(
                fixture
                    .command(&binary, fixture._temp.path())
                    .output()
                    .unwrap()
            ),
            expected
        );
        git(&fixture.root, &["add", "--all"]);
        assert!(git(&fixture.root, &["ls-files", "--", ".build"]).is_empty());
    }
}

#[test]
fn json_import_consumes_input_and_supports_key_rotation_and_native_execution() {
    let fixture = Fixture::new();
    git(&fixture.root, &["init", "-q"]);
    let input = fixture.root.join("file.json");
    fs::write(
        &input,
        r#"{"DB_PASSWORD":"json-test-secret","okta_username":"json-user","EMPTY":""}"#,
    )
    .unwrap();
    let message = success(fixture.cli(&["env", "file.json"]));
    assert!(message.contains("Plaintext JSON file consumed"));
    assert!(!message.contains("json-test-secret"));
    assert!(!input.exists());
    let old = fs::read_to_string(fixture.key()).unwrap();
    let before = fs::read(fixture.root.join(".build/.conf")).unwrap();
    fs::write(
        &input,
        r#"{"DB_PASSWORD":"updated-json-secret","ADDED":"another-value"}"#,
    )
    .unwrap();
    failure(
        fixture.cli(&["env", "--key", &"0".repeat(64), "file.json"]),
        "authentication failed",
    );
    assert!(input.exists());
    assert_eq!(fs::read(fixture.root.join(".build/.conf")).unwrap(), before);
    success(fixture.cli(&["env", "--key", &old, "file.json"]));
    assert!(!input.exists());
    assert_ne!(fs::read_to_string(fixture.key()).unwrap(), old);
    fixture.source("class Main { main() { print(Env.require('DB_PASSWORD')) print(Env.has('okta_username')) print(Env.require('ADDED')) print(Env.has('EMPTY')) } }");
    assert!(success(fixture.cli(&["run"]))
        .contains("updated-json-secret\nfalse\nanother-value\nfalse\n"));
    success(fixture.cli(&["build"]));
    assert!(success(
        fixture
            .command(fixture.root.join(".build/env-app"), fixture._temp.path())
            .output()
            .unwrap()
    )
    .contains("updated-json-secret\nfalse\nanother-value\nfalse\n"));
    let latest = fs::read_to_string(fixture.key()).unwrap();
    git(&fixture.root, &["add", "--all"]);
    assert!(git(&fixture.root, &["ls-files", "--", "file.json", ".build"]).is_empty());
    let staged = git(
        &fixture.root,
        &["diff", "--cached", "--no-ext-diff", "--no-textconv"],
    );
    assert!(!staged.contains("updated-json-secret") && !staged.contains(&latest));
}

#[test]
fn json_failures_preserve_input_and_existing_configuration_without_echoing_values() {
    let fixture = Fixture::new();
    populate(&fixture);
    let before = fs::read(fixture.root.join(".build/.conf")).unwrap();
    let input = fixture.root.join("file.json");
    for source in [
        "not-json",
        "[]",
        "{}",
        r#"{"PASSWORD":42}"#,
        r#"{"PASSWORD":"do-not-echo","INVALID-NAME":"x"}"#,
        r#"{"UBE_ENV_FILE":"do-not-echo"}"#,
        r#"{"PASSWORD":"nul\u0000byte"}"#,
    ] {
        fs::write(&input, source).unwrap();
        let output = fixture.cli(&["env", "file.json"]);
        assert!(!output.status.success());
        assert!(!String::from_utf8_lossy(&output.stderr).contains("do-not-echo"));
        assert!(!String::from_utf8_lossy(&output.stdout).contains("Encryption key: "));
        assert_eq!(fs::read_to_string(&input).unwrap(), source);
        assert_eq!(fs::read(fixture.root.join(".build/.conf")).unwrap(), before);
    }
    fs::write(&input, "x".repeat(1024 * 1024 + 1)).unwrap();
    failure(fixture.cli(&["env", "file.json"]), "size limit");
    assert!(input.exists());
    #[cfg(unix)]
    {
        let link = fixture.root.join("link.json");
        std::os::unix::fs::symlink(&input, &link).unwrap();
        failure(fixture.cli(&["env", "link.json"]), "symlink");
        assert!(input.exists());
    }
    fs::write(&input, r#"{"PASSWORD":"do-not-save"}"#).unwrap();
    git(&fixture.root, &["init", "-q"]);
    fs::write(fixture.root.join(".gitignore"), "").unwrap();
    failure(fixture.cli(&["env", "file.json"]), "does not ignore");
    assert!(input.exists());
    assert_eq!(fs::read(fixture.root.join(".build/.conf")).unwrap(), before);
}

#[cfg(unix)]
#[test]
fn failed_json_consumption_still_prints_the_key_for_the_saved_configuration() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new();
    populate(&fixture);
    fs::write(
        fixture.root.join("file.json"),
        r#"{"ADDED":"recoverable-value"}"#,
    )
    .unwrap();
    fs::set_permissions(&fixture.root, fs::Permissions::from_mode(0o500)).unwrap();
    let output = fixture.cli(&["env", "file.json"]);
    fs::set_permissions(&fixture.root, fs::Permissions::from_mode(0o700)).unwrap();
    let text = failure(output, "plaintext JSON changed or could not be removed");
    let key = text
        .lines()
        .find_map(|line| line.strip_prefix("Encryption key: "))
        .expect("A saved configuration must never lose its key on cleanup failure");
    assert!(fixture.root.join("file.json").is_file());
    fs::write(fixture.key(), key).unwrap();
    fixture.source("class Main { main() { print(Env.require('ADDED')) } }");
    assert!(success(fixture.cli(&["run"])).contains("recoverable-value"));
}

#[test]
fn json_replaces_all_values_without_old_key_while_add_preserves_entries() {
    let fixture = Fixture::new();
    populate(&fixture);
    let old = fs::read_to_string(fixture.key()).unwrap();
    fs::write(
        fixture.root.join("file.json"),
        r#"{"ONLY_NEW":"new-value"}"#,
    )
    .unwrap();
    fs::remove_file(fixture.key()).unwrap();
    let output = fixture
        .command(env!("CARGO_BIN_EXE_ube-compiler"), &fixture.root)
        .env_remove("UBE_ENV_KEY_FILE")
        .args(["env", "file.json"])
        .output()
        .unwrap();
    let text = success(output);
    let latest = text
        .lines()
        .find_map(|line| line.strip_prefix("Encryption key: "))
        .unwrap();
    assert_ne!(latest, old);
    assert!(!fixture.root.join("file.json").exists());
    fs::write(fixture.key(), latest).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(fixture.key(), fs::Permissions::from_mode(0o600)).unwrap();
    }
    success(fixture.cli(&["env", "--key", latest, "--add", "ADDED:kept-too"]));
    fixture.source("class Main { main() { print(Env.has('UBE_SECRET_TEST_PASSWORD')) print(Env.has('UBE_SECRET_EMPTY')) print(Env.require('ONLY_NEW')) print(Env.require('ADDED')) } }");
    assert!(success(fixture.cli(&["run"])).contains("false\nfalse\nnew-value\nkept-too\n"));
}

#[cfg(unix)]
#[test]
fn closed_stdout_preserves_json_and_allows_reimport() {
    use std::os::{fd::OwnedFd, unix::net::UnixStream};
    use std::process::Stdio;

    let fixture = Fixture::new();
    populate(&fixture);
    let input = fixture.root.join("file.json");
    let plaintext = r#"{"ADDED":"recover-after-output-failure"}"#;
    fs::write(&input, plaintext).unwrap();
    let (reader, writer) = UnixStream::pair().unwrap();
    drop(reader);
    let writer: OwnedFd = writer.into();
    let output = fixture
        .command(env!("CARGO_BIN_EXE_ube-compiler"), &fixture.root)
        .args(["env", "file.json"])
        .stdout(Stdio::from(writer))
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(
        error.contains("Could not print the encryption key"),
        "{error}"
    );
    assert!(!error.contains("panicked"));
    assert!(!error.contains("recover-after-output-failure"));
    assert_eq!(fs::read_to_string(&input).unwrap(), plaintext);

    // JSON replacement needs no previous key, so the preserved input recovers
    // from the unpublished key generated by the failed command.
    success(fixture.cli(&["env", "file.json"]));
    assert!(!input.exists());
    fixture.source("class Main { main() { print(Env.require('ADDED')) } }");
    assert!(success(fixture.cli(&["run"])).contains("recover-after-output-failure"));
}

#[test]
fn selected_project_without_vault_never_loads_callers_secrets() {
    let a = Fixture::new();
    let b = Fixture::new();
    populate(&b);
    a.source("class Main { main() { print(Env.has('UBE_SECRET_TEST_PASSWORD')) } }");
    let source = a.root.join("src/Main.ube");
    let output = b
        .command(env!("CARGO_BIN_EXE_ube-compiler"), &b.root)
        .args(["run", source.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(success(output).trim(), "false");

    // Native binaries must also stop at their own project boundary.
    success(a.cli(&["build"]));
    let executable = a.root.join(".build/env-app");
    assert_eq!(
        success(b.command(&executable, &b.root).output().unwrap()).trim(),
        "false"
    );

    // Selecting a vault explicitly remains supported.
    let output = b
        .command(env!("CARGO_BIN_EXE_ube-compiler"), &b.root)
        .args(["run", source.to_str().unwrap()])
        .env("UBE_ENV_FILE", b.root.join(".build/.conf"))
        .output()
        .unwrap();
    assert_eq!(success(output).trim(), "true");

    // Project selection also applies to standalone source files without a manifest.
    let standalone = a.root.parent().unwrap().join("Main.ube");
    fs::copy(source, &standalone).unwrap();
    let output = b
        .command(env!("CARGO_BIN_EXE_ube-compiler"), &b.root)
        .args(["run", standalone.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(success(output).trim(), "false");
}

#[test]
fn yaml_and_json_projects_preserve_secret_lookup_boundaries() {
    let caller = Fixture::new();
    populate(&caller);
    for format in ["yaml", "json"] {
        let project = Fixture::new();
        let data: serde_json::Value =
            toml_edit::de::from_str(&fs::read_to_string(project.root.join("ube.toml")).unwrap())
                .unwrap();
        let text = if format == "yaml" {
            serde_yaml::to_string(&data).unwrap()
        } else {
            serde_json::to_string_pretty(&data).unwrap()
        };
        fs::write(project.root.join(format!("ube.{format}")), text).unwrap();
        fs::remove_file(project.root.join("ube.toml")).unwrap();
        project.source("class Main { main() { print(Env.has('UBE_SECRET_TEST_PASSWORD')) } }");
        success(project.cli(&["build"]));
        assert_eq!(
            success(
                caller
                    .command(project.root.join(".build/env-app"), &caller.root)
                    .output()
                    .unwrap()
            )
            .trim(),
            "false"
        );
        let source = project.root.join("src/Main.ube");
        let output = caller
            .command(env!("CARGO_BIN_EXE_ube-compiler"), &caller.root)
            .args(["run", source.to_str().unwrap()])
            .output()
            .unwrap();
        assert_eq!(success(output).trim(), "false");
        populate(&project);
        assert_eq!(success(project.cli(&["run"])).trim(), "true");
    }
}
