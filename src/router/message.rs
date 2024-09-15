use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::socket::{client::CnctdClient, CnctdSocket};

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct Message {
    pub channel: String,
    pub instruction: String,
    pub data: Option<Value>
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UserMessage {
    pub data_type: Option<String>,
    pub data: Option<Value>,
    pub user_id: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UserSocketMessage {
    pub receiver_id: String,
    pub message: Message,
}


#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SocketMessage {
    pub channel: String,
    pub instruction: Option<String>,
    pub data: Option<Value>,
    pub response_channel: Option<String>,
}

impl Message {
    pub fn new(channel: &str, instruction: &str, data: Option<Value>) -> Self {
        Self {
            channel: channel.into(),
            instruction: instruction.into(),
            data,
        }
    }

    pub async fn broadcast(&self) -> anyhow::Result<()> {
        CnctdSocket::broadcast_message(self).await?;
        
        Ok(())
    }

    pub async fn to_user(&self, user_id: &str, exclude_client_id: Option<String>) -> anyhow::Result<()> {
        CnctdClient::message_user(user_id, self, exclude_client_id).await?;
        
        Ok(())
    }
}