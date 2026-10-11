//! WebSocket behavior: registration, upgrade rules, routing, push, binary
//! frames, disconnect hooks, heartbeat — through the `Connections` handle.

mod common;

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use cnctd_server::{
    auth::CnctdAuth,
    router::message::Message,
    socket::connection::{ConnectionFormat, ConnectionInfo},
    Connections, RunningServer, SocketConfig,
};
use common::{config, eventually, http, start, RecordingRouter, RecordingSocketRouter};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::net::TcpStream;
use tokio_tungstenite::{connect_async, tungstenite::Message as Frame, MaybeTlsStream, WebSocketStream};

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;
type Binaries = Arc<Mutex<Vec<(String, Vec<u8>)>>>;

struct Sock {
    server: RunningServer,
    router: RecordingSocketRouter,
    disconnects: Arc<Mutex<Vec<ConnectionInfo>>>,
    binaries: Binaries,
}

impl Sock {
    fn conns(&self) -> &Connections {
        self.server.connections()
    }

    fn ws_url(&self, query: &str) -> String {
        format!("ws://127.0.0.1:{}/ws?{}", self.server.port(), query)
    }

    async fn connect(&self, query: &str) -> Ws {
        let (ws, resp) = connect_async(self.ws_url(query)).await.expect("websocket upgrade");
        assert_eq!(resp.status().as_u16(), 101);
        ws
    }

    /// Register through the table (what the app's REST `register` op does),
    /// open the socket for it, wait until the server marks it connected.
    async fn register_and_connect(&self, subscriptions: &[&str]) -> (String, Ws) {
        let id = self
            .conns()
            .register(subscriptions.iter().map(|s| s.to_string()).collect(), None, ConnectionFormat::Json)
            .await;
        let ws = self.connect(&format!("connection_id={}", id)).await;
        self.wait_connected(&id).await;
        (id, ws)
    }

    async fn wait_connected(&self, id: &str) {
        eventually(Duration::from_secs(3), || async {
            match self.conns().info(id).await {
                Ok(info) if info.connected => Some(()),
                _ => None,
            }
        })
        .await
        .expect("connection never marked connected");
    }
}

async fn sock_with(heartbeat: Option<u64>, secret: Option<Vec<u8>>, path: Option<&str>) -> Sock {
    sock_full(heartbeat, secret, path, false).await
}

async fn sock_full(heartbeat: Option<u64>, secret: Option<Vec<u8>>, path: Option<&str>, announce: bool) -> Sock {
    sock_opts(heartbeat, secret, path, announce, None).await
}

async fn sock_opts(heartbeat: Option<u64>, secret: Option<Vec<u8>>, path: Option<&str>, announce: bool, keepalive: Option<Duration>) -> Sock {
    let router = RecordingSocketRouter::default();
    let disconnects: Arc<Mutex<Vec<ConnectionInfo>>> = Default::default();
    let binaries: Binaries = Default::default();
    let d = disconnects.clone();
    let b = binaries.clone();

    let mut cfg = config("socket-test", RecordingRouter::default());
    cfg.heartbeat = heartbeat.map(Duration::from_secs);
    let mut socket = SocketConfig::new(
        router.clone(),
        secret,
        None,
        Some(Arc::new(move |info: ConnectionInfo| d.lock().unwrap().push(info))),
    )
    .with_on_binary(Arc::new(move |id: String, bytes: Vec<u8>| {
        let b = b.clone();
        Box::pin(async move { b.lock().unwrap().push((id, bytes)) })
    }));
    if let Some(p) = path {
        socket = socket.with_path(p);
    }
    socket = socket.with_connected_announcement(announce);
    if let Some(k) = keepalive {
        socket = socket.with_keepalive(Some(k));
    }
    let server = start(cfg, socket).await;
    Sock { server, router, disconnects, binaries }
}

async fn sock() -> Sock {
    sock_with(None, None, None).await
}

async fn send_json(ws: &mut Ws, v: Value) {
    ws.send(Frame::text(v.to_string())).await.unwrap();
}

/// Next text frame as JSON within `timeout`; None if nothing arrives.
async fn next_json(ws: &mut Ws, timeout: Duration) -> Option<Value> {
    match tokio::time::timeout(timeout, ws.next()).await {
        Ok(Some(Ok(Frame::Text(t)))) => Some(serde_json::from_str(t.as_str()).expect("text frame is JSON")),
        Ok(Some(Ok(other))) => panic!("expected text frame, got {:?}", other),
        Ok(Some(Err(e))) => panic!("socket error: {}", e),
        Ok(None) => panic!("socket closed"),
        Err(_) => None,
    }
}

