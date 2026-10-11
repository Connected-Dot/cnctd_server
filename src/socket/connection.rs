//! The connection table.
//!
//! A `Connection` is the live WebSocket link from a running client to this
//! server: it exists from registration (REST or inline on the upgrade) until
//! disconnect, and every connect gets a fresh `connection_id`. Domain
//! identity (which user, which device) is attached afterwards through
//! `user_id` and the `data` blob.
//!
//! [`Connections`] is the table — a plain, cloneable handle you can construct
//! in a test. The server builds one, hands it to the socket layer and the
//! heartbeat, and installs it as the process-wide default so the
//! `CnctdConnection::*` associated functions (the API cnctd.world calls from
//! anywhere) keep working without threading a handle around.
//!
//! Lock discipline: the table lock is never held across a send or across
//! another lock. Every method collects what it needs, drops the guard, then
//! acts. (0.6 held nested read guards on a write-preferring lock and froze
//! the whole table whenever a writer queued up behind a heartbeat.)

use std::{
    collections::HashMap,
    fmt::Debug,
    sync::{Arc, RwLock as StdRwLock},
    time::Duration,
};

use anyhow::anyhow;
use chrono::{DateTime, FixedOffset, Utc};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::{json, Value};
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, warn};

use super::fanout::{Envelope, Fanout, FanoutTarget, LocalOnly, Payload};
use crate::router::message::Message;

pub use cnctd_protocol::connection::{ConnectionFormat, ConnectionInfo, UpgradeQuery};

/// 0.6 name for [`UpgradeQuery`].
pub type QueryParams = UpgradeQuery;

/// A frame queued for a connection's socket task.
#[derive(Debug, Clone)]
pub enum Outbound {
    Text(String),
    Binary(Vec<u8>),
    /// A protocol-level ping; the peer answers with a pong on its own.
    Ping,
}

pub type Sender = mpsc::UnboundedSender<Outbound>;

#[derive(Debug, Clone)]
pub struct CnctdConnection {
    pub user_id: String,
    pub ip_address: Option<String>,
    pub authenticated: bool,
    pub subscriptions: Vec<String>,
    pub format: ConnectionFormat,
    pub sender: Option<Sender>,
    pub data: Value,
    pub created_at: DateTime<FixedOffset>,
    pub updated_at: DateTime<FixedOffset>,
}

fn now() -> DateTime<FixedOffset> {
    Utc::now().with_timezone(&FixedOffset::east_opt(0).unwrap())
}

impl CnctdConnection {
    pub fn new(subscriptions: Vec<String>, ip_address: Option<String>, format: ConnectionFormat) -> Self {
        Self {
            user_id: String::new(),
            ip_address,
            authenticated: false,
            subscriptions,
            format,
            sender: None,
            data: json!({}),
            created_at: now(),
            updated_at: now(),
        }
    }

    pub fn connected(&self) -> bool {
        self.sender.as_ref().is_some_and(|s| !s.is_closed())
    }

