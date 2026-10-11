//! cnctd_server — the HTTP + WebSocket framework behind cnctd.world and the
//! other Connected Dot services.
//!
//! The app implements [`RestRouter`] (one `route(Request) -> Response` call
//! per HTTP request) and optionally a [`SocketRouter`] (one call per inbound
//! WebSocket message), hands both to [`CnctdServer`], and gets back: JSON
//! envelopes with the house macros, raw/binary/streaming/SSE responses,
//! redirects, a single-page-app fallback with per-path `<head>` tags, CORS,
//! a connection table with pub/sub push, and a heartbeat.
//!
//! Nothing in the public API names the HTTP library underneath.

pub mod auth;
pub mod router;
pub mod server;
pub mod socket;
pub mod utils;

/// Routers are written with `#[cnctd_server::async_trait]`.
pub use async_trait::async_trait;
/// Raw request bodies are handed over as `Bytes`.
pub use bytes::Bytes;

pub use router::{request::Request, response::Response, HttpMethod, RestRouter, SocketRouter};
pub use server::{CnctdServer, RunningServer, ServerConfig};
pub use socket::{
    connection::{CnctdConnection, ConnectionFormat, ConnectionInfo, Connections},
    fanout::{Fanout, LocalOnly},
    SocketConfig,
};
