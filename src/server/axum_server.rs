use std::{sync::Arc, time::Duration, net::SocketAddr};
use axum::{
    Router,
    routing::{get, post, put, delete as axum_delete},
    extract::{State, Query},
    http::{HeaderMap, StatusCode, Uri},
    response::{IntoResponse, Response, Redirect, Html},
    Json,
};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
use tower::ServiceBuilder;
use tower_http::cors::{CorsLayer, Any};
use tower_http::services::{ServeDir, ServeFile};
use local_ip_address::local_ip;

use crate::{
    router::{RestRouterFunction, SocketRouterFunction},
    server::{
        server_info::{ServerInfo, SERVER_INFO},
        handlers::{Handler, RedirectHandler},
        extractors::{ClientIp, FullPath},
    },
    socket::{CnctdSocket, SocketConfig},
};

#[derive(Debug)]
pub struct ServerConfig<R> {
    pub id: String,
    pub port: String,
    pub path: Option<String>,
    pub client_dir: Option<String>,
    pub router: R,
    pub heartbeat: Option<Duration>,
    pub allowed_origins: Option<Vec<String>>,
}

impl<R> ServerConfig<R> {
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
            path,
            port: port.into(),
            client_dir,
            router,
            heartbeat: heartbeat.map(Duration::from_secs),
            allowed_origins,
        }
    }
}

// Shared state for handlers
#[derive(Clone)]
struct AppState<R> {
    router: Arc<R>,
}

pub struct CnctdServer;