async fn next_binary(ws: &mut Ws, timeout: Duration) -> Vec<u8> {
    match tokio::time::timeout(timeout, ws.next()).await {
        Ok(Some(Ok(Frame::Binary(b)))) => b.to_vec(),
        other => panic!("no binary frame: {:?}", other.map(|_| ())),
    }
}

fn upgrade_status(err: tokio_tungstenite::tungstenite::Error) -> u16 {
    match err {
        tokio_tungstenite::tungstenite::Error::Http(resp) => resp.status().as_u16(),
        other => panic!("unexpected error: {:?}", other),
    }
}

// ---------------------------------------------------------------- lifecycle

#[tokio::test]
async fn register_then_connect_then_echo() {
    let s = sock().await;
    let id = s.conns().register(vec!["t1-chan".into()], Some("9.9.9.9".into()), ConnectionFormat::Json).await;

    let before = s.conns().info(&id).await.unwrap();
    assert!(!before.connected);
    assert_eq!(before.subscriptions, vec!["t1-chan".to_string()]);
    assert_eq!(before.ip_address.as_deref(), Some("9.9.9.9"));
    assert_eq!(before.user_id, "");
    assert!(!before.authenticated);
    assert_eq!(before.format, ConnectionFormat::Json);
    assert_eq!(before.server_id, "socket-test");
    assert_eq!(before.server_session_id, s.conns().identity().session_id);

    let mut ws = s.connect(&format!("connection_id={}", id)).await;
    s.wait_connected(&id).await;

    send_json(&mut ws, json!({"channel": "echo", "data": {"n": 1}})).await;
    let reply = next_json(&mut ws, Duration::from_secs(2)).await.expect("echo reply");
    assert_eq!(reply["echo"], json!({"channel": "echo", "data": {"n": 1}}));
    assert_eq!(reply["connection_id"], json!(id));
    assert!(s.router.calls.lock().unwrap().iter().any(|(m, cid)| m.channel == "echo" && cid == &id));
}

#[tokio::test]
async fn unknown_connection_id_is_refused_before_upgrade() {
    let s = sock().await;
    let err = connect_async(s.ws_url("connection_id=never-registered")).await.expect_err("must not upgrade");
    assert_eq!(upgrade_status(err), 404);
}

#[tokio::test]
async fn missing_id_and_subscriptions_is_400() {
    let s = sock().await;
    let err = connect_async(format!("ws://127.0.0.1:{}/ws", s.server.port())).await.expect_err("must not upgrade");
    assert_eq!(upgrade_status(err), 400);
}

#[tokio::test]
async fn inline_registration_creates_connection_from_query() {
    let s = sock().await;
    let mut ws = s.connect("subscriptions=in-a,%20in-b,&format=binary").await;

    let info = eventually(Duration::from_secs(3), || async {
        s.conns()
            .infos()
            .await
            .into_iter()
            .find(|i| i.subscriptions == vec!["in-a".to_string(), "in-b".to_string()] && i.connected)
    })
    .await
    .expect("inline-registered connection appears");
    assert_eq!(info.format, ConnectionFormat::Binary);
    assert_eq!(info.ip_address.as_deref(), Some("127.0.0.1"), "ip recorded from the upgrade");

    send_json(&mut ws, json!({"channel": "echo", "data": null})).await;
    let reply = next_json(&mut ws, Duration::from_secs(2)).await.unwrap();
    assert_eq!(reply["connection_id"], json!(info.connection_id));
}

#[tokio::test]
async fn inline_registration_defaults_to_json_format() {
    let s = sock().await;
    let _ws = s.connect("subscriptions=in-c").await;
    let info = eventually(Duration::from_secs(3), || async {
        s.conns().infos().await.into_iter().find(|i| i.subscriptions == vec!["in-c".to_string()])
    })
    .await
    .unwrap();
    assert_eq!(info.format, ConnectionFormat::Json);
}

#[tokio::test]
async fn token_at_upgrade_authenticates_the_connection() {
    let secret = b"socket-secret".to_vec();
    let s = sock_with(None, Some(secret.clone()), None).await;
    let id = s.conns().register(vec![], None, ConnectionFormat::Json).await;
    let token = CnctdAuth::get_jwt(secret, "user-7").unwrap();
    let _ws = s.connect(&format!("connection_id={}&token={}", id, token)).await;
    s.wait_connected(&id).await;
    let info = s.conns().info(&id).await.unwrap();
    assert_eq!(info.user_id, "user-7");
    assert!(info.authenticated);

    let id2 = s.conns().register(vec![], None, ConnectionFormat::Json).await;
    let err = connect_async(s.ws_url(&format!("connection_id={}&token=garbage", id2))).await.err().unwrap();
    assert_eq!(upgrade_status(err), 401);
}

