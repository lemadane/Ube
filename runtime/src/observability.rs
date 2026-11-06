//! Opt-in, loopback-only management listener. Never serializes application state.
use std::{
    collections::BTreeMap,
    ffi::{c_char, CStr},
    io::{Read, Write},
    net::{Ipv4Addr, TcpListener, TcpStream},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, OnceLock,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Counter,
    Gauge,
    Summary,
}
struct Measurement {
    kind: Kind,
    value: f64,
    count: u64,
}
struct Listener {
    stop: Arc<AtomicBool>,
    thread: JoinHandle<()>,
}
struct State {
    started: Instant,
    ready: AtomicBool,
    metrics: Mutex<BTreeMap<String, Measurement>>,
    listener: Mutex<Option<Listener>>,
}
fn state() -> &'static State {
    static STATE: OnceLock<State> = OnceLock::new();
    STATE.get_or_init(|| State {
        started: Instant::now(),
        ready: AtomicBool::new(false),
        metrics: Mutex::new(BTreeMap::new()),
        listener: Mutex::new(None),
    })
}
fn fail(message: impl Into<String>) {
    crate::exceptions::set_pending(crate::exceptions::generic(message.into()));
}
fn names(name: &str, kind: Kind) -> Vec<String> {
    if kind == Kind::Summary {
        vec![name.into(), format!("{name}_count"), format!("{name}_sum")]
    } else {
        vec![name.into()]
    }
}
fn update(name: &str, kind: Kind, value: f64) -> Result<(), String> {
    if name.is_empty()
        || name.len() > 128
        || name.starts_with("ube_")
        || !name
            .bytes()
            .enumerate()
            .all(|(i, c)| c.is_ascii_alphabetic() || c == b'_' || (i > 0 && c.is_ascii_digit()))
    {
        return Err(
            "Metric names must be ASCII identifiers of at most 128 bytes; ube_ is reserved".into(),
        );
    }
    if !value.is_finite() || (kind != Kind::Gauge && value < 0.0) {
        return Err(
            "Metric values must be finite; counters and timings must be nonnegative".into(),
        );
    }
    let mut metrics = state().metrics.lock().unwrap();
    if let Some(existing) = metrics.get(name) {
        if existing.kind != kind {
            return Err("Metric already registered with a different type".into());
        }
    } else {
        if metrics.len() >= 1024 {
            return Err("Application metric limit (1024) reached".into());
        }
        let exported = names(name, kind);
        if metrics.iter().any(|(key, metric)| {
            key == name || names(key, metric.kind).iter().any(|n| exported.contains(n))
        }) {
            return Err("Metric name conflicts with an existing exported series".into());
        }
    }
    let metric = metrics.entry(name.into()).or_insert(Measurement {
        kind,
        value: 0.0,
        count: 0,
    });
    let next = if kind == Kind::Gauge {
        value
    } else {
        metric.value + value
    };
    if !next.is_finite() || metric.count == u64::MAX {
        return Err("Metric overflow".into());
    }
    metric.value = next;
    metric.count += 1;
    Ok(())
}
fn record(name: *const c_char, kind: Kind, value: f64) {
    let result = if name.is_null() {
        Err("Metric name cannot be null".into())
    } else {
        unsafe { CStr::from_ptr(name) }
            .to_str()
            .map_err(|_| "Metric name must be UTF-8".into())
            .and_then(|name| update(name, kind, value))
    };
    if let Err(error) = result {
        fail(error);
    }
}
#[no_mangle]
pub extern "C" fn ube_metrics_increment(name: *const c_char) {
    record(name, Kind::Counter, 1.0);
}
#[no_mangle]
pub extern "C" fn ube_metrics_add(name: *const c_char, value: f64) {
    record(name, Kind::Counter, value);
}
#[no_mangle]
pub extern "C" fn ube_metrics_gauge(name: *const c_char, value: f64) {
    record(name, Kind::Gauge, value);
}
#[no_mangle]
pub extern "C" fn ube_metrics_record(name: *const c_char, seconds: f64) {
    record(name, Kind::Summary, seconds);
}
#[no_mangle]
pub extern "C" fn ube_health_ready(ready: bool) {
    state().ready.store(ready, Ordering::Release);
}

