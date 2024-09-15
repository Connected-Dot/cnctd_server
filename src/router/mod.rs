pub mod response;
pub mod message;
pub mod request;
pub mod error;


use std::{future::Future, pin::Pin};
use std::fmt::Debug;
use serde::{Deserialize, Serialize};
use serde_json::Value;


use self::error::ErrorResponse;
use self::response::SuccessResponse;


#[derive(Debug)]
pub enum HttpMethod {
    GET,
    POST,
    PUT,
    DELETE,
}

pub trait RestRouterFunction: Send + Sync + Clone
where
    Self: Send + Sync + Clone,
{
    fn route(&self, method: HttpMethod, path: String, data: Value, auth_token: Option<String>, client_id: Option<String>) -> Pin<Box<dyn Future<Output = Result<SuccessResponse, ErrorResponse>> + Send>>;
    fn route_redirect(&self, path: String, data: Value, auth_token: Option<String>, client_id: Option<String>) -> Pin<Box<dyn Future<Output = String> + Send>>;
}

pub trait SocketRouterFunction<Req, Resp>: Send + Sync + Clone
where
    Self: Send + Sync + Clone,
    Req: Serialize + for<'de> Deserialize<'de> + Clone + Debug,
    Resp: Serialize + for<'de> Deserialize<'de> + Clone + Debug,
{
    fn route(&self, msg: Req, client_id: String) -> Pin<Box<dyn Future<Output = Option<Resp>> + Send>>;
}

