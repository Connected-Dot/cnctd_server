use std::{collections::{HashMap, HashSet}, sync::Arc};
use futures_util::StreamExt;
use tokio::sync::{mpsc, RwLock};
use warp::{filters::ws::Message as SocketMessage, ws::WebSocket, Filter};

use crate::router::RouterFunction;

type UserId = String;
type Channel = String;
type ClientSender = mpsc::UnboundedSender<warp::ws::Message>;

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

    pub async fn initiate_ws_connection<R>(socket_server: Arc<Self>, socket: WebSocket, router: Arc<R>)
    where
        R: RouterFunction + 'static,
    {
        socket_server.handle_ws_connection(socket, router).await;
    }

    pub async fn start<R>(port: &str, router: R) -> anyhow::Result<()>
        where R: RouterFunction + 'static + Clone
    {
        let socket_server = Arc::new(Self::new());
        let router = Arc::new(router);

        let ws_route = warp::path("ws")
            .and(warp::ws())
            .map(move |ws: warp::ws::Ws| {
                let socket_server_clone = Arc::clone(&socket_server);
                let router_clone = Arc::clone(&router);
        
                ws.on_upgrade(move |socket| {
                    Self::initiate_ws_connection(socket_server_clone, socket, router_clone)
                })
            });

        let ip_address: [u8; 4] = [0, 0, 0, 0];
        let parsed_port = port.parse::<u16>()?;
        let socket_addr = std::net::SocketAddr::from((ip_address, parsed_port));
        
        warp::serve(ws_route)
            .run(socket_addr)
            .await;

        Ok(())
    }

    async fn handle_ws_connection<R>(&self, ws: WebSocket, router: Arc<R>)
            where  R: RouterFunction + 'static
        {

        let (mut tx, mut rx) = ws.split();
        while let Some(result) = rx.next().await {
            match result {
                Ok(ws_msg) => {
                    if ws_msg.is_text() {
                        let text = ws_msg.to_str().unwrap(); // It's essential to handle errors appropriately in production code
                        println!("Received WebSocket message: {}", text); // Print the raw text of the WebSocket message
                
                        // Attempt to deserialize the text message into your custom Message struct
                        // Note: This step may not be necessary if you're just inspecting the message
                        // if let Ok(msg) = serde_json::from_str::<SocketMessage>(text) {
                            // Placeholder: Handle the message with your router or other logic
                            // This example does not proceed with routing or sending a response
                            // but you can implement that logic as needed based on your requirements
                        // }
                    }
                },
                Err(e) => {
                    eprintln!("WebSocket error: {:?}", e);
                    break;
                },
            }
        }
    }
}