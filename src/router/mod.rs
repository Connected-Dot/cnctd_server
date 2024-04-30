pub mod response;
pub mod message;
pub mod request;


use std::{future::Future, pin::Pin};
use std::fmt::Debug;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::server::handlers::RedirectQuery;

use self::response::{ErrorResponse, SuccessResponse};


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
    fn route(&self, method: HttpMethod, path: String, data: Option<Value>, auth_token: Option<String>) -> Pin<Box<dyn Future<Output = Result<SuccessResponse, ErrorResponse>> + Send>>;
    fn route_redirect(&self, msg: RedirectQuery) -> Pin<Box<dyn Future<Output = String> + Send>>;
}

pub trait SocketRouterFunction<Req, Resp>: Send + Sync + Clone
where
    Self: Send + Sync + Clone,
    Req: Serialize + for<'de> Deserialize<'de> + Clone + Debug,
    Resp: Serialize + for<'de> Deserialize<'de> + Clone + Debug,
{
    fn route(&self, msg: Req) -> Pin<Box<dyn Future<Output = Resp> + Send>>;
}

