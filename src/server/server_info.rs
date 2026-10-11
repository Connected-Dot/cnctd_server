//! What a server says about itself on the `server-info` heartbeat channel.
//! The struct is `cnctd_protocol::ServerInfo`; this is how the server fills it.

use chrono::Utc;

pub use cnctd_protocol::server_info::{ServerInfo, ServerStatus};

use crate::socket::connection::ServerIdentity;

pub(crate) fn new_server_info(
    identity: &ServerIdentity,
    local_ip: &str,
    public_ip: Option<String>,
    port: u16,
    websocket: bool,
    redis_url: Option<String>,
) -> ServerInfo {
    ServerInfo {
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

/// Refresh the heartbeat fields: one-minute load average, percent memory used.
pub(crate) fn refresh(info: &mut ServerInfo, current_connections: usize) {
    info.last_heartbeat = Utc::now();
    info.current_connections = current_connections;
    if let Ok(load) = sys_info::loadavg() {
        info.cpu_usage = load.one as f32;
    }
    if let Ok(mem) = sys_info::mem_info() {
        if mem.total > 0 {
            let used = mem.total.saturating_sub(mem.avail) as f32;
            info.memory_usage = used / mem.total as f32 * 100.0;
        }
    }
}
