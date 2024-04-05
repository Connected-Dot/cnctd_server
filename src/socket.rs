use anyhow::anyhow;
use futures_util::{SinkExt, StreamExt};
use local_ip_address::local_ip;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use state::InitCell;
use warp::filters::ws::Ws;
use warp::reject::Reject;
use warp::ws::{Message as WebSocketMessage, WebSocket};
use warp::Filter;
use std::collections::HashMap;
use tokio::sync::{mpsc, RwLock};
use std::{sync::Arc, fmt::Debug};

use crate::handlers::{ClientQuery, Handler};
use crate::message::Message;
use crate::router::SocketRouterFunction;

#[derive(Debug)]
struct NoClientId;

impl Reject for NoClientId {}

#[derive(Debug)]
pub struct SocketConfig<R> {
    pub router: R,
    pub secret: Option<Vec<u8>>
}

impl<R> SocketConfig<R> {
    pub fn new(router: R, secret: Option<Vec<u8>>) -> Self {
        Self {
            router,
            secret
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct ClientInfo {
    client_id: String,
    user_id: String,
    subscriptions: Vec<String>,
    connected: bool,
}

#[derive(Debug, Deserialize)]
struct QueryParams {
    pub client_id: Option<String>,
}

type Sender = mpsc::UnboundedSender<std::result::Result<warp::ws::Message, warp::Error>>;

#[derive(Debug, Clone)]
pub struct Client {
    pub user_id: String,
    pub subscriptions: Vec<String>,
    pub sender: Option<Sender>,
}

impl Client {
    pub fn new(user_id: String, subscriptions: Vec<String>) -> Self {
        Self {
            user_id,
            subscriptions,
            sender: None,
        }
    }
}

pub static CLIENTS: InitCell<Arc<RwLock<HashMap<String, Client>>>> = InitCell::new();

pub struct CnctdSocket;

impl CnctdSocket {
    pub fn build_route<M, Resp, R>(router: R, secret: Option<Vec<u8>>) -> warp::filters::BoxedFilter<(impl warp::Reply,)>
    where
        M: Serialize + DeserializeOwned + Send + Sync + Debug + Clone + 'static,
        Resp: Serialize + DeserializeOwned + Send + Sync + Debug + Clone + 'static, 
        R: SocketRouterFunction<M, Resp> + 'static,
    {
        CLIENTS.set(Arc::new(RwLock::new(HashMap::new())));

        let registration_route = warp::path("register")
            .and(warp::get())
            .and(warp::header::optional("Authorization"))
            .and(warp::query::<ClientQuery>())
            .and_then(move |auth_token: Option<String>, client_query: ClientQuery| {
                let secret_clone = secret.clone();
                async move {
                    Handler::register_socket_client(client_query, secret_clone, auth_token).await
                }
            });

        let websocket_route = warp::path("ws")
            .and(warp::ws())
            .and(warp::any().map(move || router.clone()))
            .and(warp::query::<QueryParams>())
            .and_then(move |ws: Ws, router: R, params: QueryParams| {

                async move {
                    // Check for the presence of client_id
                    let client_id = match params.client_id {
                        Some(id) => id,
                        None => {
                            return Err(warp::reject::custom(NoClientId))
                        },
                    };
                    

                    // Proceed with connection setup
                    Ok(ws.on_upgrade(move |socket| {
                        Self::handle_connection(socket, router, client_id)
                    }))
                }
            });
              
        let routes = registration_route.or(websocket_route);

        routes.boxed()

    }
    pub async fn start<M, Resp, R>(port: &str, router: R, secret: Option<Vec<u8>>) -> anyhow::Result<()>
    where
        M: Serialize + DeserializeOwned + Send + Sync + Debug + Clone + 'static,
        Resp: Serialize + DeserializeOwned + Send + Sync + Debug + Clone + 'static, 
        R: SocketRouterFunction<M, Resp> + 'static,
    {
        CLIENTS.set(Arc::new(RwLock::new(HashMap::new())));
    
        let my_local_ip = local_ip()?;
        println!("WebSocket server running at ws://{}:{}", my_local_ip, port);
        let ip_address: [u8; 4] = [0, 0, 0, 0];
        let parsed_port = port.parse::<u16>()?;
        let socket_addr = std::net::SocketAddr::from((ip_address, parsed_port));
        let routes = Self::build_route(router, secret);

        warp::serve(routes).run(socket_addr).await;
    
        Ok(())
        
    }

    pub async fn broadcast_message(msg: &Message) -> anyhow::Result<()> {
        let clients = CLIENTS.get().read().await;
        
        for (user_id, client) in clients.iter() {
            if client.subscriptions.contains(&msg.channel) {
                Self::message_user(&user_id, &msg).await?;
            }
        }
    
        Ok(())
    }

    pub async fn message_user(client_id: &str, msg: &Message) -> anyhow::Result<()> {
        let client = Self::get_client(client_id).await?;
        
        // Serialize the message only if a sender exists
        if let Some(sender) = &client.sender {
            let serialized_msg = serde_json::to_string(msg).map_err(|e| anyhow!("Serialization error: {}", e))?;
            
            // Attempt to send the serialized message
            if let Err(e) = sender.send(Ok(warp::ws::Message::text(serialized_msg))) {
                eprintln!("Send error: {}", e);
            }
        } else {
            return Err(anyhow!("Client with id {} has no active sender", client_id));
        }
        
        Ok(())
    }
    

    pub async fn get_clients() -> Vec<ClientInfo> {
        let clients = CLIENTS.get().read().await;
        clients.iter().map(|(client_id, client)| ClientInfo {
            client_id: client_id.into(), 
            user_id: client.user_id.to_string(),
            subscriptions: client.subscriptions.clone(),
            connected: client.sender.is_some(),
        }).collect()
    }
    
    async fn handle_connection<M, Resp, R>(
        websocket: WebSocket,
        router: R,
        client_id: String,
    ) where 
        M: Serialize + DeserializeOwned + Send + Sync + Debug + Clone + 'static,
        Resp: Serialize + DeserializeOwned + Send + Sync + Debug + Clone + 'static, 
        R: SocketRouterFunction<M, Resp> + 'static,
    {
        let (mut ws_tx, mut ws_rx) = websocket.split();
        let (resp_tx, mut resp_rx) = mpsc::unbounded_channel::<Result<WebSocketMessage, warp::Error>>();
    
        {
            let clients = CLIENTS.get();
            let mut clients_lock = clients.write().await;
    
            if let Some(client) = clients_lock.get_mut(&client_id) {
                // Update the sender for the client
                client.sender = Some(resp_tx.clone());
                println!("Updated client sender: {:?}", client);
            } else {
                // Log error or handle case where client_id is not found
                eprintln!("Client with id {} not found.", client_id);
                return;
            }
        }
        
        // Incoming message handling
        let process_incoming = async move {
            while let Some(result) = ws_rx.next().await {
                match result {
                    Ok(msg) => {
                        if let Ok(message_str) = msg.to_str() {
                            println!("Message string: {}", message_str);
                            if let Ok(message) = serde_json::from_str::<M>(message_str) {
                                let response = router.route(message).await;
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
        Self::remove_client(&client_id).await;
        
    }

    pub async fn remove_client(client_id: &str) {
        let clients = CLIENTS.get();
        let mut clients_lock = clients.write().await;
    
        if let Some(client) = clients_lock.get(client_id) {
            let should_remove = client.sender.as_ref().map_or(true, |sender| sender.is_closed());
    
            if should_remove {
                println!("Removing client: {}", client_id);
                clients_lock.remove(client_id);
            } else {
                println!("Client {} is active; no removal necessary.", client_id);
            }
        }
    }

    pub async fn get_client(client_id: &str) -> anyhow::Result<Client> {
        let clients = CLIENTS.get().read().await;
        let client = clients.get(client_id).ok_or_else(|| anyhow!("No matching client"))?;

        Ok(client.to_owned())
    }
}

