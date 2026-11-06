//! Encrypted project secrets. Keys and plaintext are never embedded in builds.
use ring::{
    aead,
    rand::{SecureRandom, SystemRandom},
};
use std::{
    collections::BTreeMap,
    ffi::{c_char, CStr},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};
use zeroize::{Zeroize, Zeroizing};

type Result<T> = std::result::Result<T, String>;
const MAGIC: &[u8; 8] = b"UBEENV01";
const HEADER: usize = 8 + 16 + 12;
const LIMIT: u64 = 1024 * 1024;

struct Values(BTreeMap<String, String>);
impl Drop for Values {
    fn drop(&mut self) {
        for value in self.0.values_mut() {
            value.zeroize();
        }
    }
}

fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn values(bytes: &[u8]) -> Result<Values> {
    // Do not include parser diagnostics: malformed input may contain secrets.
    let map = serde_json::from_slice::<BTreeMap<String, String>>(bytes)
        .map_err(|_| "Secrets must be a JSON object whose values are strings".to_owned())?;
    let values = Values(map);
    for (key, value) in &values.0 {
        if !valid_name(key) || key.starts_with("UBE_ENV_") {
            return Err(
                "Secret names must be environment identifiers and must not start with UBE_ENV_"
                    .into(),
            );
        }
        if value.as_bytes().contains(&0) {
            return Err("Secret values must not contain null bytes".into());
        }
    }
    Ok(values)
}

fn bounded_read(path: &Path, limit: u64) -> Result<Zeroizing<Vec<u8>>> {
    let file = File::open(path).map_err(|_| format!("Could not read {}", path.display()))?;
    let mut bytes = Zeroizing::new(Vec::new());
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Could not read secret data".to_owned())?;
    if bytes.len() as u64 > limit {
        return Err("Secret data exceeds the size limit".into());
    }
    Ok(bytes)
}

fn external_key() -> Option<PathBuf> {
    std::env::var_os("UBE_ENV_KEY_FILE").map(PathBuf::from)
}

fn read_key(path: &Path) -> Result<Zeroizing<Vec<u8>>> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|_| "Secret key is unavailable; restore it or set UBE_ENV_KEY_FILE".to_owned())?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err("Secret key must be a regular file, not a symlink".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(
                "Secret key permissions must restrict access to its owner (chmod 600)".into(),
            );
        }
    }
    let key = bounded_read(path, 128)?;
    if key.len() == 32 {
        return Ok(key);
    }
    let text = std::str::from_utf8(&key)
        .map_err(|_| "Secret key must be 32 raw bytes or 64 hexadecimal characters")?
        .trim();
    if text.len() != 64 {
        return Err("Secret key must be 32 raw bytes or 64 hexadecimal characters".into());
    }
    Ok(Zeroizing::new(hex::decode(text).map_err(|_| {
        "Secret key must contain valid hexadecimal characters"
    })?))
}

fn key_for() -> Result<Zeroizing<Vec<u8>>> {
    let path = external_key()
        .ok_or("Decryption key required; set UBE_ENV_KEY_FILE to a protected key file")?;
    read_key(&path)
}

fn open_bytes(bytes: Zeroizing<Vec<u8>>) -> Result<Zeroizing<Vec<u8>>> {
    open_with_key(bytes, &key_for()?)
}

fn open_with_key(mut bytes: Zeroizing<Vec<u8>>, key_bytes: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
    if bytes.len() < HEADER + aead::AES_256_GCM.tag_len() || &bytes[..8] != MAGIC {
        return Err("Invalid or unsupported encrypted environment file".into());
    }
    let key = aead::LessSafeKey::new(
        aead::UnboundKey::new(&aead::AES_256_GCM, key_bytes)
            .map_err(|_| "Invalid secret key".to_owned())?,
    );
    let header: [u8; HEADER] = bytes[..HEADER].try_into().unwrap();
    let nonce = aead::Nonce::assume_unique_for_key(header[24..36].try_into().unwrap());
    let plaintext = key
        .open_in_place(nonce, aead::Aad::from(&header), &mut bytes[HEADER..])
        .map_err(|_| {
            "Secret vault authentication failed: wrong key or modified vault".to_owned()
        })?;
    // Copy the authenticated plaintext into a buffer that is wiped on drop.
    Ok(Zeroizing::new(plaintext.to_vec()))
}

