pub mod response;
pub mod message;

use std::{future::Future, pin::Pin};
use std::fmt::Debug;
use serde::{Deserialize, Serialize};

use crate::server::handlers::FileQuery;


pub trait RestRouterFunction<M, Resp>: Send + Sync + Clone
where
    Self: Send + Sync + Clone,
    M: Serialize + for<'de> Deserialize<'de> + Clone + Debug,
    Resp: Serialize + for<'de> Deserialize<'de> + Clone + Debug,
{
    fn route(&self, msg: M, auth_token: Option<String>) -> Pin<Box<dyn Future<Output = Resp> + Send>>;
    fn route_get_file(&self, msg: FileQuery) -> Pin<Box<dyn Future<Output = String> + Send>>;
}

pub trait SocketRouterFunction<M, Resp>: Send + Sync + Clone
where
    Self: Send + Sync + Clone,
    M: Serialize + for<'de> Deserialize<'de> + Clone + Debug,
    Resp: Serialize + for<'de> Deserialize<'de> + Clone + Debug,
{
    fn route(&self, msg: M) -> Pin<Box<dyn Future<Output = Resp> + Send>>;
}

