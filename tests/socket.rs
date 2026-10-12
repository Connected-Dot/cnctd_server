//! WebSocket behavior of cnctd_server 0.6.x: registration, routing, push,
//! binary frames, disconnect hooks, heartbeat. Server here has a socket
//! config with on_disconnect + on_binary hooks and a 1-second heartbeat.

mod common;

use std::{
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};

use cnctd_server::{
    router::message::Message,
    server::{CnctdServer, ServerConfig},
    socket::{
        connection::{CnctdConnection, ConnectionFormat, ConnectionInfo},
        CnctdSocket, SocketConfig,
    },
};
use common::{eventually, http, RecordingRouter, RecordingSocketRouter};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::net::TcpStream;
use tokio_tungstenite::{connect_async, tungstenite::Message as Frame, MaybeTlsStream, WebSocketStream};

const PORT: u16 = 48120;
const SERVER_ID: &str = "socket-test";

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

struct Harness {
    socket_router: RecordingSocketRouter,
    disconnects: Arc<Mutex<Vec<ConnectionInfo>>>,
    binaries: Arc<Mutex<Vec<(String, Vec<u8>)>>>,
}

static HARNESS: OnceLock<Harness> = OnceLock::new();

fn server() -> &'static Harness {
    HARNESS.get_or_init(|| {
        let rest = RecordingRouter::default();
        let socket_router = RecordingSocketRouter::default();
        let disconnects: Arc<Mutex<Vec<ConnectionInfo>>> = Default::default();
        let binaries: Arc<Mutex<Vec<(String, Vec<u8>)>>> = Default::default();

        let sr = socket_router.clone();
        let d = disconnects.clone();
        let b = binaries.clone();
        common::spawn_server(PORT, move || async move {
            let server_config = ServerConfig::new(SERVER_ID, &PORT.to_string(), None, rest, Some(1), None, None);
            let socket_config = SocketConfig::new(
                sr,
                Some(b"test-secret".to_vec()),
                None,
                Some(Arc::new(move |info: ConnectionInfo| {
                    d.lock().unwrap().push(info);
                })),
            )
            .with_on_binary(Arc::new(move |id: String, bytes: Vec<u8>| {
                let b = b.clone();
                Box::pin(async move {
                    b.lock().unwrap().push((id, bytes));
                })
            }));
            CnctdServer::start(server_config, Some(socket_config)).await
        });

        Harness { socket_router, disconnects, binaries }
    })
}

async fn connect(query: &str) -> Ws {
    let (ws, resp) = connect_async(format!("ws://127.0.0.1:{}/ws?{}", PORT, query))
        .await
        .expect("websocket upgrade");
    assert_eq!(resp.status().as_u16(), 101);
    ws
}

/// Register over the framework API (what cnctd.world's REST `register` op
/// does) and open the socket for it.
async fn register_and_connect(subscriptions: &[&str]) -> (String, Ws) {
    server();
    let id = CnctdConnection::register_connection(subscriptions.iter().map(|s| s.to_string()).collect(), None)
        .await
        .unwrap();
    let ws = connect(&format!("connection_id={}", id)).await;
    wait_connected(&id).await;
    (id, ws)
}

async fn wait_connected(id: &str) {
    let id = id.to_string();
    eventually(Duration::from_secs(3), || {
        let id = id.clone();
        async move {
            match CnctdConnection::get_connection_info(&id).await {
                Ok(info) if info.connected => Some(()),
                _ => None,
            }
        }
    })
    .await
    .expect("connection never marked connected");
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

async fn next_binary(ws: &mut Ws, timeout: Duration) -> Option<Vec<u8>> {
    match tokio::time::timeout(timeout, ws.next()).await {
        Ok(Some(Ok(Frame::Binary(b)))) => Some(b.to_vec()),
        Ok(Some(Ok(other))) => panic!("expected binary frame, got {:?}", other),
        other => panic!("no binary frame: {:?}", other.map(|_| ())),
    }
}

// ---------------------------------------------------------------- lifecycle

#[tokio::test]
async fn register_then_connect_then_echo() {
    let h = server();
    let id = CnctdConnection::register_connection(vec!["t1-chan".into()], Some("9.9.9.9".into()))
        .await
        .unwrap();

    // Registered but not yet connected: known, with no sender.
    let before = CnctdConnection::get_connection_info(&id).await.unwrap();
    assert!(!before.connected);
    assert_eq!(before.subscriptions, vec!["t1-chan".to_string()]);
    assert_eq!(before.ip_address.as_deref(), Some("9.9.9.9"));
    assert_eq!(before.user_id, "");
    assert!(!before.authenticated);
    assert_eq!(before.format, ConnectionFormat::Json);
    assert_eq!(before.server_id, SERVER_ID);
    assert!(!before.server_session_id.is_empty());

    let mut ws = connect(&format!("connection_id={}", id)).await;
    wait_connected(&id).await;

    send_json(&mut ws, json!({"channel": "echo", "data": {"n": 1}})).await;
    let reply = next_json(&mut ws, Duration::from_secs(2)).await.expect("echo reply");
    assert_eq!(reply["echo"], json!({"channel": "echo", "data": {"n": 1}}));
    assert_eq!(reply["connection_id"], json!(id));

    let routed = h.socket_router.calls.lock().unwrap().iter().any(|(m, cid)| m.channel == "echo" && cid == &id);
    assert!(routed, "socket router saw the message with the connection id");
}

#[tokio::test]
async fn connect_with_unknown_connection_id_upgrades_then_closes() {
    // KNOWN QUIRK, pinned: the upgrade is accepted before the id is checked,
    // so an unknown id gets a 101 and then the server drops the socket.
    // 1.0 should refuse the upgrade instead.
    server();
    let mut ws = connect("connection_id=never-registered").await;
    let closed = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            match ws.next().await {
                None | Some(Err(_)) | Some(Ok(Frame::Close(_))) => break,
                Some(Ok(_)) => continue,
            }
        }
    })
    .await;
    assert!(closed.is_ok(), "server did not close the socket for an unknown id");
}