fn random<const N: usize>() -> Result<[u8; N]> {
    let mut bytes = [0u8; N];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| "System randomness is unavailable".to_owned())?;
    Ok(bytes)
}

fn outside_project(path: &Path, root: &Path) -> Result<()> {
    let absolute = fs::canonicalize(path)
        .map_err(|_| "Secret key is unavailable; could not resolve its location".to_owned())?;
    if absolute.starts_with(root) {
        return Err("Secret encryption keys must be stored outside the project directory".into());
    }
    Ok(())
}

pub struct ImportOutcome {
    pub key: Zeroizing<String>,
    input: PathBuf,
    original: Zeroizing<Vec<u8>>,
}

impl ImportOutcome {
    /// Call only after the generated key has been successfully written and flushed.
    pub fn consume(&self) -> bool {
        let unchanged = bounded_read(&self.input, LIMIT)
            .map(|latest| *latest == *self.original)
            .unwrap_or(false);
        unchanged && fs::remove_file(&self.input).is_ok()
    }
}

/// Save encrypted values, retaining the input until the caller delivers the key.
pub fn import_file(root: &Path, input: &Path, current_key: Option<&str>) -> Result<ImportOutcome> {
    if input.extension().and_then(|extension| extension.to_str()) != Some("json") {
        return Err("Input must be a .json file".into());
    }
    let metadata = fs::symlink_metadata(input).map_err(|_| "Could not read the input JSON file")?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err("Input JSON must be a regular file, not a symlink".into());
    }
    let input_path =
        fs::canonicalize(input).map_err(|_| "Could not resolve the input JSON file")?;
    if external_key()
        .and_then(|path| fs::canonicalize(path).ok())
        .as_ref()
        == Some(&input_path)
    {
        return Err("Input JSON cannot be the encryption key file".into());
    }
    let bytes = bounded_read(input, LIMIT)?;
    let mut map = values(&bytes)?;
    let entries = Zeroizing::new(std::mem::take(&mut map.0).into_iter().collect::<Vec<_>>());
    let key = save(root, &entries, current_key, true)?;
    Ok(ImportOutcome {
        key,
        input: input.to_path_buf(),
        original: bytes,
    })
}

struct EditGuard(PathBuf);
impl Drop for EditGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// Validate and atomically add or update a batch, authenticating existing values first.
pub fn add(
    root: &Path,
    entries: &[(String, String)],
    current_key: Option<&str>,
) -> Result<Zeroizing<String>> {
    save(root, entries, current_key, false)
}

