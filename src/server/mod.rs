//! Configure and run a server.

mod cors;
mod handlers;
pub mod server_info;
mod spa;

use std::{
    fmt::Debug,
    future::{Future, IntoFuture},
    net::{IpAddr, Ipv4Addr, SocketAddr},
    pin::Pin,
    sync::Arc,
    time::Duration,
};

use axum::{routing::any, Router};
use local_ip_address::local_ip;
use serde::{de::DeserializeOwned, Serialize};
use serde_json::{json, Value};
use tokio::{
    net::TcpListener,
    sync::{oneshot, RwLock},
    task::JoinHandle,
};
use tracing::{error, info};

pub use server_info::{ServerInfo, ServerStatus};

use crate::{
    router::{message::Message, request::split_resource_and_operation, NoSocketRouter, RestRouter, SocketRouter},
    socket::{
        connection::Connections,
        fanout::{Fanout, LocalOnly},
        SocketConfig,
    },
};

use self::handlers::AppState;

/// Extra `<head>` HTML for a page (non-file) request, chosen by the app from
/// the request path — a shared link's title, description and preview image.
/// `None` serves `index.html` unchanged.
pub type PageMetaFn = Arc<dyn Fn(String) -> Pin<Box<dyn Future<Output = Option<String>> + Send>> + Send + Sync>;

#[derive(Clone)]
pub struct PageMeta(pub PageMetaFn);

impl Debug for PageMeta {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PageMeta(..)")
    }
}

pub struct ServerConfig<R> {
    pub id: String,
    /// Port to listen on; `"0"` picks a free one (see [`RunningServer::port`]).
    pub port: String,
    /// API prefix without slashes. `api`.
    pub path: Option<String>,
    /// Directory of the built web client to serve, with `index.html` as the
    /// fallback for every path the API does not claim.
    pub client_dir: Option<String>,
    pub router: R,
    /// How often to push a `server-info` heartbeat to its subscribers.
    pub heartbeat: Option<Duration>,
    /// CORS origins. `None` allows any origin.
    pub allowed_origins: Option<Vec<String>>,
    pub page_meta: Option<PageMeta>,
    /// Largest request body accepted, in bytes. `None` is unlimited.
    pub max_body: Option<usize>,
    /// Reported in the heartbeat. The app knows this better than we do.
    pub public_ip: Option<String>,
    /// Cross-instance push. `None` is a single instance.
    pub fanout: Option<Arc<dyn Fanout>>,
    /// Interface to bind. All of them.
    pub bind_addr: IpAddr,
}

impl<R> Debug for ServerConfig<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServerConfig")
            .field("id", &self.id)
            .field("port", &self.port)
            .field("path", &self.path)
            .field("client_dir", &self.client_dir)
            .field("heartbeat", &self.heartbeat)
            .field("allowed_origins", &self.allowed_origins)
            .field("page_meta", &self.page_meta)
            .field("max_body", &self.max_body)
            .field("bind_addr", &self.bind_addr)
            .finish_non_exhaustive()
    }
}

impl<R> ServerConfig<R> {
    /// `heartbeat` is in seconds.
    pub fn new(
        id: &str,
        port: &str,
        client_dir: Option<String>,
        router: R,
        heartbeat: Option<u64>,
        allowed_origins: Option<Vec<String>>,
        path: Option<String>,
    ) -> Self {
        Self {
            id: id.into(),
            port: port.into(),
            path,
            client_dir,
            router,
            heartbeat: heartbeat.map(Duration::from_secs),
            allowed_origins,
            page_meta: None,
            max_body: None,
            public_ip: None,
            fanout: None,
            bind_addr: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        }
    }

    /// Let the app add `<head>` tags to `index.html` per request path.
    pub fn with_page_meta(mut self, page_meta: PageMetaFn) -> Self {
        self.page_meta = Some(PageMeta(page_meta));
        self
    }

