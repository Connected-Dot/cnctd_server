use futures_util::{FutureExt, StreamExt};
use local_ip_address::local_ip;
use tokio_stream::wrappers::UnboundedReceiverStream;
use warp::ws::{Message as WebSocketMessage, WebSocket};
use warp::Filter;
use std::collections::HashMap;
use tokio::sync::{mpsc, RwLock};
use std::sync::Arc;

use crate::router::RouterFunction;

type Users = Arc<RwLock<HashMap<String, mpsc::UnboundedSender<Result<WebSocketMessage, warp::Error>>>>>;

pub struct CnctdSocket;

impl CnctdSocket {
    pub async fn start<R>(port: &str, router: R) -> anyhow::Result<()>
    where
        R: RouterFunction + Clone + 'static,
    {
        let users = Users::default();
    
        let users_clone = users.clone();
        let websocket_route = warp::path("ws")
            .and(warp::ws())
            .and(warp::any().map(move || users_clone.clone()))
            .and(warp::any().map(move || router.clone()))
            .map(|ws: warp::ws::Ws, users: Users, router: R| {
                ws.on_upgrade(move |socket| handle_connection(socket, users, router))
            });
        
        let my_local_ip = local_ip()?;
    
        println!("socket server running at http://{}:{}", my_local_ip, port);
        let ip_address: [u8; 4] = [0, 0, 0, 0];
        let parsed_port = port.parse::<u16>()?;
        let socket_addr = std::net::SocketAddr::from((ip_address, parsed_port));

        warp::serve(websocket_route).run(socket_addr).await;
    
        Ok(())
    }
}


async fn handle_connection<R>(
    websocket: WebSocket,
    users: Users,
    router: R,
) where
    R: RouterFunction,
{
    // Example placeholder implementation
    let (user_ws_tx, mut user_ws_rx) = websocket.split();
    let (tx, rx) = mpsc::unbounded_channel();
    let rx = UnboundedReceiverStream::new(rx);

    // This user's unique ID
    let user_id = "user_id_example".to_string();

    users.write().await.insert(user_id.clone(), tx);

    let broadcast_incoming = user_ws_rx.for_each(|message| async {
        if let Ok(msg) = message {
            // Here you would use your router to handle the message
            // and possibly broadcast a response.
        }
    });

    let receive_outgoing = rx.forward(user_ws_tx).map(|result| {
        if let Err(e) = result {
            // handle error
        }
    });

    tokio::select! {
        _ = broadcast_incoming => (),
        _ = receive_outgoing => (),
    }

    // Remove user from users list when disconnected
    users.write().await.remove(&user_id);
}

