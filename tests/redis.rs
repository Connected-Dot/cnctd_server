//! Cross-instance push over Redis (feature `redis`). Two servers in one
//! process, each with its own connection table, joined by one Redis channel.
//! Needs a Redis at `REDIS_URL` (default redis://127.0.0.1:6379); skips with
//! a note when none answers.

#![cfg(feature = "redis")]

mod common;

use std::{sync::Arc, time::Duration};

use cnctd_server::{
    socket::connection::ConnectionFormat, Connections, Fanout, RedisFanout, RunningServer, SocketConfig,
};
use common::{config, eventually, start, RecordingRouter, RecordingSocketRouter};
use futures_util::StreamExt;
use serde_json::{json, Value};
use tokio::net::TcpStream;
use tokio_tungstenite::{connect_async, tungstenite::Message as Frame, MaybeTlsStream, WebSocketStream};

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

fn redis_url() -> String {
    std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string())
}

/// None when Redis is not reachable (the test then returns early).
async fn redis_available() -> bool {
    match redis::Client::open(redis_url()).and_then(|c| Ok(c)) {
        Ok(client) => client.get_multiplexed_async_connection().await.is_ok(),
        Err(_) => false,
    }
}

macro_rules! need_redis {
    () => {
        if !redis_available().await {
            eprintln!("SKIP: no Redis at {}", redis_url());
            return;
        }
    };
}

/// Two instances of the same server on one private Redis channel.
async fn pair() -> (RunningServer, RunningServer) {
    let channel = format!("cnctd_server:test:{}", uuid::Uuid::new_v4());
    let mut servers = Vec::new();
    for _ in 0..2 {
        let fanout: Arc<dyn Fanout> = Arc::new(RedisFanout::connect_on_channel(&redis_url(), &channel).await.unwrap());
        let cfg = config("redis-test", RecordingRouter::default()).with_fanout(fanout);
        servers.push(start(cfg, SocketConfig::new(RecordingSocketRouter::default(), None, None, None)).await);
    }
    let b = servers.pop().unwrap();
    let a = servers.pop().unwrap();
    // Give both subscribers a moment to be live before anything is published.
    tokio::time::sleep(Duration::from_millis(200)).await;
    (a, b)
}

async fn client(server: &RunningServer, subscriptions: &[&str], user: Option<&str>) -> (String, Ws) {
    let conns: &Connections = server.connections();
    let id = conns
        .register(subscriptions.iter().map(|s| s.to_string()).collect(), None, ConnectionFormat::Json)
        .await;
    if let Some(u) = user {
        conns.update_user_id(&id, u).await.unwrap();
    }
    let (ws, _) = connect_async(format!("ws://127.0.0.1:{}/ws?connection_id={}", server.port(), id)).await.unwrap();
    eventually(Duration::from_secs(3), || async { conns.info(&id).await.ok().filter(|i| i.connected).map(|_| ()) })
        .await
        .expect("connected");
    (id, ws)
}

async fn next_json(ws: &mut Ws, timeout: Duration) -> Option<Value> {
    match tokio::time::timeout(timeout, ws.next()).await {
        Ok(Some(Ok(Frame::Text(t)))) => Some(serde_json::from_str(t.as_str()).unwrap()),
        Ok(other) => panic!("unexpected frame: {:?}", other.map(|_| ())),
        Err(_) => None,
    }
}

#[tokio::test]
async fn channel_push_reaches_subscribers_on_both_instances_exactly_once() {
    need_redis!();
    let (a, b) = pair().await;
    let (_a_id, mut a_ws) = client(&a, &["news"], None).await;
    let (_b_id, mut b_ws) = client(&b, &["news"], None).await;

    b.connections().message_subscribers("news", &json!({"n": 1}), None).await.unwrap();

    assert_eq!(next_json(&mut a_ws, Duration::from_secs(3)).await, Some(json!({"n": 1})), "crossed to A");
    assert_eq!(next_json(&mut b_ws, Duration::from_secs(3)).await, Some(json!({"n": 1})), "local on B");
    assert!(next_json(&mut b_ws, Duration::from_millis(500)).await.is_none(), "B did not redeliver its own envelope");
    assert!(next_json(&mut a_ws, Duration::from_millis(300)).await.is_none(), "exactly once on A");
}

