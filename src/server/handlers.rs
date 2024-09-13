use std::time::Duration;
use std::sync::Arc;
use std::fmt::Debug;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use warp::{reject::Rejection, reply::Reply};
use warp::hyper::Uri;

use crate::router::error::{ErrorCode, ErrorResponse};
use crate::router::response::{SuccessCode, SuccessResponse};
use crate::router::HttpMethod;
use crate::{auth::CnctdAuth, router::{RestRouterFunction}, socket::{Client, CnctdSocket, CLIENTS}};

pub type Result<T> = std::result::Result<T, Rejection>;


pub trait RedirectHandler<Value>: Send + Sync
where
    Value: Serialize + DeserializeOwned + Send + Sync,
{
    fn handle(&self, data: Value) -> anyhow::Result<String>;
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ClientQuery {
    subscriptions: Option<String>,
    unauth_id: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RedirectQuery {
    pub path: String,
    pub client_id: String,
    pub size: Option<String>,
}

pub struct Handler;

impl Handler {
    pub async fn post<R>(path: String, data: Value, auth_token: Option<String>, client_id: Option<String>, router: Arc<R>) -> Result<impl warp::Reply>
    where
        R: RestRouterFunction,
    {
        match router.route(HttpMethod::POST, path, data, auth_token, client_id).await {
            Ok(response) => {
                let status = &response.status.to_warp_status_code();
                let json = warp::reply::json(&response);

                Ok(warp::reply::with_status(json, status.clone()))
            }
            Err(e) => {
                let status = &e.status.to_warp_status_code();
                let json = warp::reply::json(&e);
            
                Ok(warp::reply::with_status(json, status.clone()))
            }
        }
    }
    
    pub async fn get<R>(path: String, data: Value, auth_token: Option<String>, client_id: Option<String>, router: Arc<R>) -> Result<impl warp::Reply>
    where
        R: RestRouterFunction,
        
    {
        match router.route(HttpMethod::GET, path, data, auth_token, client_id).await {
            Ok(response) => {
                let status = &response.status.to_warp_status_code();
                let json = warp::reply::json(&response);

                Ok(warp::reply::with_status(json, status.clone()))
            }
            Err(e) => {
                let status = &e.status.to_warp_status_code();
                let json = warp::reply::json(&e);
            
                Ok(warp::reply::with_status(json, status.clone()))
            }
        }
    }
    
    pub async fn put<R>(path: String, data: Value, auth_token: Option<String>, client_id: Option<String>, router: Arc<R>) -> Result<impl warp::Reply>
    where
        R: RestRouterFunction,
        
    {
        match router.route(HttpMethod::PUT, path, data, auth_token, client_id).await {
            Ok(response) => {
                let status = &response.status.to_warp_status_code();
                let json = warp::reply::json(&response);

                Ok(warp::reply::with_status(json, status.clone()))
            }
            Err(e) => {
                let status = &e.status.to_warp_status_code();
                let json = warp::reply::json(&e);
            
                Ok(warp::reply::with_status(json, status.clone()))
            }
        }
    }

    pub async fn delete<R>(path: String, data: Value, auth_token: Option<String>, client_id: Option<String>, router: Arc<R>) -> Result<impl warp::Reply>
    where

        R: RestRouterFunction,
        
    {
        match router.route(HttpMethod::DELETE, path, data, auth_token, client_id).await {
            Ok(response) => {
                let status = &response.status.to_warp_status_code();
                let json = warp::reply::json(&response);

                Ok(warp::reply::with_status(json, status.clone()))
            }
            Err(e) => {
                let status = &e.status.to_warp_status_code();
                let json = warp::reply::json(&e);
            
                Ok(warp::reply::with_status(json, status.clone()))
            }
        }
    }

    pub async fn get_redirect<R>(data: RedirectQuery, router: Arc<R>) -> Result<impl warp::Reply>
    where

        R: RestRouterFunction,
    {
        // println!("File router. path: {}", path);
        // println!("File HANDLER, data: {:?}", data);
        let url = router.route_redirect(data).await;
        // println!("File HANDLER, url: {}", url);
        match url.parse::<Uri>() {
            Ok(uri) => Ok(warp::redirect::found(uri).into_response()),
            Err(_) => Err(warp::reject::not_found())
        }

        // Ok(warp::redirect::found(url.parse::<Uri>().unwrap()).into_response())
    }

    pub async fn api_redirect<Value, H>(data: Value, handler: Arc<H>) -> Result<impl warp::Reply>
    where
        Value: Serialize + DeserializeOwned + Send + Sync,
        H: RedirectHandler<Value>,
    {
        match handler.handle(data) {
            Ok(html_response) => Ok(warp::reply::html(html_response)),
            Err(_) => Err(warp::reject::reject()),
        }
    }

    pub async fn register_socket_client(client_query: ClientQuery, secret: Option<Vec<u8>>, auth_token: Option<String>) -> Result<impl warp::Reply> {
        let client_id = uuid::Uuid::new_v4().to_string();
        let subs = match client_query.subscriptions {
            Some(subs) => {
                subs.trim_matches(|c| c == '[' || c == ']')
                .split(',')
                .map(|s| s.trim().to_string())
                .collect()
            }
            None => vec![]
        };
        
        let user_id = match client_query.unauth_id {
            Some(user_id) => user_id,
            None => {
                match &secret {
                    Some(secret) => {
                        let auth_token = match &auth_token {
                            Some(token) => token,
                            None => {
                                let error = ErrorResponse::new(Some(ErrorCode::Unauthorized), Some("No auth_token provided".into()));
                                return Ok(warp::reply::json(&error))
                            }
                        };
        
                        // Perform the verification
                        match CnctdAuth::verify_auth_token(secret.clone(), auth_token) {
                            Ok(user_id) => user_id,
                            Err(e) => {
                                let error = ErrorResponse::new(Some(ErrorCode::Unauthorized), Some(e.to_string()));
                                return Ok(warp::reply::json(&error))
                            }
                        }
                    }
                    None => return Ok(warp::reply::json(&ErrorResponse::new(Some(ErrorCode::Unauthorized), Some("No user_id provided".into()))))
                }
            }
        };
        
        let client = Client::new(user_id, subs);
        let clients_lock = match CLIENTS.try_get() {
            Some(clients) => clients,
            None => {
                let error = ErrorResponse::new(Some(ErrorCode::InternalServerError), Some("Failed to get CLIENTS lock".into()));
                return Ok(warp::reply::json(&error))
            }
        };
        let mut clients = clients_lock.write().await; // Lock the CLIENTS for write access
        
        clients.insert(client_id.clone(), client);
        println!("clients length: {:?}", clients.len());

        let response = SuccessResponse::new(Some(SuccessCode::Created), Some("Client registered".into()), Some(client_id.clone().into()));
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

