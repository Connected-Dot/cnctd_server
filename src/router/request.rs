use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct Request {
    pub user_id: String,
    pub key: Option<String>,
    pub data: Option<Value>,
}