#[tokio::test]
async fn user_push_crosses_instances_and_honours_exclude() {
    need_redis!();
    let (a, b) = pair().await;
    let (a_id, mut a_ws) = client(&a, &[], Some("user-r")).await;
    let (_b_id, mut b_ws) = client(&b, &[], Some("user-r")).await;

    a.connections().message_user("user-r", &json!({"u": 1}), Some(a_id.clone())).await.unwrap();
    assert_eq!(next_json(&mut b_ws, Duration::from_secs(3)).await, Some(json!({"u": 1})));
    assert!(next_json(&mut a_ws, Duration::from_millis(400)).await.is_none(), "excluded on its own instance");

    a.connections().message_user("user-r", &json!({"u": 2}), None).await.unwrap();
    assert_eq!(next_json(&mut a_ws, Duration::from_secs(3)).await, Some(json!({"u": 2})));
    assert_eq!(next_json(&mut b_ws, Duration::from_secs(3)).await, Some(json!({"u": 2})));
}

#[tokio::test]
async fn send_to_a_connection_held_by_the_other_instance() {
    need_redis!();
    let (a, b) = pair().await;
    let (a_id, mut a_ws) = client(&a, &[], None).await;

    // B does not hold a_id, but with a fan-out it cannot know that nobody
    // does, so it publishes instead of failing.
    b.connections().send(&a_id, &json!({"direct": true})).await.unwrap();
    assert_eq!(next_json(&mut a_ws, Duration::from_secs(3)).await, Some(json!({"direct": true})));

    b.connections().send_binary(&a_id, vec![7, 7]).await.unwrap();
    match tokio::time::timeout(Duration::from_secs(3), a_ws.next()).await {
        Ok(Some(Ok(Frame::Binary(bytes)))) => assert_eq!(bytes.to_vec(), vec![7, 7]),
        other => panic!("expected binary frame: {:?}", other.map(|_| ())),
    }
}

#[tokio::test]
async fn key_value_push_crosses_instances() {
    need_redis!();
    let (a, b) = pair().await;
    let (a_id, mut a_ws) = client(&a, &[], None).await;
    a.connections().add_data(&a_id, json!({"team": "red"})).await.unwrap();

    b.connections().message_key_value_owners("team", "red", &json!({"kv": 1}), None).await.unwrap();
    assert_eq!(next_json(&mut a_ws, Duration::from_secs(3)).await, Some(json!({"kv": 1})));
}

#[tokio::test]
async fn redis_url_on_the_socket_config_is_enough() {
    need_redis!();
    // No explicit fan-out: the server builds one from `redis_url`, keyed by
    // server id, so two instances with the same id find each other.
    let id = format!("redis-url-{}", uuid::Uuid::new_v4());
    let mut servers = Vec::new();
    for _ in 0..2 {
        let cfg = config(&id, RecordingRouter::default());
        let socket = SocketConfig::new(RecordingSocketRouter::default(), None, Some(redis_url()), None);
        servers.push(start(cfg, socket).await);
    }
    tokio::time::sleep(Duration::from_millis(200)).await;
    let (b, a) = (servers.pop().unwrap(), servers.pop().unwrap());
    let (_a_id, mut a_ws) = client(&a, &["via-url"], None).await;

    b.connections().message_subscribers("via-url", &json!({"ok": 1}), None).await.unwrap();
    assert_eq!(next_json(&mut a_ws, Duration::from_secs(3)).await, Some(json!({"ok": 1})));
    assert_eq!(a.server_info().read().await.redis_url.as_deref(), Some(redis_url().as_str()));
}

#[tokio::test]
async fn shutdown_stops_the_subscriber() {
    need_redis!();
    let (a, b) = pair().await;
    let (_b_id, mut b_ws) = client(&b, &["after"], None).await;
    a.shutdown().await.unwrap();
    // B still works on its own.
    b.connections().message_subscribers("after", &json!({"still": 1}), None).await.unwrap();
    assert_eq!(next_json(&mut b_ws, Duration::from_secs(3)).await, Some(json!({"still": 1})));
}
