use std::time::Duration;
use std::sync::Arc;
use std::fmt::Debug;
use anyhow::anyhow;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::json;
use warp::reject::Rejection;

use crate::{auth::CnctdAuth, router::{response::Response, RestRouterFunction}, socket::{Client, CnctdSocket, CLIENTS}};

pub type Result<T> = std::result::Result<T, Rejection>;

pub trait RedirectHandler<M>: Send + Sync
where
    M: Serialize + DeserializeOwned + Send + Sync,
{
    fn handle(&self, msg: M) -> anyhow::Result<String>;
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ClientQuery {
    user_id: Option<String>,
    subscriptions: Option<String>,
}

pub struct Handler;

impl Handler {
    pub async fn post<M, Resp, R>(msg: M, auth_token: Option<String>, router: Arc<R>) -> Result<impl warp::Reply>
    where
        M: Serialize + DeserializeOwned + Send + Sync + Debug + Clone, 
        Resp: Serialize + DeserializeOwned + Send + Sync + Debug + Clone, 
        R: RestRouterFunction<M, Resp>,
    {
        let response = router.route(msg, auth_token).await;
        Ok(warp::reply::json(&response))
    }
    
    pub async fn get<M, Resp, R>(msg: M, auth_token: Option<String>, router: Arc<R>) -> Result<impl warp::Reply>
    where
        M: Serialize + DeserializeOwned + Send + Sync + Debug + Clone,
        Resp: Serialize + DeserializeOwned + Send + Sync + Debug + Clone, 
        R: RestRouterFunction<M, Resp>,
        
    {
        let response = router.route(msg, auth_token).await;
    
        Ok(warp::reply::json(&response))
    }

    pub async fn get_redirect<M, H>(msg: M, handler: Arc<H>) -> Result<impl warp::Reply>
    where
        M: Serialize + DeserializeOwned + Send + Sync,
        H: RedirectHandler<M>,
    {
        match handler.handle(msg) {
            Ok(html_response) => Ok(warp::reply::html(html_response)),
            Err(_) => Err(warp::reject::reject()),
        }
    }

    pub async fn register_socket_client(client_query: ClientQuery, secret: Option<Vec<u8>>, auth_token: Option<String>) -> Result<impl warp::Reply> {
        
        let subs = match client_query.subscriptions {
            Some(subs) => {
                subs.trim_matches(|c| c == '[' || c == ']')
                .split(',')
                .map(|s| s.trim().to_string())
                .collect()
            }
            None => vec![]
        };
        
        if let Some(secret) = &secret {
            // Ensure both user_id and auth_token are provided
            let user_id = match &client_query.user_id {
                Some(id) => id,
                None => {
                    let response = Response::failure(Some("No user ID provided".into()), None);
                    return Ok(warp::reply::json(&response))
                }
            };
            let auth_token = match &auth_token {
                Some(token) => token,
                None => {
                    let response = Response::failure(Some("No auth token provided".into()), None);
                    return Ok(warp::reply::json(&response))
                }
            };

            // Perform the verification
            match CnctdAuth::verify_auth_token(secret.clone(), auth_token, user_id) {
                Ok(_) => (),
                Err(_) => {
                    let response = Response::failure(Some("Failed to authorize user".into()), None);
                    return Ok(warp::reply::json(&response))
                }
            }
        }

        let client_id = uuid::Uuid::new_v4().to_string();
        let client = Client::new(client_query.user_id.unwrap_or(client_id.clone()), subs);
        let clients_lock = match CLIENTS.try_get() {
            Some(clients) => clients,
            None => {
                let response = Response::failure(Some("Failed to get clients lock".into()), None);
                return Ok(warp::reply::json(&response))
            }
        };
        let mut clients = clients_lock.write().await; // Lock the CLIENTS for write access
        
        clients.insert(client_id.clone(), client);

        let response = Response::success(None, Some(json!(&client_id)));
        let client_id_clone = client_id.clone();
        
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(10)).await;
            
            match CnctdSocket::get_client(&client_id_clone).await {
                Ok(client) => {
                    if client.sender.is_none() { 
                        println!("Client never connected. Removing");
                        CnctdSocket::remove_client(&client_id_clone).await 
                    } else {
                        println!("Client connected. No need to remove");
                        Ok(())
                    }
                }
                Err(_e) => Ok(())
            }
        });

        Ok(warp::reply::json(&response))
    }

}

