use std::{collections::HashMap, fmt::Debug, sync::Arc};
use axum::{
    extract::{
        ws::{Message as AxumMessage, WebSocket, WebSocketUpgrade},
        Query, State,
    },
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Router,
};
use futures_util::{SinkExt, StreamExt};
use serde::{de::DeserializeOwned, Serialize};
use state::InitCell;
use tokio::sync::{mpsc, RwLock};
use anyhow::anyhow;

use crate::router::message::Message;
use crate::router::SocketRouterFunction;
use crate::server::server_info::ServerInfo;
use crate::socket::client::{ClientInfo, CnctdClient, QueryParams};
use super::SocketConfig;

pub static CLIENTS: InitCell<Arc<RwLock<HashMap<String, CnctdClient>>>> = InitCell::new();

#[derive(Clone)]
struct WsState<R> {
    router: R,
    redis: bool,
    on_disconnect: Option<Arc<dyn Fn(ClientInfo) + Send + Sync>>,
}

pub struct CnctdSocket;

impl CnctdSocket {
    pub fn build_axum_routes<M, Resp, R>(config: SocketConfig<R>) -> Router
    where
        M: Serialize + DeserializeOwned + Send + Sync + Debug + Clone + 'static,
        Resp: Serialize + DeserializeOwned + Send + Sync + Debug + Clone + 'static,
        R: SocketRouterFunction<M, Resp> + 'static,
    {
        CLIENTS.set(Arc::new(RwLock::new(HashMap::new())));

        // Initialize Redis if configured
        let redis = if let Some(ref url) = config.redis_url {
            match cnctd_redis::CnctdRedis::start(url) {
                Ok(_) => {
                    println!("Redis started!");
                    tokio::spawn(async {
                        ServerInfo::set_redis_active(true).await;
                    });
                    true
                }
                Err(e) => {
                    println!("Error starting Redis pool: {:?}", e);
                    false
                }
            }
        } else {
            false
        };

        let state = WsState {
            router: config.router,
            redis,
            on_disconnect: config.on_disconnect,
        };

        Router::new()
            .route("/ws", get(ws_handler::<M, Resp, R>))
            .with_state(state)
    }

    async fn handle_connection<M, Resp, R>(
        websocket: WebSocket,
        router: R,
        client_id: String,
        redis: bool,
        on_disconnect: Option<Arc<dyn Fn(ClientInfo) + Send + Sync>>,
    ) where
        M: Serialize + DeserializeOwned + Send + Sync + Debug + Clone + 'static,
        Resp: Serialize + DeserializeOwned + Send + Sync + Debug + Clone + 'static,
        R: SocketRouterFunction<M, Resp> + 'static,
    {
        let (mut ws_tx, mut ws_rx) = websocket.split();
        let (resp_tx, mut resp_rx) = mpsc::unbounded_channel::<Result<AxumMessage, axum::Error>>();

        // Update client sender in CLIENTS map
        {
            let clients = CLIENTS.get();
            let mut clients_lock = clients.write().await;

            if let Some(client) = clients_lock.get_mut(&client_id) {
                // Update the sender for the client
                client.sender = Some(resp_tx.clone());

                if redis {
                    match Self::push_client_to_redis(&client_id, &client.clone()).await {
                        Ok(_) => println!("Client pushed to Redis"),
                        Err(e) => eprintln!("Error pushing client to Redis: {:?}", e),
                    }
                }
                println!("Updated client sender: {:?}", client);
            } else {
                // Log error or handle case where client_id is not found
                eprintln!("Client with id {} not found.", client_id);
                return;
            }
        }

        let client_id_clone = client_id.clone();

        // Handle incoming messages
        let process_incoming = async move {
            while let Some(result) = ws_rx.next().await {
                match result {
                    Ok(msg) => {
                        if let AxumMessage::Text(text) = msg {
                            if let Ok(message) = serde_json::from_str::<M>(&text) {
                                match router.route(message, client_id_clone.clone()).await {
                                    Some(response) => {
                                        if let Ok(response_str) = serde_json::to_string(&response) {
                                            let _ = resp_tx.send(Ok(AxumMessage::Text(response_str)));
                                        }
                                    }
                                    None => {}
                                }
                            }
                        }
                    }
                    Err(e) => eprintln!("WebSocket receive error: {:?}", e),
                }
            }
        };

        // Handle outgoing messages
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

        // Handle disconnect
        if let Some(callback) = on_disconnect {
            if let Ok(client_info) = CnctdClient::get_client_info(&client_id).await {
                callback(client_info);
            }
        }

        // Cleanup
        match Self::remove_client(&client_id).await {
            Ok(_) => {}
            Err(e) => eprintln!("Error removing client: {:?}", e),
        };

        if redis {
            match Self::remove_client_from_redis(&client_id).await {
                Ok(_) => {}
                Err(e) => eprintln!("Error removing client from Redis: {:?}", e),
            }
        }
    }

    pub async fn broadcast_message(msg: &Message) -> anyhow::Result<()> {
        let clients = CLIENTS
            .try_get()
            .ok_or_else(|| anyhow!("Clients not initialized"))?
            .read()
            .await;

        for (client_id, client) in clients.iter() {
            if client.subscriptions.contains(&msg.channel) {
                CnctdClient::message_client(&client_id, msg).await?;
            }
        }

        Ok(())
    }

    async fn remove_client(client_id: &str) -> anyhow::Result<()> {
        let clients = CLIENTS
            .try_get()
            .ok_or_else(|| anyhow!("Clients not initialized"))?;
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

        Ok(())
    }

    async fn push_client_to_redis(client_id: &str, client: &CnctdClient) -> anyhow::Result<()> {
        let client_info = client.to_client_info(client_id).await;
        cnctd_redis::CnctdRedis::hset("clients", &client_id, client_info)?;

        Ok(())
    }

    async fn remove_client_from_redis(client_id: &str) -> anyhow::Result<()> {
        cnctd_redis::CnctdRedis::hset("clients", client_id, ())?;

        Ok(())
    }
}

async fn ws_handler<M, Resp, R>(
    ws: WebSocketUpgrade,
    State(state): State<WsState<R>>,
    Query(params): Query<QueryParams>,
) -> impl IntoResponse
where
    M: Serialize + DeserializeOwned + Send + Sync + Debug + Clone + 'static,
    Resp: Serialize + DeserializeOwned + Send + Sync + Debug + Clone + 'static,
    R: SocketRouterFunction<M, Resp> + 'static,
{
    let client_id = match params.client_id {
        Some(id) => id,
        None => {
            return (StatusCode::BAD_REQUEST, "Missing client_id parameter").into_response();
        }
    };

    ws.on_upgrade(move |socket| {
        CnctdSocket::handle_connection(
            socket,
            state.router,
            client_id,
            state.redis,
            state.on_disconnect,
        )
    })
}
