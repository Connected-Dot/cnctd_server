pub mod response;
pub mod message;
pub mod request;


use std::{future::Future, pin::Pin};
use std::fmt::Debug;
use serde::{Deserialize, Serialize};

use crate::server::handlers::RedirectQuery;

use self::response::{ErrorResponse, SuccessResponse};


#[derive(Debug)]
pub enum HttpMethod {
    GET,
    POST,
    PUT,
    DELETE,
}

pub trait RestRouterFunction<DataIn, DataOut>: Send + Sync + Clone
where
    Self: Send + Sync + Clone,
    DataIn: Serialize + for<'de> Deserialize<'de> + Clone + Debug,
    DataOut: Serialize + for<'de> Deserialize<'de> + Clone + Debug,
{
    fn route(&self, method: HttpMethod, path: String, msg: DataIn, auth_token: Option<String>) -> Pin<Box<dyn Future<Output = Result<SuccessResponse<DataOut>, ErrorResponse>> + Send>>;
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

