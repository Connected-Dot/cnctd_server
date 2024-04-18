pub mod server_info;
pub mod handlers;

use std::{fmt::Debug, sync::Arc, time::Duration};

use local_ip_address::local_ip;
use serde::{de::DeserializeOwned, Serialize};
use warp::Filter;

use crate::{
    router::{RestRouterFunction, SocketRouterFunction}, server::server_info::{ServerInfo, SERVER_INFO}, socket::{CnctdSocket, SocketConfig}, utils::{cors, spa}
};

use self::handlers::{Handler, RedirectHandler};




#[derive(Debug)]
pub struct ServerConfig<R> {
    pub id: String,
    pub port: String,
    pub client_dir: Option<String>,
    pub router: R,
    pub heartbeat: Option<Duration>,
}

impl<R> ServerConfig<R> {
    pub fn new(id: &str, port: &str, client_dir: Option<String>, router: R, heartbeat: Option<u64>) -> Self {
        Self {
            id: id.into(),
            port: port.into(),
            client_dir,
            router,
            heartbeat: heartbeat.map(Duration::from_secs),
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

                let rest_routes = Self::build_routes::<M, Resp, R>(&server_router);
                let routes = rest_routes
                    .or(CnctdSocket::build_route(config.clone()))
                    .or(web_app)
                    .with(cors())
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
                let rest_routes = Self::build_routes::<M, Resp, R>(&server_router);
                let routes = rest_routes
                    .or(web_app)
                    .with(cors())
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