pub(crate) fn init() {
    let _ = state();
}

fn prometheus() -> String {
    let mut output = format!(
        "# TYPE ube_uptime_seconds gauge\nube_uptime_seconds {}\n# TYPE ube_ready gauge\nube_ready {}\n# TYPE ube_gc_objects gauge\nube_gc_objects {}\n# TYPE ube_gc_reclaimed_total counter\nube_gc_reclaimed_total {}\n",
        state().started.elapsed().as_secs_f64(), u8::from(state().ready.load(Ordering::Acquire)),
        crate::gc::ube_gc_object_count(), crate::gc::ube_gc_reclaimed_count());
    output.push_str(&crate::cache::prometheus());
    let (registered, active) = crate::concurrency::observability_counts();
    output.push_str(&format!("# TYPE ube_tasks_registered gauge\nube_tasks_registered {registered}\n# TYPE ube_tasks_active gauge\nube_tasks_active {active}\n"));
    #[cfg(target_os = "linux")]
    if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
        if let Some(kib) = status.lines().find_map(|line| {
            line.strip_prefix("VmRSS:")
                .and_then(|v| v.split_whitespace().next())
                .and_then(|v| v.parse::<u64>().ok())
        }) {
            output.push_str(&format!("# TYPE ube_process_resident_memory_bytes gauge\nube_process_resident_memory_bytes {}\n", kib * 1024));
        }
    }
    for (name, metric) in state().metrics.lock().unwrap().iter() {
        let kind = match metric.kind {
            Kind::Counter => "counter",
            Kind::Gauge => "gauge",
            Kind::Summary => "summary",
        };
        output.push_str(&format!("# TYPE {name} {kind}\n"));
        if metric.kind == Kind::Summary {
            output.push_str(&format!(
                "{name}_count {}\n{name}_sum {}\n",
                metric.count, metric.value
            ));
        } else {
            output.push_str(&format!("{name} {}\n", metric.value));
        }
    }
    output
}
fn response(method: &str, path: &str) -> (u16, &'static str, String) {
    if method != "GET" {
        return (405, "text/plain", "Method not allowed\n".into());
    }
    match path {
        "/health/live" => (200, "application/json", "{\"status\":\"UP\"}\n".into()),
        "/health/ready" => {
            let ready = state().ready.load(Ordering::Acquire);
            (
                if ready { 200 } else { 503 },
                "application/json",
                format!("{{\"status\":\"{}\"}}\n", if ready { "UP" } else { "DOWN" }),
            )
        }
        "/metrics" => (
            200,
            "text/plain; version=0.0.4; charset=utf-8",
            prometheus(),
        ),
        _ => (404, "text/plain", "Not found\n".into()),
    }
}
fn serve(mut stream: TcpStream) {
    let _ = stream.set_read_timeout(Some(Duration::from_millis(100)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(100)));
    let deadline = Instant::now() + Duration::from_millis(200);
    let mut request = Vec::new();
    let mut chunk = [0; 512];
    while request.len() < 4096 && Instant::now() < deadline {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(n) => request.extend_from_slice(&chunk[..n]),
        }
        if request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
            break;
        }
    }
    if !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
        return;
    }
    let request = String::from_utf8_lossy(&request);
    let mut parts = request.lines().next().unwrap_or("").split_whitespace();
    let method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("");
    let (status, content_type, body) = response(method, path);
    let reason = match status {
        200 => "OK",
        503 => "Service Unavailable",
        405 => "Method Not Allowed",
        _ => "Not Found",
    };
    let allow = if status == 405 { "Allow: GET\r\n" } else { "" };
    let _ = write!(stream, "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n{allow}\r\n{body}", body.len());
}
pub(crate) fn start(port: u16) -> Result<(), String> {
    let mut slot = state().listener.lock().unwrap();
    if slot.is_some() {
        return Err("Observability is already running; stop it before restarting".into());
    }
    let listener = (|| -> std::io::Result<TcpListener> {
        let socket = socket2::Socket::new(
            socket2::Domain::IPV4,
            socket2::Type::STREAM,
            Some(socket2::Protocol::TCP),
        )?;
        socket.set_reuse_address(true)?;
        socket.bind(&std::net::SocketAddr::from((Ipv4Addr::LOCALHOST, port)).into())?;
        socket.listen(128)?;
        Ok(socket.into())
    })()
    .map_err(|error| format!("Cannot start observability: {error}"))?;
    listener
        .set_nonblocking(true)
        .map_err(|error| error.to_string())?;
    let stop = Arc::new(AtomicBool::new(false));
    let signal = stop.clone();
    let thread = thread::Builder::new()
        .name("ube-observability".into())
        .spawn(move || {
            while !signal.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((stream, _)) => serve(stream),
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5))
                    }
                    Err(_) => break,
                }
            }
        })
        .map_err(|error| format!("Cannot start observability thread: {error}"))?;
    *slot = Some(Listener { stop, thread });
    Ok(())
}
#[no_mangle]
pub extern "C" fn ube_observability_start(port: i64) {
    if !(1..=65535).contains(&port) {
        fail("Observability port must be between 1 and 65535");
        return;
    }
    if let Err(error) = start(port as u16) {
        fail(error);
    }
}
#[no_mangle]
pub extern "C" fn ube_observability_stop() {
    // Serialize stop/restart so the old socket is closed before another start.
    let mut slot = state().listener.lock().unwrap();
    if let Some(listener) = slot.take() {
        listener.stop.store(true, Ordering::Release);
        let _ = listener.thread.join();
    }
}
pub fn cleanup() {
    ube_health_ready(false);
    ube_observability_stop();
    state().metrics.lock().unwrap().clear();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn measurements_are_validated_and_exported_without_secrets() {
        assert!(update("test_counter_total", Kind::Counter, 2.0).is_ok());
        assert!(update("test_counter_total", Kind::Counter, 3.0).is_ok());
        assert!(update("test_gauge", Kind::Gauge, -3.0).is_ok());
        assert!(update("test_latency_seconds", Kind::Summary, 0.25).is_ok());
        assert!(update("test_latency_seconds", Kind::Summary, 0.5).is_ok());
        for name in ["", "bad-name", "secret\nvalue", "ube_reserved", "1bad"] {
            assert!(update(name, Kind::Counter, 1.0).is_err());
        }
        assert!(update("test_counter_total", Kind::Gauge, 1.0).is_err());
        assert!(update("test_latency_seconds_sum", Kind::Gauge, 1.0).is_err());
        assert!(update("negative", Kind::Counter, -1.0).is_err());
        assert!(update("nan", Kind::Gauge, f64::NAN).is_err());
        assert!(update("infinite", Kind::Summary, f64::INFINITY).is_err());
        assert!(update("reverse_sum", Kind::Gauge, 1.0).is_ok());
        assert!(update("reverse", Kind::Summary, 1.0).is_err());
        assert!(update("test_latency_seconds_sum", Kind::Summary, 1.0).is_err());
        assert!(update("overflow", Kind::Counter, f64::MAX).is_ok());
        assert!(update("overflow", Kind::Counter, f64::MAX).is_err());
        let writers: Vec<_> = (0..8).map(|_| std::thread::spawn(|| {
            for _ in 0..1000 { update("concurrent_total", Kind::Counter, 1.0).unwrap(); }
        })).collect();
        for writer in writers { writer.join().unwrap(); }
        let output = prometheus();
        assert!(output.contains("concurrent_total 8000\n"));
        assert!(output.contains("test_counter_total 5\n"));
        assert!(output.contains("test_gauge -3\n"));
        assert!(output.contains("test_latency_seconds_count 2\n"));
        assert!(output.contains("test_latency_seconds_sum 0.75\n"));
        assert_eq!(response("POST", "/metrics").0, 405);
        assert_eq!(response("GET", "/env").0, 404);
    }
}