impl CnctdServer {
    pub async fn start<R, SReq, SResp, SR>(
        server_config: ServerConfig<R>,
        socket_config: Option<SocketConfig<SR>>,
    ) -> anyhow::Result<()>
    where
        R: RestRouterFunction + 'static,
        SReq: Serialize + DeserializeOwned + Send + Sync + std::fmt::Debug + Clone + 'static,
        SResp: Serialize + DeserializeOwned + Send + Sync + std::fmt::Debug + Clone + 'static,
        SR: SocketRouterFunction<SReq, SResp> + 'static,
    {
        let my_local_ip = local_ip()?;
        let parsed_port = server_config.port.parse::<u16>()?;
        let socket_addr = SocketAddr::from(([0, 0, 0, 0], parsed_port));

        // Build the application router
        let app = Self::build_app(server_config.clone(), socket_config.clone()).await?;

        // Setup server info
        match socket_config {
            Some(ref config) => {
                let server_info = ServerInfo::new(
                    &server_config.id,
                    &my_local_ip.to_string(),
                    parsed_port,
                    100,
                    true,
                    config.redis_url.clone(),
                )
                .await;

                SERVER_INFO.set(server_info.clone());
                println!("server and socket running at http://{}:{}", my_local_ip, parsed_port);
                println!("server info: {:?}", server_info);
            }
            None => {
                let server_info = ServerInfo::new(
                    &server_config.id,
                    &my_local_ip.to_string(),
                    parsed_port,
                    100,
                    true,
                    None,
                )
                .await;

                SERVER_INFO.set(server_info.clone());
                println!("server running at http://{}:{}", my_local_ip, parsed_port);
                println!("server info: {:?}", server_info);
            }
        }

        // Start heartbeat task if configured
        if let Some(heartbeat) = server_config.heartbeat {
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(heartbeat).await;
                    match ServerInfo::update().await {
                        Ok(_) => {}
                        Err(e) => {
                            eprintln!("Error updating server info: {:?}", e);
                        }
                    }
                }
            });
        }

        // Start server
        let listener = tokio::net::TcpListener::bind(socket_addr).await?;
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await?;

        Ok(())
    }

    async fn build_app<R, SReq, SResp, SR>(
        server_config: ServerConfig<R>,
        socket_config: Option<SocketConfig<SR>>,
    ) -> anyhow::Result<Router>
    where
        R: RestRouterFunction + 'static,
        SReq: Serialize + DeserializeOwned + Send + Sync + std::fmt::Debug + Clone + 'static,
        SResp: Serialize + DeserializeOwned + Send + Sync + std::fmt::Debug + Clone + 'static,
        SR: SocketRouterFunction<SReq, SResp> + 'static,
    {
        let state = AppState {
            router: Arc::new(server_config.router),
        };

        // Build REST routes
        let path_prefix = server_config.path.unwrap_or_else(|| "api".to_string());
        let rest_router = Self::build_rest_routes::<R>(path_prefix);

        // Build complete router
        let mut app = Router::new()
            .nest("/redirect", Self::build_redirect_routes::<R>())
            .merge(rest_router)
            .with_state(state);

        // Add WebSocket routes if configured
        if let Some(config) = socket_config {
            let ws_router = CnctdSocket::build_axum_routes(config);
            app = app.merge(ws_router);
        }

        // Add static file serving / SPA
        if let Some(client_dir) = server_config.client_dir {
            let serve_dir = ServeDir::new(&client_dir)
                .not_found_service(ServeFile::new(format!("{}/index.html", client_dir)));
            app = app.fallback_service(serve_dir);
        }

        // Add CORS layer
        let cors = Self::build_cors(server_config.allowed_origins);
        app = app.layer(cors);

        Ok(app)
    }

    fn build_rest_routes<R>(path_prefix: String) -> Router<AppState<R>>
    where
        R: RestRouterFunction + 'static,
    {
        Router::new()
            .route(
                &format!("/{}/*path", path_prefix),
                post(Self::handle_post::<R>)
                    .get(Self::handle_get::<R>)
                    .put(Self::handle_put::<R>)
                    .delete(Self::handle_delete::<R>),
            )
    }

    fn build_redirect_routes<R>() -> Router<AppState<R>>
    where
        R: RestRouterFunction + 'static,
    {
        Router::new().route("/*path", get(Self::handle_redirect::<R>))
    }

    fn build_cors(allowed_origins: Option<Vec<String>>) -> CorsLayer {
        let mut cors = CorsLayer::new()
            .allow_methods([
                axum::http::Method::GET,
                axum::http::Method::POST,
                axum::http::Method::PUT,
                axum::http::Method::DELETE,
                axum::http::Method::OPTIONS,
            ])
            .allow_headers([
                axum::http::header::USER_AGENT,
                axum::http::header::REFERER,
                axum::http::header::ORIGIN,
                axum::http::header::CONTENT_TYPE,
                axum::http::header::AUTHORIZATION,
                axum::http::HeaderName::from_static("client-id"),
                axum::http::HeaderName::from_static("access-control-request-method"),
                axum::http::HeaderName::from_static("access-control-request-headers"),
            ]);

        if let Some(origins) = allowed_origins {
            cors = cors.allow_origin(
                origins
                    .into_iter()
                    .filter_map(|o| o.parse().ok())
                    .collect::<Vec<_>>(),
            );
        } else {
            cors = cors.allow_origin(Any);
        }

        cors
    }

    // Handler functions using the axum_* methods
    async fn handle_post<R>(
        State(state): State<AppState<R>>,
        FullPath(path): FullPath,
        headers: HeaderMap,
        ClientIp(ip_address): ClientIp,
        Json(data): Json<Value>,
    ) -> Response
    where
        R: RestRouterFunction,
    {
        let auth_token = headers
            .get("authorization")
            .and_then(|h| h.to_str().ok())
            .map(|s| s.to_string());

        let client_id = headers
            .get("client-id")
            .and_then(|h| h.to_str().ok())
            .map(|s| s.to_string());

        Handler::axum_post(path, data, auth_token, client_id, ip_address, state.router).await
    }

    async fn handle_get<R>(
        State(state): State<AppState<R>>,
        FullPath(path): FullPath,
        headers: HeaderMap,
        ClientIp(ip_address): ClientIp,
        Query(data): Query<Value>,
    ) -> Response
    where
        R: RestRouterFunction,
    {
        let auth_token = headers
            .get("authorization")
            .and_then(|h| h.to_str().ok())
            .map(|s| s.to_string());

        let client_id = headers
            .get("client-id")
            .and_then(|h| h.to_str().ok())
            .map(|s| s.to_string());

        Handler::axum_get(path, data, auth_token, client_id, ip_address, state.router).await
    }

    async fn handle_put<R>(
        State(state): State<AppState<R>>,
        FullPath(path): FullPath,
        headers: HeaderMap,
        ClientIp(ip_address): ClientIp,
        Json(data): Json<Value>,
    ) -> Response
    where
        R: RestRouterFunction,
    {
        let auth_token = headers
            .get("authorization")
            .and_then(|h| h.to_str().ok())
            .map(|s| s.to_string());

        let client_id = headers
            .get("client-id")
            .and_then(|h| h.to_str().ok())
            .map(|s| s.to_string());

        Handler::axum_put(path, data, auth_token, client_id, ip_address, state.router).await
    }

    async fn handle_delete<R>(
        State(state): State<AppState<R>>,
        FullPath(path): FullPath,
        headers: HeaderMap,
        ClientIp(ip_address): ClientIp,
        Query(data): Query<Value>,
    ) -> Response
    where
        R: RestRouterFunction,
    {
        let auth_token = headers
            .get("authorization")
            .and_then(|h| h.to_str().ok())
            .map(|s| s.to_string());

        let client_id = headers
            .get("client-id")
            .and_then(|h| h.to_str().ok())
            .map(|s| s.to_string());

        Handler::axum_delete(path, data, auth_token, client_id, ip_address, state.router).await
    }

    async fn handle_redirect<R>(
        State(state): State<AppState<R>>,
        FullPath(path): FullPath,
        headers: HeaderMap,
        Query(data): Query<Value>,
    ) -> Response
    where
        R: RestRouterFunction,
    {
        let auth_token = headers
            .get("authorization")
            .and_then(|h| h.to_str().ok())
            .map(|s| s.to_string());

        let client_id = headers
            .get("client-id")
            .and_then(|h| h.to_str().ok())
            .map(|s| s.to_string());

        let url = state
            .router
            .route_redirect(path, data, auth_token, client_id)
            .await;

        match url.parse::<Uri>() {
            Ok(uri) => Redirect::to(&uri.to_string()).into_response(),
            Err(_) => StatusCode::NOT_FOUND.into_response(),
        }
    }

    pub async fn start_redirect<Req, H>(
        port: &str,
        handler: H,
        mut shutdown_rx: tokio::sync::mpsc::Receiver<()>,
    ) -> anyhow::Result<()>
    where
        Req: Serialize + DeserializeOwned + Send + Sync + 'static,
        H: RedirectHandler<Req> + Clone + Send + Sync + 'static,
    {
        let handler = Arc::new(handler);
        let my_local_ip = local_ip().unwrap_or([0, 0, 0, 0].into());

        let app = Router::new()
            .route("/", get({
                let handler = handler.clone();
                move |Query(data): Query<Req>| {
                    let handler = handler.clone();
                    async move {
                        match handler.handle(data) {
                            Ok(html_response) => Html(html_response).into_response(),
                            Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
                        }
                    }
                }
            }))
            .layer(Self::build_cors(None));

        let parsed_port = port.parse::<u16>().unwrap_or(8543);
        let socket_addr = SocketAddr::from(([0, 0, 0, 0], parsed_port));

        println!("server running at http://{}:{}", my_local_ip, port);

        let listener = tokio::net::TcpListener::bind(socket_addr).await?;

        tokio::select! {
            _ = shutdown_rx.recv() => {
                println!("Shutdown signal received, stopping server...");
                Ok(())
            }
            result = axum::serve(listener, app) => {
                result.map_err(|e| anyhow::anyhow!("Server error: {}", e))
            }
        }
    }

    pub fn path_to_resource_and_operation(path: &str) -> (String, Option<String>) {
        let mut parts = path.trim_start_matches("/api/").split('/');
        let resource = parts.next().unwrap_or_default().to_string();
        let operation = parts.next().map(|s| s.to_string());

        (resource, operation)
    }
}
