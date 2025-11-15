pub mod client;
pub mod axum_socket;

// Re-export for backward compatibility
pub use axum_socket::CnctdSocket;
pub use axum_socket::CLIENTS;

use client::{ClientInfo};
use std::{sync::Arc, fmt::Debug};
use serde::{Serialize, de::DeserializeOwned};

#[derive(Clone)]
pub struct SocketConfig<R> {
    pub router: R,
    pub secret: Option<Vec<u8>>,
    pub redis_url: Option<String>,
    pub on_disconnect: Option<Arc<dyn Fn(ClientInfo) + Send + Sync>>,
}

impl<R> SocketConfig<R> {
    pub fn new(
        router: R,
        secret: Option<Vec<u8>>,
        redis_url: Option<String>,
        on_disconnect: Option<Arc<dyn Fn(ClientInfo) + Send + Sync>>,
    ) -> Self {
        Self {
            router,
            secret,
            redis_url,
            on_disconnect,
        }
    }
}
