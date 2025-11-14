use std::sync::Arc;
use std::fmt::Debug;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;

// Keep Warp imports for backward compatibility during migration
use warp::{reject::Rejection, reply::Reply};
use warp::hyper::Uri as WarpUri;

// Add Axum imports
use axum::{
    response::{IntoResponse, Response},
    http::StatusCode,
    Json,
};

use crate::router::HttpMethod;
use crate::router::RestRouterFunction;

pub type Result<T> = std::result::Result<T, Rejection>;

pub trait RedirectHandler<Value>: Send + Sync
where
    Value: Serialize + DeserializeOwned + Send + Sync,
{
    fn handle(&self, data: Value) -> anyhow::Result<String>;
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RedirectQuery {
    pub path: String,
    pub client_id: String,
    pub size: Option<String>,
}

pub struct Handler;

impl Handler {
    // Warp handlers (keep for backward compatibility during migration)
    pub async fn post<R>(path: String, data: Value, auth_token: Option<String>, client_id: Option<String>, ip_address: Option<String>, router: Arc<R>) -> Result<impl warp::Reply>
    where
        R: RestRouterFunction,
    {
        match router.route(HttpMethod::POST, path, data, auth_token, client_id, ip_address).await {
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
    
    pub async fn get<R>(path: String, data: Value, auth_token: Option<String>, client_id: Option<String>, ip_address: Option<String>, router: Arc<R>) -> Result<impl warp::Reply>
    where
        R: RestRouterFunction,
    {
        match router.route(HttpMethod::GET, path, data, auth_token, client_id, ip_address).await {
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
    
    pub async fn put<R>(path: String, data: Value, auth_token: Option<String>, client_id: Option<String>, ip_address: Option<String>, router: Arc<R>) -> Result<impl warp::Reply>
    where
        R: RestRouterFunction,
    {
        match router.route(HttpMethod::PUT, path, data, auth_token, client_id, ip_address).await {
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

    pub async fn delete<R>(path: String, data: Value, auth_token: Option<String>, client_id: Option<String>, ip_address: Option<String>, router: Arc<R>) -> Result<impl warp::Reply>
    where
        R: RestRouterFunction,
    {
        match router.route(HttpMethod::DELETE, path, data, auth_token, client_id, ip_address).await {
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

    pub async fn get_redirect<R>(path: String, data: Value, auth_token: Option<String>, client_id: Option<String>, router: Arc<R>) -> Result<impl warp::Reply>
    where

        R: RestRouterFunction,
    {
        println!("File router. path: {}", path);
        println!("File HANDLER, data: {:?}", data);
        let url = router.route_redirect(path, data, auth_token, client_id).await;
        println!("File HANDLER, url: {}", url);
        match url.parse::<WarpUri>() {
            Ok(uri) => Ok(warp::redirect::found(uri).into_response()),
            Err(_) => Err(warp::reject::not_found())
        }

        // Ok(warp::redirect::found(url.parse::<Uri>().unwrap()).into_response())
    }

    pub async fn api_redirect<V, H>(data: V, handler: Arc<H>) -> Result<impl warp::Reply>
    where
        V: Serialize + DeserializeOwned + Send + Sync,
        H: RedirectHandler<V>,
    {
        match handler.handle(data) {
            Ok(html_response) => Ok(warp::reply::html(html_response)),
            Err(_) => Err(warp::reject::reject()),
        }
    }

    // New Axum handlers
    pub async fn axum_post<R>(path: String, data: Value, auth_token: Option<String>, client_id: Option<String>, ip_address: Option<String>, router: Arc<R>) -> Response
    where
        R: RestRouterFunction,
    {
        match router.route(HttpMethod::POST, path, data, auth_token, client_id, ip_address).await {
            Ok(response) => {
                let status = response.status.to_axum_status_code();
                (status, Json(response)).into_response()
            }
            Err(e) => {
                let status = e.status.to_axum_status_code();
                (status, Json(e)).into_response()
            }
        }
    }
    
    pub async fn axum_get<R>(path: String, data: Value, auth_token: Option<String>, client_id: Option<String>, ip_address: Option<String>, router: Arc<R>) -> Response
    where
        R: RestRouterFunction,
    {
        match router.route(HttpMethod::GET, path, data, auth_token, client_id, ip_address).await {
            Ok(response) => {
                let status = response.status.to_axum_status_code();
                (status, Json(response)).into_response()
            }
            Err(e) => {
                let status = e.status.to_axum_status_code();
                (status, Json(e)).into_response()
            }
        }
    }
    
    pub async fn axum_put<R>(path: String, data: Value, auth_token: Option<String>, client_id: Option<String>, ip_address: Option<String>, router: Arc<R>) -> Response
    where
        R: RestRouterFunction,
    {
        match router.route(HttpMethod::PUT, path, data, auth_token, client_id, ip_address).await {
            Ok(response) => {
                let status = response.status.to_axum_status_code();
                (status, Json(response)).into_response()
            }
            Err(e) => {
                let status = e.status.to_axum_status_code();
                (status, Json(e)).into_response()
            }
        }
    }

    pub async fn axum_delete<R>(path: String, data: Value, auth_token: Option<String>, client_id: Option<String>, ip_address: Option<String>, router: Arc<R>) -> Response
    where
        R: RestRouterFunction,
    {
        match router.route(HttpMethod::DELETE, path, data, auth_token, client_id, ip_address).await {
            Ok(response) => {
                let status = response.status.to_axum_status_code();
                (status, Json(response)).into_response()
            }
            Err(e) => {
                let status = e.status.to_axum_status_code();
                (status, Json(e)).into_response()
            }
        }
    }
}
