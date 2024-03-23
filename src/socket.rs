use futures_util::{sink::SinkExt, stream::StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use warp::ws::{Message as SocketMessage, WebSocket};
use warp::Filter;

#[derive(Serialize, Deserialize, Debug)]
pub struct GenericMessage {
    pub msg_type: String,
    pub payload: Value,
}

type UserId = String;
type Channel = String;
type ClientSender = mpsc::UnboundedSender<SocketMessage>;

#[derive(Debug, Clone)]
pub struct Client {
    user_id: UserId,
    sender: ClientSender,
    subscriptions: HashSet<Channel>,
}

#[derive(Debug, Clone)]
pub struct CnctdSocket {
    clients: Arc<RwLock<HashMap<UserId, Client>>>,
    subscriptions: Arc<RwLock<HashMap<Channel, HashSet<UserId>>>>,
}

impl CnctdSocket {
    pub fn new() -> Self {
        Self {
            clients: Arc::new(RwLock::new(HashMap::new())),
            subscriptions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn start(port: &str) -> anyhow::Result<()> {
        let socket_server = Arc::new(Self::new());

        let ws_route = warp::path("ws")
            .and(warp::ws())
            .map(move |ws: warp::ws::Ws| {
                let server_clone = Arc::clone(&socket_server);
                ws.on_upgrade(move |socket| server_clone.handle_ws_connection(socket))
            });


        let ip_address: [u8; 4] = [0, 0, 0, 0];
        let parsed_port = port.parse::<u16>()?;
        let socket_addr = std::net::SocketAddr::from((ip_address, parsed_port));
        
        warp::serve(ws_route)
            .run(socket_addr)
            .await;

        Ok(())
    }

    async fn handle_ws_connection(&self, ws: WebSocket) {
        let (mut sender, mut receiver) = ws.split();

        while let Some(result) = receiver.next().await {
            match result {
                Ok(msg) if msg.is_text() => {
                    if let Ok(text) = msg.to_str() {
                        if let Ok(generic_msg) = serde_json::from_str::<GenericMessage>(text) {
                            println!("Received message: {:?}", generic_msg);

                            let response = GenericMessage {
                                msg_type: "response".to_string(),
                                payload: Value::String("Processed your message".to_string()),
                            };
                            
                            if let Ok(response_text) = serde_json::to_string(&response) {
                                let _ = sender.send(SocketMessage::text(response_text)).await;
                                // Optionally handle send error here
                            }
                        }
                    }
                },
                Err(e) => {
                    eprintln!("WebSocket error: {:?}", e);
                    break;
                },
                _ => {} // Optionally handle binary messages or close messages here
            }
        }
    }
}

#[tokio::main]
async fn main() {
    let port = "8080"; // Example port
    if let Err(e) = CnctdSocket::start(port).await {
        eprintln!("Error starting server: {:?}", e);
    }
}