fn save(
    root: &Path,
    entries: &[(String, String)],
    current_key: Option<&str>,
    replace: bool,
) -> Result<Zeroizing<String>> {
    let provided_key = current_key
        .map(|text| {
            if text.len() != 64 {
                return Err("The encryption key must contain 64 hexadecimal characters");
            }
            hex::decode(text)
                .map(Zeroizing::new)
                .map_err(|_| "The encryption key must contain 64 hexadecimal characters")
        })
        .transpose()?;
    if entries.is_empty() {
        return Err("At least one environment value is required".into());
    }
    for (name, value) in entries {
        if !valid_name(name) || name.starts_with("UBE_ENV_") {
            return Err(
                "Variable names must be environment identifiers and must not start with UBE_ENV_"
                    .into(),
            );
        }
        if value.as_bytes().contains(&0) {
            return Err("Environment values must not contain null bytes".into());
        }
        if value.len() as u64 > LIMIT {
            return Err("Secret data exceeds the size limit".into());
        }
    }
    let root =
        fs::canonicalize(root).map_err(|_| "Could not resolve the project directory".to_owned())?;
    let cache = root.join(".build");
    if cache
        .symlink_metadata()
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
    {
        return Err(".build cannot be a symlink".into());
    }
    if cache.exists() && !cache.join(".ube-build").is_file() {
        return Err(".build already exists and is not managed by Ube; move it aside first".into());
    }
    fs::create_dir_all(&cache)
        .map_err(|_| "Could not create the encrypted environment directory".to_owned())?;
    fs::write(cache.join(".ube-build"), "Ube build outputs\n")
        .map_err(|_| "Could not mark the Ube build directory".to_owned())?;
    let lock = cache.join("env.lock");
    OpenOptions::new().write(true).create_new(true).open(&lock)
        .map_err(|_| "Could not lock the environment for editing. If no edit is running, remove the stale .build/env.lock and retry.".to_owned())?;
    let _guard = EditGuard(lock);
    let vault = cache.join(".conf");
    let existing = match vault.symlink_metadata() {
        Ok(metadata) => {
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                return Err("Encrypted environment must be a regular file".into());
            }
            true
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(_) => return Err("Could not inspect the encrypted environment file".into()),
    };
    let mut map = if existing && (!replace || provided_key.is_some()) {
        let bytes = bounded_read(&vault, LIMIT + 128)?;
        let plaintext = match provided_key.as_ref() {
            Some(key) => open_with_key(bytes, key)?,
            None => {
                let location = external_key()
                    .ok_or("Current decryption key required; use --key or set UBE_ENV_KEY_FILE")?;
                outside_project(&location, &root)?;
                open_bytes(bytes)?
            }
        };
        let old = values(&plaintext)?;
        if replace {
            Values(BTreeMap::new())
        } else {
            old
        }
    } else {
        Values(BTreeMap::new())
    };
    for (name, value) in entries {
        if let Some(mut previous) = map.0.insert(name.to_owned(), value.to_owned()) {
            previous.zeroize();
        }
    }
    let plaintext = Zeroizing::new(
        serde_json::to_vec(&map.0).map_err(|_| "Could not encode environment values".to_owned())?,
    );
    if plaintext.len() as u64 > LIMIT {
        return Err("Secret data exceeds the size limit".into());
    }
    let id = random::<16>()?;
    let key_bytes = Zeroizing::new(random::<32>()?.to_vec());
    let nonce_bytes = random::<12>()?;
    let mut header = Vec::from(MAGIC.as_slice());
    header.extend_from_slice(&id);
    header.extend_from_slice(&nonce_bytes);
    let key = aead::LessSafeKey::new(
        aead::UnboundKey::new(&aead::AES_256_GCM, &key_bytes)
            .map_err(|_| "Invalid secret key".to_owned())?,
    );
    let mut ciphertext = Zeroizing::new(plaintext.to_vec());
    key.seal_in_place_append_tag(
        aead::Nonce::assume_unique_for_key(nonce_bytes),
        aead::Aad::from(&header),
        &mut *ciphertext,
    )
    .map_err(|_| "Could not encrypt the environment".to_owned())?;
    header.extend_from_slice(&ciphertext);
    let mut temporary = tempfile::NamedTempFile::new_in(&cache)
        .map_err(|_| "Could not create the encrypted environment file".to_owned())?;
    temporary
        .write_all(&header)
        .and_then(|_| temporary.as_file().sync_all())
        .map_err(|_| "Could not save the encrypted environment file".to_owned())?;
    if *open_with_key(bounded_read(temporary.path(), LIMIT + 128)?, &key_bytes)? != *plaintext {
        return Err("Encrypted environment verification failed".into());
    }
    if existing {
        temporary.persist(&vault)
    } else {
        temporary.persist_noclobber(&vault)
    }
    .map_err(|_| {
        "Could not publish the encrypted environment file; existing values were preserved"
            .to_owned()
    })?;
    Ok(Zeroizing::new(hex::encode(&key_bytes)))
}

