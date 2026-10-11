//! Push message shapes, and the two convenience senders that go through the
//! server's connection table.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::socket::connection::Connections;

/// What subscribers receive on a channel:
/// `{"channel": ..., "instruction": ..., "data": ...}`.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct Message {
    pub channel: String,
    pub instruction: String,
    pub data: Option<Value>,
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

/// What clients send: a channel, an optional instruction and data, and an
/// optional `response_channel` for request/response over the socket.
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

    /// To every connection subscribed to `self.channel`, on the running
    /// server (see [`Connections::global`]).
    pub async fn broadcast(&self) -> anyhow::Result<()> {
        Connections::global()?.broadcast(self).await
    }

    /// To every connection of `user_id`, except `exclude_connection_id`.
    pub async fn to_user(&self, user_id: &str, exclude_connection_id: Option<String>) -> anyhow::Result<()> {
        Connections::global()?.message_user(user_id, self, exclude_connection_id).await
    }
}
