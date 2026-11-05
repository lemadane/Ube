//! JWT verification and OAuth 2.0 authorization-code clients with S256 PKCE.
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    ffi::{c_char, c_void, CStr},
    io::Read,
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use url::Url;
type Result<T> = std::result::Result<T, String>;
type Handle = *mut c_void;
const MAX_TOKEN: usize = 1024 * 1024;
const FLOW_TTL: Duration = Duration::from_secs(5 * 60);
#[derive(Clone)]
struct Client {
    authorize: Url,
    token: Url,
    client_id: String,
    secret: String,
    redirect: String,
}
struct Transaction {
    client: usize,
    binding: String,
    verifier: String,
    expires: Instant,
}
struct Token {
    access: String,
    kind: String,
    refresh: Option<String>,
    expires: i64,
    scope: Option<String>,
}
#[derive(Default)]
struct Registry {
    next: usize,
    clients: HashMap<usize, Arc<Client>>,
    claims: HashMap<usize, Value>,
    tokens: HashMap<usize, Token>,
    pending: HashMap<String, Transaction>,
}
impl Registry {
    fn id(&mut self) -> Result<usize> {
        self.next = self
            .next
            .checked_add(1)
            .ok_or("authentication handle limit reached")?;
        Ok(self.next)
    }
}
fn registry() -> &'static Mutex<Registry> {
    static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
    REGISTRY.get_or_init(Default::default)
}
fn run<T: Default>(f: impl FnOnce() -> Result<T>) -> T {
    match f() {
        Ok(value) => value,
        Err(error) => {
            crate::exceptions::set_pending(crate::exceptions::generic(format!(
                "Authentication: {error}"
            )));
            T::default()
        }
    }
}
fn text(ptr: *const c_char) -> Result<String> {
    if ptr.is_null() {
        return Err("arguments must not be null".into());
    }
    unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .map(str::to_owned)
        .map_err(|_| "invalid UTF-8 argument".into())
}
fn string(value: &str) -> Result<*mut c_char> {
    if value.as_bytes().contains(&0) {
        return Err("string contains a null byte".into());
    }
    let ptr = crate::memory::ube_alloc(value.len() + 1, 1) as *mut c_char;
    if ptr.is_null() {
        return Err("could not allocate string".into());
    }
    unsafe {
        std::ptr::copy_nonoverlapping(value.as_ptr(), ptr as *mut u8, value.len());
        *ptr.add(value.len()) = 0;
    }
    Ok(ptr)
}
fn now() -> Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|t| t.as_secs())
        .map_err(|_| "system clock is invalid".into())
}
fn algorithm(value: &str) -> Result<Algorithm> {
    match value {
        "HS256" => Ok(Algorithm::HS256),
        "RS256" => Ok(Algorithm::RS256),
        _ => Err("supported JWT algorithms are HS256 and RS256".into()),
    }
}
fn symmetric_key(key: &str) -> Result<()> {
    if key.len() < 32 || key.contains("-----BEGIN") {
        return Err("HS256 requires a secret of at least 32 bytes, not a PEM key".into());
    }
    Ok(())
}
fn jwt_claims(claims: &Value) -> Result<()> {
    if !claims.is_object() {
        return Err("JWT claims must be a JSON object".into());
    }
    let exp = claims
        .get("exp")
        .and_then(Value::as_u64)
        .ok_or("JWT requires an integer exp claim")?;
    if exp <= now()? {
        return Err("JWT has expired".into());
    }
    if claims
        .get("iss")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .is_none()
    {
        return Err("JWT requires an issuer".into());
    }
    let audience = claims.get("aud").ok_or("JWT requires an audience")?;
    if !audience.as_str().is_some_and(|s| !s.is_empty())
        && !audience.as_array().is_some_and(|a| {
            !a.is_empty() && a.iter().all(|v| v.as_str().is_some_and(|s| !s.is_empty()))
        })
    {
        return Err("JWT audience must be a string or nonempty string array".into());
    }
    for name in ["nbf", "iat"] {
        if let Some(value) = claims.get(name) {
            if value.as_u64().is_none() {
                return Err(format!("JWT {name} must be an integer"));
            }
        }
    }
    Ok(())
}
fn sign(claims: &str, key: &str, alg: &str) -> Result<String> {
    if claims.len() > MAX_TOKEN {
        return Err("JWT claims are too large".into());
    }
    let claims: Value = serde_json::from_str(claims).map_err(|_| "invalid JWT claims JSON")?;
    jwt_claims(&claims)?;
    let alg = algorithm(alg)?;
    let key = match alg {
        Algorithm::HS256 => {
            symmetric_key(key)?;
            EncodingKey::from_secret(key.as_bytes())
        }
        _ => EncodingKey::from_rsa_pem(key.as_bytes()).map_err(|_| "invalid RSA private key")?,
    };
    let token =
        jsonwebtoken::encode(&Header::new(alg), &claims, &key).map_err(|_| "JWT signing failed")?;
    if token.len() > MAX_TOKEN {
        return Err("encoded JWT is too large".into());
    }
    Ok(token)
}
fn verify(token: &str, key: &str, alg: &str, issuer: &str, audience: &str) -> Result<Value> {
    if token.len() > MAX_TOKEN || issuer.is_empty() || audience.is_empty() {
        return Err("JWT verification requires a bounded token, issuer, and audience".into());
    }
    let header = token.split('.').next().ok_or("invalid JWT")?;
    let header: Value = serde_json::from_slice(
        &URL_SAFE_NO_PAD
            .decode(header)
            .map_err(|_| "invalid JWT header")?,
    )
    .map_err(|_| "invalid JWT header")?;
    if header.get("crit").is_some() || header.get("b64").is_some() {
        return Err("unsupported JWT critical extensions".into());
    }
    let alg = algorithm(alg)?;
    let key = match alg {
        Algorithm::HS256 => {
            symmetric_key(key)?;
            DecodingKey::from_secret(key.as_bytes())
        }
        _ => DecodingKey::from_rsa_pem(key.as_bytes()).map_err(|_| "invalid RSA public key")?,
    };
    let mut validation = Validation::new(alg);
    validation.leeway = 0;
    validation.validate_nbf = true;
    validation.set_required_spec_claims(&["exp", "iss", "aud"]);
    validation.set_issuer(&[issuer]);
    validation.set_audience(&[audience]);
    let claims = jsonwebtoken::decode::<Value>(token, &key, &validation)
        .map_err(|_| "JWT verification failed")?
        .claims;
    jwt_claims(&claims)?;
    if claims
        .get("iat")
        .and_then(Value::as_u64)
        .is_some_and(|iat| iat > now().unwrap_or(0))
    {
        return Err("JWT issued-at time is in the future".into());
    }
    Ok(claims)
}
#[no_mangle]
pub extern "C" fn ube_jwt_sign(
    claims: *const c_char,
    key: *const c_char,
    alg: *const c_char,
) -> *mut c_char {
    run(|| string(&sign(&text(claims)?, &text(key)?, &text(alg)?)?))
}
#[no_mangle]
pub extern "C" fn ube_jwt_verify(
    token: *const c_char,
    key: *const c_char,
    alg: *const c_char,
    issuer: *const c_char,
    audience: *const c_char,
) -> Handle {
    run(|| {
        let claims = verify(
            &text(token)?,
            &text(key)?,
            &text(alg)?,
            &text(issuer)?,
            &text(audience)?,
        )?;
        let mut r = registry().lock().unwrap();
        let id = r.id()?;
        r.claims.insert(id, claims);
        Ok(id as Handle)
    })
}
#[no_mangle]
pub extern "C" fn ube_jwt_claims_get(handle: Handle, key: *const c_char) -> *mut c_char {
    run(|| {
        let key = text(key)?;
        let r = registry().lock().unwrap();
        let claims = r
            .claims
            .get(&(handle as usize))
            .ok_or("invalid or closed JwtClaims")?;
        match claims.get(&key) {
            None | Some(Value::Null) => Ok(std::ptr::null_mut()),
            Some(Value::String(value)) => string(value),
            Some(_) => Err("claim is not a string; use claims.json() for other JSON values".into()),
        }
    })
}
#[no_mangle]
pub extern "C" fn ube_jwt_claims_json(handle: Handle) -> *mut c_char {
    run(|| {
        let r = registry().lock().unwrap();
        let claims = r
            .claims
            .get(&(handle as usize))
            .ok_or("invalid or closed JwtClaims")?;
        string(&claims.to_string())
    })
}
#[no_mangle]
pub extern "C" fn ube_jwt_claims_close(handle: Handle) {
    registry().lock().unwrap().claims.remove(&(handle as usize));
}
fn endpoint(value: &str) -> Result<Url> {
    let url = Url::parse(value).map_err(|_| "invalid OAuth URL")?;
    let local = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    if (url.scheme() != "https" && !(url.scheme() == "http" && local))
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err("OAuth URLs require HTTPS (HTTP is permitted only on loopback), without credentials or fragments".into());
    }
    Ok(url)
}
fn client(handle: Handle) -> Result<Arc<Client>> {
    registry()
        .lock()
        .unwrap()
        .clients
        .get(&(handle as usize))
        .cloned()
        .ok_or_else(|| "invalid or closed OAuth2 client".into())
}
fn random() -> Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes).map_err(|_| "could not generate secure OAuth randomness")?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}
fn challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}
#[no_mangle]
pub extern "C" fn ube_oauth2_create(
    authorize: *const c_char,
    token: *const c_char,
    id: *const c_char,
    secret: *const c_char,
    redirect: *const c_char,
) -> Handle {
    run(|| {
        let authorize = endpoint(&text(authorize)?)?;
        let token = endpoint(&text(token)?)?;
        let redirect = text(redirect)?;
        endpoint(&redirect)?;
        let client_id = text(id)?;
        let secret = text(secret)?;
        if client_id.is_empty() {
            return Err("OAuth client ID must not be empty".into());
        }
        let reserved = [
            "response_type",
            "client_id",
            "redirect_uri",
            "scope",
            "state",
            "code_challenge",
            "code_challenge_method",
        ];
        if authorize
            .query_pairs()
            .any(|(key, _)| reserved.contains(&key.as_ref()))
        {
            return Err("authorization endpoint contains reserved OAuth parameters".into());
        }
        let mut r = registry().lock().unwrap();
        let handle = r.id()?;
        r.clients.insert(
            handle,
            Arc::new(Client {
                authorize,
                token,
                client_id,
                secret,
                redirect,
            }),
        );
        Ok(handle as Handle)
    })
}
#[no_mangle]
pub extern "C" fn ube_oauth2_authorize(
    handle: Handle,
    session: Handle,
    scope: *const c_char,
) -> *mut c_char {
    run(|| {
        let client = client(handle)?;
        let scope = text(scope)?;
        if !scope.bytes().all(|b| {
            b == b' ' || b == 0x21 || (0x23..=0x5b).contains(&b) || (0x5d..=0x7e).contains(&b)
        }) {
            return Err("invalid OAuth scope".into());
        }
        let binding = crate::sessions::identity(session, true)?;
        let state = random()?;
        let verifier = random()?;
        let mut url = client.authorize.clone();
        url.query_pairs_mut()
            .append_pair("response_type", "code")
            .append_pair("client_id", &client.client_id)
            .append_pair("redirect_uri", client.redirect.as_str())
            .append_pair("state", &state)
            .append_pair("code_challenge", &challenge(&verifier))
            .append_pair("code_challenge_method", "S256");
        if !scope.is_empty() {
            url.query_pairs_mut().append_pair("scope", &scope);
        }
        let mut r = registry().lock().unwrap();
        r.pending.retain(|_, flow| flow.expires > Instant::now());
        r.pending.insert(
            state,
            Transaction {
                client: handle as usize,
                binding,
                verifier,
                expires: Instant::now() + FLOW_TTL,
            },
        );
        string(url.as_str())
    })
}
fn encoded(value: &str) -> String {
    url::form_urlencoded::Serializer::new(String::new())
        .append_pair("", value)
        .finish()
        .trim_start_matches('=')
        .into()
}
fn token_request(client: Arc<Client>, mut fields: Vec<(String, String)>) -> Result<Token> {
    // reqwest's blocking runtime is created/dropped outside any Ube Tokio context.
    std::thread::spawn(move || {
        let http = reqwest::blocking::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(15))
            .connect_timeout(Duration::from_secs(5))
            .build()
            .map_err(|_| "could not initialize OAuth HTTPS client")?;
        let mut request = http
            .post(client.token.clone())
            .header("Accept", "application/json");
        if client.secret.is_empty() {
            fields.push(("client_id".into(), client.client_id.clone()));
        } else {
            request = request.basic_auth(encoded(&client.client_id), Some(encoded(&client.secret)));
        }
        let response = request
            .form(&fields)
            .send()
            .map_err(|_| "OAuth token request failed")?;
        if !response.status().is_success() {
            return Err("OAuth token endpoint rejected the request".into());
        }
        let mut bytes = Vec::new();
        response
            .take((MAX_TOKEN + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|_| "could not read OAuth token response")?;
        if bytes.len() > MAX_TOKEN {
            return Err("OAuth token response is too large".into());
        }
        let value: Value =
            serde_json::from_slice(&bytes).map_err(|_| "invalid OAuth token JSON")?;
        if value.get("error").is_some() {
            return Err("OAuth token endpoint returned an error".into());
        }
        let access = value
            .get("access_token")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or("OAuth response is missing access_token")?
            .to_owned();
        let kind = value
            .get("token_type")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or("OAuth response is missing token_type")?
            .to_owned();
        if !kind.eq_ignore_ascii_case("Bearer") {
            return Err("only Bearer OAuth tokens are supported".into());
        }
        let optional = |name| -> Result<Option<String>> {
            match value.get(name) {
                None => Ok(None),
                Some(Value::String(v)) => Ok(Some(v.clone())),
                _ => Err(format!("OAuth {name} must be a string")),
            }
        };
        let expires = match value.get("expires_in") {
            None => -1,
            Some(v) => v
                .as_i64()
                .filter(|v| *v >= 0)
                .ok_or("invalid OAuth expires_in")?,
        };
        Ok(Token {
            access,
            kind,
            refresh: optional("refresh_token")?,
            expires,
            scope: optional("scope")?,
        })
    })
    .join()
    .map_err(|_| "OAuth token worker failed")?
}
fn save_token(token: Token) -> Result<Handle> {
    let mut r = registry().lock().unwrap();
    let id = r.id()?;
    r.tokens.insert(id, token);
    Ok(id as Handle)
}
#[no_mangle]
pub extern "C" fn ube_oauth2_exchange(
    handle: Handle,
    session: Handle,
    code: *const c_char,
    state: *const c_char,
) -> Handle {
    run(|| {
        let client = client(handle)?;
        let code = text(code)?;
        let state = text(state)?;
        if code.is_empty() || code.len() > 8192 || state.len() != 43 {
            return Err("invalid OAuth callback code or state".into());
        }
        let binding = crate::sessions::identity(session, false)?;
        let flow = {
            let mut r = registry().lock().unwrap();
            r.pending.retain(|_, flow| flow.expires > Instant::now());
            let flow = r
                .pending
                .get(&state)
                .ok_or("OAuth state is invalid, expired, or already used")?;
            if flow.client != handle as usize || flow.binding != binding {
                return Err("OAuth state does not belong to this client and session".into());
            }
            r.pending.remove(&state).unwrap()
        };
        save_token(token_request(
            client.clone(),
            vec![
                ("grant_type".into(), "authorization_code".into()),
                ("code".into(), code),
                ("redirect_uri".into(), client.redirect.to_string()),
                ("code_verifier".into(), flow.verifier),
            ],
        )?)
    })
}
#[no_mangle]
pub extern "C" fn ube_oauth2_refresh(handle: Handle, refresh: *const c_char) -> Handle {
    run(|| {
        let client = client(handle)?;
        let refresh = text(refresh)?;
        if refresh.is_empty() {
            return Err("refresh token must not be empty".into());
        }
        let mut token = token_request(
            client,
            vec![
                ("grant_type".into(), "refresh_token".into()),
                ("refresh_token".into(), refresh.clone()),
            ],
        )?;
        if token.refresh.is_none() {
            token.refresh = Some(refresh);
        }
        save_token(token)
    })
}
#[no_mangle]
pub extern "C" fn ube_oauth2_close(handle: Handle) {
    let mut r = registry().lock().unwrap();
    r.clients.remove(&(handle as usize));
    r.pending.retain(|_, flow| flow.client != handle as usize);
}
fn token_text(handle: Handle, field: impl FnOnce(&Token) -> Option<&str>) -> *mut c_char {
    run(|| {
        let r = registry().lock().unwrap();
        let token = r
            .tokens
            .get(&(handle as usize))
            .ok_or("invalid or closed OAuth2Token")?;
        match field(token) {
            Some(v) => string(v),
            None => Ok(std::ptr::null_mut()),
        }
    })
}
#[no_mangle]
pub extern "C" fn ube_oauth2_token_access(handle: Handle) -> *mut c_char {
    token_text(handle, |t| Some(&t.access))
}
#[no_mangle]
pub extern "C" fn ube_oauth2_token_type(handle: Handle) -> *mut c_char {
    token_text(handle, |t| Some(&t.kind))
}
#[no_mangle]
pub extern "C" fn ube_oauth2_token_refresh(handle: Handle) -> *mut c_char {
    token_text(handle, |t| t.refresh.as_deref())
}
#[no_mangle]
pub extern "C" fn ube_oauth2_token_scope(handle: Handle) -> *mut c_char {
    token_text(handle, |t| t.scope.as_deref())
}
#[no_mangle]
pub extern "C" fn ube_oauth2_token_expires(handle: Handle) -> i64 {
    run(|| {
        registry()
            .lock()
            .unwrap()
            .tokens
            .get(&(handle as usize))
            .map(|t| t.expires)
            .ok_or_else(|| "invalid or closed OAuth2Token".into())
    })
}
#[no_mangle]
pub extern "C" fn ube_oauth2_token_close(handle: Handle) {
    registry().lock().unwrap().tokens.remove(&(handle as usize));
}
pub(crate) fn cleanup() {
    let mut r = registry().lock().unwrap();
    r.clients.clear();
    r.claims.clear();
    r.tokens.clear();
    r.pending.clear();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        ffi::CString,
        io::{BufRead, BufReader, Write},
        net::TcpListener,
        sync::mpsc,
    };
    const SECRET: &str = "test-only-32-byte-secret-0123456789";
    fn claims() -> Value {
        serde_json::json!({"sub":"user-123", "iss":"ube-test", "aud":"ube-api", "exp":4102444800u64})
    }
    fn c(value: &str) -> CString {
        CString::new(value).unwrap()
    }
    fn read(ptr: *mut c_char) -> String {
        assert!(!ptr.is_null());
        unsafe { CStr::from_ptr(ptr) }.to_str().unwrap().into()
    }
    fn no_error() {
        assert_eq!(crate::exceptions::take_pending(), 0);
    }
    fn rejected() {
        assert_ne!(crate::exceptions::take_pending(), 0);
    }
    #[test]
    fn jwt_hs256_validates_signature_algorithm_and_registered_claims() {
        let claims = claims();
        let token = sign(&claims.to_string(), SECRET, "HS256").unwrap();
        assert_eq!(
            verify(&token, SECRET, "HS256", "ube-test", "ube-api").unwrap(),
            claims
        );
        for (key, alg, issuer, audience) in [
            (
                "other-test-32-byte-secret-0123456789",
                "HS256",
                "ube-test",
                "ube-api",
            ),
            (SECRET, "none", "ube-test", "ube-api"),
            (SECRET, "HS256", "wrong", "ube-api"),
            (SECRET, "HS256", "ube-test", "wrong"),
        ] {
            assert!(verify(&token, key, alg, issuer, audience).is_err());
        }
        let mut parts = token.split('.').map(str::to_owned).collect::<Vec<_>>();
        parts[1] = URL_SAFE_NO_PAD
            .encode(br#"{"sub":"admin","iss":"ube-test","aud":"ube-api","exp":4102444800}"#);
        assert!(verify(&parts.join("."), SECRET, "HS256", "ube-test", "ube-api").is_err());
        for patch in [
            serde_json::json!({"exp":1}),
            serde_json::json!({"exp":null}),
            serde_json::json!({"nbf":4102444799u64}),
            serde_json::json!({"nbf":"4102444799"}),
            serde_json::json!({"exp":"4102444800"}),
            serde_json::json!({"iat":4102444799u64}),
            serde_json::json!({"iss":null}),
            serde_json::json!({"aud":null}),
        ] {
            let mut bad = claims.clone();
            for (key, value) in patch.as_object().unwrap() {
                bad[key] = value.clone();
            }
            let token = jsonwebtoken::encode(
                &Header::new(Algorithm::HS256),
                &bad,
                &EncodingKey::from_secret(SECRET.as_bytes()),
            )
            .unwrap();
            assert!(
                verify(&token, SECRET, "HS256", "ube-test", "ube-api").is_err(),
                "accepted {bad}"
            );
        }
        assert!(sign(&claims.to_string(), "short", "HS256").is_err());
        assert!(sign("[]", SECRET, "HS256").is_err());
        let mut large = claims.clone();
        large["blob"] = Value::String("x".repeat(MAX_TOKEN * 3 / 4));
        assert!(sign(&large.to_string(), SECRET, "HS256").is_err());
        assert!(verify("not-a-token", SECRET, "HS256", "ube-test", "ube-api").is_err());
    }
    #[test]
    fn jwt_rs256_and_claim_handle_lifetime() {
        // These keys are generated fixtures, never application credentials.
        let private = include_str!("../tests/fixtures/jwt-test-private.pem");
        let public = include_str!("../tests/fixtures/jwt-test-public.pem");
        let token = sign(&claims().to_string(), private, "RS256").unwrap();
        assert_eq!(
            verify(&token, public, "RS256", "ube-test", "ube-api").unwrap()["sub"],
            "user-123"
        );
        assert!(verify(&token, public, "HS256", "ube-test", "ube-api").is_err());
        assert!(sign(&claims().to_string(), public, "RS256").is_err());
        let handle = ube_jwt_verify(
            c(&token).as_ptr(),
            c(public).as_ptr(),
            c("RS256").as_ptr(),
            c("ube-test").as_ptr(),
            c("ube-api").as_ptr(),
        );
        no_error();
        assert_eq!(
            read(ube_jwt_claims_get(handle, c("sub").as_ptr())),
            "user-123"
        );
        assert!(ube_jwt_claims_get(handle, c("missing").as_ptr()).is_null());
        no_error();
        assert!(ube_jwt_claims_get(handle, c("exp").as_ptr()).is_null());
        rejected();
        assert_eq!(
            serde_json::from_str::<Value>(&read(ube_jwt_claims_json(handle))).unwrap(),
            claims()
        );
        ube_jwt_claims_close(handle);
        assert!(ube_jwt_claims_json(handle).is_null());
        rejected();
    }
    struct Captured {
        headers: String,
        fields: HashMap<String, String>,
    }
    fn mock(
        replies: Vec<(u16, &'static str)>,
    ) -> (
        String,
        mpsc::Receiver<Captured>,
        std::thread::JoinHandle<()>,
    ) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let (tx, rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            for (status, body) in replies {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut reader = BufReader::new(&mut stream);
                let mut headers = String::new();
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" || line.is_empty() {
                        break;
                    }
                    headers.push_str(&line);
                }
                let length: usize = headers
                    .lines()
                    .filter_map(|line| line.split_once(':'))
                    .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                    .unwrap()
                    .1
                    .trim()
                    .parse()
                    .unwrap();
                let mut bytes = vec![0; length];
                reader.read_exact(&mut bytes).unwrap();
                drop(reader);
                tx.send(Captured {
                    headers,
                    fields: url::form_urlencoded::parse(&bytes)
                        .map(|(k, v)| (k.into_owned(), v.into_owned()))
                        .collect(),
                })
                .unwrap();
                write!(stream, "HTTP/1.1 {status} Response\r\nContent-Type: application/json\r\nLocation: http://127.0.0.1:1/must-not-follow\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
        });
        (format!("http://{address}/token"), rx, worker)
    }
    fn create(token: &str, secret: &str) -> Handle {
        let handle = ube_oauth2_create(
            c("https://provider.example/authorize?prompt=consent").as_ptr(),
            c(token).as_ptr(),
            c("client/id").as_ptr(),
            c(secret).as_ptr(),
            c("https://client.example:443/callback").as_ptr(),
        );
        no_error();
        assert!(!handle.is_null());
        handle
    }
    #[test]
    fn oauth_pkce_state_binding_exchange_refresh_and_replay() {
        assert_eq!(
            challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        let (url, received, worker) = mock(vec![
            (
                200,
                r#"{"access_token":"access","token_type":"Bearer","refresh_token":"refresh","expires_in":3600,"scope":"read write"}"#,
            ),
            (200, r#"{"access_token":"updated","token_type":"Bearer"}"#),
        ]);
        let handle = create(&url, "a+b");
        let store = crate::sessions::Store::default();
        let first = crate::sessions::begin(store.clone(), "").unwrap();
        let url = read(ube_oauth2_authorize(
            handle,
            first as Handle,
            c("read write").as_ptr(),
        ));
        no_error();
        let params: HashMap<_, _> = Url::parse(&url)
            .unwrap()
            .query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        assert_eq!(params["code_challenge_method"], "S256");
        assert_eq!(params["response_type"], "code");
        assert_eq!(
            params["redirect_uri"],
            "https://client.example:443/callback"
        );
        let cookie = crate::sessions::finish(first, true).unwrap();
        let session = crate::sessions::begin(store, cookie.split(';').next().unwrap()).unwrap();
        let other = crate::sessions::begin(Default::default(), "").unwrap();
        assert!(ube_oauth2_exchange(
            handle,
            other as Handle,
            c("code").as_ptr(),
            c(&params["state"]).as_ptr()
        )
        .is_null());
        rejected();
        crate::sessions::finish(other, false);
        let token = ube_oauth2_exchange(
            handle,
            session as Handle,
            c("a+/=&").as_ptr(),
            c(&params["state"]).as_ptr(),
        );
        no_error();
        assert!(!token.is_null());
        assert_eq!(read(ube_oauth2_token_access(token)), "access");
        assert_eq!(ube_oauth2_token_expires(token), 3600);
        let request = received.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(request.fields["code"], "a+/=&");
        assert_eq!(
            request.fields["redirect_uri"],
            "https://client.example:443/callback"
        );
        assert_eq!(
            challenge(&request.fields["code_verifier"]),
            params["code_challenge"]
        );
        use base64::engine::general_purpose::STANDARD;
        assert!(request.headers.to_ascii_lowercase().contains(
            &format!(
                "authorization: basic {}",
                STANDARD.encode("client%2Fid:a%2Bb")
            )
            .to_ascii_lowercase()
        ));
        assert!(ube_oauth2_exchange(
            handle,
            session as Handle,
            c("code").as_ptr(),
            c(&params["state"]).as_ptr()
        )
        .is_null());
        rejected();
        let refreshed = ube_oauth2_refresh(handle, c("refresh").as_ptr());
        no_error();
        assert_eq!(read(ube_oauth2_token_access(refreshed)), "updated");
        assert_eq!(read(ube_oauth2_token_refresh(refreshed)), "refresh");
        assert_eq!(ube_oauth2_token_expires(refreshed), -1);
        let request = received.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(request.fields["grant_type"], "refresh_token");
        worker.join().unwrap();
        ube_oauth2_token_close(token);
        ube_oauth2_token_close(refreshed);
        ube_oauth2_close(handle);
        crate::sessions::finish(session, true);
        assert!(ube_oauth2_token_access(token).is_null());
        rejected();
    }
    #[test]
    fn oauth_public_clients_errors_redirects_and_expired_state() {
        for url in [
            "http://provider.example/token",
            "https://user:pass@provider.example/token",
            "https://provider.example/token#fragment",
        ] {
            assert!(endpoint(url).is_err());
        }
        let (url, received, worker) = mock(vec![
            (
                200,
                r#"{"access_token":"public-access","token_type":"bearer"}"#,
            ),
            (302, ""),
            (
                200,
                r#"{"error":"invalid_client","error_description":"do not disclose"}"#,
            ),
        ]);
        let handle = create(&url, "");
        let config = client(handle).unwrap();
        let token = token_request(
            config.clone(),
            vec![("grant_type".into(), "refresh_token".into())],
        )
        .unwrap();
        assert_eq!(token.access, "public-access");
        let request = received.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(request.fields["client_id"], "client/id");
        assert!(!request
            .headers
            .to_ascii_lowercase()
            .contains("authorization:"));
        assert!(
            matches!(token_request(config.clone(), vec![]), Err(message) if message == "OAuth token endpoint rejected the request")
        );
        received.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(
            matches!(token_request(config, vec![]), Err(message) if message == "OAuth token endpoint returned an error")
        );
        received.recv_timeout(Duration::from_secs(2)).unwrap();
        worker.join().unwrap();
        let session = crate::sessions::begin(Default::default(), "").unwrap();
        let url = read(ube_oauth2_authorize(
            handle,
            session as Handle,
            c("read").as_ptr(),
        ));
        let state = Url::parse(&url)
            .unwrap()
            .query_pairs()
            .find(|(key, _)| key == "state")
            .unwrap()
            .1
            .into_owned();
        registry()
            .lock()
            .unwrap()
            .pending
            .get_mut(&state)
            .unwrap()
            .expires = Instant::now() - Duration::from_secs(1);
        assert!(ube_oauth2_exchange(
            handle,
            session as Handle,
            c("code").as_ptr(),
            c(&state).as_ptr()
        )
        .is_null());
        rejected();
        ube_oauth2_close(handle);
        crate::sessions::finish(session, false);
    }
}

#[cfg(test)]
mod tls_tests {
    use super::*;
    #[test]
    fn token_endpoint_rejects_untrusted_tls_certificates() {
        let certificate = rcgen::generate_simple_self_signed(vec!["127.0.0.1".into()]).unwrap();
        let config = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![certificate.cert.der().clone()],
            rustls::pki_types::PrivatePkcs8KeyDer::from(certificate.key_pair.serialize_der())
                .into(),
        )
        .unwrap();
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let worker = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let connection = rustls::ServerConnection::new(Arc::new(config)).unwrap();
            let mut stream = rustls::StreamOwned::new(connection, stream);
            let mut byte = [0];
            assert!(stream.read(&mut byte).is_err());
        });
        let config = Client {
            authorize: Url::parse("https://provider.example/authorize").unwrap(),
            token: Url::parse(&format!("https://{address}/token")).unwrap(),
            client_id: "id".into(),
            secret: String::new(),
            redirect: "https://client.example/callback".into(),
        };
        assert!(
            matches!(token_request(Arc::new(config), vec![]), Err(message) if message == "OAuth token request failed")
        );
        worker.join().unwrap();
    }
}
