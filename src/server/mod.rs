pub mod server_info;
pub mod handlers;

use std::{fmt::Debug, sync::Arc, time::Duration};

// use async_graphql_warp::{graphql, GraphQLResponse};
use local_ip_address::local_ip;
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
use warp::{filters::path::FullPath, Filter};

use crate::{
    router::{RestRouterFunction, SocketRouterFunction}, server::server_info::{ServerInfo, SERVER_INFO}, socket::{CnctdSocket, SocketConfig}, utils::{cors, spa}
};

use self::handlers::{Handler, RedirectHandler};
// use async_graphql::{self, http::GraphiQLSource, EmptySubscription, Schema};



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
    pub fn new(id: &str, port: &str, client_dir: Option<String>, router: R, heartbeat: Option<u64>, allowed_origins: Option<Vec<String>>, path: Option<String>) -> Self {
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

pub struct CnctdServer;

impl CnctdServer {
    pub async fn start<R, SReq, SResp, SR>(server_config: ServerConfig<R>, socket_config: Option<SocketConfig<SR>>) 
    -> anyhow::Result<()>
    where
        R: RestRouterFunction + 'static,
        SReq: Serialize + DeserializeOwned + Send + Sync + Debug + Clone + 'static,
        SResp: Serialize + DeserializeOwned + Send + Sync + Debug + Clone + 'static, 
        SR: SocketRouterFunction<SReq, SResp> + 'static,
    {
        let server_router = Arc::new(server_config.router);

        let web_app = spa(server_config.client_dir);
            
        let my_local_ip = local_ip()?;
        let ip_address: [u8; 4] = [0, 0, 0, 0];
        let parsed_port = server_config.port.parse::<u16>()?;
        let socket = std::net::SocketAddr::from((ip_address, parsed_port));
        


        
        match socket_config {
            Some(config) => {
                let server_info = ServerInfo::new(
                    &server_config.id, 
                    &my_local_ip.to_string(), 
                    parsed_port, 
                    100, 
                    true,
                    config.redis_url.clone(),
                ).await;

                SERVER_INFO.set(server_info.clone());                

                let rest_routes = Self::build_rest_routes::<R>(&server_router, server_config.path.clone());
                let socket_routes = CnctdSocket::build_routes(config.clone());
                // let graphql_routes = CnctdGraphQL::build_routes(graphql_config);


                let routes = rest_routes
                    .or(socket_routes)
                    // .or(graphql_routes)
                    .or(web_app)
                    .with(cors(server_config.allowed_origins))
                    .boxed();

                println!("server and socket running at http://{}:{}", my_local_ip, parsed_port);
                println!("server info: {:?}", server_info);
                
                
                
                if server_config.heartbeat.is_some() {
                    let heartbeat = server_config.heartbeat.unwrap();
                    tokio::spawn(async move {
                        loop {
                            tokio::time::sleep(heartbeat).await;
                            match ServerInfo::update().await {
                                Ok(_) => {},
                                Err(e) => {
                                    eprintln!("Error updating server info: {:?}", e);
                                }
                            }
                        }
                    });
                }
                
                warp::serve(routes).run(socket).await;
            }
            None => {
                let rest_routes = Self::build_rest_routes::<R>(&server_router, server_config.path.clone());
                let routes = rest_routes
                    .or(web_app)
                    .with(cors(server_config.allowed_origins))
                    .boxed();

                let server_info = ServerInfo::new(
                    &server_config.id, 
                    &my_local_ip.to_string(), 
                    parsed_port, 
                    100, 
                    true,
                    None,
                ).await;
                
                println!("server running at http://{}:{}", my_local_ip, parsed_port);
                println!("server info: {:?}", server_info);

                SERVER_INFO.set(server_info);
                
                if server_config.heartbeat.is_some() {
                    let heartbeat = server_config.heartbeat.unwrap();
                    tokio::spawn(async move {
                        loop {
                            tokio::time::sleep(heartbeat).await;
                            ServerInfo::update().await.unwrap();
                        }
                    });
                }

                

                warp::serve(routes).run(socket).await;
            }
        }
        
        Ok(())
    }

    pub async fn start_redirect<Req, H>(port: &str, handler: H, mut shutdown_rx: tokio::sync::mpsc::Receiver<()>) -> anyhow::Result<()> 
    where
        Req: Serialize + DeserializeOwned + Send + Sync + 'static,
        H: RedirectHandler<Req> + 'static,
    {
        let handler = Arc::new(handler);
        let cors = cors(None);
        let my_local_ip = local_ip().unwrap_or([0, 0, 0, 0].into());
    
        let rest_route = warp::path::end()
            .and(warp::get())
            .and(warp::query::<Req>())
            .and(with_handler(handler.clone()))
            .and_then(|data, handler| {
                Handler::api_redirect(data, handler)
            });
    
        let routes = rest_route.with(cors).boxed();
        let ip_address: [u8; 4] = [0, 0, 0, 0];
        let parsed_port = port.parse::<u16>().unwrap_or(8543);
        let socket = std::net::SocketAddr::from((ip_address, parsed_port));
        
        println!("server running at http://{}:{}", my_local_ip, port);

        tokio::select! {
            _ = shutdown_rx.recv() => {
                println!("Shutdown signal received, stopping server...");
                Ok(())
            }
            _ = async {
                warp::serve(routes).run(socket).await;
                
                loop {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }

            } => Ok(())
        }
    }

    fn build_rest_routes<R>(router: &Arc<R>, path: Option<String>) -> warp::filters::BoxedFilter<(impl warp::Reply,)>
    where
        R: RestRouterFunction + 'static,
    {
        let cloned_router_for_post = Arc::clone(&router);
        let cloned_router_for_get = Arc::clone(&router);
        let cloned_router_for_get_binary = Arc::clone(&router);
        let cloned_router_for_put = Arc::clone(&router);
        let cloned_router_for_delete = Arc::clone(&router);
        let cloned_router_for_redirect = Arc::clone(&router);

        let path_string = path.unwrap_or_else(|| "api".to_string());
        let path_filter = warp::path(path_string.clone()).boxed();

        // Accept optional JSON body for POST — capture the raw bytes AND the
        // parsed Value. Bytes are needed by routes that verify byte-exact
        // signatures over the body (Stripe webhooks, GitHub webhooks); the
        // parsed Value is the convenience path for everything else. If the
        // body is missing or unparseable, Value falls back to `{}`. This
        // also allows webhooks from external services (e.g. Sonos) that may
        // send bodies without Content-Type, and lets clients POST without a
        // body for simple operations.
        let optional_json_body = warp::body::bytes()
            .map(|bytes: warp::hyper::body::Bytes| {
                let value = if bytes.is_empty() {
                    serde_json::json!({})
                } else {
                    serde_json::from_slice(&bytes).unwrap_or_else(|_| serde_json::json!({}))
                };
                (bytes, value)
            });

        let post_route = path_filter.clone()
            .and(warp::post())
            .and(warp::path::full())
            .and(warp::header::optional("Authorization"))
            .and(warp::header::optional("Client-Id"))
            .and(warp::header::optional::<String>("x-forwarded-for"))  // Check for the X-Forwarded-For header
            .and(warp::addr::remote())  // Fallback to the remote address if no X-Forwarded-For header
            .and(warp::header::headers_cloned())  // All headers — for raw-body routes (webhook signature verification, etc)
            .and(optional_json_body)
            .and_then(move |path: FullPath, auth_header: Option<String>, connection_id: Option<String>, x_forwarded_for: Option<String>, remote_addr: Option<std::net::SocketAddr>, header_map: warp::http::HeaderMap, body: (warp::hyper::body::Bytes, Value)| {
                let ip_address = x_forwarded_for.or_else(|| remote_addr.map(|addr| addr.ip().to_string()));  // Use X-Forwarded-For or fallback to remote addr
                let router_clone = cloned_router_for_post.clone();
                let (body_bytes, data) = body;
                // Convert headers to a lowercase-keyed HashMap so handlers don't
                // have to worry about case. Skips headers with non-UTF-8 values.
                let mut headers = std::collections::HashMap::with_capacity(header_map.len());
                for (k, v) in header_map.iter() {
                    if let Ok(vs) = v.to_str() {
                        headers.insert(k.as_str().to_lowercase(), vs.to_string());
                    }
                }
                async move {
                    Handler::post(path.as_str().to_string(), body_bytes, headers, data, auth_header, connection_id, ip_address, router_clone).await
                }
            });

        let get_binary_route = path_filter.clone()
            .and(warp::get())
            .and(warp::path::full())
            .and(warp::header::optional("Authorization"))
            .and(warp::header::optional("Client-Id"))
            .and(warp::header::optional::<String>("x-forwarded-for"))
            .and(warp::addr::remote())
            .and(warp::query::<Value>())
            .and_then(move |path: FullPath, auth_header: Option<String>, connection_id: Option<String>, x_forwarded_for: Option<String>, remote_addr: Option<std::net::SocketAddr>, data: Value| {
                let ip_address = x_forwarded_for.or_else(|| remote_addr.map(|addr| addr.ip().to_string()));
                let router_clone = cloned_router_for_get_binary.clone();
                async move {
                    Handler::get_binary(path.as_str().to_string(), data, auth_header, connection_id, ip_address, router_clone).await
                }
            });

        let get_route = path_filter.clone()
            .and(warp::get())
            .and(warp::path::full())
            .and(warp::header::optional("Authorization"))
            .and(warp::header::optional("Client-Id"))
            .and(warp::header::optional::<String>("x-forwarded-for"))  // Check for the X-Forwarded-For header
            .and(warp::addr::remote())  // Fallback to the remote address if no X-Forwarded-For header
            .and(warp::query::<Value>())
            .and_then(move |path: FullPath, auth_header: Option<String>, connection_id: Option<String>, x_forwarded_for: Option<String>, remote_addr: Option<std::net::SocketAddr>, data: Value| {
                let ip_address = x_forwarded_for.or_else(|| remote_addr.map(|addr| addr.ip().to_string()));  // Use X-Forwarded-For or fallback to remote addr
                let router_clone = cloned_router_for_get.clone();
                async move {
                    Handler::get(path.as_str().to_string(), data, auth_header, connection_id, ip_address, router_clone).await
                }
            });

        let optional_json_body_put = warp::body::bytes()
            .map(|bytes: warp::hyper::body::Bytes| {
                if bytes.is_empty() {
                    serde_json::json!({})
                } else {
                    serde_json::from_slice(&bytes).unwrap_or_else(|_| serde_json::json!({}))
                }
            });

        let put_route = path_filter.clone()
            .and(warp::put())
            .and(warp::path::full())
            .and(warp::header::optional("Authorization"))
            .and(warp::header::optional("Client-Id"))
            .and(warp::header::optional::<String>("x-forwarded-for"))  // Check for the X-Forwarded-For header
            .and(warp::addr::remote())  // Fallback to the remote address if no X-Forwarded-For header
            .and(optional_json_body_put)
            .and_then(move |path: FullPath, auth_header: Option<String>, connection_id: Option<String>, x_forwarded_for: Option<String>, remote_addr: Option<std::net::SocketAddr>, data: Value| {
                let ip_address = x_forwarded_for.or_else(|| remote_addr.map(|addr| addr.ip().to_string()));  // Use X-Forwarded-For or fallback to remote addr
                let router_clone = cloned_router_for_put.clone();
                async move {
                    Handler::put(path.as_str().to_string(), data, auth_header, connection_id, ip_address, router_clone).await
                }
            });

        let delete_route = path_filter.clone()
            .and(warp::delete())
            .and(warp::path::full())
            .and(warp::header::optional("Authorization"))
            .and(warp::header::optional("Client-Id"))
            .and(warp::header::optional::<String>("x-forwarded-for"))  // Check for the X-Forwarded-For header
            .and(warp::addr::remote())  // Fallback to the remote address if no X-Forwarded-For header
            .and(warp::query::<Value>())
            .and_then(move |path: FullPath, auth_header: Option<String>, connection_id: Option<String>, x_forwarded_for: Option<String>, remote_addr: Option<std::net::SocketAddr>, data: Value| {
                let ip_address = x_forwarded_for.or_else(|| remote_addr.map(|addr| addr.ip().to_string()));  // Use X-Forwarded-For or fallback to remote addr
                let router_clone = cloned_router_for_delete.clone();
                async move {
                    Handler::delete(path.as_str().to_string(), data, auth_header, connection_id, ip_address, router_clone).await
                }
            });

        let redirect_route = warp::path("redirect")
            .and(warp::get())
            .and(warp::path::full())
            .and(warp::header::optional("Authorization"))
            .and(warp::header::optional("Client-Id"))
            .and(warp::query::<Value>())
            .and_then(move |path: FullPath, auth_header: Option<String>, connection_id: Option<String>, data: Value| {
                let router_clone = cloned_router_for_redirect.clone();
                async move {
                    Handler::get_redirect(path.as_str().to_string(), data, auth_header, connection_id, router_clone).await
                }
            });

        let routes = redirect_route
            .or(post_route)
            .or(get_binary_route)
            .or(get_route)
            .or(put_route)
            .or(delete_route);

        routes.boxed()
    }

    pub fn path_to_resource_and_operation(path: &str) -> (String, Option<String>) {
        let mut parts = path.trim_start_matches("/api/").split('/');
        let resource = parts.next().unwrap_or_default().to_string();
        let operation = parts.next().map(|s| s.to_string());
    
        (resource, operation)
    }
}

fn with_handler<Req, H>(handler: Arc<H>) -> impl Filter<Extract = (Arc<H>,), Error = std::convert::Infallible> + Clone
where
    Req: Serialize + DeserializeOwned + Send + Sync,
    H: RedirectHandler<Req>,
{
    warp::any().map(move || handler.clone())
}