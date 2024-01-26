use std::{future::Future, pin::Pin};

use crate::{message::Message, response::Response};

pub trait RouterFunction: Send + Sync {
    fn route(&self, msg: Message) -> Pin<Box<dyn Future<Output = Response> + Send>>;
}
