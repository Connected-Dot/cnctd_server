//! The associated-function API (`CnctdConnection::*`, `Message::broadcast`,
//! `CnctdSocket::broadcast_message`) that cnctd.world calls from anywhere.
//! It goes through the most recently started server, so this file runs ONE
//! server and every test here shares it.

mod common;

use std::{sync::OnceLock, time::Duration};

use cnctd_server::{
    router::message::Message,
    socket::{connection::CnctdConnection, CnctdSocket, ConnectionFormat},
    SocketConfig,
};
use common::{config, eventually, start, RecordingRouter, RecordingSocketRouter};
use futures_util::StreamExt;
use serde_json::{json, Value};
use tokio_tungstenite::{connect_async, tungstenite::Message as Frame};

static PORT: OnceLock<u16> = OnceLock::new();

/// One server for the whole binary, on its own thread and runtime so it
/// outlives every test's runtime.
async fn port() -> u16 {
    *PORT.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel::<u16>();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
            rt.block_on(async move {
                let server = start(
                    config("global-test", RecordingRouter::default()),
                    SocketConfig::new(RecordingSocketRouter::default(), None, None, None),
                )
                .await;
                tx.send(server.port()).unwrap();
                std::future::pending::<()>().await;
                drop(server);
            });
        });
        rx.recv().expect("server reports its port")
    })
}

async fn open(id: &str) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>> {
    let (ws, _) = connect_async(format!("ws://127.0.0.1:{}/ws?connection_id={}", port().await, id)).await.unwrap();
    eventually(Duration::from_secs(3), || async {
        CnctdConnection::get_connection_info(id).await.ok().filter(|i| i.connected).map(|_| ())
    })
    .await
    .expect("connected");
    ws
}

async fn next_json(ws: &mut tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>) -> Value {
    match tokio::time::timeout(Duration::from_secs(2), ws.next()).await {
        Ok(Some(Ok(Frame::Text(t)))) => serde_json::from_str(t.as_str()).unwrap(),
        other => panic!("expected a text frame: {:?}", other.map(|_| ())),
    }
}

#[tokio::test]
async fn register_message_and_broadcast_through_the_static_api() {
    port().await;
    let id = CnctdConnection::register_connection(vec!["g-chan".into()], None).await.unwrap();
    let mut ws = open(&id).await;

    CnctdConnection::message_connection(&id, &json!({"direct": 1})).await.unwrap();
    assert_eq!(next_json(&mut ws).await, json!({"direct": 1}));

    Message::new("g-chan", "ping", None).broadcast().await.unwrap();
    assert_eq!(next_json(&mut ws).await["instruction"], json!("ping"));

    CnctdSocket::broadcast_message(&Message::new("g-chan", "pong", Some(json!(2)))).await.unwrap();
    assert_eq!(next_json(&mut ws).await["instruction"], json!("pong"));

    CnctdConnection::message_subscribers("g-chan", &json!({"subs": true}), None).await.unwrap();
    assert_eq!(next_json(&mut ws).await, json!({"subs": true}));

    assert!(CnctdConnection::get_subscriber_connection_ids("g-chan").await.contains(&id));
    assert!(CnctdConnection::get_subscriber_connections("g-chan").await.contains(&(id.clone(), ConnectionFormat::Json)));
    assert!(CnctdConnection::get_connections().await.unwrap().contains_key(&id));
    assert!(CnctdConnection::get_connection_infos().await.unwrap().iter().any(|i| i.connection_id == id));
}

#[tokio::test]
async fn user_targeting_through_the_static_api() {
    port().await;
    let id = CnctdConnection::register_connection_with_format(vec![], None, ConnectionFormat::Binary).await.unwrap();
    let mut ws = open(&id).await;
    assert_eq!(CnctdConnection::get_connection(&id).await.unwrap().format, ConnectionFormat::Binary);

    CnctdConnection::update_user_id(&id, "g-user").await.unwrap();
    assert_eq!(CnctdConnection::get_connection_ids("g-user").await.unwrap(), vec![id.clone()]);

    Message::new("dm", "hi", None).to_user("g-user", None).await.unwrap();
    assert_eq!(next_json(&mut ws).await["channel"], json!("dm"));

    CnctdConnection::message_user("g-user", &json!({"u": 1}), None).await.unwrap();
    assert_eq!(next_json(&mut ws).await, json!({"u": 1}));

    CnctdConnection::add_data(&id, json!({"team": "blue"})).await.unwrap();
    CnctdConnection::message_key_value_owners("team", "blue", &json!({"kv": 1}), None).await.unwrap();
    assert_eq!(next_json(&mut ws).await, json!({"kv": 1}));

    CnctdConnection::message_connection_binary(&id, vec![4, 5]).await.unwrap();
    match tokio::time::timeout(Duration::from_secs(2), ws.next()).await {
        Ok(Some(Ok(Frame::Binary(b)))) => assert_eq!(b.to_vec(), vec![4, 5]),
        other => panic!("expected a binary frame: {:?}", other.map(|_| ())),
    }
}
