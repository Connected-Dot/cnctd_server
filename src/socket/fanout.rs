//! Cross-instance push. Local delivery always happens inside
//! [`Connections`](super::connection::Connections); a `Fanout` only has to
//! tell the *other* instances about a push so they can deliver to the
//! connections they hold. One instance needs nothing — [`LocalOnly`].
//!
//! A Redis-backed implementation publishes the envelope and runs a
//! subscriber that calls `Connections::deliver_local` for envelopes whose
//! `origin` is not its own session id.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::connection::Connections;

/// Who a push is for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FanoutTarget {
    /// Every connection subscribed to the channel.
    Channel(String),
    /// Every connection whose `user_id` matches.
    User(String),
    /// One connection by id.
    Connection(String),
    /// Every connection whose `data[key] == value` (string compare).
    KeyValue { key: String, value: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Payload {
    Text(String),
    Binary(Vec<u8>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope {
    /// Session id of the instance that published it. Subscribers skip their own.
    pub origin: String,
    pub target: FanoutTarget,
    pub payload: Payload,
    /// A connection id that must not receive it (the sender's own device).
    pub exclude: Option<String>,
}

#[async_trait]
pub trait Fanout: Send + Sync + 'static {
    /// Tell other instances. Local connections were already served.
    async fn publish(&self, envelope: Envelope) -> anyhow::Result<()>;

    /// Called once by the server with its table, before it serves. A remote
    /// fan-out starts its subscriber here and delivers into `connections`.
    async fn attach(&self, _connections: Connections) -> anyhow::Result<()> {
        Ok(())
    }

    /// Called when the server shuts down.
    fn shutdown(&self) {}

    /// `true` when there are no other instances to tell, which lets
    /// `Connections` report "no such connection" instead of guessing that
    /// another instance might hold it.
    fn is_local_only(&self) -> bool {
        false
    }
}

/// The default: a single instance, nothing to tell.
#[derive(Debug, Clone, Copy, Default)]
pub struct LocalOnly;

#[async_trait]
impl Fanout for LocalOnly {
    async fn publish(&self, _envelope: Envelope) -> anyhow::Result<()> {
        Ok(())
    }

    fn is_local_only(&self) -> bool {
        true
    }
}
