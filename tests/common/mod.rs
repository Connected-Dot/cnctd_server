//! Shared harness for the behavioral tests.
//!
//! cnctd_server 0.6.x keeps its connection table and server info in process-wide
//! statics, so there can be exactly one server per test *process*. Each file
//! under `tests/` is its own binary, so each file starts its own server (on its
//! own port) once, on a dedicated thread with its own runtime, and every test in
//! the file talks to that one server over real sockets.
//!
//! The routers here record what the framework hands them, so tests can assert
//! on the exact request the app would have seen — method, path, parsed data,
//! raw bytes, headers, auth token, connection id, ip address.

#![allow(dead_code)]

use std::{
    collections::HashMap,
    future::Future,
    net::TcpStream,
    pin::Pin,
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};

use cnctd_server::{
    router::{
        error::{ErrorCode, ErrorResponse},
        response::{BinaryResponse, SuccessCode, SuccessResponse},
        HttpMethod, RestRouterFunction, SocketRouterFunction,
    },
    Bytes,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// One call the REST router received, by dispatch stage.
#[derive(Debug, Clone)]
pub struct Call {
    /// "raw" | "binary" | "route" | "redirect"
    pub stage: &'static str,
    pub method: String,
    pub path: String,
    pub data: Value,
    pub raw_body: Option<Vec<u8>>,
    pub headers: Option<HashMap<String, String>>,
    pub auth_token: Option<String>,
    pub connection_id: Option<String>,
    pub ip_address: Option<String>,
}

#[derive(Clone, Default)]
pub struct RecordingRouter {
    pub calls: Arc<Mutex<Vec<Call>>>,
}

impl RecordingRouter {
    pub fn record(&self, call: Call) {
        self.calls.lock().unwrap().push(call);
    }

    /// Calls whose path contains `needle`, in arrival order.
    pub fn calls_for(&self, needle: &str) -> Vec<Call> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|c| c.path.contains(needle))
            .cloned()
            .collect()
    }
}

fn boxed<T: Send + 'static>(v: T) -> Pin<Box<dyn Future<Output = T> + Send>> {
    Box::pin(async move { v })
}

/// Path families the recording router understands. Everything is keyed off
/// the path so tests can use unique paths and run in parallel.
///
/// `/api/echo/*`           -> 200 envelope whose data is everything received
/// `/api/status/created`   -> 201 Created envelope with msg + data
/// `/api/status/msg`       -> 200 envelope with msg only
/// `/api/fail/bad-request` -> 400, `/api/fail/unauthorized` -> 401,
/// `/api/fail/internal`    -> 500, `/api/fail/default` -> ErrorResponse::new(None, ..)
/// `/api/bin/*`            -> route_binary answers with octet-stream "BIN:<method>"
/// `/api/bin-status/*`     -> route_binary answers with a 202
/// `/api/bin-fail/*`       -> route_binary answers Err(forbidden)
/// `/api/raw/*`            -> route_with_raw_body answers with a JSON body
///                            carrying the raw bytes and lowercase headers
/// `/api/raw-fail/*`       -> route_with_raw_body answers Err(unauthorized)
/// `/api/order/all`        -> raw answers Some (so binary/route never run)
/// `/api/order/skip-raw`   -> raw None, binary Some
/// `/api/order/json`       -> raw None, binary None, route answers
/// `/redirect/ok/*`        -> "https://example.com/landed?from=<path>"
/// `/redirect/bad/*`       -> an unparseable URL
impl RestRouterFunction for RecordingRouter {
    fn route(
        &self,
        method: HttpMethod,
        path: String,
        data: Value,
        auth_token: Option<String>,
        connection_id: Option<String>,
        ip_address: Option<String>,
    ) -> Pin<Box<dyn Future<Output = Result<SuccessResponse, ErrorResponse>> + Send>> {
        let method_s = format!("{:?}", method);
        self.record(Call {
            stage: "route",
            method: method_s.clone(),
            path: path.clone(),
            data: data.clone(),
            raw_body: None,
            headers: None,
            auth_token: auth_token.clone(),
            connection_id: connection_id.clone(),
            ip_address: ip_address.clone(),
        });

        let out = if path.starts_with("/api/echo/") || path.starts_with("/api/order/json") {
            Ok(SuccessResponse::new(
                None,
                None,
                Some(json!({
                    "method": method_s,
                    "path": path,
                    "data": data,
                    "auth_token": auth_token,
                    "connection_id": connection_id,
                    "ip_address": ip_address,
                })),
            ))
        } else if path == "/api/status/created" {
            Ok(SuccessResponse::new(
                Some(SuccessCode::Created),
                Some("made".into()),
                Some(json!({"id": 7})),
            ))
        } else if path == "/api/status/msg" {
            Ok(SuccessResponse::new(None, Some("just a message".into()), None))
        } else if path == "/api/fail/bad-request" {
            Err(ErrorResponse::new(Some(ErrorCode::BadRequest), Some("missing thing".into())))
        } else if path == "/api/fail/unauthorized" {
            Err(ErrorResponse::new(Some(ErrorCode::Unauthorized), Some("a token is required".into())))
        } else if path == "/api/fail/internal" {
            Err(ErrorResponse::new(Some(ErrorCode::InternalServerError), Some("internal failure".into())))
        } else if path == "/api/fail/default" {
            Err(ErrorResponse::new(None, Some("nothing here".into())))
        } else {
            Err(ErrorResponse::new(Some(ErrorCode::NotFound), Some(format!("unrouted: {}", path))))
        };
        boxed(out)
    }

