use std::{collections::HashMap, time::Duration, fmt::Debug};

use chrono::{DateTime, FixedOffset};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::mpsc;
use anyhow::anyhow;
use warp::reject::Rejection;

use crate::{server::server_info::ServerInfo, socket::{CnctdSocket, CLIENTS}};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ClientInfo {
    pub client_id: String,
    pub user_id: String,
    pub ip_address: Option<String>,
    pub authenticated: bool,
    pub subscriptions: Vec<String>,
    pub data: Value,
    pub connected: bool,
    pub server_id: String,
    pub server_session_id: String,
    pub created_at: DateTime<FixedOffset>,
    pub updated_at: DateTime<FixedOffset>,
}

#[derive(Debug, Deserialize)]
pub struct QueryParams {
    pub client_id: Option<String>,
}

type Sender = mpsc::UnboundedSender<std::result::Result<warp::ws::Message, warp::Error>>;
pub type Result<T> = std::result::Result<T, Rejection>;

#[derive(Debug, Clone)]
pub struct CnctdClient {
    pub user_id: String,
    pub ip_address: Option<String>,
    pub authenticated: bool,
    pub subscriptions: Vec<String>,
    pub sender: Option<Sender>,
    pub data: Value,
    pub created_at: DateTime<FixedOffset>,
    pub updated_at: DateTime<FixedOffset>,
}

impl CnctdClient {
    pub fn new(subscriptions: Vec<String>, ip_address: Option<String>) -> Self {
        Self {
            user_id: "".to_string(),
            ip_address,
            authenticated: false,
            subscriptions,
            sender: None,
            data: json!({}),
            created_at: chrono::offset::Utc::now().with_timezone(&chrono::offset::FixedOffset::east_opt(0).unwrap()),
            updated_at: chrono::offset::Utc::now().with_timezone(&chrono::offset::FixedOffset::east_opt(0).unwrap()),
        }
    }

    pub async fn register_client(subscriptions: Vec<String>, ip_address: Option<String>) -> anyhow::Result<String> {
        let client_id = uuid::Uuid::new_v4().to_string();
        
        let client = Self::new(subscriptions, ip_address);
        let clients_lock = match CLIENTS.try_get() {
            Some(clients) => clients,
            None => {
                return Err(anyhow!("Clients not initialized"));
            }
        };
        let mut clients = clients_lock.write().await; // Lock the CLIENTS for write access
        
        clients.insert(client_id.clone(), client);
        println!("clients length: {:?}", clients.len());

        // let response = SuccessResponse::new(Some(SuccessCode::Created), Some("Client registered".into()), Some(client_id.clone().into()));
        let client_id_clone = client_id.clone();
        
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(10)).await;
            
