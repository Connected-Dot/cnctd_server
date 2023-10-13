use std::{net::SocketAddr, sync::Arc};

use serde::{Serialize, Deserialize};
use serde_json::Value;
use warp::{reject::Rejection, reply::Reply};

use crate::RouterFunction;

pub type Result<T> = std::result::Result<T, Rejection>;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct Message {
    channel: String,
    instruction: String,
    data: Option<Value>
}

impl Message {
    pub fn new(channel: &str, instruction: &str, data: Option<Value>) -> Self {
        Self {
            channel: channel.into(),
            instruction: instruction.into(),
            data,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Response {
    pub success: bool,
    pub msg: Option<String>,
    pub data: Option<Value>,
}

impl Response {
    pub fn success(msg: Option<String>, data: Option<Value>) -> Self {
        Self { success: true, msg, data }
    }
    pub fn failure(msg: Option<String>, data: Option<Value>) -> Self {
        Self { success: false, msg, data }
    }
}

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