#[tokio::test]
async fn connect_without_id_or_subscriptions_is_not_upgraded() {
    // KNOWN QUIRK, pinned: the ws filter rejects, and like every warp
    // rejection here that falls through the `.or()` chain to the SPA
    // catch-all — so the client gets a 200 HTML page instead of an upgrade.
    // 1.0 should answer 400.
    server();
    let err = connect_async(format!("ws://127.0.0.1:{}/ws", PORT)).await.err().expect("must not upgrade");
    match err {
        tokio_tungstenite::tungstenite::Error::Http(resp) => assert_eq!(resp.status().as_u16(), 200),
        other => panic!("unexpected error: {:?}", other),
    }
}

#[tokio::test]
async fn inline_registration_creates_connection_from_query() {
    server();
    let mut ws = connect("subscriptions=in-a,%20in-b,&format=binary").await;

    let info = eventually(Duration::from_secs(3), || async {
        CnctdConnection::get_connection_infos()
            .await
            .unwrap()
            .into_iter()
            .find(|i| i.subscriptions == vec!["in-a".to_string(), "in-b".to_string()] && i.connected)
    })
    .await
    .expect("inline-registered connection appears");
    assert_eq!(info.format, ConnectionFormat::Binary);
    assert_eq!(info.ip_address, None, "inline registration records no ip");

    send_json(&mut ws, json!({"channel": "echo", "data": null})).await;
    let reply = next_json(&mut ws, Duration::from_secs(2)).await.unwrap();
    assert_eq!(reply["connection_id"], json!(info.connection_id));
}

#[tokio::test]
async fn inline_registration_defaults_to_json_format() {
    server();
    let _ws = connect("subscriptions=in-c").await;
    let info = eventually(Duration::from_secs(3), || async {
        CnctdConnection::get_connection_infos()
            .await
            .unwrap()
            .into_iter()
            .find(|i| i.subscriptions == vec!["in-c".to_string()])
    })
    .await
    .unwrap();
    assert_eq!(info.format, ConnectionFormat::Json);
}

#[tokio::test]
async fn disconnect_fires_callback_and_removes_connection() {
    let h = server();
    let (id, mut ws) = register_and_connect(&["dc"]).await;
    ws.close(None).await.unwrap();

    let info = eventually(Duration::from_secs(3), || {
        let id = id.clone();
        let d = h.disconnects.clone();
        async move { d.lock().unwrap().iter().find(|i| i.connection_id == id).cloned() }
    })
    .await
    .expect("on_disconnect called with this connection");
    assert_eq!(info.subscriptions, vec!["dc".to_string()]);

    eventually(Duration::from_secs(3), || {
        let id = id.clone();
        async move { CnctdConnection::get_connection(&id).await.err().map(|_| ()) }
    })
    .await
    .expect("connection removed from the table");
}

#[tokio::test]
async fn registered_but_never_connected_is_dropped_after_ten_seconds() {
    server();
    let id = CnctdConnection::register_connection(vec!["ghost".into()], None).await.unwrap();
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(CnctdConnection::get_connection(&id).await.is_ok(), "still there at 1s");
    tokio::time::sleep(Duration::from_secs(10)).await;
    assert!(CnctdConnection::get_connection(&id).await.is_err(), "gone after the 10s grace");
}

// ---------------------------------------------------------------- routing

