use std::sync::Arc;

use chrono::{DateTime, Utc};
use cnctd_redis::CnctdRedis;
use serde::{Deserialize, Serialize};
use state::InitCell;
use tokio::sync::RwLock;

use crate::socket::CnctdSocket;

pub static SERVER_INFO: InitCell<Arc<RwLock<ServerInfo>>> = InitCell::new();

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ServerInfo {
    pub id: String,
    pub ip_address: String,
    pub port: u16,
    pub last_heartbeat: DateTime<Utc>,
    pub start_time: DateTime<Utc>,
    pub cpu_usage: f32,
    pub memory_usage: f32,
    pub status: ServerStatus,
    pub max_connections: usize,
    pub current_connections: usize,
    pub websocket: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq)]
pub enum ServerStatus {
    Active,
    Idle,
    ShuttingDown,
}

impl ServerInfo {
    pub fn new(ip_address: &str, port: u16, max_connections: usize, websocket: bool) -> Self {
        let id = uuid::Uuid::new_v4();
        Self {
            id: id.into(),
            ip_address: ip_address.into(),
            port,
            last_heartbeat: Utc::now(),
            start_time: Utc::now(),
            cpu_usage: 0.0,
            memory_usage: 0.0,
            status: ServerStatus::Active,
            max_connections,
            current_connections: 0,
            websocket
        }
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
            let clients = CnctdSocket::get_clients().await;
            server_info.current_connections = clients.len();
        }
        
        CnctdRedis::set("server-info", server_info.clone())?;

        Ok(())
    }

    pub fn get() -> Arc<RwLock<ServerInfo>> {
        SERVER_INFO.get().clone()
    }
}