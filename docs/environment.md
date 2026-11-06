# Encrypted environment values

Add your configuration once instead of supplying every variable on each run:

```sh
ube env --add "DB_PASSWORD":"your-password", "okta_username":"username_value", "okta_secret": "secrete_value"
# Save the printed key separately, then supply its protected file:
UBE_ENV_KEY_FILE=/secure/my-app.key ube run
```

`ube env [--key <current-key>] --add <key>:<value>` finds the nearest `ube.toml`, adds or updates one or more
values, and saves the encrypted configuration in `.build/.conf`. Other values
are preserved. Separate entries with commas; spaces after the colon are allowed.
A comma at the end of an argument separates entries, while commas inside a value
are preserved. If a name occurs twice in a batch, the last value wins. The entire
batch is validated before saving; an invalid entry leaves all existing values unchanged. Repeating a name updates its value. Split happens at the first
colon, so a value can contain additional colons and spaces when shell-quoted:

```sh
ube env --key "<current-encryption-key>" --add "SERVICE_URL":"https://example.com:8443/api", "DISPLAY_NAME":"My application", "OPTIONAL_TOKEN":""
# Replace the key in protected storage with the new key printed by this edit.
```

Names must match `[A-Za-z_][A-Za-z0-9_]*`; names beginning with `UBE_ENV_` are
reserved. Empty values are valid. Values cannot contain null bytes. Total
stored secret data is limited to 1 MiB. `--keep` and `--replace` are not supported.
Ube never creates a plaintext configuration file.

## Import and consume JSON

```json
{
  "DB_PASSWORD": "your-password",
  "okta_username": "username_value",
  "okta_secret": "secret_value"
}
```

```sh
ube env file.json
# Replace the complete configuration; the old key is not required:
ube env updated.json
# Optionally verify the current key before replacement:
ube env --key "<current-encryption-key>" updated.json
```

The input must be a regular `.json` file with a nonempty, flat object of string
values, at most 1 MiB. The same name and value validation applies as for `--add`.
Import replaces the complete configuration: names absent from the JSON are
removed. Unlike `--add`, it does not require the previous key, so a complete JSON
file can reset the configuration even if the old key is lost. If `--key` is
explicitly supplied, the old configuration must authenticate before replacement.
It uses the same atomic save, key rotation, printed-key warning, and Git checks.
After the encrypted save is verified, Ube prints and flushes the new key before
removing the input JSON automatically. If writing the key fails, the JSON remains
available for another import, which generates a fresh key.
No plaintext backup or decrypted configuration file is created.

Invalid JSON, rejected values, wrong keys, Git protection failures, and failed
encryption or saves leave the input intact. If the save succeeds but the file
changes during import or cannot be deleted, Ube still prints the new key and
returns an error explaining that the plaintext file was preserved. Save that
new key and remove the plaintext manually after checking it.

Consumption is deletion, not secure erasure: it cannot remove copies from editor
history, backups, or Git objects. Previously staged or committed credentials
still need appropriate cleanup and rotation if exposed.

**Command-line encryption keys and values can appear in shell history and process listings.**
Shell quoting preserves characters but does not hide argument values. Avoid
running these commands in recorded terminals or CI steps that log arguments.
The command warns about this risk and does not echo the supplied value itself.

**Keep the encryption key private.** Never commit it or expose it in chats,
screenshots, terminal recordings, or logs. Keep a secure backup in a password
manager or secret manager. The command also displays this key-handling warning.

Configuration uses AES-256-GCM authenticated encryption. Each save uses a fresh
nonce and is decrypted to verify it before an atomic publish. Existing vaults
must authenticate successfully before an edit. Wrong keys, missing keys,
modified vaults, and failed saves preserve the prior configuration. Concurrent
edits are rejected rather than silently overwriting each other's changes.

## Reading values in Ube

```ube
// src/Main.ube
class Main {
    main() {
        final password = Env.require('DB_PASSWORD')
        // Pass password to the client that needs it; avoid logging secrets.
        string? optionalToken = Env.get('OPTIONAL_TOKEN')
        final configured = Env.has('API_TOKEN')
    }
}
```

