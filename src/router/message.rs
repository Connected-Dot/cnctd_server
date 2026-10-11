//! Push message shapes (from `cnctd_protocol`) and the convenience senders
//! that go through the running server's connection table.

pub use cnctd_protocol::message::{Message, SocketMessage, UserMessage, UserSocketMessage};

use crate::socket::connection::Connections;

/// `Message::broadcast()` and `Message::to_user()` on the running server.
/// Bring the trait into scope: `use cnctd_server::router::message::Push;`.
#[async_trait::async_trait]
pub trait Push {
    /// To every connection subscribed to the message's channel (see
    /// [`Connections::global`]).
    async fn broadcast(&self) -> anyhow::Result<()>;

    /// To every connection of `user_id`, except `exclude_connection_id`.
    async fn to_user(&self, user_id: &str, exclude_connection_id: Option<String>) -> anyhow::Result<()>;
}

#[async_trait::async_trait]
impl Push for Message {
    async fn broadcast(&self) -> anyhow::Result<()> {
        Connections::global()?.broadcast(self).await
    }

    async fn to_user(&self, user_id: &str, exclude_connection_id: Option<String>) -> anyhow::Result<()> {
        Connections::global()?.message_user(user_id, self, exclude_connection_id).await
    }
}
