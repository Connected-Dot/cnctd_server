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