    pub fn with_max_body(mut self, bytes: usize) -> Self {
        self.max_body = Some(bytes);
        self
    }

    pub fn with_public_ip(mut self, ip: &str) -> Self {
        self.public_ip = Some(ip.to_string());
        self
    }

    pub fn with_fanout(mut self, fanout: Arc<dyn Fanout>) -> Self {
        self.fanout = Some(fanout);
        self
    }

    pub fn with_bind_addr(mut self, addr: IpAddr) -> Self {
        self.bind_addr = addr;
        self
    }
}

/// A server that is up. Drop it or call [`RunningServer::shutdown`] to stop.
pub struct RunningServer {
    addr: SocketAddr,
    connections: Connections,
    fanout: Arc<dyn Fanout>,
    server_info: Arc<RwLock<ServerInfo>>,
    shutdown: Option<oneshot::Sender<()>>,
    serve_task: JoinHandle<std::io::Result<()>>,
    heartbeat_task: Option<JoinHandle<()>>,
}

impl RunningServer {
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    pub fn port(&self) -> u16 {
        self.addr.port()
    }

    /// `http://127.0.0.1:<port>` — handy in tests.
    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.addr.port())
    }

    pub fn connections(&self) -> &Connections {
        &self.connections
    }

    pub fn server_info(&self) -> Arc<RwLock<ServerInfo>> {
        self.server_info.clone()
    }

    /// Stop accepting, let in-flight requests finish, return.
    pub async fn shutdown(mut self) -> anyhow::Result<()> {
        if let Some(h) = self.heartbeat_task.take() {
            h.abort();
        }
        self.fanout.shutdown();
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        (&mut self.serve_task).await??;
        Ok(())
    }

    /// Run until the process gets SIGINT or SIGTERM, then shut down.
    pub async fn wait_for_signal(self) -> anyhow::Result<()> {
        shutdown_signal().await;
        info!("shutdown signal received");
        self.shutdown().await
    }
}

