use std::sync::Arc;

use chrono::{DateTime, Utc};
use cnctd_redis::CnctdRedis;
use serde::{Deserialize, Serialize};
use serde_json::json;
use state::InitCell;
use tokio::sync::RwLock;

use crate::{router::message::Message, socket::{CnctdSocket, CLIENTS}};

pub static SERVER_INFO: InitCell<Arc<RwLock<ServerInfo>>> = InitCell::new();

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ServerInfo {
    pub id: String,
    pub session_id: String,
    pub local_ip: String,
    pub public_ip: String,
    pub port: u16,
    pub last_heartbeat: DateTime<Utc>,
    pub start_time: DateTime<Utc>,
    pub cpu_usage: f32,
    pub memory_usage: f32,
    pub status: ServerStatus,
    pub max_connections: usize,
    pub current_connections: usize,
    pub websocket: bool,
    pub redis_url: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq)]
pub enum ServerStatus {
    Active,
    Idle,
    ShuttingDown,
}

impl ServerInfo {
    pub async fn new(id: &str, local_ip: &str, port: u16, max_connections: usize, websocket: bool, redis_url: Option<String>) -> Arc<RwLock<Self>> {
        let session_id = uuid::Uuid::new_v4();
        let public_ip = match public_ip::addr().await {
            Some(ip) => ip.to_string(),
            None => "".to_string(),
        };
        let server_info = Self {
            id: id.into(),
            session_id: session_id.to_string(),
            local_ip: local_ip.into(),
            public_ip,
            port,
            last_heartbeat: Utc::now(),
            start_time: Utc::now(),
            cpu_usage: 0.0,
            memory_usage: 0.0,
            status: ServerStatus::Active,
            max_connections,
            current_connections: 0,
            websocket,
            redis_url
        };
        if server_info.redis_url.is_some() {
            match CnctdRedis::hset("server_info", &server_info.id, server_info.clone()) {
                Ok(_) => (),
                Err(e) => eprintln!("Error setting server info in Redis: {}", e),
            }   
        }
        Arc::new(RwLock::new(server_info))
    }

    pub async fn update() -> anyhow::Result<()> {
        let server_info = SERVER_INFO.get().clone();
        let mut server_info = server_info.write().await;
        
        server_info.last_heartbeat = Utc::now();

        sys_info::cpu_num().map(|cpu| {
            server_info.cpu_usage = cpu as f32;
        })?;
        sys_info::mem_info().map(|mem| {
            server_info.memory_usage = mem.total as f32;
        })?;

        if server_info.websocket {
            let clients = CLIENTS.get().read().await;
            server_info.current_connections = clients.len();
            let msg = Message::new("server-info", "heartbeat", Some(json!(server_info.clone())));
            CnctdSocket::broadcast_message(&msg).await?;
        }
        
        if server_info.redis_url.is_some() {
            CnctdRedis::hset("server_info", &server_info.id, server_info.clone())?;
        }

        Ok(())
    }

    pub fn get() -> Arc<RwLock<ServerInfo>> {
        SERVER_INFO.get().clone()
    }

    pub async fn get_server_id() -> String {
        SERVER_INFO.get().read().await.id.clone()
    }
}