| Method | Result | Missing variable |
| --- | --- | --- |
| `Env.get(string)` | `string?` | `null` |
| `Env.require(string)` | `string` | Throws a catchable `Error` |
| `Env.has(string)` | `bool` | `false` |

All three methods are static and require one non-null string name. Empty string
values are present values. Real process environment variables take precedence
over stored values. Vault values are not exported into the process environment
or automatically passed as environment variables to child processes.

Compilation never decrypts the vault or embeds the vault, its contents, or the
key in the executable. Runtime access is lazy: code that never accesses a vault
value needs no key. Each lookup that falls back to the vault authenticates and
decrypts the complete vault, selects the requested value, and discards the
remaining decrypted values. There is no persistent decrypted map or key cache.
Transient decryption and value buffers are wiped when dropped;
a returned value is a normal Ube string and remains readable in program memory
while the program uses it. Missing keys, damaged vaults, and authentication
failures throw catchable errors instead of returning a misleading missing value.

## Where the encryption key lives

Every successful `ube env --add` generates a fresh random 256-bit encryption key,
re-encrypts the complete configuration, verifies and saves it atomically, then
prints `Encryption key: <64 hexadecimal characters>`. **Only the latest key
opens the current configuration.** Old keys may still open old copies or backups.
Failed edits do not print a new key or replace the existing configuration.

Ube never saves the generated key, either inside or outside the project. Save it
in a password manager or deployment secret manager immediately. Losing the latest
key means losing access to the current configuration.

For later edits, supply the latest printed key directly:

```sh
ube env --key "<current-encryption-key>" --add "DB_PASSWORD":"new-password"
```

The provided key decrypts existing entries; the successful save prints a new
key that replaces it. Ube never echoes the supplied key. Alternatively, for edits
and runtime access, set `UBE_ENV_KEY_FILE` to a protected file
containing the current key. It accepts the printed 64-character hexadecimal key
(with an optional final newline), or 32 raw bytes. On Unix, the file must have
owner-only permissions (`chmod 600`); elsewhere configure equivalent OS access
controls. Editing rejects key files inside the project and symlinked key files.

After each successful edit, replace the old key in your protected storage with
the newly printed key. Ube does not update that file for you. Key generation and
printing do not depend on a home directory or local key cache. Supply the same
latest key through your deployment secret manager when running the application.

## Vault discovery and deployment

`ube run` and `ube test` bind temporary executables to the source project,
even when that project has no vault. They never automatically load a different
project's vault from the caller's working directory. A built executable searches
its own directory and ancestors for `.build/.conf`, stopping at the nearest
`ube.toml`. Finding a project without a vault also stops lookup; it does not
fall back to another project. Working directory discovery is used only when
neither a vault nor a project boundary was found beside the executable.
An explicit `UBE_ENV_FILE` overrides automatic discovery. For a deployed
binary away from its project, explicitly configure:

```sh
UBE_ENV_FILE=/srv/my-app/.build/.conf \
UBE_ENV_KEY_FILE=/run/secrets/my-app-env.key \
/srv/my-app/my-app
```

These environment variables contain file paths, not secret values. Transfer the
encrypted vault and its key separately. A binary alone does not contain the
vault and cannot retrieve its encrypted values without both files.

`ube install`, `ube remove`, and `ube clean` preserve `.build/.conf`. Generated
projects already ignore `.build/`. Inside a Git working tree, `ube env` checks
Git's effective ignore rules and index before editing. It refuses to save if
the configuration is tracked, is not ignored, or Git cannot verify repository
state. These checks work in nested projects and linked worktrees. Ube never
changes the Git index automatically. If the configuration was previously tracked,
remove it from tracking explicitly and rotate any credentials previously exposed.

 Keep any old plaintext configuration or
exports out of Git too; encryption does not erase existing backups, terminal
history, or previously committed secrets.

This protects secrets at rest when a project directory is copied or leaked
without its key. It does not protect against malicious editor extensions,
programs running as your account that can read the key, an administrator, or a
debugger inspecting the running program. It does not encrypt application logs,
crash dumps, or values you explicitly write elsewhere.
