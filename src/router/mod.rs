pub mod response;
pub mod message;

use std::{future::Future, pin::Pin};
use std::fmt::Debug;
use serde::{Deserialize, Serialize};

use crate::server::handlers::RedirectQuery;

#[derive(Debug)]
pub enum HttpMethod {
    GET,
    POST,
    PUT,
    DELETE,
}

pub trait RestRouterFunction<M, Resp>: Send + Sync + Clone
where
    Self: Send + Sync + Clone,
    M: Serialize + for<'de> Deserialize<'de> + Clone + Debug,
    Resp: Serialize + for<'de> Deserialize<'de> + Clone + Debug,
{
    fn route(&self, method: HttpMethod, path: String, msg: M, auth_token: Option<String>) -> Pin<Box<dyn Future<Output = Resp> + Send>>;
    // fn route_get(&self, path: String, msg: M, auth_token: Option<String>) -> Pin<Box<dyn Future<Output = Resp> + Send>>;
    // fn route_post(&self, path: String, msg: M, auth_token: Option<String>) -> Pin<Box<dyn Future<Output = Resp> + Send>>;
    // fn route_put(&self, path: String, msg: M, auth_token: Option<String>) -> Pin<Box<dyn Future<Output = Resp> + Send>>;
    // fn route_delete(&self, path: String, msg: M, auth_token: Option<String>) -> Pin<Box<dyn Future<Output = Resp> + Send>>;
    fn route_redirect(&self, msg: RedirectQuery) -> Pin<Box<dyn Future<Output = String> + Send>>;
}

pub trait SocketRouterFunction<M, Resp>: Send + Sync + Clone
where
    Self: Send + Sync + Clone,
    M: Serialize + for<'de> Deserialize<'de> + Clone + Debug,
    Resp: Serialize + for<'de> Deserialize<'de> + Clone + Debug,
{
    fn route(&self, msg: M) -> Pin<Box<dyn Future<Output = Resp> + Send>>;
}

