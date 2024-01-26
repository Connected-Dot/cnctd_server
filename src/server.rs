use std::{collections::HashMap, sync::{Arc, Mutex}};

use local_ip_address::local_ip;
use warp::{filters::ws::WebSocket, Filter};

use crate::{handlers::Handler, message::Message, router::RouterFunction, utils::{cors, spa}};

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