    fn route_redirect(
        &self,
        path: String,
        data: Value,
        auth_token: Option<String>,
        connection_id: Option<String>,
    ) -> Pin<Box<dyn Future<Output = String> + Send>> {
        self.record(Call {
            stage: "redirect",
            method: "GET".into(),
            path: path.clone(),
            data,
            raw_body: None,
            headers: None,
            auth_token,
            connection_id,
            ip_address: None,
        });
        let url = if path.starts_with("/redirect/ok/") {
            format!("https://example.com/landed?from={}", path.trim_start_matches("/redirect/ok/"))
        } else {
            "this is not a url".to_string()
        };
        boxed(url)
    }

    fn route_binary(
        &self,
        method: HttpMethod,
        path: String,
        data: Value,
        auth_token: Option<String>,
        connection_id: Option<String>,
        ip_address: Option<String>,
    ) -> Pin<Box<dyn Future<Output = Result<Option<BinaryResponse>, ErrorResponse>> + Send>> {
        let method_s = format!("{:?}", method);
        self.record(Call {
            stage: "binary",
            method: method_s.clone(),
            path: path.clone(),
            data,
            raw_body: None,
            headers: None,
            auth_token,
            connection_id,
            ip_address,
        });
        let out = if path.starts_with("/api/bin/") || path == "/api/order/skip-raw" {
            Ok(Some(BinaryResponse::octet_stream(format!("BIN:{}", method_s).into_bytes())))
        } else if path.starts_with("/api/bin-status/") {
            Ok(Some(BinaryResponse::new(b"accepted".to_vec(), "text/plain").with_status(202)))
        } else if path.starts_with("/api/bin-fail/") {
            Err(ErrorResponse::new(Some(ErrorCode::Forbidden), Some("no binary for you".into())))
        } else {
            Ok(None)
        };
        boxed(out)
    }

    fn route_with_raw_body(
        &self,
        method: HttpMethod,
        path: String,
        body_bytes: Bytes,
        data: Value,
        headers: HashMap<String, String>,
        auth_token: Option<String>,
        connection_id: Option<String>,
        ip_address: Option<String>,
    ) -> Pin<Box<dyn Future<Output = Result<Option<BinaryResponse>, ErrorResponse>> + Send>> {
        let method_s = format!("{:?}", method);
        self.record(Call {
            stage: "raw",
            method: method_s,
            path: path.clone(),
            data: data.clone(),
            raw_body: Some(body_bytes.to_vec()),
            headers: Some(headers.clone()),
            auth_token,
            connection_id,
            ip_address,
        });
        let out = if path.starts_with("/api/raw/") || path == "/api/order/all" {
            let body = json!({
                "raw": String::from_utf8_lossy(&body_bytes),
                "data": data,
                "headers": headers,
            });
            Ok(Some(BinaryResponse::new(serde_json::to_vec(&body).unwrap(), "application/json")))
        } else if path.starts_with("/api/raw-fail/") {
            Err(ErrorResponse::new(Some(ErrorCode::Unauthorized), Some("bad signature".into())))
        } else {
            Ok(None)
        };
        boxed(out)
    }
}

/// The wire shape the socket tests speak. Anything that doesn't parse as this
/// is dropped by the framework, which is itself a behavior under test.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestMsg {
    pub channel: String,
    pub data: Option<Value>,
}

#[derive(Clone, Default)]
pub struct RecordingSocketRouter {
    pub calls: Arc<Mutex<Vec<(TestMsg, String)>>>,
}

/// `channel == "echo"` answers with the message and the connection id it came
/// from; everything else is swallowed (`None`).
impl SocketRouterFunction<TestMsg, Value> for RecordingSocketRouter {
    fn route(&self, msg: TestMsg, connection_id: String) -> Pin<Box<dyn Future<Output = Option<Value>> + Send>> {
        self.calls.lock().unwrap().push((msg.clone(), connection_id.clone()));
        let out = if msg.channel == "echo" {
            Some(json!({"echo": msg, "connection_id": connection_id}))
        } else {
            None
        };
        boxed(out)
    }
}

/// Run `start` (the server future) on its own thread + runtime, then block
/// until the port accepts TCP connections. Startup includes a public-IP
/// lookup over the network, so the wait is generous.
pub fn spawn_server<F, Fut>(port: u16, start: F)
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: Future<Output = anyhow::Result<()>>,
{
    std::thread::Builder::new()
        .name(format!("cnctd-server-{}", port))
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("server runtime");
            if let Err(e) = rt.block_on(start()) {
                eprintln!("server on {} exited with error: {:?}", port, e);
            }
        })
        .expect("server thread");

    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return;
        }
        assert!(Instant::now() < deadline, "server on port {} never came up", port);
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// A reqwest client that does not follow redirects (so tests can read the
/// 302 the framework emits) and has no proxy surprises.
pub fn http() -> reqwest::Client {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .build()
        .unwrap()
}

pub fn base(port: u16) -> String {
    format!("http://127.0.0.1:{}", port)
}

/// Poll `check` until it returns Some or `timeout` passes.
pub async fn eventually<T, F, Fut>(timeout: Duration, mut check: F) -> Option<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Option<T>>,
{
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(v) = check().await {
            return Some(v);
        }
        if Instant::now() >= deadline {
            return None;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

pub static ONCE: OnceLock<()> = OnceLock::new();
