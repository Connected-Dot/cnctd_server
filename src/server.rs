use std::{collections::HashMap, sync::{Arc, Mutex}, time::Duration};
use futures_util::{StreamExt, SinkExt};
use local_ip_address::local_ip;
use serde::{de::DeserializeOwned, Serialize};
use warp::{filters::ws::{WebSocket, Ws}, Filter};

use crate::{handlers::{Handler, RedirectHandler}, message::Message, router::RouterFunction, utils::{cors, spa}};

pub struct CnctdServer {
    ws_clients: Arc<Mutex<HashMap<String, WebSocket>>>,
}

impl CnctdServer {
    pub async fn start<R>(port: &str, client_dir: Option<String>, router: R) 
    -> anyhow::Result<()>
    where
        R: RouterFunction + 'static
    {
        let router = Arc::new(router);

        let cors = cors();
        let my_local_ip = local_ip()?;
    
        let cloned_router_for_post = Arc::clone(&router);
        let cloned_router_for_get = Arc::clone(&router);
        
        let rest_route = warp::path::end()
        .and(
            warp::post()
                .and(warp::body::json())
                .and_then(move |msg| {
                    let router_clone = cloned_router_for_post.clone();
                    Handler::post(msg, router_clone)
                })
            .or(
                warp::get()
                    .and(warp::query::<Message>())
                    .and_then(move |msg| {
                        let router_clone = cloned_router_for_get.clone();
                        Handler::get(msg, router_clone)
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

    pub async fn start_with_socket<R, S>(port: &str, client_dir: Option<String>, router: R, shared_state: S) 
        -> anyhow::Result<()>
        where
            R: RouterFunction + 'static,
            S: Clone + Send + Sync + 'static // Your shared state must be thread-safe
    {
        let router = Arc::new(router);
        let shared_state = Arc::new(Mutex::new(shared_state)); // Wrap shared state in Arc<Mutex<>> for safe multi-thread access

        let cors = cors();
        let my_local_ip = local_ip()?;

        let cloned_router_for_post = Arc::clone(&router);
        let cloned_router_for_get = Arc::clone(&router);
        let cloned_router_for_ws = Arc::clone(&router);
        let cloned_state_for_ws = Arc::clone(&shared_state);

        let rest_route = warp::path::end()
            .and(
                warp::post()
                    .and(warp::body::json())
                    .and_then(move |msg| {
                        let router_clone = cloned_router_for_post.clone();
                        Handler::post(msg, router_clone)
                    })
                .or(
                    warp::get()
                        .and(warp::query::<Message>())
                        .and_then(move |msg| {
                            let router_clone = cloned_router_for_get.clone();
                            Handler::get(msg, router_clone)
                        })
                )
            );

        let ws_route = warp::path("ws")
            .and(warp::ws())
            .map(move |ws: Ws| {
                let router_clone = cloned_router_for_ws.clone();
                let state_clone = cloned_state_for_ws.clone();
                ws.on_upgrade(move |websocket| handle_websocket_connection(websocket, router_clone, state_clone))
            });

        let directory = client_dir.clone(); // Simplified Option cloning
        let web_app = spa(directory);
        let routes = rest_route.or(web_app).or(ws_route).with(cors).boxed();
        
        println!("server running at http://{}:{}", my_local_ip, port);
        let ip_address: [u8; 4] = [0, 0, 0, 0];
        let parsed_port = port.parse::<u16>()?;
        let socket = std::net::SocketAddr::from((ip_address, parsed_port));
        
        warp::serve(routes).run(socket).await;

        Ok(())
    }


    // pub async fn handle_ws_connection(&self, ws: Ws, id: String) -> impl warp::Reply {
    //     ws.on_upgrade(move |socket| {
    //         self.add_ws_client(id, socket)
    //     })
    // }

    fn add_ws_client(&self, id: String, socket: WebSocket) {
        let mut clients = self.ws_clients.lock().unwrap();
        clients.insert(id, socket);
    }

    pub async fn send_ws_message(&self, id: &str, message: Message) {
        let clients = self.ws_clients.lock().unwrap();
        if let Some(client) = clients.get(id) {
            // Serialize the message and send it through the WebSocket
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


async fn handle_websocket_connection<S>(websocket: WebSocket, router: Arc<dyn RouterFunction>, shared_state: Arc<Mutex<S>>) {
    // Implement WebSocket connection handling, including message reception and sending
    // You can leverage the router and shared_state here as needed
    let (mut tx, mut rx) = websocket.split();

    while let Some(result) = rx.next().await {
        match result {
            Ok(msg) => {
                if msg.is_text() {
                    let msg_text = msg.to_str().unwrap(); // Consider proper error handling here
                    // Process message using router or directly, updating shared_state as necessary
                }
                // Example: Echoing received messages back to the client
                if let Err(e) = tx.send(msg).await {
                    eprintln!("error sending message: {}", e);
                    break;
                }
            }
            Err(e) => {
                eprintln!("websocket error: {}", e);
                break;
            }
        }
    }
}