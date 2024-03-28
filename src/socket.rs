use futures_util::{FutureExt, SinkExt, StreamExt};
use local_ip_address::local_ip;
use tokio_stream::wrappers::UnboundedReceiverStream;
use warp::ws::{Message as WebSocketMessage, WebSocket};
use warp::Filter;
use std::collections::HashMap;
use tokio::sync::{mpsc, Mutex, RwLock};
use std::sync::Arc;

use crate::message::Message;
use crate::router::RouterFunction;

type Users = Arc<RwLock<HashMap<String, mpsc::UnboundedSender<Result<WebSocketMessage, warp::Error>>>>>;

pub struct CnctdSocket;

impl CnctdSocket {
    pub async fn start<R>(port: &str, router: R) -> anyhow::Result<()>
    where
        R: RouterFunction + Clone + 'static,
    {
        let users = Users::default();
    
        let users_clone = users.clone();
        let websocket_route = warp::ws()
            .and(warp::any().map(move || users_clone.clone()))
            .and(warp::any().map(move || router.clone()))
            .map(|ws: warp::ws::Ws, users: Users, router: R| {
                ws.on_upgrade(move |socket| handle_connection(socket, users, router))
            });
        
        let my_local_ip = local_ip()?;
        println!("WebSocket server running at ws://{}:{}", my_local_ip, port);
        let ip_address: [u8; 4] = [0, 0, 0, 0];
        let parsed_port = port.parse::<u16>()?;
        let socket_addr = std::net::SocketAddr::from((ip_address, parsed_port));

        warp::serve(websocket_route).run(socket_addr).await;
    
        Ok(())
    }

    pub async fn broadcast_message(users: Users, user_id: Option<String>, message: String) -> anyhow::Result<()> {
        let message = WebSocketMessage::text(message);
        let users = users.read().await;
        
        match user_id {
            Some(user_id) => {
                if let Some(sender) = users.get(&user_id) {
                    sender.send(Ok(message)).ok();
                }
            },
            None => {
                for sender in users.values() {
                    sender.send(Ok(message.clone())).ok();
                }
            }
        }
    
        Ok(())
    }
}

async fn handle_connection<R>(
    websocket: WebSocket,
    users: Users,
    router: R,
) where R: RouterFunction,
{
    let (user_ws_tx, mut user_ws_rx) = websocket.split();
    let user_ws_tx = Arc::new(Mutex::new(user_ws_tx));
    let (tx, rx) = mpsc::unbounded_channel();
    let rx_stream = UnboundedReceiverStream::new(rx);

    let user_id = "user_id_example".to_string(); // This should be uniquely generated or identified
    users.write().await.insert(user_id.clone(), tx);

    let user_ws_tx_clone = user_ws_tx.clone();
    let process_incoming = user_ws_rx.for_each(move |message| {
        let user_ws_tx = user_ws_tx_clone.clone();
        async move {
            if let Ok(msg) = message {
                if let Ok(message_str) = msg.to_str() {
                    if let Ok(message) = serde_json::from_str::<Message>(message_str) {
                        let response = router.route(message).await;
                        if let Ok(response_str) = serde_json::to_string(&response) {
                            let ws_response = WebSocketMessage::text(response_str);
                            let mut tx = user_ws_tx.lock().await;
                            if let Err(e) = tx.send(ws_response).await {
                                eprintln!("Error sending response: {}", e);
                            }
                        }
                    }
                }
            }
        }
    });

    let process_outgoing = async move {
        let mut rx_stream = rx_stream;
        while let Some(message_result) = rx_stream.next().await {
            match message_result {
                Ok(message) => {
                    let mut lock = user_ws_tx.lock().await;
                    if let Err(e) = lock.send(message).await {
                        eprintln!("Error sending message: {}", e);
                    }
                },
                Err(e) => eprintln!("Error preparing message for sending: {}", e),
            }
        }
    };

    tokio::select! {
        _ = process_incoming => (),
        _ = process_outgoing => (),
    }

    // Assuming removal is handled correctly elsewhere or after the loop
    users.write().await.remove(&user_id);
}
