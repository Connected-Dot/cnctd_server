use std::{sync::Arc, pin::Pin, future::Future};
use local_ip_address::local_ip;
use serde::{Serialize, Deserialize};
use serde_json::Value;
use warp::{Filter, filters::BoxedFilter};

use crate::{utils::{cors, spa}, handlers::{post_handler, get_handler}};

mod utils;
mod handlers;

pub trait RouterFunction: Send + Sync {
    fn route(&self, msg: Message) -> Pin<Box<dyn Future<Output = Response> + Send>>;
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct Message {
    pub channel: String,
    pub instruction: String,
    pub data: Option<Value>
}

impl Message {
    pub fn new(channel: &str, instruction: &str, data: Option<Value>) -> Self {
        Self {
            channel: channel.into(),
            instruction: instruction.into(),
            data,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Response {
    pub success: bool,
    pub msg: Option<String>,
    pub data: Option<Value>,
}

impl Response {
    pub fn success(msg: Option<String>, data: Option<Value>) -> Self {
        Self { success: true, msg, data }
    }
    pub fn failure(msg: Option<String>, data: Option<Value>) -> Self {
        Self { success: false, msg, data }
    }
}

pub struct CnctdServer;

impl CnctdServer {
    pub async fn start<R>(port: &str, client_dir: Option<&str>, router: R) 
    where
        R: RouterFunction + 'static,  // Notice the 'static lifetime here
    {
        let router = Arc::new(router);  // Wrap the router in an Arc

        let cors = cors();
        let my_local_ip = local_ip().unwrap();
    
        let cloned_router_for_post = Arc::clone(&router);
        let cloned_router_for_get = Arc::clone(&router);
        
        let rest_route = warp::path!("/")
        .and(
            warp::post()
                .and(warp::body::json())
                .and_then(move |msg| {
                    let router_clone = cloned_router_for_post.clone();
                    post_handler(msg, router_clone)
                })
            .or(
                warp::get()
                    .and(warp::query::<Message>())
                    .and_then(move |msg| {
                        let router_clone = cloned_router_for_get.clone();
                        get_handler(msg, router_clone)
                    })
            )
        );

        let directory = match client_dir {
            Some(client_dir) => Some(client_dir.to_string()),
            None => None,
        };
        let web_app = spa(directory);
        let routes = rest_route.or(web_app).with(cors).boxed();
        
            
        
        println!("server running at http://{}:{}", my_local_ip, port);
        let ip_address: [u8; 4] = [0, 0, 0, 0];
        let parsed_port = port.parse::<u16>().expect("Failed to parse port into u16");
        let socket = std::net::SocketAddr::from((ip_address, parsed_port));
        
        warp::serve(routes).run(socket).await        
    }
}