#[tokio::test]
async fn disconnect_fires_callback_and_removes_connection() {
    let s = sock().await;
    let (id, mut ws) = s.register_and_connect(&["dc"]).await;
    ws.close(None).await.unwrap();

    let info = eventually(Duration::from_secs(3), || async {
        s.disconnects.lock().unwrap().iter().find(|i| i.connection_id == id).cloned()
    })
    .await
    .expect("on_disconnect called with this connection");
    assert_eq!(info.subscriptions, vec!["dc".to_string()]);

    eventually(Duration::from_secs(3), || async { (!s.conns().contains(&id).await).then_some(()) })
        .await
        .expect("connection removed from the table");
}

#[tokio::test]
async fn registered_but_never_connected_is_dropped_after_ten_seconds() {
    let s = sock().await;
    let id = s.conns().register(vec!["ghost".into()], None, ConnectionFormat::Json).await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(s.conns().contains(&id).await, "still there at 1s");
    tokio::time::sleep(Duration::from_secs(10)).await;
    assert!(!s.conns().contains(&id).await, "gone after the grace period");
}

#[tokio::test]
async fn custom_socket_path() {
    let s = sock_with(None, None, Some("/socket")).await;
    let id = s.conns().register(vec![], None, ConnectionFormat::Json).await;
    let url = format!("ws://127.0.0.1:{}/socket?connection_id={}", s.server.port(), id);
    let (_ws, resp) = connect_async(url).await.unwrap();
    assert_eq!(resp.status().as_u16(), 101);
    let err = connect_async(s.ws_url(&format!("connection_id={}", id))).await.err().unwrap();
    assert_eq!(upgrade_status(err), 404, "/ws is not served");
}

#[tokio::test]
async fn two_servers_have_separate_tables() {
    let a = sock().await;
    let b = sock().await;
    let id = a.conns().register(vec![], None, ConnectionFormat::Json).await;
    assert!(a.conns().contains(&id).await);
    assert!(!b.conns().contains(&id).await);
    assert_ne!(a.conns().identity().session_id, b.conns().identity().session_id);
}

// ---------------------------------------------------------------- routing

#[tokio::test]
async fn non_json_text_and_unanswered_channels_are_dropped_silently() {
    let s = sock().await;
    let (_id, mut ws) = s.register_and_connect(&["quiet"]).await;

    ws.send(Frame::text("this is not json")).await.unwrap();
    send_json(&mut ws, json!({"channel": "silent", "data": 1})).await;
    assert!(next_json(&mut ws, Duration::from_millis(400)).await.is_none(), "nothing comes back");

    send_json(&mut ws, json!({"channel": "echo", "data": "after"})).await;
    let reply = next_json(&mut ws, Duration::from_secs(2)).await.unwrap();
    assert_eq!(reply["echo"]["data"], json!("after"));
}

#[tokio::test]
async fn rest_routes_are_served_from_the_same_port() {
    let s = sock().await;
    let resp = http()
        .get(format!("{}/api/echo/alongside", s.server.url()))
        .header("Client-Id", "ws-conn")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["data"]["connection_id"], json!("ws-conn"));
}

// ---------------------------------------------------------------- push

#[tokio::test]
async fn broadcast_reaches_only_subscribers_of_the_channel() {
    let s = sock().await;
    let (_a, mut a) = s.register_and_connect(&["bc-yes"]).await;
    let (_b, mut b) = s.register_and_connect(&["bc-no"]).await;

    s.conns().broadcast(&Message::new("bc-yes", "hello", Some(json!({"k": 1})))).await.unwrap();

    let got = next_json(&mut a, Duration::from_secs(2)).await.unwrap();
    assert_eq!(got, json!({"channel": "bc-yes", "instruction": "hello", "data": {"k": 1}}));
    assert!(next_json(&mut b, Duration::from_millis(400)).await.is_none());
}

