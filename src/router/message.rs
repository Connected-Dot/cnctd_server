use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct Message {
    pub channel: String,
    pub instruction: String,
    pub data: Option<Value>
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UserMessage {
    pub channel: String,
    pub instruction: String,
    pub data: Option<Value>,
    pub user_id: String,
}


#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SocketMessage {
    pub channel: String,
    pub instruction: String,
    pub data: Option<Value>,
    pub response_channel: String,
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