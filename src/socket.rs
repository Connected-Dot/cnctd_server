use anyhow::anyhow;
use futures_util::{FutureExt, SinkExt, StreamExt};
use local_ip_address::local_ip;
use serde::{Deserialize, Serialize};
use state::InitCell;
use tokio_stream::wrappers::UnboundedReceiverStream;
use warp::filters::ws::Ws;
use warp::ws::{Message as WebSocketMessage, WebSocket};
use warp::Filter;
use warp::http::StatusCode;
use std::collections::HashMap;
use tokio::sync::{mpsc, Mutex, RwLock};
use std::sync::Arc;

use crate::message::Message;
use crate::router::RouterFunction;

#[derive(Serialize, Deserialize)]
pub struct ClientInfo {
    user_id: String,
    subscriptions: Vec<String>,
    sender_count: usize,
}

#[derive(Debug, Clone)]
pub struct Client {
    pub subscriptions: Vec<String>,
    pub senders: Vec<mpsc::UnboundedSender<std::result::Result<warp::ws::Message, warp::Error>>>,
}

impl Default for Client {
    fn default() -> Self {
        Client {
            subscriptions: Vec::new(),
            senders: Vec::new(),
        }
    }
}

static CLIENTS: InitCell<Arc<RwLock<HashMap<String, Client>>>> = InitCell::new();

pub struct CnctdSocket;

impl CnctdSocket {
    pub async fn start<R>(port: &str, router: R) -> anyhow::Result<()>
    where
        R: RouterFunction + Clone + 'static,
    {

        CLIENTS.set(Arc::new(RwLock::new(HashMap::new())));
    
        let websocket_route = warp::ws()
            .and(warp::any().map(move || router.clone()))
            .and(warp::query::<HashMap<String, String>>())
            .map(|ws: Ws, router: R, params: HashMap<String, String>| -> Box<dyn warp::Reply> {
                // Now the parameters are in the correct order: ws, router, params
                if let Some(user_id) = params.get("user_id") {
                    let user_id_cloned = user_id.clone();
                    Box::new(ws.on_upgrade(move |socket| {
                        handle_connection(socket, router, user_id_cloned)
                    })) as Box<dyn warp::Reply>
                } else {
                    Box::new(warp::reply::with_status("user_id is required", StatusCode::BAD_REQUEST)) as Box<dyn warp::Reply>
                }
            });
    
        let my_local_ip = local_ip()?;
        println!("WebSocket server running at ws://{}:{}", my_local_ip, port);
        let ip_address: [u8; 4] = [0, 0, 0, 0];
        let parsed_port = port.parse::<u16>()?;
        let socket_addr = std::net::SocketAddr::from((ip_address, parsed_port));

        warp::serve(websocket_route).run(socket_addr).await;
    
        Ok(())
    }

    // pub async fn broadcast_message(msg: Message) -> anyhow::Result<()> {
    //     let clients = CLIENTS.get().read().await;
        
    //     if let Some(user_id) = user_id {
    //         if let Some(client) = clients.get(&user_id) {
    //             if let Some(sender) = &client.sender {
    //                 sender.send(Ok(WebSocketMessage::text(message))).ok();
    //             }
    //         }
    //     } else {
    //         for client in clients.values() {
    //             if let Some(sender) = &client.sender {
    //                 sender.send(Ok(WebSocketMessage::text(message.clone()))).ok();
    //             }
    //         }
    //     }
    
    //     Ok(())
    // }

    pub async fn message_user(user_id: &str, msg: &Message) -> anyhow::Result<()> {
        let clients = CLIENTS.get().read().await;
        let client = clients.get(user_id).ok_or_else(|| anyhow!("No matching user"))?;
        
        let serialized_msg = serde_json::to_string(msg).map_err(|e| anyhow!("Serialization error: {}", e))?;
        
        for sender in &client.senders {
            if let Err(e) = sender.send(Ok(warp::ws::Message::text(serialized_msg.clone()))) {
                eprintln!("Send error: {}", e);
            }
        }
        
        Ok(())
    }

    pub async fn get_clients() -> Vec<ClientInfo> {
        let clients = CLIENTS.get().read().await;
        clients.iter().map(|(user_id, client)| ClientInfo {
            user_id: user_id.clone(),
            subscriptions: client.subscriptions.clone(),
            sender_count: client.senders.len(),
        }).collect()
    }
    
}

async fn handle_connection<R>(
    websocket: WebSocket,
    router: R,
    user_id: String,
) where R: RouterFunction + Clone + 'static,
{
    let (mut ws_tx, mut ws_rx) = websocket.split();
    let (resp_tx, mut resp_rx) = mpsc::unbounded_channel::<Result<WebSocketMessage, warp::Error>>();

    // Adding the new sender to the client's list of connections
    {
        let clients = CLIENTS.get();
        let mut clients_lock = clients.write().await;
        clients_lock.entry(user_id.clone())
            .or_insert_with(Client::default)
            .senders.push(resp_tx.clone());
        println!("added to clients: {:?}", clients_lock);
    }
    
    // Incoming message handling
    let router_clone = router.clone();
    let process_incoming = async move {
        while let Some(result) = ws_rx.next().await {
            match result {
                Ok(msg) => {
                    if let Ok(message_str) = msg.to_str() {
                        if let Ok(message) = serde_json::from_str::<Message>(message_str) {
                            let response = router_clone.route(message).await;
                            if let Ok(response_str) = serde_json::to_string(&response) {
                                let _ = resp_tx.send(Ok(WebSocketMessage::text(response_str)));
                            }
                        }
                    }
                },
                Err(e) => eprintln!("WebSocket receive error: {:?}", e),
            }
        }
    };

    // Outgoing message handling
    let send_responses = async move {
        while let Some(response) = resp_rx.recv().await {
            if let Ok(msg) = response {
                if ws_tx.send(msg).await.is_err() {
                    eprintln!("WebSocket send error");
                    break;
                }
            }
        }
    };

    tokio::select! {
        _ = process_incoming => {},
        _ = send_responses => {},
    };

    // Clean up after disconnection
    let clients = CLIENTS.get();

    let mut clients_lock = clients.write().await;
    if let Some(client) = clients_lock.get_mut(&user_id) {
        println!("Cleaning up clients for user_id: {}", user_id);
        // Retain senders that are NOT closed (i.e., still active)
        client.senders.retain(|sender| !sender.is_closed());
        println!("Active senders remaining for client: {:?}", client.senders.len());
    
        if client.senders.is_empty() {
            println!("No active senders left for {}, removing client.", user_id);
            clients_lock.remove(&user_id);
        } else {
            println!("There are still active senders for {}, not removing client.", user_id);
        }
    }
    
}