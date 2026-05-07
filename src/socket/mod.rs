pub mod connection;

use anyhow::anyhow;
use connection::{ConnectionFormat, ConnectionInfo, CnctdConnection, QueryParams};
use cnctd_redis::CnctdRedis;
use futures_util::{SinkExt, StreamExt};
use local_ip_address::local_ip;
use serde::de::DeserializeOwned;
use serde::Serialize;
use state::InitCell;
use warp::filters::ws::Ws;
use warp::reject::Reject;
use warp::ws::{Message as WebSocketMessage, WebSocket};
use warp::Filter;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use tokio::sync::{mpsc, RwLock};
use std::{sync::Arc, fmt::Debug};

use crate::router::message::Message;
use crate::router::SocketRouterFunction;
use crate::server::server_info::ServerInfo;

/// Callback type for handling incoming binary WebSocket frames.
/// Arguments: (connection_id, raw_bytes)
pub type OnBinaryHandler = Arc<dyn Fn(String, Vec<u8>) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>;

#[derive(Debug)]
struct NoConnectionId;

impl Reject for NoConnectionId {}

#[derive(Clone)]
pub struct SocketConfig<R> {
    pub router: R,
    pub secret: Option<Vec<u8>>,
    pub redis_url: Option<String>,
    pub on_disconnect: Option<Arc<dyn Fn(ConnectionInfo) + Send + Sync>>,
    pub on_binary: Option<OnBinaryHandler>,
}

impl<R> SocketConfig<R> {
    pub fn new(router: R, secret: Option<Vec<u8>>, redis_url: Option<String>, on_disconnect: Option<Arc<dyn Fn(ConnectionInfo) + Send + Sync>>,) -> Self {
        Self {
            router,
            secret,
            redis_url,
            on_disconnect,
            on_binary: None,
        }
    }

    pub fn with_on_binary(mut self, handler: OnBinaryHandler) -> Self {
        self.on_binary = Some(handler);
        self
    }
}



pub static CONNECTIONS: InitCell<Arc<RwLock<HashMap<String, CnctdConnection>>>> = InitCell::new();

pub struct CnctdSocket;

impl CnctdSocket {
    pub fn build_routes<M, Resp, R>(config: SocketConfig<R>) -> warp::filters::BoxedFilter<(impl warp::Reply,)>
    where
        M: Serialize + DeserializeOwned + Send + Sync + Debug + Clone + 'static,
        Resp: Serialize + DeserializeOwned + Send + Sync + Debug + Clone + 'static,
        R: SocketRouterFunction<M, Resp> + 'static,
    {
        CONNECTIONS.set(Arc::new(RwLock::new(HashMap::new())));

        let redis;

        match config.redis_url {
            Some(url) => {
                match cnctd_redis::CnctdRedis::start(&url) {
                    Ok(_) => {
                        println!("Redis started!");
                        tokio::spawn(async {
                            ServerInfo::set_redis_active(true).await;
                        });
                        redis = true
                    },
                    Err(e) => {
                        println!("Error starting Redis pool: {:?}", e);
                        redis = false
                    }
                }
            }
            None => redis = false
        };

        let websocket_route = warp::path("ws")
            .and(warp::ws())
            .and(warp::any().map(move || config.router.clone()))
            .and(warp::query::<QueryParams>())
            .and_then(move |ws: Ws, router: R, params: QueryParams| {
                let on_disconnect = config.on_disconnect.clone();
                let on_binary = config.on_binary.clone();

                async move {
                    // Resolve connection_id: either from query param or via inline registration
                    let connection_id = match params.connection_id {
                        Some(id) => id,
                        None => {
                            // Support inline registration: if subscriptions are provided,
                            // auto-register a new connection (used by ESP32 and other lightweight
                            // clients that don't have an HTTP client for the REST registration step).
                            if let Some(ref subs_str) = params.subscriptions {
                                let subscriptions: Vec<String> = subs_str
                                    .split(',')
                                    .map(|s| s.trim().to_string())
                                    .filter(|s| !s.is_empty())
                                    .collect();
                                let format = ConnectionFormat::from_str_opt(params.format.as_deref());
                                match CnctdConnection::register_connection_with_format(
                                    subscriptions,
                                    None,
                                    format,
                                ).await {
                                    Ok(id) => {
                                        println!("Inline-registered connection: {}", id);
                                        id
                                    }
                                    Err(e) => {
                                        eprintln!("Inline registration failed: {:?}", e);
                                        return Err(warp::reject::custom(NoConnectionId));
                                    }
                                }
                            } else {
                                return Err(warp::reject::custom(NoConnectionId));
                            }
                        },
                    };

                    // Proceed with connection setup
                    Ok(ws.on_upgrade(move |socket| {
                        Self::handle_connection(socket, router, connection_id, redis, on_disconnect.clone(), on_binary)
                    }))
                }
            });


        let routes = websocket_route;

        routes.boxed()

    }
    pub async fn start<M, Resp, R>(port: &str, router: R, secret: Option<Vec<u8>>, redis_url: Option<String>, on_disconnect: Option<Arc<dyn Fn(ConnectionInfo) + Send + Sync>>,) -> anyhow::Result<()>
    where
        M: Serialize + DeserializeOwned + Send + Sync + Debug + Clone + 'static,
        Resp: Serialize + DeserializeOwned + Send + Sync + Debug + Clone + 'static,
        R: SocketRouterFunction<M, Resp> + 'static,
    {
        CONNECTIONS.set(Arc::new(RwLock::new(HashMap::new())));

        let my_local_ip = local_ip()?;
        println!("WebSocket server running at ws://{}:{}", my_local_ip, port);
        let ip_address: [u8; 4] = [0, 0, 0, 0];
        let parsed_port = port.parse::<u16>()?;
        let socket_addr = std::net::SocketAddr::from((ip_address, parsed_port));
        let config = SocketConfig::new(router, secret, redis_url, on_disconnect);
        let routes = Self::build_routes(config);

        warp::serve(routes).run(socket_addr).await;

        Ok(())

    }