#[tokio::test]
async fn non_json_text_and_unanswered_channels_are_dropped_silently() {
    server();
    let (_id, mut ws) = register_and_connect(&["quiet"]).await;

    ws.send(Frame::text("this is not json")).await.unwrap();
    send_json(&mut ws, json!({"channel": "silent", "data": 1})).await;
    assert!(next_json(&mut ws, Duration::from_millis(400)).await.is_none(), "nothing comes back");

    // ...and the connection is still alive and routing.
    send_json(&mut ws, json!({"channel": "echo", "data": "after"})).await;
    let reply = next_json(&mut ws, Duration::from_secs(2)).await.unwrap();
    assert_eq!(reply["echo"]["data"], json!("after"));
}

#[tokio::test]
async fn rest_routes_are_served_from_the_same_port() {
    server();
    let resp = http()
        .get(format!("http://127.0.0.1:{}/api/echo/alongside", PORT))
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
    server();
    let (_a, mut a) = register_and_connect(&["bc-yes"]).await;
    let (_b, mut b) = register_and_connect(&["bc-no"]).await;

    CnctdSocket::broadcast_message(&Message::new("bc-yes", "hello", Some(json!({"k": 1}))))
        .await
        .unwrap();

    let got = next_json(&mut a, Duration::from_secs(2)).await.unwrap();
    assert_eq!(got, json!({"channel": "bc-yes", "instruction": "hello", "data": {"k": 1}}));
    assert!(next_json(&mut b, Duration::from_millis(400)).await.is_none());
}

#[tokio::test]
async fn message_broadcast_helper_matches_socket_broadcast() {
    server();
    let (_a, mut a) = register_and_connect(&["bc-helper"]).await;
    Message::new("bc-helper", "ping", None).broadcast().await.unwrap();
    let got = next_json(&mut a, Duration::from_secs(2)).await.unwrap();
    assert_eq!(got, json!({"channel": "bc-helper", "instruction": "ping", "data": null}));
}

#[tokio::test]
async fn message_subscribers_honours_exclude_and_message_connection_targets_one() {
    server();
    let (_a, mut a) = register_and_connect(&["ms"]).await;
    let (b_id, mut b) = register_and_connect(&["ms"]).await;

    CnctdConnection::message_subscribers("ms", &json!({"m": 1}), Some(b_id.clone())).await.unwrap();
    assert_eq!(next_json(&mut a, Duration::from_secs(2)).await.unwrap(), json!({"m": 1}));
    assert!(next_json(&mut b, Duration::from_millis(400)).await.is_none(), "excluded");

    CnctdConnection::message_connection(&b_id, &json!({"m": 2})).await.unwrap();
    assert_eq!(next_json(&mut b, Duration::from_secs(2)).await.unwrap(), json!({"m": 2}));
    assert!(next_json(&mut a, Duration::from_millis(400)).await.is_none());
}

#[tokio::test]
async fn message_connection_to_unknown_or_unopened_connection_is_an_error() {
    server();
    assert!(CnctdConnection::message_connection("nobody", &json!(1)).await.is_err());
    let id = CnctdConnection::register_connection(vec![], None).await.unwrap();
    let err = CnctdConnection::message_connection(&id, &json!(1)).await.unwrap_err();
    assert!(err.to_string().contains("no active sender"), "{}", err);
}

#[tokio::test]
async fn message_user_reaches_every_connection_of_the_user_except_the_excluded() {
    server();
    let (a_id, mut a) = register_and_connect(&["mu"]).await;
    let (b_id, mut b) = register_and_connect(&["mu"]).await;

    let info = CnctdConnection::update_user_id(&a_id, "user-x").await.unwrap();
    assert_eq!(info.user_id, "user-x");
    assert_eq!(info.connection_id, a_id);
    CnctdConnection::update_user_id(&b_id, "user-x").await.unwrap();

    let mut ids = CnctdConnection::get_connection_ids("user-x").await.unwrap();
    ids.sort();
    let mut expected = vec![a_id.clone(), b_id.clone()];
    expected.sort();
    assert_eq!(ids, expected);

    CnctdConnection::message_user("user-x", &json!({"u": 1}), Some(a_id.clone())).await.unwrap();
    assert_eq!(next_json(&mut b, Duration::from_secs(2)).await.unwrap(), json!({"u": 1}));
    assert!(next_json(&mut a, Duration::from_millis(400)).await.is_none());

    Message::new("direct", "dm", Some(json!("hi"))).to_user("user-x", None).await.unwrap();
    assert_eq!(next_json(&mut a, Duration::from_secs(2)).await.unwrap()["instruction"], json!("dm"));
    assert_eq!(next_json(&mut b, Duration::from_secs(2)).await.unwrap()["instruction"], json!("dm"));
}

