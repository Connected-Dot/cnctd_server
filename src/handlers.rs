use std::sync::Arc;
use serde::{de::DeserializeOwned, Serialize};
use warp::reject::Rejection;

use crate::{message::Message, router::RouterFunction};

pub type Result<T> = std::result::Result<T, Rejection>;

pub trait RedirectHandler<M>: Send + Sync
where
    M: Serialize + DeserializeOwned + Send + Sync,
{
    fn handle(&self, msg: M) -> anyhow::Result<String>;
}


pub struct Handler;

impl Handler {
    pub async fn post<R>(msg: Message, router: Arc<R>) -> Result<impl warp::Reply>
    where
        R: RouterFunction,
    {
        println!("post message...channel: {}, instruction: {}", msg.channel, msg.instruction);
        let response = router.route(msg).await;
        Ok(warp::reply::json(&response))
    }
    
    pub async fn get<R>(msg: Message, router: Arc<R>) -> Result<impl warp::Reply>
    where
        R: RouterFunction,
    {
        println!("get message...channel: {}, instruction: {}", msg.channel, msg.instruction);
        let response = router.route(msg).await;
    
        Ok(warp::reply::json(&response))
    }

    pub async fn get_redirect<M, H>(msg: M, handler: Arc<H>) -> Result<impl warp::Reply>
    where
        M: Serialize + DeserializeOwned + Send + Sync,
        H: RedirectHandler<M>,
    {
        match handler.handle(msg) {
            Ok(html_response) => Ok(warp::reply::html(html_response)),
            Err(_) => Err(warp::reject::reject()),
        }
    }
}