    pub fn to_connection_info(&self, connection_id: &str, identity: &ServerIdentity) -> ConnectionInfo {
        ConnectionInfo {
            connection_id: connection_id.to_string(),
            user_id: self.user_id.clone(),
            ip_address: self.ip_address.clone(),
            authenticated: self.authenticated,
            subscriptions: self.subscriptions.clone(),
            format: self.format.clone(),
            data: self.data.clone(),
            connected: self.sender.is_some(),
            server_id: identity.id.clone(),
            server_session_id: identity.session_id.clone(),
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

/// Which server (and which run of it) a connection belongs to.
#[derive(Debug, Clone)]
pub struct ServerIdentity {
    pub id: String,
    /// Fresh per process start.
    pub session_id: String,
}

/// How long a registered connection may stay unopened before it is dropped.
pub const UNOPENED_GRACE: Duration = Duration::from_secs(10);

#[derive(Clone)]
pub struct Connections {
    table: Arc<RwLock<HashMap<String, CnctdConnection>>>,
    identity: Arc<ServerIdentity>,
    fanout: Arc<dyn Fanout>,
}

impl Debug for Connections {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Connections").field("identity", &self.identity).finish_non_exhaustive()
    }
}

static GLOBAL: StdRwLock<Option<Connections>> = StdRwLock::new(None);

impl Connections {
    pub fn new(server_id: &str, fanout: Arc<dyn Fanout>) -> Self {
        Self {
            table: Default::default(),
            identity: Arc::new(ServerIdentity {
                id: server_id.to_string(),
                session_id: uuid::Uuid::new_v4().to_string(),
            }),
            fanout,
        }
    }

    /// A table with no cross-instance fan-out. What tests want.
    pub fn standalone(server_id: &str) -> Self {
        Self::new(server_id, Arc::new(LocalOnly))
    }

    pub fn identity(&self) -> &ServerIdentity {
        &self.identity
    }

    /// Make this the table behind `CnctdConnection::*`, `Message::broadcast`
    /// and `CnctdSocket::broadcast_message`. The server does this on start;
    /// the most recently started server wins.
    pub fn install_global(&self) {
        *GLOBAL.write().unwrap() = Some(self.clone());
    }

    /// The running server's table. An error, not a panic, when no server
    /// has started.
    pub fn global() -> anyhow::Result<Connections> {
        GLOBAL
            .read()
            .unwrap()
            .clone()
            .ok_or_else(|| anyhow!("no server is running: connections not initialized"))
    }

    // ------------------------------------------------------------ lifecycle

    /// Create a connection the client will open within [`UNOPENED_GRACE`];
    /// returns its id. Unopened ones are dropped after the grace period.
    pub async fn register(&self, subscriptions: Vec<String>, ip_address: Option<String>, format: ConnectionFormat) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        let n = {
            let mut table = self.table.write().await;
            table.insert(id.clone(), CnctdConnection::new(subscriptions, ip_address, format));
            table.len()
        };
        debug!(connection_id = %id, connections = n, "registered");

        let this = self.clone();
        let grace_id = id.clone();
        tokio::spawn(async move {
            tokio::time::sleep(UNOPENED_GRACE).await;
            let unopened = {
                let table = this.table.read().await;
                table.get(&grace_id).is_some_and(|c| c.sender.is_none())
            };
            if unopened {
                debug!(connection_id = %grace_id, "never opened; removing");
                this.table.write().await.remove(&grace_id);
            }
        });

        id
    }

    /// Attach the socket task's sender. Errors when the id is unknown.
    pub(crate) async fn attach_sender(&self, connection_id: &str, sender: Sender) -> anyhow::Result<()> {
        let mut table = self.table.write().await;
        let c = table.get_mut(connection_id).ok_or_else(|| anyhow!("no matching connection"))?;
        c.sender = Some(sender);
        c.updated_at = now();
        Ok(())
    }

    /// Drop a connection whose socket is gone.
    pub async fn remove(&self, connection_id: &str) -> bool {
        self.table.write().await.remove(connection_id).is_some()
    }

    pub async fn contains(&self, connection_id: &str) -> bool {
        self.table.read().await.contains_key(connection_id)
    }

    pub async fn count(&self) -> usize {
        self.table.read().await.len()
    }

    // ------------------------------------------------------------ reads

    pub async fn get(&self, connection_id: &str) -> anyhow::Result<CnctdConnection> {
        self.table
            .read()
            .await
            .get(connection_id)
            .cloned()
            .ok_or_else(|| anyhow!("No matching connection"))
    }

    pub async fn info(&self, connection_id: &str) -> anyhow::Result<ConnectionInfo> {
        Ok(self.get(connection_id).await?.to_connection_info(connection_id, &self.identity))
    }

    pub async fn infos(&self) -> Vec<ConnectionInfo> {
        self.table
            .read()
            .await
            .iter()
            .map(|(id, c)| c.to_connection_info(id, &self.identity))
            .collect()
    }

    pub async fn all(&self) -> HashMap<String, CnctdConnection> {
        self.table.read().await.clone()
    }

    pub async fn ids_for_user(&self, user_id: &str) -> Vec<String> {
        self.table
            .read()
            .await
            .iter()
            .filter(|(_, c)| c.user_id == user_id)
            .map(|(id, _)| id.clone())
            .collect()
    }

    pub async fn subscriber_ids(&self, channel: &str) -> Vec<String> {
        self.table
            .read()
            .await
            .iter()
            .filter(|(_, c)| c.subscriptions.iter().any(|s| s == channel))
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// `(connection_id, format)` for every subscriber: dispatch JSON to some,
    /// binary to others.
    pub async fn subscriber_formats(&self, channel: &str) -> Vec<(String, ConnectionFormat)> {
        self.table
            .read()
            .await
            .iter()
            .filter(|(_, c)| c.subscriptions.iter().any(|s| s == channel))
            .map(|(id, c)| (id.clone(), c.format.clone()))
            .collect()
    }

    // ------------------------------------------------------------ data blob

    pub async fn add_data(&self, connection_id: &str, data: Value) -> anyhow::Result<()> {
        self.with_mut(connection_id, |c| {
            c.data = data;
            Ok(())
        })
        .await
    }

    pub async fn update_data_field(&self, connection_id: &str, key: &str, value: Value) -> anyhow::Result<()> {
        self.with_mut(connection_id, |c| match c.data {
            Value::Object(ref mut obj) => {
                obj.insert(key.to_string(), value);
                Ok(())
            }
            _ => Err(anyhow!("Data is not a JSON object")),
        })
        .await
    }

    pub async fn remove_data_field(&self, connection_id: &str, key: &str) -> anyhow::Result<()> {
        self.with_mut(connection_id, |c| match c.data {
            Value::Object(ref mut obj) => {
                obj.remove(key);
                Ok(())
            }
            _ => Err(anyhow!("Data is not a JSON object")),
        })
        .await
    }

    pub async fn get_data(&self, connection_id: &str) -> anyhow::Result<Value> {
        Ok(self.get(connection_id).await.map_err(|_| anyhow!("Connection not found"))?.data)
    }

    pub async fn get_data_field(&self, connection_id: &str, key: &str) -> anyhow::Result<Value> {
        match self.get_data(connection_id).await? {
            Value::Object(obj) => obj.get(key).cloned().ok_or_else(|| anyhow!("Key not found")),
            _ => Err(anyhow!("Data is not a JSON object")),
        }
    }

    /// Is `value` in the string array at `data[key]`?
    pub async fn check_key_value_pair(&self, connection_id: &str, key: &str, value: &str) -> anyhow::Result<bool> {
        let Ok(Value::Object(obj)) = self.get_data(connection_id).await else { return Ok(false) };
        let Some(field) = obj.get(key) else { return Ok(false) };
        let list: Vec<String> = serde_json::from_value(field.clone())?;
        Ok(list.iter().any(|v| v == value))
    }

    /// Is the string at `data[key]` one of `values`?
    pub async fn check_if_any_kvp_matches(&self, connection_id: &str, key: &str, values: Vec<String>) -> anyhow::Result<bool> {
        let Ok(Value::Object(obj)) = self.get_data(connection_id).await else { return Ok(false) };
        Ok(obj.get(key).and_then(Value::as_str).is_some_and(|s| values.iter().any(|v| v == s)))
    }

    // ------------------------------------------------------------ subscriptions

    pub async fn add_subscription(&self, connection_id: &str, channel: &str) -> anyhow::Result<()> {
        self.add_subscriptions(connection_id, vec![channel.to_string()]).await
    }

    pub async fn remove_subscription(&self, connection_id: &str, channel: &str) -> anyhow::Result<()> {
        self.remove_subscriptions(connection_id, vec![channel.to_string()]).await
    }

    pub async fn add_subscriptions(&self, connection_id: &str, channels: Vec<String>) -> anyhow::Result<()> {
        self.with_mut(connection_id, |c| {
            for ch in channels {
                if !c.subscriptions.contains(&ch) {
                    c.subscriptions.push(ch);
                }
            }
            Ok(())
        })
        .await
    }

    pub async fn remove_subscriptions(&self, connection_id: &str, channels: Vec<String>) -> anyhow::Result<()> {
        self.with_mut(connection_id, |c| {
            c.subscriptions.retain(|s| !channels.contains(s));
            Ok(())
        })
        .await
    }

    // ------------------------------------------------------------ identity

    pub async fn update_user_id(&self, connection_id: &str, user_id: &str) -> anyhow::Result<ConnectionInfo> {
        let mut table = self.table.write().await;
        let c = table.get_mut(connection_id).ok_or_else(|| anyhow!("Connection not found"))?;
        c.user_id = user_id.to_string();
        c.updated_at = now();
        Ok(c.to_connection_info(connection_id, &self.identity))
    }

    pub async fn update_authenticated(&self, connection_id: &str, authenticated: bool) -> anyhow::Result<()> {
        self.with_mut(connection_id, |c| {
            c.authenticated = authenticated;
            Ok(())
        })
        .await
    }

    /// Mutate one connection under the write lock. Missing ids are a no-op
    /// (as they always were) except where the caller's closure says otherwise.
    async fn with_mut<F>(&self, connection_id: &str, f: F) -> anyhow::Result<()>
    where
        F: FnOnce(&mut CnctdConnection) -> anyhow::Result<()>,
    {
        let mut table = self.table.write().await;
        match table.get_mut(connection_id) {
            Some(c) => {
                c.updated_at = now();
                f(c)
            }
            None => Ok(()),
        }
    }

    // ------------------------------------------------------------ sending

    /// Push `msg` (as JSON) to one connection. Errors when the connection is
    /// unknown or has no open socket — unless another instance might hold
    /// it, in which case it is handed to the fan-out.
    pub async fn send<M: Serialize>(&self, connection_id: &str, msg: &M) -> anyhow::Result<()> {
        self.send_payload(connection_id, Payload::Text(to_json(msg)?)).await
    }

    /// Push a raw binary frame to one connection.
    pub async fn send_binary(&self, connection_id: &str, data: Vec<u8>) -> anyhow::Result<()> {
        self.send_payload(connection_id, Payload::Binary(data)).await
    }

    async fn send_payload(&self, connection_id: &str, payload: Payload) -> anyhow::Result<()> {
        let sender = {
            let table = self.table.read().await;
            match table.get(connection_id) {
                Some(c) => Ok(c.sender.clone()),
                None => Err(()),
            }
        };
        match sender {
            Ok(Some(s)) => {
                if s.send(outbound(&payload)).is_err() {
                    warn!(connection_id, "send failed: socket task gone");
                }
                Ok(())
            }
            Ok(None) => Err(anyhow!("Connection {} has no active sender", connection_id)),
            Err(()) if self.fanout.is_local_only() => Err(anyhow!("No matching connection")),
            Err(()) => self.publish(FanoutTarget::Connection(connection_id.to_string()), payload, None).await,
        }
    }

    pub async fn send_many<M: Serialize>(&self, connection_ids: &[String], msg: &M) -> anyhow::Result<()> {
        let payload = Payload::Text(to_json(msg)?);
        for id in connection_ids {
            if let Err(e) = self.send_payload(id, payload.clone()).await {
                warn!(connection_id = %id, "failed to message connection: {}", e);
            }
        }
        Ok(())
    }

    /// Every connection of `user_id`, here and on other instances.
    pub async fn message_user<M: Serialize>(&self, user_id: &str, msg: &M, exclude: Option<String>) -> anyhow::Result<()> {
        let payload = Payload::Text(to_json(msg)?);
        let target = FanoutTarget::User(user_id.to_string());
        self.deliver_local(&target, &payload, exclude.as_deref()).await;
        self.publish(target, payload, exclude).await
    }

    /// Every subscriber of `channel`, here and on other instances.
    pub async fn message_subscribers<M: Serialize>(&self, channel: &str, msg: &M, exclude: Option<String>) -> anyhow::Result<()> {
        let payload = Payload::Text(to_json(msg)?);
        let target = FanoutTarget::Channel(channel.to_string());
        self.deliver_local(&target, &payload, exclude.as_deref()).await;
        self.publish(target, payload, exclude).await
    }

    /// Every connection whose `data[key] == value`.
    pub async fn message_key_value_owners<M: Serialize>(
        &self,
        key: &str,
        value: &str,
        msg: &M,
        exclude: Option<String>,
    ) -> anyhow::Result<()> {
        let payload = Payload::Text(to_json(msg)?);
        let target = FanoutTarget::KeyValue {
            key: key.to_string(),
            value: value.to_string(),
        };
        self.deliver_local(&target, &payload, exclude.as_deref()).await;
        self.publish(target, payload, exclude).await
    }

    /// A [`Message`] to every subscriber of its channel.
    pub async fn broadcast(&self, msg: &Message) -> anyhow::Result<()> {
        self.message_subscribers(&msg.channel, msg, None).await
    }

    /// Deliver to the matching connections held by THIS instance. Returns how
    /// many were sent to. Remote fan-outs call this for envelopes they
    /// receive from other instances.
    pub async fn deliver_local(&self, target: &FanoutTarget, payload: &Payload, exclude: Option<&str>) -> usize {
        let senders: Vec<Sender> = {
            let table = self.table.read().await;
            table
                .iter()
                .filter(|(id, _)| exclude != Some(id.as_str()))
                .filter(|(id, c)| match target {
                    FanoutTarget::Channel(ch) => c.subscriptions.iter().any(|s| s == ch),
                    FanoutTarget::User(u) => &c.user_id == u,
                    FanoutTarget::Connection(cid) => *id == cid,
                    FanoutTarget::KeyValue { key, value } => {
                        c.data.get(key).and_then(Value::as_str) == Some(value.as_str())
                    }
                })
                .filter_map(|(_, c)| c.sender.clone())
                .collect()
        };
        let mut sent = 0;
        for s in senders {
            if s.send(outbound(payload)).is_ok() {
                sent += 1;
            }
        }
        sent
    }

    async fn publish(&self, target: FanoutTarget, payload: Payload, exclude: Option<String>) -> anyhow::Result<()> {
        if self.fanout.is_local_only() {
            return Ok(());
        }
        self.fanout
            .publish(Envelope {
                origin: self.identity.session_id.clone(),
                target,
                payload,
                exclude,
            })
            .await
    }
}

fn to_json<M: Serialize>(msg: &M) -> anyhow::Result<String> {
    serde_json::to_string(msg).map_err(|e| anyhow!("Serialization error: {}", e))
}

fn outbound(p: &Payload) -> Outbound {
    match p {
        Payload::Text(s) => Outbound::Text(s.clone()),
        Payload::Binary(b) => Outbound::Binary(b.clone()),
    }
}

// ---------------------------------------------------------------------------
// The associated-function API, unchanged from 0.6, over the running server's
// table. Each one is a one-liner into `Connections::global()`.
// ---------------------------------------------------------------------------

impl CnctdConnection {
    pub async fn register_connection(subscriptions: Vec<String>, ip_address: Option<String>) -> anyhow::Result<String> {
        Ok(Connections::global()?.register(subscriptions, ip_address, ConnectionFormat::Json).await)
    }

    pub async fn register_connection_with_format(
        subscriptions: Vec<String>,
        ip_address: Option<String>,
        format: ConnectionFormat,
    ) -> anyhow::Result<String> {
        Ok(Connections::global()?.register(subscriptions, ip_address, format).await)
    }

    pub async fn add_data(connection_id: &str, data: Value) -> anyhow::Result<()> {
        Connections::global()?.add_data(connection_id, data).await
    }

    pub async fn update_data_field(connection_id: &str, key: &str, value: Value) -> anyhow::Result<()> {
        Connections::global()?.update_data_field(connection_id, key, value).await
    }

    pub async fn remove_data_field(connection_id: &str, key: &str) -> anyhow::Result<()> {
        Connections::global()?.remove_data_field(connection_id, key).await
    }

    pub async fn get_data(connection_id: &str) -> anyhow::Result<Value> {
        Connections::global()?.get_data(connection_id).await
    }

    pub async fn get_data_field(connection_id: &str, key: &str) -> anyhow::Result<Value> {
        Connections::global()?.get_data_field(connection_id, key).await
    }

    pub async fn check_key_value_pair(connection_id: &str, key: &str, value: &str) -> anyhow::Result<bool> {
        Connections::global()?.check_key_value_pair(connection_id, key, value).await
    }

    pub async fn check_if_any_kvp_matches(connection_id: &str, key: &str, values: Vec<String>) -> anyhow::Result<bool> {
        Connections::global()?.check_if_any_kvp_matches(connection_id, key, values).await
    }

    pub async fn add_subscription(connection_id: &str, channel: &str) -> anyhow::Result<()> {
        Connections::global()?.add_subscription(connection_id, channel).await
    }

    pub async fn remove_subscription(connection_id: &str, channel: &str) -> anyhow::Result<()> {
        Connections::global()?.remove_subscription(connection_id, channel).await
    }

    pub async fn add_multiple_subscriptions(connection_id: &str, channels: Vec<String>) -> anyhow::Result<()> {
        Connections::global()?.add_subscriptions(connection_id, channels).await
    }

    pub async fn remove_multiple_subscriptions(connection_id: &str, channels: Vec<String>) -> anyhow::Result<()> {
        Connections::global()?.remove_subscriptions(connection_id, channels).await
    }

    pub async fn update_user_id(connection_id: &str, user_id: &str) -> anyhow::Result<ConnectionInfo> {
        Connections::global()?.update_user_id(connection_id, user_id).await
    }

    pub async fn update_authenticated(connection_id: &str, authenticated: bool) -> anyhow::Result<()> {
        Connections::global()?.update_authenticated(connection_id, authenticated).await
    }

    pub async fn get_connections() -> anyhow::Result<HashMap<String, Self>> {
        Ok(Connections::global()?.all().await)
    }

    /// `None` only when no server is running.
    pub async fn get_connection_ids(user_id: &str) -> Option<Vec<String>> {
        Some(Connections::global().ok()?.ids_for_user(user_id).await)
    }

    pub async fn get_connection_infos() -> anyhow::Result<Vec<ConnectionInfo>> {
        Ok(Connections::global()?.infos().await)
    }

    pub async fn get_connection_info(connection_id: &str) -> anyhow::Result<ConnectionInfo> {
        Connections::global()?.info(connection_id).await
    }

    pub async fn get_connection(connection_id: &str) -> anyhow::Result<Self> {
        Connections::global()?.get(connection_id).await
    }

    pub async fn get_subscriber_connection_ids(channel: &str) -> Vec<String> {
        match Connections::global() {
            Ok(c) => c.subscriber_ids(channel).await,
            Err(_) => Vec::new(),
        }
    }

    pub async fn get_subscriber_connections(channel: &str) -> Vec<(String, ConnectionFormat)> {
        match Connections::global() {
            Ok(c) => c.subscriber_formats(channel).await,
            Err(_) => Vec::new(),
        }
    }

    pub async fn message_connection<M>(connection_id: &str, msg: &M) -> anyhow::Result<()>
    where
        M: Serialize + Debug + DeserializeOwned + Clone,
    {
        Connections::global()?.send(connection_id, msg).await
    }

    pub async fn message_connection_binary(connection_id: &str, data: Vec<u8>) -> anyhow::Result<()> {
        Connections::global()?.send_binary(connection_id, data).await
    }

    pub async fn message_multiple_connections<M>(connection_ids: Vec<String>, msg: &M) -> anyhow::Result<()>
    where
        M: Serialize + Debug + DeserializeOwned + Clone,
    {
        Connections::global()?.send_many(&connection_ids, msg).await
    }

    pub async fn message_user<M>(user_id: &str, msg: &M, exclude_connection_id: Option<String>) -> anyhow::Result<()>
    where
        M: Serialize + Debug + DeserializeOwned + Clone,
    {
        Connections::global()?.message_user(user_id, msg, exclude_connection_id).await
    }

    pub async fn message_subscribers<M>(channel: &str, msg: &M, exclude_connection_id: Option<String>) -> anyhow::Result<()>
    where
        M: Serialize + Debug + DeserializeOwned + Clone,
    {
        Connections::global()?.message_subscribers(channel, msg, exclude_connection_id).await
    }

    pub async fn message_key_value_owners<M>(
        data_key: &str,
        data_value: &str,
        msg: &M,
        exclude_connection_id: Option<String>,
    ) -> anyhow::Result<()>
    where
        M: Serialize + Debug + DeserializeOwned + Clone,
    {
        Connections::global()?
            .message_key_value_owners(data_key, data_value, msg, exclude_connection_id)
            .await
    }
}