            match Self::get_client(&client_id_clone).await {
                Ok(client) => {
                    if client.sender.is_none() { 
                        println!("Client never connected. Removing");
                        CnctdSocket::remove_client(&client_id_clone).await 
                    } else {
                        println!("Client connected. No need to remove");
                        Ok(())
                    }
                }
                Err(_e) => Ok(())
            }
        });

        Ok(client_id)
    }


    pub async fn to_client_info(&self, client_id: &str) -> ClientInfo {
        let (server_id, server_session_id) = ServerInfo::get_server_and_session_id().await;
        ClientInfo {
            client_id: client_id.to_string(),
            user_id: self.user_id.clone(),
            ip_address: self.ip_address.clone(),
            authenticated: self.authenticated,
            subscriptions: self.subscriptions.clone(),
            data: self.data.clone(),
            connected: self.sender.is_some(),
            server_id,
            server_session_id,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }

    
    pub async fn add_data(client_id: &str, data: Value) -> anyhow::Result<()> {
        let clients = CLIENTS.try_get().ok_or_else(|| anyhow!("Clients not initialized"))?;
        let mut clients_lock = clients.write().await;
    
        if let Some(client) = clients_lock.get_mut(client_id) {
            client.data = data;
        }
    
        Ok(())
    }

    pub async fn update_data_field(client_id: &str, key: &str, value: Value) -> anyhow::Result<()> {
        let clients = CLIENTS.try_get().ok_or_else(|| anyhow!("Clients not initialized"))?;
        let mut clients_lock = clients.write().await;
    
        if let Some(client) = clients_lock.get_mut(client_id) {
            if let Value::Object(ref mut obj) = client.data {
                obj.insert(key.to_string(), value);
            } else {
                // Handle case where data is not an object
                return Err(anyhow!("Data is not a JSON object"));
            }
        }
        Ok(())
    }

    pub async fn remove_data_field(client_id: &str, key: &str) -> anyhow::Result<()> {
        let clients = CLIENTS.try_get().ok_or_else(|| anyhow!("Clients not initialized"))?;
        let mut clients_lock = clients.write().await;
    
        if let Some(client) = clients_lock.get_mut(client_id) {
            if let Value::Object(ref mut obj) = client.data {
                obj.remove(key);
            } else {
                // Handle case where data is not an object
                return Err(anyhow!("Data is not a JSON object"));
            }
        }
        Ok(())
    }

    pub async fn get_data(client_id: &str) -> anyhow::Result<Value> {
        let clients = CLIENTS.try_get().ok_or_else(|| anyhow!("Clients not initialized"))?;
        let clients_lock = clients.read().await;
    
        if let Some(client) = clients_lock.get(client_id) {
            Ok(client.data.clone())
        } else {
            Err(anyhow!("Client not found"))
        }
    }

    pub async fn get_data_field(client_id: &str, key: &str) -> anyhow::Result<Value> {
        let clients = CLIENTS.try_get().ok_or_else(|| anyhow!("Clients not initialized"))?;
        let clients_lock = clients.read().await;
    
        if let Some(client) = clients_lock.get(client_id) {
            if let Value::Object(ref obj) = client.data {
                if let Some(value) = obj.get(key) {
                    Ok(value.clone())
                } else {
                    Err(anyhow!("Key not found"))
                }
            } else {
                Err(anyhow!("Data is not a JSON object"))
            }
        } else {
            Err(anyhow!("Client not found"))
        }
    }

    pub async fn check_key_value_pair(client_id: &str, key: &str, value: &str) -> anyhow::Result<bool> {
        let clients = CLIENTS.try_get().ok_or_else(|| anyhow!("Clients not initialized"))?;
        let clients_lock = clients.read().await;
    
        if let Some(client) = clients_lock.get(client_id) {
            if let Value::Object(ref obj) = client.data {
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

    pub async fn check_if_any_kvp_matches(client_id: &str, key: &str, values: Vec<String>) -> anyhow::Result<bool> {
        let clients = CLIENTS.try_get().ok_or_else(|| anyhow!("Clients not initialized"))?;
        let clients_lock = clients.read().await;
    
        if let Some(client) = clients_lock.get(client_id) {
            if let Value::Object(ref obj) = client.data {
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

    pub async fn add_subscription(client_id: &str, channel: &str) -> anyhow::Result<()> {
        let clients = CLIENTS.try_get().ok_or_else(|| anyhow!("Clients not initialized"))?;
        let mut clients_lock = clients.write().await;
    
        if let Some(client) = clients_lock.get_mut(client_id) {
            if !client.subscriptions.contains(&channel.to_string()) {
                client.subscriptions.push(channel.to_string());
            }
        }
    
        Ok(())
    }

    pub async fn remove_subscription(client_id: &str, channel: &str) -> anyhow::Result<()> {
        let clients = CLIENTS.try_get().ok_or_else(|| anyhow!("Clients not initialized"))?;
        let mut clients_lock = clients.write().await;
    
        if let Some(client) = clients_lock.get_mut(client_id) {
            if let Some(index) = client.subscriptions.iter().position(|sub| sub == channel) {
                client.subscriptions.remove(index);
            }
        }
    
        Ok(())
    }

    pub async fn add_multiple_subscriptions(client_id: &str, channels: Vec<String>) -> anyhow::Result<()> {
        let clients = CLIENTS.try_get().ok_or_else(|| anyhow!("Clients not initialized"))?;
        let mut clients_lock = clients.write().await;
    
        if let Some(client) = clients_lock.get_mut(client_id) {
            for channel in channels {
                if !client.subscriptions.contains(&channel) {
                    client.subscriptions.push(channel);
                }
            }
        }
    
        Ok(())
    }

    pub async fn remove_multiple_subscriptions(client_id: &str, channels: Vec<String>) -> anyhow::Result<()> {
        let clients = CLIENTS.try_get().ok_or_else(|| anyhow!("Clients not initialized"))?;
        let mut clients_lock = clients.write().await;
    
        if let Some(client) = clients_lock.get_mut(client_id) {
            for channel in channels {
                if let Some(index) = client.subscriptions.iter().position(|sub| sub == &channel) {
                    client.subscriptions.remove(index);
                }
            }
        }
    
        Ok(())
    }

    pub async fn update_client_user_id(client_id: &str, user_id: &str) -> anyhow::Result<()> {
        let clients = CLIENTS.try_get().ok_or_else(|| anyhow!("Clients not initialized"))?;
        let mut clients_lock = clients.write().await;
    
        if let Some(client) = clients_lock.get_mut(client_id) {
            client.user_id = user_id.to_string();
        }
    
        Ok(())
    }

    pub async fn update_client_authenticated(client_id: &str, authenticated: bool) -> anyhow::Result<()> {
        let clients = CLIENTS.try_get().ok_or_else(|| anyhow!("Clients not initialized"))?;
        let mut clients_lock = clients.write().await;
    
        if let Some(client) = clients_lock.get_mut(client_id) {
            client.authenticated = authenticated;
        }
    
        Ok(())
    }

    pub async fn get_clients() -> anyhow::Result<HashMap<String, Self>> {
        let clients = CLIENTS.try_get().ok_or_else(|| anyhow!("Clients not initialized"))?.read().await;
        Ok(clients.clone())
    }

    pub async fn get_client_ids(user_id: &str) -> Option<Vec<String>> {
        // Attempt to get the read lock on the clients
        let clients = CLIENTS.try_get()?.read().await;
        
        let client_ids = clients.iter().filter_map(|(client_id, client)| {
            if client.user_id.as_str() == user_id {
                Some(client_id.clone())
            } else {
                None
            }
        }).collect::<Vec<String>>();

        Some(client_ids)
    }

    pub async fn get_client_infos() -> anyhow::Result<Vec<ClientInfo>> {
        let clients = CLIENTS.try_get().ok_or_else(|| anyhow!("Clients not initialized"))?.read().await;
        let (server_id, server_session_id) = ServerInfo::get_server_and_session_id().await;
        let clients = clients.iter().map(|(client_id, client)| ClientInfo {
            client_id: client_id.into(), 
            user_id: client.user_id.to_string(),
            ip_address: client.ip_address.clone(),
            authenticated: client.authenticated,
            subscriptions: client.subscriptions.clone(),
            data: client.data.clone(),
            connected: client.sender.is_some(),
            server_id: server_id.clone(),
            server_session_id: server_session_id.clone(),
            created_at: client.created_at,
            updated_at: client.updated_at,
        }).collect();

        Ok(clients)
    }

    pub async fn get_client_info(client_id: &str) -> anyhow::Result<ClientInfo> {
        let client = Self::get_client(client_id).await?;
        Ok(client.to_client_info(client_id).await)
    }

    pub async fn get_client(client_id: &str) -> anyhow::Result<Self> {
        let clients = CLIENTS.try_get().ok_or_else(|| anyhow!("Clients not initialized"))?.read().await;
        let client = clients.get(client_id).ok_or_else(|| anyhow!("No matching client"))?;

        Ok(client.to_owned())
    }

    pub async fn get_subscriber_client_ids(channel: &str) -> Vec<String> {
        let clients = CLIENTS.try_get().expect("Clients not initialized").read().await;
        let client_ids = clients.iter().filter_map(|(client_id, client)| {
            if client.subscriptions.contains(&channel.to_string()) {
                Some(client_id.clone())
            } else {
                None
            }
        }).collect::<Vec<String>>();

        client_ids
    }

    pub async fn message_client<M>(client_id: &str, msg: &M) -> anyhow::Result<()>
    where M: Serialize + Debug + DeserializeOwned + Clone {
        let client = Self::get_client(client_id).await?;
        
        // Serialize the message only if a sender exists
        if let Some(sender) = &client.sender {
            let serialized_msg = serde_json::to_string(msg).map_err(|e| anyhow!("Serialization error: {}", e))?;
            
            // Attempt to send the serialized message
            if let Err(e) = sender.send(Ok(warp::ws::Message::text(serialized_msg))) {
                eprintln!("Send error: {}", e);
            }
        } else {
            return Err(anyhow!("Client with id {} has no active sender", client_id));
        }
        
        Ok(())
    }

    pub async fn message_multiple_clients<M>(client_ids: Vec<String>, msg: &M) -> anyhow::Result<()>
    where M: Serialize + Debug + DeserializeOwned + Clone {
        for client_id in client_ids {
            let _ = Self::message_client(&client_id, msg);
        }

        Ok(())
    }

    pub async fn message_user<M>(user_id: &str, msg: &M, exclude_client_id: Option<String>) -> anyhow::Result<()>
    where M: Serialize + Debug + DeserializeOwned + Clone {
        let client_ids = Self::get_client_ids(user_id).await.ok_or_else(|| anyhow!("No client found for user_id: {}", user_id))?;
        
        client_ids.iter().for_each(|client_id| {
            if let Some(exclude_id) = &exclude_client_id {
                if client_id == exclude_id {
                    return;
                }
            }
            let _ = Self::message_client(client_id, msg);
        });

        Ok(())
    }
    
    pub async fn message_subscribers<M>(channel: &str, msg: &M, exclude_client_id: Option<String>) -> anyhow::Result<()>
    where M: Serialize + Debug + DeserializeOwned + Clone {
        let client_ids = Self::get_subscriber_client_ids(channel).await;
        
        client_ids.iter().for_each(|client_id| {
            if let Some(exclude_id) = &exclude_client_id {
                if client_id == exclude_id {
                    return;
                }
            }
            let _ = Self::message_client(client_id, msg);
        });

        Ok(())
    }

    pub async fn message_key_value_owners<M>(data_key: &str, data_value: &str, msg: &M, exclude_client_id: Option<String>) -> anyhow::Result<()>
    where M: Serialize + Debug + DeserializeOwned + Clone {
        let clients = CLIENTS.try_get().ok_or_else(|| anyhow!("Clients not initialized"))?.read().await;
        for (client_id, client) in clients.iter() {
            if let Value::Object(obj) = &client.data {
                if let Some(value) = obj.get(data_key) {
                    if value.as_str() == Some(data_value) {
                        println!("Found matching key-value pair: {} - {}", data_key, data_value);
                        if let Some(exclude_id) = &exclude_client_id {
                            if client_id == exclude_id {
                                continue;
                            }
                        }
                        let _ = Self::message_client(client_id, msg);
                    }
                }
            }
        }

        Ok(())
    }

   
}