    pub async fn broadcast_message(msg: &Message) -> anyhow::Result<()> {
        let connections = CONNECTIONS.try_get().ok_or_else(|| anyhow!("Connections not initialized"))?.read().await;

        for (connection_id, connection) in connections.iter() {
            if connection.subscriptions.contains(&msg.channel) {
                CnctdConnection::message_connection(&connection_id, msg).await?;
            }
        }

        Ok(())
    }



    async fn handle_connection<M, Resp, R>(
        websocket: WebSocket,
        router: R,
        connection_id: String,
        redis: bool,
        on_disconnect: Option<Arc<dyn Fn(ConnectionInfo) + Send + Sync>>,
        on_binary: Option<OnBinaryHandler>,
    ) where
        M: Serialize + DeserializeOwned + Send + Sync + Debug + Clone + 'static,
        Resp: Serialize + DeserializeOwned + Send + Sync + Debug + Clone + 'static,
        R: SocketRouterFunction<M, Resp> + 'static,
    {
        let (mut ws_tx, mut ws_rx) = websocket.split();
        let (resp_tx, mut resp_rx) = mpsc::unbounded_channel::<Result<WebSocketMessage, warp::Error>>();

        {
            let connections = CONNECTIONS.get();
            let mut connections_lock = connections.write().await;

            if let Some(connection) = connections_lock.get_mut(&connection_id.clone()) {
                connection.sender = Some(resp_tx.clone());

                if redis {
                    match Self::push_connection_to_redis(&connection_id, &connection.clone()).await {
                        Ok(_) => println!("pajama party"),
                        Err(e) => eprintln!("Error pushing connection to Redis: {:?}", e),
                    }
                }
                println!("Updated connection sender: {:?}", connection);
            } else {
                eprintln!("Connection with id {} not found.", connection_id);
                return;
            }
        }

        let connection_id_clone = connection_id.clone();
        // Incoming message handling
        let process_incoming = async move {
            while let Some(result) = ws_rx.next().await {
                match result {
                    Ok(msg) => {
                        if msg.is_binary() {
                            if let Some(ref handler) = on_binary {
                                let bytes = msg.into_bytes();
                                handler(connection_id_clone.clone(), bytes).await;
                            }
                        } else if let Ok(message_str) = msg.to_str() {
                            if let Ok(message) = serde_json::from_str::<M>(message_str) {
                                match router.route(message, connection_id_clone.clone()).await {
                                    Some(response) => {
                                        if let Ok(response_str) = serde_json::to_string(&response) {
                                            let _ = resp_tx.send(Ok(WebSocketMessage::text(response_str)));
                                        }
                                    },
                                    None => {}
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

        if let Some(callback) = on_disconnect {
            let info = CnctdConnection::get_connection_info(&connection_id).await.unwrap();
            callback(info);
        }

        // Clean up after disconnection
        match Self::remove_connection(&connection_id).await {
            Ok(_) => {},
            Err(e) => eprintln!("Error removing connection: {:?}", e),
        };

        if redis {
            match Self::remove_connection_from_redis(&connection_id).await {
                Ok(_) => {},
                Err(e) => eprintln!("Error removing connection from Redis: {:?}", e),
            }
        }



    }

    pub async fn remove_connection(connection_id: &str) -> anyhow::Result<()> {
        let connections = CONNECTIONS.try_get().ok_or_else(|| anyhow!("Connections not initialized"))?;
        let mut connections_lock = connections.write().await;

        if let Some(connection) = connections_lock.get(connection_id) {
            let should_remove = connection.sender.as_ref().map_or(true, |sender| sender.is_closed());

            if should_remove {
                println!("Removing connection: {}", connection_id);
                connections_lock.remove(connection_id);
            } else {
                println!("Connection {} is active; no removal necessary.", connection_id);
            }
        }

        Ok(())
    }



    pub async fn push_connection_to_redis(connection_id: &str, connection: &CnctdConnection) -> anyhow::Result<()> {
        let info = connection.to_connection_info(connection_id).await;
        CnctdRedis::hset("connections", &connection_id, info)?;

        Ok(())
    }

    pub async fn remove_connection_from_redis(connection_id: &str) -> anyhow::Result<()> {
        CnctdRedis::hset("connections", connection_id, ())?;

        Ok(())
    }

}
