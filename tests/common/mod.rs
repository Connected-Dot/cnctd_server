//! Shared harness for the behavioral tests.
//!
//! Every test starts its own server on an ephemeral port (`"0"`) bound to
//! loopback, so tests run in parallel with no shared state. The routers
//! record the exact [`Request`] the framework handed them.

#![allow(dead_code)]

use std::{
    future::Future,
    net::{IpAddr, Ipv4Addr},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use bytes::Bytes;
use cnctd_server::{
    async_trait,
    router::{
        error::{ErrorCode, ErrorResponse},
        response::{BinaryResponse, SseEvent, SuccessCode, SuccessResponse},
        NoSocketRouter,
    },
    CnctdServer, Request, Response, RestRouter, RunningServer, ServerConfig, SocketConfig, SocketRouter,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Default)]
pub struct RecordingRouter {
    pub calls: Arc<Mutex<Vec<Request>>>,
}

impl RecordingRouter {
    /// Calls whose path contains `needle`, in arrival order.
    pub fn calls_for(&self, needle: &str) -> Vec<Request> {
        self.calls.lock().unwrap().iter().filter(|r| r.path.contains(needle)).cloned().collect()
    }
}

/// Everything the router received, as JSON, so tests can assert on it from
/// the HTTP side.
pub fn echo(req: &Request) -> Value {
    let (resource, operation) = req.resource_and_operation();
    json!({
        "method": req.method.as_str(),
        "path": req.path,
        "prefix": req.prefix,
        "query": req.query,
        "data": req.data,
        "body": String::from_utf8_lossy(&req.body),
        "headers": req.headers,
        "auth_token": req.auth_token,
        "bearer": req.bearer(),
        "connection_id": req.connection_id,
        "ip_address": req.ip_address,
        "resource": resource,
        "operation": operation,
    })
}

/// Path families the recording router understands:
///
/// `/<prefix>/echo/*`     -> 200 envelope whose data is everything received
/// `/api/status/created`  -> 201 Created with msg + data; `/api/status/accepted` -> 202
/// `/api/status/msg`      -> 200 with msg only
/// `/api/fail/<kind>`     -> bad-request 400, unauthorized 401, conflict 409,
///                           internal 500, default (no code) 404
/// `/api/bin/*`           -> octet-stream "BIN:<method>"
/// `/api/bin-status/*`    -> text/plain with a 202
/// `/api/bin-json/*`      -> a non-envelope JSON body echoing raw body + headers
/// `/api/stream/*`        -> a three-chunk text/plain stream
/// `/api/sse/*`           -> three server-sent events then end
/// `/redirect/ok/*`       -> 302 to https://example.com/landed?from=<rest>
/// `/redirect/bad/*`      -> a redirect to something that is not a URL
#[async_trait]
impl RestRouter for RecordingRouter {
    async fn route(&self, req: Request) -> Result<Response, ErrorResponse> {
        self.calls.lock().unwrap().push(req.clone());
        let path = req.path.as_str();
        let method = req.method.as_str();

        if path.contains("/echo/") {
            return Ok(SuccessResponse::new(None, None, Some(echo(&req))).into());
        }
        if let Some(rest) = path.strip_prefix("/redirect/ok/") {
            return Ok(Response::Redirect(format!("https://example.com/landed?from={}", rest)));
        }
        if path.starts_with("/redirect/bad") {
            return Ok(Response::Redirect("this is not a url".into()));
        }
        if path.starts_with("/api/bin/") {
            return Ok(BinaryResponse::octet_stream(format!("BIN:{}", method).into_bytes()).into());
        }
        if path.starts_with("/api/bin-status/") {
            return Ok(BinaryResponse::new(b"accepted".to_vec(), "text/plain").with_status(202).into());
        }
        if path.starts_with("/api/bin-json/") {
            return Ok(BinaryResponse::json(&json!({
                "raw": String::from_utf8_lossy(&req.body),
                "data": req.data,
                "headers": req.headers,
            }))
            .into());
        }
        if path.starts_with("/api/stream/") {
            let chunks = vec![Ok(Bytes::from("one,")), Ok(Bytes::from("two,")), Ok(Bytes::from("three"))];
            return Ok(Response::stream("text/plain", Box::pin(futures_util::stream::iter(chunks))));
        }
        if path.starts_with("/api/sse/") {
            let events = vec![
                SseEvent::data("a"),
                SseEvent::named("tick", "b"),
                SseEvent::json(Some("obj"), &json!({"n": 1})),
            ];
            return Ok(Response::sse(futures_util::stream::iter(events)));
        }
        match path {
            "/api/status/created" => Ok(SuccessResponse::new(
                Some(SuccessCode::Created),
                Some("made".into()),
                Some(json!({"id": 7})),
            )
            .into()),
            "/api/status/accepted" => Ok(SuccessResponse::new(Some(SuccessCode::Accepted), None, None).into()),
            "/api/status/msg" => Ok(SuccessResponse::new(None, Some("just a message".into()), None).into()),
            "/api/fail/bad-request" => Err(ErrorResponse::new(Some(ErrorCode::BadRequest), Some("missing thing".into()))),
            "/api/fail/unauthorized" => Err(ErrorResponse::new(Some(ErrorCode::Unauthorized), Some("who are you".into()))),
            "/api/fail/conflict" => Err(ErrorResponse::new(Some(ErrorCode::Conflict), Some("already".into()))),
            "/api/fail/internal" => Err(ErrorResponse::new(Some(ErrorCode::InternalServerError), Some("boom".into()))),
            "/api/fail/default" => Err(ErrorResponse::new(None, Some("nothing here".into()))),
            _ => Err(ErrorResponse::new(Some(ErrorCode::NotFound), Some(format!("unrouted: {}", path)))),
        }
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
/// from; everything else is swallowed.
#[async_trait]
impl SocketRouter<TestMsg, Value> for RecordingSocketRouter {
    async fn route(&self, msg: TestMsg, connection_id: String) -> Option<Value> {
        self.calls.lock().unwrap().push((msg.clone(), connection_id.clone()));
        if msg.channel == "echo" {
            Some(json!({"echo": msg, "connection_id": connection_id}))
        } else {
            None
        }
    }
}

/// Loopback, ephemeral port, no heartbeat, any origin.
pub fn config(id: &str, router: RecordingRouter) -> ServerConfig<RecordingRouter> {
    ServerConfig::new(id, "0", None, router, None, None, None).with_bind_addr(IpAddr::V4(Ipv4Addr::LOCALHOST))
}

pub async fn start_rest(config: ServerConfig<RecordingRouter>) -> RunningServer {
    CnctdServer::bind::<_, Value, Value, NoSocketRouter>(config, None).await.expect("server binds")
}

pub async fn start(config: ServerConfig<RecordingRouter>, socket: SocketConfig<RecordingSocketRouter>) -> RunningServer {
    CnctdServer::bind::<_, TestMsg, Value, _>(config, Some(socket)).await.expect("server binds")
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
