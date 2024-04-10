use serde::{Deserialize, Serialize};
use serde_json::Value;

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

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SocketResponse {
    pub success: bool,
    pub msg: Option<String>,
    pub data: Option<Value>,
    pub response_channel: String,
}

impl SocketResponse {
    pub fn success(msg: Option<String>, data: Option<Value>, response_channel: String) -> Self {
        Self {
            success: true,
            msg,
            data,
            response_channel
        }
    }

    pub fn failure(msg: Option<String>, data: Option<Value>, response_channel: String) -> Self {
        Self {
            success: false,
            msg,
            data,
            response_channel
        }
    }
}