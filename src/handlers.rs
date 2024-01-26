use std::sync::Arc;

use warp::reject::Rejection;

use crate::{message::Message, router::RouterFunction};

pub type Result<T> = std::result::Result<T, Rejection>;

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
}

