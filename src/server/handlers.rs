use std::time::Duration;
use std::sync::Arc;
use std::fmt::Debug;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::json;
use warp::{reject::Rejection, reply::Reply};
use warp::hyper::Uri;

use crate::router::response::{ErrorCode, ErrorResponse, SuccessCode, SuccessResponse};
use crate::router::HttpMethod;
use crate::{auth::CnctdAuth, router::{RestRouterFunction}, socket::{Client, CnctdSocket, CLIENTS}};

pub type Result<T> = std::result::Result<T, Rejection>;


pub trait RedirectHandler<DataIn>: Send + Sync
where
    DataIn: Serialize + DeserializeOwned + Send + Sync,
{
    fn handle(&self, msg: DataIn) -> anyhow::Result<String>;
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ClientQuery {
    user_id: Option<String>,
    subscriptions: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RedirectQuery {
    pub path: String,
    pub client_id: String,
    pub size: Option<String>,
}

pub struct Handler;

impl Handler {
    pub async fn post<DataIn, DataOut, R>(path: String, msg: DataIn, auth_token: Option<String>, router: Arc<R>) -> Result<impl warp::Reply>
    where
        DataIn: Serialize + DeserializeOwned + Send + Sync + Debug + Clone, 
        DataOut: Serialize + DeserializeOwned + Send + Sync + Debug + Clone, 
        R: RestRouterFunction<DataIn, DataOut>,
    {
        match router.route(HttpMethod::POST, path, msg, auth_token).await {
            Ok(response) => {
                let status_code = &response.status_code.to_warp_status_code();
                let json = warp::reply::json(&response);

                Ok(warp::reply::with_status(json, status_code.clone()))
            }
            Err(e) => {
                let status_code = &e.status_code.to_warp_status_code();
                let json = warp::reply::json(&e);
            
                Ok(warp::reply::with_status(json, status_code.clone()))
            }
        }
    }
    
    pub async fn get<DataIn, DataOut, R>(path: String, msg: DataIn, auth_token: Option<String>, router: Arc<R>) -> Result<impl warp::Reply>
    where
        DataIn: Serialize + DeserializeOwned + Send + Sync + Debug + Clone,
        DataOut: Serialize + DeserializeOwned + Send + Sync + Debug + Clone, 
        R: RestRouterFunction<DataIn, DataOut>,
        
    {
        match router.route(HttpMethod::GET, path, msg, auth_token).await {
            Ok(response) => {
                let status_code = &response.status_code.to_warp_status_code();
                let json = warp::reply::json(&response);

                Ok(warp::reply::with_status(json, status_code.clone()))
            }
            Err(e) => {
                let status_code = &e.status_code.to_warp_status_code();
                let json = warp::reply::json(&e);
            
                Ok(warp::reply::with_status(json, status_code.clone()))
            }
        }
    }
    
    pub async fn put<DataIn, DataOut, R>(path: String, msg: DataIn, auth_token: Option<String>, router: Arc<R>) -> Result<impl warp::Reply>
    where
        DataIn: Serialize + DeserializeOwned + Send + Sync + Debug + Clone,
        DataOut: Serialize + DeserializeOwned + Send + Sync + Debug + Clone, 
        R: RestRouterFunction<DataIn, DataOut>,
        
    {
        match router.route(HttpMethod::PUT, path, msg, auth_token).await {
            Ok(response) => {
                let status_code = &response.status_code.to_warp_status_code();
                let json = warp::reply::json(&response);

                Ok(warp::reply::with_status(json, status_code.clone()))
            }
            Err(e) => {
                let status_code = &e.status_code.to_warp_status_code();
                let json = warp::reply::json(&e);
            
                Ok(warp::reply::with_status(json, status_code.clone()))
            }
        }
    }

    pub async fn delete<DataIn, DataOut, R>(path: String, msg: DataIn, auth_token: Option<String>, router: Arc<R>) -> Result<impl warp::Reply>
    where
        DataIn: Serialize + DeserializeOwned + Send + Sync + Debug + Clone,
        DataOut: Serialize + DeserializeOwned + Send + Sync + Debug + Clone, 
        R: RestRouterFunction<DataIn, DataOut>,
        
    {
        match router.route(HttpMethod::DELETE, path, msg, auth_token).await {
            Ok(response) => {
                let status_code = &response.status_code.to_warp_status_code();
                let json = warp::reply::json(&response);

                Ok(warp::reply::with_status(json, status_code.clone()))
            }
            Err(e) => {
                let status_code = &e.status_code.to_warp_status_code();
                let json = warp::reply::json(&e);
            
                Ok(warp::reply::with_status(json, status_code.clone()))
            }
        }
    }

    pub async fn get_redirect<DataIn, DataOut, R>(msg: RedirectQuery, router: Arc<R>) -> Result<impl warp::Reply>
    where
        DataIn: Serialize + DeserializeOwned + Send + Sync + Debug + Clone,
        DataOut: Serialize + DeserializeOwned + Send + Sync + Debug + Clone, 
        R: RestRouterFunction<DataIn, DataOut>,
    {
        // println!("File router. path: {}", path);
        println!("File HANDLER, msg: {:?}", msg);
        let url = router.route_redirect(msg).await;
        println!("File HANDLER, url: {}", url);
        match url.parse::<Uri>() {
            Ok(uri) => Ok(warp::redirect::found(uri).into_response()),
            Err(_) => Err(warp::reject::not_found())
        }

        // Ok(warp::redirect::found(url.parse::<Uri>().unwrap()).into_response())
    }

    pub async fn api_redirect<DataIn, H>(msg: DataIn, handler: Arc<H>) -> Result<impl warp::Reply>
    where
        DataIn: Serialize + DeserializeOwned + Send + Sync,
        H: RedirectHandler<DataIn>,
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
                    let error = ErrorResponse::new(Some(ErrorCode::Unauthorized), Some("No user_id provided".into()));
                    return Ok(warp::reply::json(&error))
                }
            };
            let auth_token = match &auth_token {
                Some(token) => token,
                None => {
                    let error = ErrorResponse::new(Some(ErrorCode::Unauthorized), Some("No auth_token provided".into()));
                    return Ok(warp::reply::json(&error))
                }
            };

            // Perform the verification
            match CnctdAuth::verify_auth_token(secret.clone(), auth_token, user_id) {
                Ok(_) => (),
                Err(_) => {
                    let error = ErrorResponse::new(Some(ErrorCode::Unauthorized), Some("Invalid auth_token".into()));
                    return Ok(warp::reply::json(&error))
                }
            }
        }

        let client_id = uuid::Uuid::new_v4().to_string();
        let client = Client::new(client_query.user_id.unwrap_or(client_id.clone()), subs);
        let clients_lock = match CLIENTS.try_get() {
            Some(clients) => clients,
            None => {
                let error = ErrorResponse::new(Some(ErrorCode::InternalServerError), Some("Failed to get CLIENTS lock".into()));
                return Ok(warp::reply::json(&error))
            }
        };
        let mut clients = clients_lock.write().await; // Lock the CLIENTS for write access
        
        clients.insert(client_id.clone(), client);

        let response = SuccessResponse::new(Some(SuccessCode::Created), Some("Client registered".into()), Some(json!({ "client_id": client_id })));
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

