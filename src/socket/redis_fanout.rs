//! Cross-instance push over Redis pub/sub. Feature `redis`.
//!
//! Every instance of a server (same `ServerConfig.id`) publishes its pushes
//! to one Redis channel and subscribes to it. An instance delivers the
//! envelopes it receives to its own connections, skipping the ones it
//! published itself (matched by `origin`, the session id). Local delivery
//! for the publishing instance already happened in `Connections`.
//!
//! The subscriber reconnects with backoff when Redis goes away; publishes
//! during an outage fail (and are logged) rather than queue.

use std::{sync::Arc, time::Duration};

use anyhow::Context;
use async_trait::async_trait;
use futures_util::StreamExt;
use redis::{aio::MultiplexedConnection, AsyncCommands, Client};
use tokio::{sync::Mutex, task::JoinHandle};
use tracing::{debug, info, warn};

use super::{
    connection::Connections,
    fanout::{Envelope, Fanout},
};

pub struct RedisFanout {
    client: Client,
    channel: String,
    publisher: Mutex<Option<MultiplexedConnection>>,
    subscriber: Mutex<Option<JoinHandle<()>>>,
}

impl RedisFanout {
    /// Connect to `url` and fan out on the channel for `server_id`
    /// (`cnctd_server:fanout:<server_id>`). Fails if Redis is unreachable.
    pub async fn connect(url: &str, server_id: &str) -> anyhow::Result<Self> {
        Self::connect_on_channel(url, &format!("cnctd_server:fanout:{}", server_id)).await
    }

    pub async fn connect_on_channel(url: &str, channel: &str) -> anyhow::Result<Self> {
        let client = Client::open(url).context("redis url")?;
        let conn = client
            .get_multiplexed_async_connection()
            .await
            .context("redis connection for fan-out publishing")?;
        info!(channel, "redis fan-out connected");
        Ok(Self {
            client,
            channel: channel.to_string(),
            publisher: Mutex::new(Some(conn)),
            subscriber: Mutex::new(None),
        })
    }

    pub fn channel(&self) -> &str {
        &self.channel
    }

    async fn subscribe_loop(client: Client, channel: String, connections: Connections) {
        let own = connections.identity().session_id.clone();
        let mut backoff = Duration::from_millis(250);
        loop {
            match Self::run_subscriber(&client, &channel, &connections, &own).await {
                Ok(()) => {
                    debug!(channel, "redis subscription ended; resubscribing");
                    backoff = Duration::from_millis(250);
                }
                Err(e) => {
                    warn!(channel, "redis subscription failed: {}; retrying in {:?}", e, backoff);
                    tokio::time::sleep(backoff).await;
                    backoff = (backoff * 2).min(Duration::from_secs(30));
                }
            }
        }
    }

    async fn run_subscriber(client: &Client, channel: &str, connections: &Connections, own: &str) -> anyhow::Result<()> {
        let mut pubsub = client.get_async_pubsub().await.context("redis pubsub connection")?;
        pubsub.subscribe(channel).await.context("redis subscribe")?;
        info!(channel, "redis fan-out subscribed");
        let mut stream = pubsub.on_message();
        while let Some(msg) = stream.next().await {
            let payload: Vec<u8> = match msg.get_payload() {
                Ok(p) => p,
                Err(e) => {
                    warn!("fan-out message without a payload: {}", e);
                    continue;
                }
            };
            let envelope: Envelope = match serde_json::from_slice(&payload) {
                Ok(e) => e,
                Err(e) => {
                    warn!("fan-out message is not an envelope: {}", e);
                    continue;
                }
            };
            if envelope.origin == own {
                continue;
            }
            let n = connections
                .deliver_local(&envelope.target, &envelope.payload, envelope.exclude.as_deref())
                .await;
            debug!(?envelope.target, delivered = n, "fan-out delivered");
        }
        Ok(())
    }
}

#[async_trait]
impl Fanout for RedisFanout {
    async fn publish(&self, envelope: Envelope) -> anyhow::Result<()> {
        let payload = serde_json::to_vec(&envelope)?;
        let mut guard = self.publisher.lock().await;
        if guard.is_none() {
            *guard = Some(
                self.client
                    .get_multiplexed_async_connection()
                    .await
                    .context("redis reconnect for fan-out publishing")?,
            );
        }
        let conn = guard.as_mut().expect("just set");
        match conn.publish::<_, _, ()>(&self.channel, payload).await {
            Ok(()) => Ok(()),
            Err(e) => {
                // Drop the connection so the next publish reconnects.
                *guard = None;
                Err(anyhow::anyhow!("redis publish failed: {}", e))
            }
        }
    }

    async fn attach(&self, connections: Connections) -> anyhow::Result<()> {
        let handle = tokio::spawn(Self::subscribe_loop(self.client.clone(), self.channel.clone(), connections));
        let mut guard = self.subscriber.lock().await;
        if let Some(old) = guard.replace(handle) {
            old.abort();
        }
        Ok(())
    }

    fn shutdown(&self) {
        if let Ok(mut guard) = self.subscriber.try_lock() {
            if let Some(h) = guard.take() {
                h.abort();
            }
        }
    }
}

impl std::fmt::Debug for RedisFanout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RedisFanout").field("channel", &self.channel).finish_non_exhaustive()
    }
}

/// `Arc<RedisFanout>` is what `ServerConfig::with_fanout` wants.
impl From<RedisFanout> for Arc<dyn Fanout> {
    fn from(f: RedisFanout) -> Self {
        Arc::new(f)
    }
}