impl Drop for RunningServer {
    fn drop(&mut self) {
        if let Some(h) = self.heartbeat_task.take() {
            h.abort();
        }
        self.fanout.shutdown();
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}

pub struct CnctdServer;

impl CnctdServer {
    /// Bind and serve in the background; returns the handle. What tests and
    /// embedders use.
    pub async fn bind<R, SReq, SResp, SR>(
        config: ServerConfig<R>,
        socket_config: Option<SocketConfig<SR>>,
    ) -> anyhow::Result<RunningServer>
    where
        R: RestRouter,
        SReq: DeserializeOwned + Send + 'static,
        SResp: Serialize + Send + 'static,
        SR: SocketRouter<SReq, SResp>,
    {
        let prefix = config.path.clone().unwrap_or_else(|| "api".to_string());
        let port: u16 = config.port.parse()?;
        let fanout: Arc<dyn Fanout> = match config.fanout.clone() {
            Some(f) => f,
            None => match socket_config.as_ref().and_then(|s| s.redis_url.as_deref()) {
                #[cfg(feature = "redis")]
                Some(url) => Arc::new(crate::socket::redis_fanout::RedisFanout::connect(url, &config.id).await?),
                #[cfg(not(feature = "redis"))]
                Some(_) => {
                    tracing::warn!("SocketConfig.redis_url set but the `redis` feature is off; running single-instance");
                    Arc::new(LocalOnly)
                }
                None => Arc::new(LocalOnly),
            },
        };
        let connections = Connections::new(&config.id, fanout.clone());
        connections.install_global();
        fanout.attach(connections.clone()).await?;

        let listener = TcpListener::bind(SocketAddr::new(config.bind_addr, port)).await?;
        let addr = listener.local_addr()?;
        let local = local_ip().map(|ip| ip.to_string()).unwrap_or_else(|_| "127.0.0.1".to_string());

        let server_info = Arc::new(RwLock::new(ServerInfo::new(
            connections.identity(),
            &local,
            config.public_ip.clone(),
            addr.port(),
            socket_config.is_some(),
            socket_config.as_ref().and_then(|s| s.redis_url.clone()),
        )));

        let state = AppState {
            router: Arc::new(config.router),
            prefix: prefix.clone(),
            max_body: config.max_body,
        };

        let mut app: Router = Router::new()
            .route(&format!("/{}", prefix), any(handlers::api::<R>))
            .route(&format!("/{}/{{*rest}}", prefix), any(handlers::api::<R>))
            .route("/redirect", any(handlers::api::<R>))
            .route("/redirect/{*rest}", any(handlers::api::<R>))
            .with_state(state);

        let has_socket = socket_config.is_some();
        if let Some(sc) = socket_config {
            app = app.merge(crate::socket::router::<SReq, SResp, SR>(sc, connections.clone()));
        }

        app = match &config.client_dir {
            Some(dir) => app.fallback_service(spa::router(dir, config.page_meta.clone().map(|m| m.0))),
            None => app.fallback(handlers::not_found),
        };

        let app = app.layer(cors::layer(config.allowed_origins.clone()));

        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        let serve_task = tokio::spawn(
            axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .into_future(),
        );

        let heartbeat_task = config.heartbeat.map(|every| {
            let connections = connections.clone();
            let server_info = server_info.clone();
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(every).await;
                    let count = connections.count().await;
                    let snapshot = {
                        let mut info = server_info.write().await;
                        info.refresh(count);
                        info.clone()
                    };
                    let msg = Message::new("server-info", "heartbeat", Some(json!(snapshot)));
                    if let Err(e) = connections.broadcast(&msg).await {
                        error!("heartbeat broadcast failed: {}", e);
                    }
                }
            })
        });

        info!(
            id = %config.id,
            "server{} running at http://{}:{}",
            if has_socket { " and socket" } else { "" },
            local,
            addr.port()
        );

        Ok(RunningServer {
            addr,
            connections,
            fanout,
            server_info,
            shutdown: Some(shutdown_tx),
            serve_task,
            heartbeat_task,
        })
    }

    /// Serve until SIGINT / SIGTERM. The classic entry point.
    pub async fn start<R, SReq, SResp, SR>(
        server_config: ServerConfig<R>,
        socket_config: Option<SocketConfig<SR>>,
    ) -> anyhow::Result<()>
    where
        R: RestRouter,
        SReq: DeserializeOwned + Send + 'static,
        SResp: Serialize + Send + 'static,
        SR: SocketRouter<SReq, SResp>,
    {
        Self::bind(server_config, socket_config).await?.wait_for_signal().await
    }

    /// Serve until `signal` resolves.
    pub async fn start_with_shutdown<R, SReq, SResp, SR, F>(
        server_config: ServerConfig<R>,
        socket_config: Option<SocketConfig<SR>>,
        signal: F,
    ) -> anyhow::Result<()>
    where
        R: RestRouter,
        SReq: DeserializeOwned + Send + 'static,
        SResp: Serialize + Send + 'static,
        SR: SocketRouter<SReq, SResp>,
        F: Future<Output = ()> + Send,
    {
        let running = Self::bind(server_config, socket_config).await?;
        signal.await;
        running.shutdown().await
    }

    /// REST + static files only, no WebSocket; serve until SIGINT / SIGTERM.
    pub async fn start_without_socket<R: RestRouter>(server_config: ServerConfig<R>) -> anyhow::Result<()> {
        Self::start::<R, Value, Value, NoSocketRouter>(server_config, None).await
    }

    /// `/api/dots/create` -> `("dots", Some("create"))`. Assumes the default
    /// `api` prefix; [`crate::Request::resource_and_operation`] knows the
    /// real one.
    pub fn path_to_resource_and_operation(path: &str) -> (String, Option<String>) {
        split_resource_and_operation(path, "api")
    }
}
