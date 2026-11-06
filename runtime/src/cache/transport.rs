use super::{store::Store, Result};
use ring::hmac;
use serde_json::{json, Value};
use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
const MAX_FRAME: usize = 8 * 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(2);
const AUTH_CONTEXT: &[u8] = b"Ube cache protocol v1";
pub(super) fn address(value: &str) -> Result<SocketAddr> {
    let address: SocketAddr = value
        .parse()
        .map_err(|_| "Cache address must be an IP address and port, such as 127.0.0.1:6380")?;
    if address.port() == 0 {
        return Err("Cache port must be nonzero".into());
    }
    Ok(address)
}

pub(super) fn token_valid(token: &str) -> Result<()> {
    if !(16..=4096).contains(&token.len()) {
        Err("Cache authentication token must contain 16 to 4096 bytes".into())
    } else {
        Ok(())
    }
}
fn read_frame(stream: &mut TcpStream) -> Result<Value> {
    let deadline = Instant::now() + TIMEOUT;
    fn receive(stream: &mut TcpStream, bytes: &mut [u8], deadline: Instant) -> Result<()> {
        let mut position = 0;
        while position < bytes.len() {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .ok_or("Cache request timed out")?;
            stream
                .set_read_timeout(Some(remaining))
                .map_err(|e| e.to_string())?;
            let n = stream
                .read(&mut bytes[position..])
                .map_err(|e| format!("Cache read failed: {e}"))?;
            if n == 0 {
                return Err("Cache connection closed before a complete message".into());
            }
            position += n;
        }
        Ok(())
    }
    let mut length = [0; 4];
    receive(stream, &mut length, deadline)?;
    let length = u32::from_be_bytes(length) as usize;
    if length == 0 || length > MAX_FRAME {
        return Err("Invalid cache message size".into());
    }
    let mut bytes = vec![0; length];
    receive(stream, &mut bytes, deadline)?;
    serde_json::from_slice(&bytes).map_err(|_| "Invalid cache JSON message".into())
}
fn write_frame(stream: &mut TcpStream, value: &Value) -> Result<()> {
    let bytes = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    if bytes.len() > MAX_FRAME {
        return Err("Cache message exceeds the wire limit".into());
    }
    let deadline = Instant::now() + TIMEOUT;
    for part in [&(bytes.len() as u32).to_be_bytes()[..], &bytes[..]] {
        let mut offset = 0;
        while offset < part.len() {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .ok_or("Cache write timed out")?;
            stream
                .set_write_timeout(Some(remaining))
                .map_err(|e| e.to_string())?;
            let count = stream
                .write(&part[offset..])
                .map_err(|e| format!("Cache write failed: {e}"))?;
            if count == 0 {
                return Err("Cache connection closed during write".into());
            }
            offset += count;
        }
    }
    Ok(())
}

pub(super) fn request(
    address: SocketAddr,
    token: &str,
    op: &str,
    key: &str,
    value: &str,
    number: i64,
) -> Result<Value> {
    if key.is_empty() || key.len() > 1024 || value.len() > super::store::MAX_VALUE {
        return Err("Cache key or value exceeds the supported size".into());
    }
    let mut stream = TcpStream::connect_timeout(&address, TIMEOUT)
        .map_err(|e| format!("Cannot connect to cache: {e}"))?;
    write_frame(
        &mut stream,
        &json!({"token":token,"op":op,"key":key,"value":value,"number":number}),
    )?;
    let response = read_frame(&mut stream)?;
    if let Some(error) = response.get("error").and_then(Value::as_str) {
        return Err(error.into());
    }
    response
        .get("result")
        .cloned()
        .ok_or("Invalid cache response".into())
}
fn process(stream: &mut TcpStream, store: &Mutex<Store>, secret: &hmac::Key) -> Result<Value> {
    let message = read_frame(stream)?;
    let token = message
        .get("token")
        .and_then(Value::as_str)
        .ok_or("Cache authentication failed")?;
    token_valid(token).map_err(|_| "Cache authentication failed")?;
    let provided = hmac::sign(
        &hmac::Key::new(hmac::HMAC_SHA256, token.as_bytes()),
        AUTH_CONTEXT,
    );
    hmac::verify(secret, AUTH_CONTEXT, provided.as_ref())
        .map_err(|_| "Cache authentication failed")?;
    let op = message
        .get("op")
        .and_then(Value::as_str)
        .ok_or("Missing cache operation")?;
    let key = message
        .get("key")
        .and_then(Value::as_str)
        .ok_or("Missing cache key")?;
    let value = message
        .get("value")
        .and_then(Value::as_str)
        .ok_or("Missing cache value")?;
    let number = message
        .get("number")
        .and_then(Value::as_i64)
        .ok_or("Invalid cache numeric argument")?;
    store.lock().unwrap().execute(op, key, value, number)
}
pub(super) struct Server {
    stop: Arc<AtomicBool>,
    accept: Option<JoinHandle<()>>,
    workers: Vec<JoinHandle<()>>,
}
impl Server {
    pub(super) fn start(bind: SocketAddr, token: &str, store: Arc<Mutex<Store>>) -> Result<Self> {
        token_valid(token)?;
        let listener: TcpListener = (|| -> std::io::Result<_> {
            let socket = socket2::Socket::new(
                if bind.is_ipv6() {
                    socket2::Domain::IPV6
                } else {
                    socket2::Domain::IPV4
                },
                socket2::Type::STREAM,
                Some(socket2::Protocol::TCP),
            )?;
            socket.set_reuse_address(true)?;
            socket.bind(&bind.into())?;
            socket.listen(128)?;
            Ok(socket.into())
        })()
        .map_err(|e| format!("Cannot bind cache listener: {e}"))?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let secret = Arc::new(hmac::Key::new(hmac::HMAC_SHA256, token.as_bytes()));
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::sync_channel::<TcpStream>(64);
        let rx = Arc::new(Mutex::new(rx));
        let mut server = Self {
            stop: stop.clone(),
            accept: None,
            workers: vec![],
        };
        for _ in 0..4 {
            let (rx, stop, secret, store) =
                (rx.clone(), stop.clone(), secret.clone(), store.clone());
            let worker = thread::Builder::new()
                .name("ube-cache-worker".into())
                .spawn(move || {
                    while !stop.load(Ordering::Acquire) {
                        let received = rx.lock().unwrap().recv_timeout(Duration::from_millis(50));
                        match received {
                            Ok(mut stream) => {
                                if stop.load(Ordering::Acquire) {
                                    break;
                                }
                                let response = match process(&mut stream, &store, &secret) {
                                    Ok(value) => json!({"result":value}),
                                    Err(error) => json!({"error":error}),
                                };
                                let _ = write_frame(&mut stream, &response);
                            }
                            Err(mpsc::RecvTimeoutError::Timeout) => {}
                            Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        }
                    }
                })
                .map_err(|e| e.to_string())?;
            server.workers.push(worker);
        }
        server.accept = Some(
            thread::Builder::new()
                .name("ube-cache-listener".into())
                .spawn(move || {
                    while !stop.load(Ordering::Acquire) {
                        match listener.accept() {
                            Ok((stream, _)) => {
                                let _ = tx.try_send(stream);
                            }
                            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                                thread::sleep(Duration::from_millis(5))
                            }
                            Err(_) => break,
                        }
                    }
                })
                .map_err(|e| e.to_string())?,
        );
        Ok(server)
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(accept) = self.accept.take() {
            let _ = accept.join();
        }
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}
