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

/// The wire types, shared with every client. Also re-exported at the 0.6
/// paths below (`router::response`, `router::error`, ...).
pub use cnctd_protocol;
/// The envelope macros, usable as `cnctd_server::bad_request!` etc.
pub use cnctd_protocol::{
    bad_gateway, bad_request, conflict, created, error_response, forbidden, internal_server_error,
    method_not_allowed, not_found, request_timeout, service_unavailable, success_data, success_msg,
    success_response, too_many_requests, unauthorized,
};
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
#[cfg(feature = "redis")]
pub use socket::redis_fanout::RedisFanout;
