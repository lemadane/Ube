use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::sync::{Arc, Mutex, RwLock};
use std::path::PathBuf;

use bytes::Bytes;
use sha2::{Digest, Sha256};
use tokio::runtime::Runtime;

lazy_static_runtime: once_cell or global Tokio Runtime management
static TOKIO_RUNTIME: std::sync::OnceLock<Arc<Runtime>> = std::sync::OnceLock::new();

pub fn get_runtime() -> &'static Arc<Runtime> {
    TOKIO_RUNTIME.get_or_init(|| {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("Failed to initialize Tokio runtime for Talas QUIC/HTTP3");
        Arc::new(rt)
    })
}

/// 0-RTT Early Data Policy configuration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZeroRttPolicy {
    /// 0-RTT only allowed for safe, idempotent methods (GET, HEAD)
    SafeMethodsOnly,
    /// 0-RTT strictly disabled for all requests
    Disabled,
    /// 0-RTT enabled for all methods (not recommended for production)
    AllowAll,
}

/// Layer 1: Network & TLS Configuration
pub struct TalasNetworkConfig {
    pub cert_path: Option<String>,
    pub key_path: Option<String>,
    pub alpn_protocols: Vec<Vec<u8>>,
}

impl Default for TalasNetworkConfig {
    fn default() -> Self {
        Self {
            cert_path: None,
            key_path: None,
            alpn_protocols: vec![b"h3".to_vec()],
        }
    }
}

/// Layer 2: QUIC Transport Server Abstraction
pub struct TalasQuicServer {
    pub bind_port: u16,
    pub zero_rtt_policy: ZeroRttPolicy,
    pub active_connections: Arc<Mutex<usize>>,
}

impl TalasQuicServer {
    pub fn new(port: u16) -> Self {
        Self {
            bind_port: port,
            zero_rtt_policy: ZeroRttPolicy::SafeMethodsOnly,
            active_connections: Arc::new(Mutex::new(0)),
        }
    }
}

/// Layer 3: Web Server Engine (HTTP/1.1, HTTP/2, HTTP/3, WebTransport)
pub struct TalasHttp3Engine {
    pub port: u16,
    pub enabled: bool,
    pub zero_rtt_policy: ZeroRttPolicy,
    pub cert_file: String,
    pub key_file: String,
    pub routes: Arc<RwLock<HashMap<String, i64>>>,
}

impl TalasHttp3Engine {
    pub fn new() -> Self {
        Self {
            port: 443,
            enabled: false,
            zero_rtt_policy: ZeroRttPolicy::SafeMethodsOnly,
            cert_file: String::new(),
            key_file: String::new(),
            routes: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

/// Helper: Compute SHA-256 for file streaming verification
pub fn compute_file_sha256(path: &str) -> Result<String, std::io::Error> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let count = std::io::Read::read(&mut file, &mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(hex::encode(hasher.finalize()))
}
