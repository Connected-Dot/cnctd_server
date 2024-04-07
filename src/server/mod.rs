pub mod server_info;
pub mod handlers;

use std::{collections::BTreeMap, fmt::Debug, sync::Arc, time::Duration};

use anyhow::anyhow;
use hmac::{Hmac, Mac};
use jwt::VerifyWithKey;
use local_ip_address::local_ip;
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
use sha2::Sha256;

use tokio::sync::RwLock;
use warp::Filter;

use crate::{
    router::{RestRouterFunction, SocketRouterFunction}, server::server_info::{ServerInfo, SERVER_INFO}, socket::{CnctdSocket, SocketConfig}, utils::{cors, spa}
};

use self::handlers::{Handler, RedirectHandler};




#[derive(Debug)]
pub struct ServerConfig<R> {
    pub port: String,
    pub client_dir: Option<String>,
    pub router: R,
    pub heartbeat: Option<Duration>,
    pub redis_url: Option<String>,
}

impl<R> ServerConfig<R> {
    pub fn new(port: &str, client_dir: Option<String>, router: R, heartbeat: Option<u64>, redis_url: Option<String>) -> Self {
        Self {
            port: port.into(),
            client_dir,
            router,
            heartbeat: heartbeat.map(Duration::from_secs),
            redis_url,
        }
    }
}

pub struct CnctdServer;

impl CnctdServer {
    pub async fn start<M, Resp, R, SM, SResp, SR>(server_config: ServerConfig<R>, socket_config: Option<SocketConfig<SR>>) 
    -> anyhow::Result<()>
    where
        M: Serialize + DeserializeOwned + Send + Sync + Debug + Clone + 'static,
        Resp: Serialize + DeserializeOwned + Send + Sync + Debug + Clone + 'static, 
        R: RestRouterFunction<M, Resp> + 'static,
        SM: Serialize + DeserializeOwned + Send + Sync + Debug + Clone + 'static,
        SResp: Serialize + DeserializeOwned + Send + Sync + Debug + Clone + 'static, 
        SR: SocketRouterFunction<SM, SResp> + 'static,
    {
        let server_router = Arc::new(server_config.router);

        let web_app = spa(server_config.client_dir);
            
        let my_local_ip = local_ip()?;
        let ip_address: [u8; 4] = [0, 0, 0, 0];
        let parsed_port = server_config.port.parse::<u16>()?;
        let socket = std::net::SocketAddr::from((ip_address, parsed_port));

        if server_config.redis_url.is_some() {
            let redis_url = server_config.redis_url.unwrap();
            println!("REDIS URL: {:?}", redis_url);
            let _ = cnctd_redis::CnctdRedis::start_pool(&redis_url)?;
        }
        

        match socket_config {
            Some(config) => {
                let rest_routes = Self::build_routes::<M, Resp, R>(&server_router);
                let routes = rest_routes
                    .or(CnctdSocket::build_route(config))
                    .or(web_app)
                    .with(cors())
                    .boxed();
                
                let server_info = Arc::new(RwLock::new(ServerInfo::new(&my_local_ip.to_string(), parsed_port, 100, true)));
                
                println!("server and socket running:\n{:?}", server_info);
                
                SERVER_INFO.set(server_info);
                
                // if server_config.heartbeat.is_some() {
                //     let heartbeat = server_config.heartbeat.unwrap();
                //     tokio::spawn(async move {
                //         loop {
                //             tokio::time::sleep(heartbeat).await;
                //             ServerInfo::update().await.unwrap();
                //         }
                //     });
                // }
                
                warp::serve(routes).run(socket).await;
            }
            None => {
                let rest_routes = Self::build_routes::<M, Resp, R>(&server_router);
                let routes = rest_routes
                    .or(web_app)
                    .with(cors())
                    .boxed();

                let server_info = Arc::new(RwLock::new(ServerInfo::new(&my_local_ip.to_string(), parsed_port, 100, false)));
                
                println!("server running:\n{:?}", server_info);

                SERVER_INFO.set(server_info);
                
                // if server_config.heartbeat.is_some() {
                //     let heartbeat = server_config.heartbeat.unwrap();
                //     tokio::spawn(async move {
                //         loop {
                //             tokio::time::sleep(heartbeat).await;
                //             ServerInfo::update().await.unwrap();
                //         }
                //     });
                // }

                

                warp::serve(routes).run(socket).await;
            }
        }
        
        Ok(())
    }

    pub async fn start_redirect<M, H>(port: &str, handler: H, mut shutdown_rx: tokio::sync::mpsc::Receiver<()>) -> anyhow::Result<()> 
    where
        M: Serialize + DeserializeOwned + Send + Sync + 'static,
        H: RedirectHandler<M> + 'static,
    {
        let handler = Arc::new(handler);
        let cors = cors();
        let my_local_ip = local_ip().unwrap_or([0, 0, 0, 0].into());
    
        let rest_route = warp::path::end()
            .and(warp::get())
            .and(warp::query::<M>())
            .and(with_handler(handler.clone()))
            .and_then(|msg, handler| {
                Handler::get_redirect(msg, handler)
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

    pub fn verify_auth_token<T: AsRef<str> + std::fmt::Debug>(secret: Vec<u8>, auth_token: &str, user_id: T) -> anyhow::Result<()> {
        let key: Hmac<Sha256> = Hmac::new_from_slice(&secret)?;
        let claims: BTreeMap<String, Value> = auth_token.verify_with_key(&key)?;
        
        let sub_claim = claims.get("sub").ok_or(anyhow!("'sub' claim not found"))?;
        let user_id_ref = user_id.as_ref();
    
        // Dynamically check the type of `sub` and compare
        let matched = match sub_claim {
            Value::String(s) => s == user_id_ref,
            Value::Number(n) => n.to_string() == user_id_ref,
            _ => return Err(anyhow!("Unexpected type for 'sub' claim")),
        };
    
        if matched {
            Ok(())
        } else {
            Err(anyhow!("User ID does not match the 'sub' claim"))
        }
    }

    fn build_routes<M, Resp, R>(router: &Arc<R>) -> warp::filters::BoxedFilter<(impl warp::Reply,)>
    where
        M: Serialize + DeserializeOwned + Send + Sync + Debug + Clone + 'static,
        Resp: Serialize + DeserializeOwned + Send + Sync + Debug + Clone + 'static, 
        R: RestRouterFunction<M, Resp> + 'static,
    {

        let cloned_router_for_post = Arc::clone(&router);
        let cloned_router_for_get = Arc::clone(&router);

        let routes = warp::path::end()
            .and(
                warp::post()
                    .and(warp::header::optional("Authorization"))
                    .and(warp::body::json())
                    .and_then(move |auth_header: Option<String>, msg: M| {
                        let router_clone = cloned_router_for_post.clone();
                        async move {
                            Handler::post(msg, auth_header, router_clone).await
                        }
                    })
                .or(
                    warp::get()
                        .and(warp::header::optional("Authorization"))
                        .and(warp::query::<M>())
                        .and_then(move |auth_header: Option<String>, msg: M| {
                            let router_clone = cloned_router_for_get.clone();
                            async move {
                                Handler::get(msg, auth_header, router_clone).await
                            }
                        })
                        
                )
            );

        routes.boxed()
    }
}

fn with_handler<M, H>(handler: Arc<H>) -> impl Filter<Extract = (Arc<H>,), Error = std::convert::Infallible> + Clone
where
    M: Serialize + DeserializeOwned + Send + Sync,
    H: RedirectHandler<M>,
{
    warp::any().map(move || handler.clone())
}