#[tokio::test]
async fn message_subscribers_honours_exclude_and_send_targets_one() {
    let s = sock().await;
    let (_a, mut a) = s.register_and_connect(&["ms"]).await;
    let (b_id, mut b) = s.register_and_connect(&["ms"]).await;

    s.conns().message_subscribers("ms", &json!({"m": 1}), Some(b_id.clone())).await.unwrap();
    assert_eq!(next_json(&mut a, Duration::from_secs(2)).await.unwrap(), json!({"m": 1}));
    assert!(next_json(&mut b, Duration::from_millis(400)).await.is_none(), "excluded");

    s.conns().send(&b_id, &json!({"m": 2})).await.unwrap();
    assert_eq!(next_json(&mut b, Duration::from_secs(2)).await.unwrap(), json!({"m": 2}));
    assert!(next_json(&mut a, Duration::from_millis(400)).await.is_none());
}

#[tokio::test]
async fn send_to_unknown_or_unopened_connection_is_an_error() {
    let s = sock().await;
    assert!(s.conns().send("nobody", &json!(1)).await.is_err());
    let id = s.conns().register(vec![], None, ConnectionFormat::Json).await;
    let err = s.conns().send(&id, &json!(1)).await.unwrap_err();
    assert!(err.to_string().contains("no active sender"), "{}", err);
}

#[tokio::test]
async fn message_user_reaches_every_connection_of_the_user_except_the_excluded() {
    let s = sock().await;
    let (a_id, mut a) = s.register_and_connect(&["mu"]).await;
    let (b_id, mut b) = s.register_and_connect(&["mu"]).await;

    let info = s.conns().update_user_id(&a_id, "user-x").await.unwrap();
    assert_eq!(info.user_id, "user-x");
    assert_eq!(info.connection_id, a_id);
    s.conns().update_user_id(&b_id, "user-x").await.unwrap();

    let mut ids = s.conns().ids_for_user("user-x").await;
    ids.sort();
    let mut expected = vec![a_id.clone(), b_id.clone()];
    expected.sort();
    assert_eq!(ids, expected);

    s.conns().message_user("user-x", &json!({"u": 1}), Some(a_id.clone())).await.unwrap();
    assert_eq!(next_json(&mut b, Duration::from_secs(2)).await.unwrap(), json!({"u": 1}));
    assert!(next_json(&mut a, Duration::from_millis(400)).await.is_none());
}

#[tokio::test]
async fn binary_frames_flow_both_ways() {
    let s = sock().await;
    let (id, mut ws) = s.register_and_connect(&["bin"]).await;

    s.conns().send_binary(&id, vec![1, 2, 3]).await.unwrap();
    assert_eq!(next_binary(&mut ws, Duration::from_secs(2)).await, vec![1, 2, 3]);

    ws.send(Frame::binary(vec![9u8, 8, 7])).await.unwrap();
    let seen = eventually(Duration::from_secs(3), || async {
        s.binaries.lock().unwrap().iter().find(|(cid, _)| cid == &id).map(|(_, b)| b.clone())
    })
    .await
    .expect("on_binary saw the frame");
    assert_eq!(seen, vec![9, 8, 7]);
    assert!(!s.router.calls.lock().unwrap().iter().any(|(_, cid)| cid == &id), "binary never reaches the JSON router");
}

#[tokio::test]
async fn heartbeat_is_broadcast_on_the_server_info_channel() {
    let s = sock_with(Some(1), None, None).await;
    let (_id, mut ws) = s.register_and_connect(&["server-info"]).await;
    let frame = next_json(&mut ws, Duration::from_secs(3)).await.expect("heartbeat within 3s");
    assert_eq!(frame["channel"], json!("server-info"));
    assert_eq!(frame["instruction"], json!("heartbeat"));
    assert_eq!(frame["data"]["id"], json!("socket-test"));
    assert_eq!(frame["data"]["websocket"], json!(true));
    assert!(frame["data"]["current_connections"].as_u64().unwrap() >= 1);
    assert_eq!(frame["data"]["session_id"], json!(s.conns().identity().session_id));
    assert!(frame["data"]["cpu_usage"].is_number());
    assert!(frame["data"]["memory_usage"].is_number());
}

// ---------------------------------------------------------------- connection table

#[tokio::test]
async fn subscriptions_can_be_changed_after_connect() {
    let s = sock().await;
    let (id, _ws) = s.register_and_connect(&["sub-0"]).await;
    let c = s.conns();

    c.add_subscription(&id, "sub-1").await.unwrap();
    c.add_subscription(&id, "sub-1").await.unwrap(); // idempotent
    c.add_subscriptions(&id, vec!["sub-2".into(), "sub-3".into()]).await.unwrap();
    assert_eq!(c.info(&id).await.unwrap().subscriptions, vec!["sub-0", "sub-1", "sub-2", "sub-3"]);

    assert!(c.subscriber_ids("sub-2").await.contains(&id));
    assert!(c.subscriber_formats("sub-2").await.contains(&(id.clone(), ConnectionFormat::Json)));

    c.remove_subscription(&id, "sub-1").await.unwrap();
    c.remove_subscriptions(&id, vec!["sub-2".into(), "sub-3".into()]).await.unwrap();
    assert_eq!(c.info(&id).await.unwrap().subscriptions, vec!["sub-0"]);
}