#[tokio::test]
async fn binary_frames_flow_both_ways() {
    let h = server();
    let (id, mut ws) = register_and_connect(&["bin"]).await;

    CnctdConnection::message_connection_binary(&id, vec![1, 2, 3]).await.unwrap();
    assert_eq!(next_binary(&mut ws, Duration::from_secs(2)).await.unwrap(), vec![1, 2, 3]);

    ws.send(Frame::binary(vec![9u8, 8, 7])).await.unwrap();
    let seen = eventually(Duration::from_secs(3), || {
        let id = id.clone();
        let b = h.binaries.clone();
        async move { b.lock().unwrap().iter().find(|(cid, _)| cid == &id).map(|(_, bytes)| bytes.clone()) }
    })
    .await
    .expect("on_binary saw the frame");
    assert_eq!(seen, vec![9, 8, 7]);
    // Binary frames never reach the JSON router.
    assert!(!h.socket_router.calls.lock().unwrap().iter().any(|(_, cid)| cid == &id));
}

#[tokio::test]
async fn heartbeat_is_broadcast_on_the_server_info_channel() {
    server();
    let (_id, mut ws) = register_and_connect(&["server-info"]).await;
    let frame = next_json(&mut ws, Duration::from_secs(3)).await.expect("heartbeat within 3s");
    assert_eq!(frame["channel"], json!("server-info"));
    assert_eq!(frame["instruction"], json!("heartbeat"));
    assert_eq!(frame["data"]["id"], json!(SERVER_ID));
    assert_eq!(frame["data"]["websocket"], json!(true));
    assert!(frame["data"]["current_connections"].as_u64().unwrap() >= 1);
    assert!(frame["data"]["session_id"].is_string());
}

// ---------------------------------------------------------------- connection table

#[tokio::test]
async fn subscriptions_can_be_changed_after_connect() {
    server();
    let (id, _ws) = register_and_connect(&["sub-0"]).await;

    CnctdConnection::add_subscription(&id, "sub-1").await.unwrap();
    CnctdConnection::add_subscription(&id, "sub-1").await.unwrap(); // idempotent
    CnctdConnection::add_multiple_subscriptions(&id, vec!["sub-2".into(), "sub-3".into()]).await.unwrap();
    let info = CnctdConnection::get_connection_info(&id).await.unwrap();
    assert_eq!(info.subscriptions, vec!["sub-0", "sub-1", "sub-2", "sub-3"]);

    assert!(CnctdConnection::get_subscriber_connection_ids("sub-2").await.contains(&id));
    let pairs = CnctdConnection::get_subscriber_connections("sub-2").await;
    assert!(pairs.contains(&(id.clone(), ConnectionFormat::Json)));

    CnctdConnection::remove_subscription(&id, "sub-1").await.unwrap();
    CnctdConnection::remove_multiple_subscriptions(&id, vec!["sub-2".into(), "sub-3".into()]).await.unwrap();
    let info = CnctdConnection::get_connection_info(&id).await.unwrap();
    assert_eq!(info.subscriptions, vec!["sub-0"]);
}

#[tokio::test]
async fn connection_data_blob_and_key_value_targeting() {
    server();
    let (id, mut ws) = register_and_connect(&["kv"]).await;

    CnctdConnection::add_data(&id, json!({"role": "kv-admin", "tags": ["a", "b"]})).await.unwrap();
    assert_eq!(CnctdConnection::get_data(&id).await.unwrap(), json!({"role": "kv-admin", "tags": ["a", "b"]}));
    assert!(CnctdConnection::check_key_value_pair(&id, "tags", "a").await.unwrap());
    assert!(!CnctdConnection::check_key_value_pair(&id, "tags", "z").await.unwrap());
    assert!(CnctdConnection::check_if_any_kvp_matches(&id, "role", vec!["kv-admin".into(), "x".into()]).await.unwrap());

    CnctdConnection::message_key_value_owners("role", "kv-admin", &json!({"kv": 1}), None).await.unwrap();
    assert_eq!(next_json(&mut ws, Duration::from_secs(2)).await.unwrap(), json!({"kv": 1}));

    CnctdConnection::update_data_field(&id, "role", json!("kv-user")).await.unwrap();
    assert_eq!(CnctdConnection::get_data_field(&id, "role").await.unwrap(), json!("kv-user"));
    CnctdConnection::remove_data_field(&id, "role").await.unwrap();
    assert!(CnctdConnection::get_data_field(&id, "role").await.is_err());

    CnctdConnection::update_authenticated(&id, true).await.unwrap();
    assert!(CnctdConnection::get_connection_info(&id).await.unwrap().authenticated);
}