fn discover(start: &Path) -> Option<Option<PathBuf>> {
    for directory in start.ancestors() {
        let vault = directory.join(".build/.conf");
        if vault.is_file() {
            return Some(Some(vault));
        }
        if ["ube.toml", "ube.yaml", "ube.json"]
            .iter()
            .any(|name| directory.join(name).is_file())
        {
            return Some(None);
        }
    }
    None
}

fn vault_path() -> Result<Option<PathBuf>> {
    if let Some(path) = std::env::var_os("UBE_ENV_FILE") {
        let path = PathBuf::from(path);
        if !path.is_file() {
            return Err("UBE_ENV_FILE does not point to an encrypted environment file".into());
        }
        return Ok(Some(path));
    }
    // The CLI binds temporary executables to their source project, even when
    // that project has no vault. Never fall through to the caller's directory.
    if let Some(root) = std::env::var_os("UBE_ENV_PROJECT_ROOT") {
        let vault = PathBuf::from(root).join(".build/.conf");
        return Ok(vault.is_file().then_some(vault));
    }
    if let Ok(executable) = std::env::current_exe() {
        if let Some(path) = executable.parent().and_then(discover) {
            return Ok(path);
        }
    }
    Ok(std::env::current_dir()
        .ok()
        .as_deref()
        .and_then(discover)
        .flatten())
}

fn lookup(name: &str) -> Result<Option<Zeroizing<String>>> {
    if !valid_name(name) || name.starts_with("UBE_ENV_") {
        return Err("Invalid environment variable name".into());
    }
    match std::env::var(name) {
        Ok(value) => return Ok(Some(Zeroizing::new(value))),
        Err(std::env::VarError::NotUnicode(_)) => {
            return Err("Environment variable is not valid UTF-8".into())
        }
        Err(std::env::VarError::NotPresent) => (),
    }
    let Some(path) = vault_path()? else {
        return Ok(None);
    };
    let plaintext = open_bytes(bounded_read(&path, LIMIT + 128)?)?;
    let mut map = values(&plaintext)?;
    Ok(map.0.remove(name).map(Zeroizing::new))
}

fn argument<'a>(ptr: *const c_char) -> Result<&'a str> {
    if ptr.is_null() {
        return Err("Environment variable name must not be null".into());
    }
    unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .map_err(|_| "Environment variable name must be UTF-8".into())
}

fn string(value: &str) -> Result<*mut c_char> {
    if value.as_bytes().contains(&0) {
        return Err("Environment value contains a null byte".into());
    }
    let ptr = crate::memory::ube_alloc(value.len() + 1, 1) as *mut c_char;
    if ptr.is_null() {
        return Err("Could not allocate an environment value".into());
    }
    unsafe {
        std::ptr::copy_nonoverlapping(value.as_ptr(), ptr as *mut u8, value.len());
        *ptr.add(value.len()) = 0;
    }
    Ok(ptr)
}

fn run<T: Default>(f: impl FnOnce() -> Result<T>) -> T {
    match f() {
        Ok(value) => value,
        Err(error) => {
            crate::exceptions::set_pending(crate::exceptions::generic(format!("Env: {error}")));
            T::default()
        }
    }
}

#[no_mangle]
pub extern "C" fn ube_env_get(name: *const c_char) -> *mut c_char {
    run(|| match lookup(argument(name)?)? {
        Some(value) => string(&value),
        None => Ok(std::ptr::null_mut()),
    })
}

#[no_mangle]
pub extern "C" fn ube_env_require(name: *const c_char) -> *mut c_char {
    run(|| {
        let value = lookup(argument(name)?)?.ok_or("Required environment variable is missing")?;
        string(&value)
    })
}

#[no_mangle]
pub extern "C" fn ube_env_has(name: *const c_char) -> bool {
    run(|| Ok(lookup(argument(name)?)?.is_some()))
}