#[tokio::test]
async fn connection_data_blob_and_key_value_targeting() {
    let s = sock().await;
    let (id, mut ws) = s.register_and_connect(&["kv"]).await;
    let c = s.conns();

    c.add_data(&id, json!({"role": "kv-admin", "tags": ["a", "b"]})).await.unwrap();
    assert_eq!(c.get_data(&id).await.unwrap(), json!({"role": "kv-admin", "tags": ["a", "b"]}));
    assert!(c.check_key_value_pair(&id, "tags", "a").await.unwrap());
    assert!(!c.check_key_value_pair(&id, "tags", "z").await.unwrap());
    assert!(c.check_if_any_kvp_matches(&id, "role", vec!["kv-admin".into(), "x".into()]).await.unwrap());

    c.message_key_value_owners("role", "kv-admin", &json!({"kv": 1}), None).await.unwrap();
    assert_eq!(next_json(&mut ws, Duration::from_secs(2)).await.unwrap(), json!({"kv": 1}));

    c.update_data_field(&id, "role", json!("kv-user")).await.unwrap();
    assert_eq!(c.get_data_field(&id, "role").await.unwrap(), json!("kv-user"));
    c.remove_data_field(&id, "role").await.unwrap();
    assert!(c.get_data_field(&id, "role").await.is_err());

    c.update_authenticated(&id, true).await.unwrap();
    assert!(c.info(&id).await.unwrap().authenticated);
}

#[tokio::test]
async fn connected_announcement_is_the_first_frame_when_opted_in() {
    let s = sock_full(None, None, None, true).await;
    // Inline registration: the client has no other way to learn its id.
    let mut ws = s.connect("subscriptions=hello").await;
    let frame = next_json(&mut ws, Duration::from_secs(2)).await.expect("connected frame");
    assert_eq!(frame["channel"], json!("socket"));
    assert_eq!(frame["instruction"], json!("connected"));
    let id = frame["data"]["connection_id"].as_str().unwrap().to_string();
    assert_eq!(frame["data"]["subscriptions"], json!(["hello"]));
    assert!(s.conns().contains(&id).await);

    // Binary-format connections are not told (their client speaks binary).
    let mut bin = s.connect("subscriptions=hello&format=binary").await;
    assert!(next_json(&mut bin, Duration::from_millis(400)).await.is_none());
}

#[tokio::test]
async fn no_announcement_by_default() {
    let s = sock().await;
    let mut ws = s.connect("subscriptions=quiet-hello").await;
    assert!(next_json(&mut ws, Duration::from_millis(400)).await.is_none(), "wire is exactly 0.6 unless opted in");
}

#[tokio::test]
async fn server_pings_idle_connections_at_the_keepalive_interval() {
    let s = sock_opts(None, None, None, false, Some(Duration::from_millis(300))).await;
    let (_id, mut ws) = s.register_and_connect(&["idle"]).await;
    let mut pings = 0;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    while pings < 2 && tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_secs(1), ws.next()).await {
            Ok(Some(Ok(Frame::Ping(_)))) => pings += 1,
            Ok(Some(Ok(other))) => panic!("unexpected frame on an idle connection: {:?}", other),
            other => panic!("socket ended: {:?}", other.map(|_| ())),
        }
    }
    assert_eq!(pings, 2, "two pings within two seconds at a 300 ms interval");
    // ...and the connection is still routing afterwards.
    send_json(&mut ws, json!({"channel": "echo", "data": "alive"})).await;
    let reply = loop {
        match next_frame_text(&mut ws).await {
            Some(v) => break v,
            None => continue,
        }
    };
    assert_eq!(reply["echo"]["data"], json!("alive"));
}

/// Next text frame as JSON, skipping pings.
async fn next_frame_text(ws: &mut Ws) -> Option<Value> {
    match tokio::time::timeout(Duration::from_secs(2), ws.next()).await {
        Ok(Some(Ok(Frame::Text(t)))) => Some(serde_json::from_str(t.as_str()).unwrap()),
        Ok(Some(Ok(Frame::Ping(_)))) => None,
        other => panic!("expected text: {:?}", other.map(|_| ())),
    }
}
