use std::{net::SocketAddr, sync::Arc};

use serde::{Serialize, Deserialize};
use serde_json::Value;
use warp::{reject::Rejection, reply::Reply};

use crate::{RouterFunction, Message};

pub type Result<T> = std::result::Result<T, Rejection>;

pub async fn post_handler<R>(msg: Message, router: Arc<R>) -> Result<impl warp::Reply>
where
    R: RouterFunction,
{
    println!("post message...channel: {}, instruction: {}", msg.channel, msg.instruction);
    let response = router.route(msg).await;
    Ok(warp::reply::json(&response))
}



pub async fn get_handler<R>(msg: Message, router: Arc<R>) -> Result<impl warp::Reply>
where
    R: RouterFunction,
{
    println!("get message...channel: {}, instruction: {}", msg.channel, msg.instruction);
    let response = router.route(msg).await;

    Ok(warp::reply::json(&response))
}