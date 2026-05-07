//! Connection — the live WebSocket link from a running client to the server.
//!
//! A Connection is ephemeral: it lives from the moment the WebSocket
//! upgrades until disconnect. Each new connect produces a fresh
//! `connection_id`. Domain-level identity (which Client / app instance,
//! which user) is attached to a Connection via its `data` blob and
//! `user_id`.
//!
//! Was previously named `CnctdClient` / `client_id` — renamed in the
//! 0.6.0 vocabulary cleanup. The semantic split is now:
//!
//!   Hardware  →  Client (running app instance, persistent — schema)
//!                  └──opens──▶ Connection (ephemeral WS link, framework)

use std::{collections::HashMap, fmt::Debug, time::Duration};

use anyhow::anyhow;
use chrono::{DateTime, FixedOffset};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::mpsc;
use warp::reject::Rejection;

use crate::{
    server::server_info::ServerInfo,
    socket::{CnctdSocket, CONNECTIONS},
};

/// Wire format a connection expects for push messages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConnectionFormat {
    Json,
    Binary,
}

impl Default for ConnectionFormat {
    fn default() -> Self {
        Self::Json
    }
}

impl ConnectionFormat {
    pub fn from_str_opt(s: Option<&str>) -> Self {
        match s {
            Some("binary") => Self::Binary,
            _ => Self::Json,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ConnectionInfo {
    pub connection_id: String,
    pub user_id: String,
    pub ip_address: Option<String>,
    pub authenticated: bool,
    pub subscriptions: Vec<String>,
    pub format: ConnectionFormat,
    pub data: Value,
    pub connected: bool,
    pub server_id: String,
    pub server_session_id: String,
    pub created_at: DateTime<FixedOffset>,
    pub updated_at: DateTime<FixedOffset>,
}

#[derive(Debug, Deserialize)]
pub struct QueryParams {
    pub connection_id: Option<String>,
    /// Comma-separated subscription channels (for inline registration without REST).
    pub subscriptions: Option<String>,
    /// Wire format: "json" (default) or "binary".
    pub format: Option<String>,
}

type Sender = mpsc::UnboundedSender<std::result::Result<warp::ws::Message, warp::Error>>;
pub type Result<T> = std::result::Result<T, Rejection>;

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

impl CnctdConnection {
    pub fn new(subscriptions: Vec<String>, ip_address: Option<String>, format: ConnectionFormat) -> Self {
        Self {
            user_id: "".to_string(),
            ip_address,
            authenticated: false,
            subscriptions,
            format,
            sender: None,
            data: json!({}),
            created_at: chrono::offset::Utc::now()
                .with_timezone(&chrono::offset::FixedOffset::east_opt(0).unwrap()),
            updated_at: chrono::offset::Utc::now()
                .with_timezone(&chrono::offset::FixedOffset::east_opt(0).unwrap()),
        }
    }

    pub async fn register_connection(
        subscriptions: Vec<String>,
        ip_address: Option<String>,
    ) -> anyhow::Result<String> {
        Self::register_connection_with_format(subscriptions, ip_address, ConnectionFormat::Json).await
    }

    pub async fn register_connection_with_format(
        subscriptions: Vec<String>,
        ip_address: Option<String>,
        format: ConnectionFormat,
    ) -> anyhow::Result<String> {
        let connection_id = uuid::Uuid::new_v4().to_string();

        let connection = Self::new(subscriptions, ip_address, format);
        let connections_lock = match CONNECTIONS.try_get() {
            Some(connections) => connections,
            None => {
                return Err(anyhow!("Connections not initialized"));
            }
        };
        let mut connections = connections_lock.write().await;

        connections.insert(connection_id.clone(), connection);
        println!("connections length: {:?}", connections.len());

        let connection_id_clone = connection_id.clone();

        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(10)).await;

            match Self::get_connection(&connection_id_clone).await {
                Ok(connection) => {
                    if connection.sender.is_none() {
                        println!("Connection never opened. Removing");
                        CnctdSocket::remove_connection(&connection_id_clone).await
                    } else {
                        println!("Connection opened. No need to remove");
                        Ok(())
                    }
                }
                Err(_e) => Ok(()),
            }
        });

        Ok(connection_id)
    }

