use std::{future::Future, pin::Pin};

use crate::{message::Message, response::Response};

pub trait RouterFunction: Send + Sync + Clone + Copy {
    fn route(&self, msg: Message) -> Pin<Box<dyn Future<Output = Response> + Send>>;
}
