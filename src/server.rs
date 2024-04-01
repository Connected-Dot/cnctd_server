use std::{sync::Arc, time::Duration, fmt::Debug};

use local_ip_address::local_ip;
use serde::{de::DeserializeOwned, Serialize};
use warp::Filter;

use crate::{handlers::{Handler, RedirectHandler}, router::RestRouterFunction, utils::{cors, spa}};

pub struct CnctdServer;

impl CnctdServer {
    pub async fn start<M, Resp, R>(port: &str, client_dir: Option<String>, router: R) 
    -> anyhow::Result<()>
    where
        M: Serialize + DeserializeOwned + Send + Sync + Debug + Clone + 'static,
        Resp: Serialize + DeserializeOwned + Send + Sync + Debug + Clone + 'static, 
        R: RestRouterFunction<M, Resp> + 'static,
    {
        let router = Arc::new(router);

        let cors = cors();
        let my_local_ip = local_ip()?;
    
        let cloned_router_for_post = Arc::clone(&router);
        let cloned_router_for_get = Arc::clone(&router);
        
        let rest_route = warp::path::end()
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

        let directory = match client_dir {
            Some(client_dir) => Some(client_dir),
            None => None,
        };
        let web_app = spa(directory);
        let routes = rest_route.or(web_app).with(cors).boxed();
        
            
        
        println!("server running at http://{}:{}", my_local_ip, port);
        let ip_address: [u8; 4] = [0, 0, 0, 0];
        let parsed_port = port.parse::<u16>()?;
        let socket = std::net::SocketAddr::from((ip_address, parsed_port));
        
        warp::serve(routes).run(socket).await;

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
}

fn with_handler<M, H>(handler: Arc<H>) -> impl Filter<Extract = (Arc<H>,), Error = std::convert::Infallible> + Clone
where
    M: Serialize + DeserializeOwned + Send + Sync,
    H: RedirectHandler<M>,
{
    warp::any().map(move || handler.clone())
}