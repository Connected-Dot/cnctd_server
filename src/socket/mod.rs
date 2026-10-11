//! The WebSocket endpoint: upgrade, per-connection send/receive loop,
//! disconnect hooks. The connection table itself is [`connection`].

pub mod connection;
pub mod fanout;
#[cfg(feature = "redis")]
pub mod redis_fanout;

use std::{future::Future, marker::PhantomData, net::SocketAddr, pin::Pin, sync::Arc, time::Duration};

use axum::{
    extract::{
        ws::{Message as Frame, WebSocket, WebSocketUpgrade},
        ConnectInfo, Query, State,
    },
    http::HeaderMap,
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use futures_util::{SinkExt, StreamExt};
use serde::{de::DeserializeOwned, Serialize};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

pub use connection::{CnctdConnection, ConnectionFormat, ConnectionInfo, Connections};

use self::connection::{Outbound, QueryParams};
use crate::{
    auth::CnctdAuth,
    router::{error::ErrorResponse, message::Message, SocketRouter},
    utils::client_ip,
};

/// Called with `(connection_id, bytes)` for every inbound binary frame.
pub type OnBinaryHandler = Arc<dyn Fn(String, Vec<u8>) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>;
/// Called once per connection, after its socket closes and before it leaves
/// the table.
pub type OnDisconnect = Arc<dyn Fn(ConnectionInfo) + Send + Sync>;

#[derive(Clone)]
pub struct SocketConfig<R> {
    pub router: R,
    /// JWT secret. With one set, `?token=<jwt>` on the upgrade authenticates
    /// the connection (sets `user_id`, `authenticated`).
    pub secret: Option<Vec<u8>>,
    /// Kept for the Redis fan-out; see `ServerConfig::with_fanout`.
    pub redis_url: Option<String>,
    pub on_disconnect: Option<OnDisconnect>,
    pub on_binary: Option<OnBinaryHandler>,
    /// Where the socket lives. `/ws`.
    pub path: String,
    /// Push `{"channel":"socket","instruction":"connected","data":<ConnectionInfo>}`
    /// as the first frame of every JSON-format connection, so a client that
    /// registered inline learns its `connection_id`. Off by default: the
    /// wire stays exactly 0.6 unless the app opts in.
    pub announce_connected: bool,
    /// Send a WebSocket ping this often so idle connections survive proxies
    /// that reset silent sockets (Cloudflare does so after ~100 s). Browsers
    /// cannot send pings themselves and answer ours automatically. 30 s;
    /// `None` turns it off.
    pub keepalive: Option<Duration>,
}

impl<R> SocketConfig<R> {
    pub fn new(router: R, secret: Option<Vec<u8>>, redis_url: Option<String>, on_disconnect: Option<OnDisconnect>) -> Self {
        Self {
            router,
            secret,
            redis_url,
            on_disconnect,
            on_binary: None,
            path: "/ws".to_string(),
            announce_connected: false,
            keepalive: Some(Duration::from_secs(30)),
        }
    }

    /// See [`SocketConfig::keepalive`].
    pub fn with_keepalive(mut self, every: Option<Duration>) -> Self {
        self.keepalive = every;
        self
    }

    /// See [`SocketConfig::announce_connected`].
    pub fn with_connected_announcement(mut self, on: bool) -> Self {
        self.announce_connected = on;
        self
    }

    pub fn with_on_binary(mut self, handler: OnBinaryHandler) -> Self {
        self.on_binary = Some(handler);
        self
    }

    pub fn with_path(mut self, path: &str) -> Self {
        self.path = if path.starts_with('/') { path.to_string() } else { format!("/{}", path) };
        self
    }
}

/// Compatibility handle: `CnctdSocket::broadcast_message` pushes a
/// [`Message`] through the running server's table.
pub struct CnctdSocket;

impl CnctdSocket {
    pub async fn broadcast_message(msg: &Message) -> anyhow::Result<()> {
        Connections::global()?.broadcast(msg).await
    }
}

struct WsState<Req, Resp, R> {
    router: Arc<R>,
    connections: Connections,
    secret: Option<Vec<u8>>,
    on_disconnect: Option<OnDisconnect>,
    on_binary: Option<OnBinaryHandler>,
    announce_connected: bool,
    keepalive: Option<Duration>,
    _marker: PhantomData<fn() -> (Req, Resp)>,
}

impl<Req, Resp, R> Clone for WsState<Req, Resp, R> {
    fn clone(&self) -> Self {
        Self {
            router: self.router.clone(),
            connections: self.connections.clone(),
            secret: self.secret.clone(),
            on_disconnect: self.on_disconnect.clone(),
            on_binary: self.on_binary.clone(),
            announce_connected: self.announce_connected,
            keepalive: self.keepalive,
            _marker: PhantomData,
        }
    }
}

pub(crate) fn router<Req, Resp, R>(config: SocketConfig<R>, connections: Connections) -> Router
where
    Req: DeserializeOwned + Send + 'static,
    Resp: Serialize + Send + 'static,
    R: SocketRouter<Req, Resp>,
{
    let state = WsState::<Req, Resp, R> {
        router: Arc::new(config.router),
        connections,
        secret: config.secret,
        on_disconnect: config.on_disconnect,
        on_binary: config.on_binary,
        announce_connected: config.announce_connected,
        keepalive: config.keepalive,
        _marker: PhantomData,
    };
    Router::new().route(&config.path, get(upgrade::<Req, Resp, R>)).with_state(state)
}

fn refuse(err: ErrorResponse) -> Response {
    (err.status.status_code(), Json(err)).into_response()
}

async fn upgrade<Req, Resp, R>(
    ws: WebSocketUpgrade,
    State(state): State<WsState<Req, Resp, R>>,
    Query(params): Query<QueryParams>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response
where
    Req: DeserializeOwned + Send + 'static,
    Resp: Serialize + Send + 'static,
    R: SocketRouter<Req, Resp>,
{
    let connections = state.connections.clone();

    // Which connection is this? A registered id, or an inline registration.
    let connection_id = match (params.connection_id, params.subscriptions) {
        (Some(id), _) => {
            if !connections.contains(&id).await {
                return refuse(ErrorResponse::new(None, Some(format!("unknown connection_id {}", id))));
            }
            id
        }
        (None, Some(subs)) => {
            let subscriptions: Vec<String> =
                subs.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
            let format = ConnectionFormat::from_str_opt(params.format.as_deref());
            let id = connections.register(subscriptions, client_ip(&headers, Some(peer)), format).await;
            info!(connection_id = %id, "inline-registered connection");
            id
        }
        (None, None) => {
            return refuse(ErrorResponse::new(
                Some(crate::router::error::ErrorCode::BadRequest),
                Some("connection_id or subscriptions query parameter required".into()),
            ));
        }
    };

    // Optional auth at upgrade.
    if let Some(token) = params.token {
        match &state.secret {
            Some(secret) => match CnctdAuth::verify_auth_token(secret.clone(), &token) {
                Ok(user_id) => {
                    let _ = connections.update_user_id(&connection_id, &user_id).await;
                    let _ = connections.update_authenticated(&connection_id, true).await;
                }
                Err(e) => {
                    return refuse(ErrorResponse::new(
                        Some(crate::router::error::ErrorCode::Unauthorized),
                        Some(format!("invalid token: {}", e)),
                    ));
                }
            },
            None => warn!(connection_id, "token given but the socket has no secret; ignoring"),
        }
    }

    ws.on_upgrade(move |socket| run::<Req, Resp, R>(socket, state, connection_id))
}

async fn run<Req, Resp, R>(socket: WebSocket, state: WsState<Req, Resp, R>, connection_id: String)
where
    Req: DeserializeOwned + Send + 'static,
    Resp: Serialize + Send + 'static,
    R: SocketRouter<Req, Resp>,
{
    let connections = state.connections;
    let (tx, mut rx) = mpsc::unbounded_channel::<Outbound>();
    if let Err(e) = connections.attach_sender(&connection_id, tx.clone()).await {
        warn!(connection_id, "could not attach socket: {}", e);
        return;
    }
    debug!(connection_id, "socket open");

    if state.announce_connected {
        if let Ok(info) = connections.info(&connection_id).await {
            if info.format == ConnectionFormat::Json {
                let hello = Message::new("socket", "connected", Some(serde_json::to_value(&info).unwrap_or_default()));
                if let Ok(s) = serde_json::to_string(&hello) {
                    let _ = tx.send(Outbound::Text(s));
                }
            }
        }
    }

    let (mut ws_tx, mut ws_rx) = socket.split();

    let outbound = async {
        while let Some(out) = rx.recv().await {
            let frame = match out {
                Outbound::Text(s) => Frame::Text(s.into()),
                Outbound::Binary(b) => Frame::Binary(b.into()),
                Outbound::Ping => Frame::Ping(Default::default()),
            };
            if ws_tx.send(frame).await.is_err() {
                break;
            }
        }
    };

    let router = state.router;
    let on_binary = state.on_binary;
    let reply_tx = tx.clone();
    let id_for_inbound = connection_id.clone();
    let inbound = async {
        while let Some(Ok(frame)) = ws_rx.next().await {
            match frame {
                Frame::Text(text) => match serde_json::from_str::<Req>(text.as_str()) {
                    Ok(msg) => {
                        if let Some(reply) = router.route(msg, id_for_inbound.clone()).await {
                            match serde_json::to_string(&reply) {
                                Ok(s) => {
                                    let _ = reply_tx.send(Outbound::Text(s));
                                }
                                Err(e) => warn!(connection_id = %id_for_inbound, "reply not serializable: {}", e),
                            }
                        }
                    }
                    Err(_) => debug!(connection_id = %id_for_inbound, "dropped text frame that is not a request"),
                },
                Frame::Binary(bytes) => {
                    if let Some(handler) = &on_binary {
                        handler(id_for_inbound.clone(), bytes.to_vec()).await;
                    }
                }
                Frame::Close(_) => break,
                Frame::Ping(_) | Frame::Pong(_) => {}
            }
        }
    };

    let keepalive_tx = tx.clone();
    let keepalive = async {
        match state.keepalive {
            Some(every) => {
                let mut tick = tokio::time::interval(every);
                tick.tick().await; // the first tick is immediate
                loop {
                    tick.tick().await;
                    if keepalive_tx.send(Outbound::Ping).is_err() {
                        break;
                    }
                }
            }
            None => std::future::pending::<()>().await,
        }
    };

    tokio::select! {
        _ = outbound => {},
        _ = inbound => {},
        _ = keepalive => {},
    }

    debug!(connection_id, "socket closed");
    if let Some(callback) = state.on_disconnect {
        if let Ok(info) = connections.info(&connection_id).await {
            callback(info);
        }
    }
    connections.remove(&connection_id).await;
}
