//! What a server says about itself on the `server-info` heartbeat channel.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::socket::connection::ServerIdentity;

#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ServerInfo {
    pub id: String,
    pub session_id: String,
    pub local_ip: String,
    /// What the app said with `ServerConfig::with_public_ip`; empty otherwise.
    /// (0.6 looked this up over the network at every boot.)
    pub public_ip: String,
    pub port: u16,
    pub last_heartbeat: DateTime<Utc>,
    pub start_time: DateTime<Utc>,
    /// One-minute load average.
    pub cpu_usage: f32,
    /// Percent of physical memory in use.
    pub memory_usage: f32,
    pub status: ServerStatus,
    pub max_connections: usize,
    pub current_connections: usize,
    pub websocket: bool,
    pub redis_url: Option<String>,
    pub redis_active: bool,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum ServerStatus {
    Active,
    Idle,
    ShuttingDown,
}

impl ServerInfo {
    pub fn new(
        identity: &ServerIdentity,
        local_ip: &str,
        public_ip: Option<String>,
        port: u16,
        websocket: bool,
        redis_url: Option<String>,
    ) -> Self {
        Self {
            id: identity.id.clone(),
            session_id: identity.session_id.clone(),
            local_ip: local_ip.to_string(),
            public_ip: public_ip.unwrap_or_default(),
            port,
            last_heartbeat: Utc::now(),
            start_time: Utc::now(),
            cpu_usage: 0.0,
            memory_usage: 0.0,
            status: ServerStatus::Active,
            max_connections: 100,
            current_connections: 0,
            websocket,
            redis_url,
            redis_active: false,
        }
    }

    /// Refresh the heartbeat fields.
    pub fn refresh(&mut self, current_connections: usize) {
        self.last_heartbeat = Utc::now();
        self.current_connections = current_connections;
        if let Ok(load) = sys_info::loadavg() {
            self.cpu_usage = load.one as f32;
        }
        if let Ok(mem) = sys_info::mem_info() {
            if mem.total > 0 {
                let used = mem.total.saturating_sub(mem.avail) as f32;
                self.memory_usage = used / mem.total as f32 * 100.0;
            }
        }
    }
}