    pub async fn to_connection_info(&self, connection_id: &str) -> ConnectionInfo {
        let (server_id, server_session_id) = ServerInfo::get_server_and_session_id().await;
        ConnectionInfo {
            connection_id: connection_id.to_string(),
            user_id: self.user_id.clone(),
            ip_address: self.ip_address.clone(),
            authenticated: self.authenticated,
            subscriptions: self.subscriptions.clone(),
            format: self.format.clone(),
            data: self.data.clone(),
            connected: self.sender.is_some(),
            server_id,
            server_session_id,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }

    pub async fn add_data(connection_id: &str, data: Value) -> anyhow::Result<()> {
        let connections = CONNECTIONS
            .try_get()
            .ok_or_else(|| anyhow!("Connections not initialized"))?;
        let mut connections_lock = connections.write().await;

        if let Some(connection) = connections_lock.get_mut(connection_id) {
            connection.data = data;
        }

        Ok(())
    }

    pub async fn update_data_field(connection_id: &str, key: &str, value: Value) -> anyhow::Result<()> {
        let connections = CONNECTIONS
            .try_get()
            .ok_or_else(|| anyhow!("Connections not initialized"))?;
        let mut connections_lock = connections.write().await;

        if let Some(connection) = connections_lock.get_mut(connection_id) {
            if let Value::Object(ref mut obj) = connection.data {
                obj.insert(key.to_string(), value);
            } else {
                return Err(anyhow!("Data is not a JSON object"));
            }
        }
        Ok(())
    }

    pub async fn remove_data_field(connection_id: &str, key: &str) -> anyhow::Result<()> {
        let connections = CONNECTIONS
            .try_get()
            .ok_or_else(|| anyhow!("Connections not initialized"))?;
        let mut connections_lock = connections.write().await;

        if let Some(connection) = connections_lock.get_mut(connection_id) {
            if let Value::Object(ref mut obj) = connection.data {
                obj.remove(key);
            } else {
                return Err(anyhow!("Data is not a JSON object"));
            }
        }
        Ok(())
    }

    pub async fn get_data(connection_id: &str) -> anyhow::Result<Value> {
        let connections = CONNECTIONS
            .try_get()
            .ok_or_else(|| anyhow!("Connections not initialized"))?;
        let connections_lock = connections.read().await;

        if let Some(connection) = connections_lock.get(connection_id) {
            Ok(connection.data.clone())
        } else {
            Err(anyhow!("Connection not found"))
        }
    }

    pub async fn get_data_field(connection_id: &str, key: &str) -> anyhow::Result<Value> {
        let connections = CONNECTIONS
            .try_get()
            .ok_or_else(|| anyhow!("Connections not initialized"))?;
        let connections_lock = connections.read().await;

        if let Some(connection) = connections_lock.get(connection_id) {
            if let Value::Object(ref obj) = connection.data {
                if let Some(value) = obj.get(key) {
                    Ok(value.clone())
                } else {
                    Err(anyhow!("Key not found"))
                }
            } else {
                Err(anyhow!("Data is not a JSON object"))
            }
        } else {
            Err(anyhow!("Connection not found"))
        }
    }

    pub async fn check_key_value_pair(
        connection_id: &str,
        key: &str,
        value: &str,
    ) -> anyhow::Result<bool> {
        let connections = CONNECTIONS
            .try_get()
            .ok_or_else(|| anyhow!("Connections not initialized"))?;
        let connections_lock = connections.read().await;

        if let Some(connection) = connections_lock.get(connection_id) {
            if let Value::Object(ref obj) = connection.data {
                if let Some(data_value) = obj.get(key) {
                    let data: Vec<String> = serde_json::from_value(data_value.clone())?;
                    if data.contains(&value.to_string()) {
                        return Ok(true);
                    } else {
                        return Ok(false);
                    }
                } else {
                    return Ok(false);
                }
            } else {
                return Ok(false);
            }
        } else {
            return Ok(false);
        }
    }

    pub async fn check_if_any_kvp_matches(
        connection_id: &str,
        key: &str,
        values: Vec<String>,
    ) -> anyhow::Result<bool> {
        let connections = CONNECTIONS
            .try_get()
            .ok_or_else(|| anyhow!("Connections not initialized"))?;
        let connections_lock = connections.read().await;

        if let Some(connection) = connections_lock.get(connection_id) {
            if let Value::Object(ref obj) = connection.data {
                if let Some(data_value) = obj.get(key) {
                    if let Some(data_str) = data_value.as_str() {
                        if values.contains(&data_str.to_string()) {
                            return Ok(true);
                        }
                    }
                }
            }
        }

        Ok(false)
    }

    pub async fn add_subscription(connection_id: &str, channel: &str) -> anyhow::Result<()> {
        let connections = CONNECTIONS
            .try_get()
            .ok_or_else(|| anyhow!("Connections not initialized"))?;
        let mut connections_lock = connections.write().await;

        if let Some(connection) = connections_lock.get_mut(connection_id) {
            if !connection.subscriptions.contains(&channel.to_string()) {
                connection.subscriptions.push(channel.to_string());
            }
        }

        Ok(())
    }

    pub async fn remove_subscription(connection_id: &str, channel: &str) -> anyhow::Result<()> {
        let connections = CONNECTIONS
            .try_get()
            .ok_or_else(|| anyhow!("Connections not initialized"))?;
        let mut connections_lock = connections.write().await;

        if let Some(connection) = connections_lock.get_mut(connection_id) {
            if let Some(index) = connection.subscriptions.iter().position(|sub| sub == channel) {
                connection.subscriptions.remove(index);
            }
        }

        Ok(())
    }

    pub async fn add_multiple_subscriptions(
        connection_id: &str,
        channels: Vec<String>,
    ) -> anyhow::Result<()> {
        let connections = CONNECTIONS
            .try_get()
            .ok_or_else(|| anyhow!("Connections not initialized"))?;
        let mut connections_lock = connections.write().await;

        if let Some(connection) = connections_lock.get_mut(connection_id) {
            for channel in channels {
                if !connection.subscriptions.contains(&channel) {
                    connection.subscriptions.push(channel);
                }
            }
        }

        Ok(())
    }

    pub async fn remove_multiple_subscriptions(
        connection_id: &str,
        channels: Vec<String>,
    ) -> anyhow::Result<()> {
        let connections = CONNECTIONS
            .try_get()
            .ok_or_else(|| anyhow!("Connections not initialized"))?;
        let mut connections_lock = connections.write().await;

        if let Some(connection) = connections_lock.get_mut(connection_id) {
            for channel in channels {
                if let Some(index) = connection.subscriptions.iter().position(|sub| sub == &channel) {
                    connection.subscriptions.remove(index);
                }
            }
        }

        Ok(())
    }

    pub async fn update_user_id(
        connection_id: &str,
        user_id: &str,
    ) -> anyhow::Result<ConnectionInfo> {
        let connections = CONNECTIONS
            .try_get()
            .ok_or_else(|| anyhow!("Connections not initialized"))?;
        let mut connections_lock = connections.write().await;

        if let Some(connection) = connections_lock.get_mut(connection_id) {
            connection.user_id = user_id.to_string();
            Ok(connection.to_connection_info(connection_id).await)
        } else {
            Err(anyhow!("Connection not found"))
        }
    }

    pub async fn update_authenticated(
        connection_id: &str,
        authenticated: bool,
    ) -> anyhow::Result<()> {
        let connections = CONNECTIONS
            .try_get()
            .ok_or_else(|| anyhow!("Connections not initialized"))?;
        let mut connections_lock = connections.write().await;

        if let Some(connection) = connections_lock.get_mut(connection_id) {
            connection.authenticated = authenticated;
        }

        Ok(())
    }

    pub async fn get_connections() -> anyhow::Result<HashMap<String, Self>> {
        let connections = CONNECTIONS
            .try_get()
            .ok_or_else(|| anyhow!("Connections not initialized"))?
            .read()
            .await;
        Ok(connections.clone())
    }

    /// Returns connection_ids for all live connections from a given user.
    pub async fn get_connection_ids(user_id: &str) -> Option<Vec<String>> {
        let connections = CONNECTIONS.try_get()?.read().await;

        let connection_ids = connections
            .iter()
            .filter_map(|(connection_id, connection)| {
                if connection.user_id.as_str() == user_id {
                    Some(connection_id.clone())
                } else {
                    None
                }
            })
            .collect::<Vec<String>>();

        Some(connection_ids)
    }

    pub async fn get_connection_infos() -> anyhow::Result<Vec<ConnectionInfo>> {
        let connections = CONNECTIONS
            .try_get()
            .ok_or_else(|| anyhow!("Connections not initialized"))?
            .read()
            .await;
        let (server_id, server_session_id) = ServerInfo::get_server_and_session_id().await;
        let infos = connections
            .iter()
            .map(|(connection_id, connection)| ConnectionInfo {
                connection_id: connection_id.into(),
                user_id: connection.user_id.to_string(),
                ip_address: connection.ip_address.clone(),
                authenticated: connection.authenticated,
                subscriptions: connection.subscriptions.clone(),
                format: connection.format.clone(),
                data: connection.data.clone(),
                connected: connection.sender.is_some(),
                server_id: server_id.clone(),
                server_session_id: server_session_id.clone(),
                created_at: connection.created_at,
                updated_at: connection.updated_at,
            })
            .collect();

        Ok(infos)
    }

    pub async fn get_connection_info(connection_id: &str) -> anyhow::Result<ConnectionInfo> {
        let connection = Self::get_connection(connection_id).await?;
        Ok(connection.to_connection_info(connection_id).await)
    }

    pub async fn get_connection(connection_id: &str) -> anyhow::Result<Self> {
        let connections = CONNECTIONS
            .try_get()
            .ok_or_else(|| anyhow!("Connections not initialized"))?
            .read()
            .await;
        let connection = connections
            .get(connection_id)
            .ok_or_else(|| anyhow!("No matching connection"))?;

        Ok(connection.to_owned())
    }

    pub async fn get_subscriber_connection_ids(channel: &str) -> Vec<String> {
        let connections = CONNECTIONS
            .try_get()
            .expect("Connections not initialized")
            .read()
            .await;
        let connection_ids = connections
            .iter()
            .filter_map(|(connection_id, connection)| {
                if connection.subscriptions.contains(&channel.to_string()) {
                    Some(connection_id.clone())
                } else {
                    None
                }
            })
            .collect::<Vec<String>>();

        connection_ids
    }

    pub async fn message_connection<M>(connection_id: &str, msg: &M) -> anyhow::Result<()>
    where
        M: Serialize + Debug + DeserializeOwned + Clone,
    {
        let connection = Self::get_connection(connection_id).await?;
        if let Some(sender) = &connection.sender {
            let serialized_msg =
                serde_json::to_string(msg).map_err(|e| anyhow!("Serialization error: {}", e))?;

            if let Err(e) = sender.send(Ok(warp::ws::Message::text(serialized_msg))) {
                eprintln!("Send error: {}", e);
            }
        } else {
            return Err(anyhow!("Connection {} has no active sender", connection_id));
        }

        Ok(())
    }

    /// Send a raw binary WebSocket frame to a specific connection.
    pub async fn message_connection_binary(connection_id: &str, data: Vec<u8>) -> anyhow::Result<()> {
        let connection = Self::get_connection(connection_id).await?;
        if let Some(sender) = &connection.sender {
            if let Err(e) = sender.send(Ok(warp::ws::Message::binary(data))) {
                eprintln!("Binary send error: {}", e);
            }
        } else {
            return Err(anyhow!("Connection {} has no active sender", connection_id));
        }
        Ok(())
    }

    /// Returns (connection_id, format) pairs for all subscribers of a channel.
    /// Useful for dispatching format-aware messages (JSON vs binary).
    pub async fn get_subscriber_connections(channel: &str) -> Vec<(String, ConnectionFormat)> {
        let connections = CONNECTIONS
            .try_get()
            .expect("Connections not initialized")
            .read()
            .await;
        connections
            .iter()
            .filter_map(|(connection_id, connection)| {
                if connection.subscriptions.contains(&channel.to_string()) {
                    Some((connection_id.clone(), connection.format.clone()))
                } else {
                    None
                }
            })
            .collect()
    }

    pub async fn message_multiple_connections<M>(connection_ids: Vec<String>, msg: &M) -> anyhow::Result<()>
    where
        M: Serialize + Debug + DeserializeOwned + Clone,
    {
        for connection_id in connection_ids {
            if let Err(e) = Self::message_connection(&connection_id, msg).await {
                eprintln!("Failed to message connection {}: {}", connection_id, e);
            }
        }

        Ok(())
    }

    pub async fn message_user<M>(
        user_id: &str,
        msg: &M,
        exclude_connection_id: Option<String>,
    ) -> anyhow::Result<()>
    where
        M: Serialize + Debug + DeserializeOwned + Clone,
    {
        let connection_ids = Self::get_connection_ids(user_id)
            .await
            .ok_or_else(|| anyhow!("No connection found for user_id: {}", user_id))?;
        let all_connections = Self::get_connections().await?;
        for entry in all_connections.iter() {
            println!(
                "[message_user] Connection ID: {}, User ID: {}",
                entry.0, entry.1.user_id
            );
        }
        println!(
            "[message_user] Looking for connections with user_id: {}, found {} connections: {:?}",
            user_id,
            connection_ids.len(),
            connection_ids
        );
        for connection_id in connection_ids.iter() {
            if let Some(exclude_id) = &exclude_connection_id {
                if connection_id == exclude_id {
                    continue;
                }
            }
            if let Err(e) = Self::message_connection(connection_id, msg).await {
                eprintln!("Failed to message connection {}: {}", connection_id, e);
            }
        }

        Ok(())
    }

    pub async fn message_subscribers<M>(
        channel: &str,
        msg: &M,
        exclude_connection_id: Option<String>,
    ) -> anyhow::Result<()>
    where
        M: Serialize + Debug + DeserializeOwned + Clone,
    {
        let connection_ids = Self::get_subscriber_connection_ids(channel).await;

        for connection_id in connection_ids.iter() {
            if let Some(exclude_id) = &exclude_connection_id {
                if connection_id == exclude_id {
                    continue;
                }
            }
            if let Err(e) = Self::message_connection(connection_id, msg).await {
                eprintln!("Failed to message subscriber {}: {}", connection_id, e);
            }
        }

        Ok(())
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
        let connections = CONNECTIONS
            .try_get()
            .ok_or_else(|| anyhow!("Connections not initialized"))?
            .read()
            .await;
        let matching_ids: Vec<String> = connections
            .iter()
            .filter_map(|(connection_id, connection)| {
                if let Value::Object(obj) = &connection.data {
                    if let Some(value) = obj.get(data_key) {
                        if value.as_str() == Some(data_value) {
                            return Some(connection_id.clone());
                        }
                    }
                }
                None
            })
            .collect();
        drop(connections);

        for connection_id in matching_ids {
            if let Some(exclude_id) = &exclude_connection_id {
                if &connection_id == exclude_id {
                    continue;
                }
            }
            if let Err(e) = Self::message_connection(&connection_id, msg).await {
                eprintln!("Failed to message connection {}: {}", connection_id, e);
            }
        }

        Ok(())
    }
}
