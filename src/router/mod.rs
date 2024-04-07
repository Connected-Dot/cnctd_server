pub mod response;
pub mod message;

use std::{future::Future, pin::Pin};
use std::fmt::Debug;
use serde::{Deserialize, Serialize};


pub trait RestRouterFunction<M, Resp>: Send + Sync + Clone
where
    Self: Send + Sync + Clone,
    M: Serialize + for<'de> Deserialize<'de> + Clone + Debug,
    Resp: Serialize + for<'de> Deserialize<'de> + Clone + Debug,
{
    fn route(&self, msg: M, auth_token: Option<String>) -> Pin<Box<dyn Future<Output = Resp> + Send>>;
}

pub trait SocketRouterFunction<M, Resp>: Send + Sync + Clone
where
    Self: Send + Sync + Clone,
    M: Serialize + for<'de> Deserialize<'de> + Clone + Debug,
    Resp: Serialize + for<'de> Deserialize<'de> + Clone + Debug,
{
    fn route(&self, msg: M) -> Pin<Box<dyn Future<Output = Resp> + Send>>;
}