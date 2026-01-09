# CLAUDE.md - cnctd_server

> This file provides context about the cnctd_server library for Claude to reference in conversations.

## Project Summary

**cnctd_server** is a Rust HTTP/WebSocket server framework built on top of [warp](https://github.com/seanmonstar/warp). It provides routing abstractions, response/error helpers, WebSocket client management, and Redis integration. This is the primary server framework used by cnctd.world and other Connected Dot projects.

## Key Features

- **REST Routing**: Trait-based HTTP routing with GET/POST/PUT/DELETE support
- **WebSocket Support**: Full WebSocket handling with client management and pub/sub
- **Response Helpers**: Macros for success and error responses (`success_data!`, `bad_request!`, etc.)
- **Redis Integration**: Optional Redis-backed client state and pub/sub
- **JWT Authentication**: Built-in JWT token handling via the `auth` module
- **Server Info**: System information and server status utilities

## Architecture

### Module Structure

```
src/
├── lib.rs           # Public exports
├── router/
│   ├── mod.rs       # HttpMethod enum, router traits
│   ├── response.rs  # SuccessResponse, success_* macros
│   ├── error.rs     # ErrorResponse, error_* macros
│   ├── message.rs   # Message types for pub/sub
│   └── request.rs   # Request handling utilities
├── socket/
│   ├── mod.rs       # CnctdSocket, WebSocket server
│   └── client.rs    # CnctdClient, client management
├── server/
│   ├── mod.rs       # Server module exports
│   ├── server_info.rs  # ServerInfo struct
│   └── handlers.rs  # Warp handler builders
├── auth/
│   └── mod.rs       # JWT authentication helpers
└── utils/
    └── mod.rs       # General utilities
```

### Core Types

```rust
// HTTP Methods
pub enum HttpMethod {
    GET, POST, PUT, DELETE
}

// Router trait for REST endpoints
pub trait RestRouterFunction: Send + Sync + Clone {
    fn route(
        &self,
        method: HttpMethod,
        path: String,
        data: Value,
        auth_token: Option<String>,
        client_id: Option<String>,
        ip_address: Option<String>,
    ) -> Pin<Box<dyn Future<Output = Result<SuccessResponse, ErrorResponse>> + Send>>;
}

// Router trait for WebSocket messages
pub trait SocketRouterFunction<Req, Resp>: Send + Sync + Clone {
    fn route(&self, msg: Req, client_id: String)
        -> Pin<Box<dyn Future<Output = Option<Resp>> + Send>>;
}

// Response types
pub struct SuccessResponse {
    pub success: bool,
    pub status: SuccessCode,  // OK, Created, Accepted, NoContent
    pub msg: Option<String>,
    pub data: Option<Value>,
}

pub struct ErrorResponse {
    pub success: bool,
    pub status: ErrorCode,  // BadRequest, Unauthorized, NotFound, etc.
    pub msg: Option<String>,
}
```

### Response Macros

```rust
// Success responses
success_data!(json_value)           // 200 OK with data
success_msg!("message")             // 200 OK with message
created!("message", json_value)     // 201 Created

// Error responses
bad_request!("message")             // 400
unauthorized!("message")            // 401
forbidden!("message")               // 403
not_found!("message")               // 404
internal_server_error!("message")   // 500
service_unavailable!("message")     // 503
```

### WebSocket Server

```rust
use cnctd_server::socket::{CnctdSocket, SocketConfig, CnctdClient};

// Start WebSocket server
CnctdSocket::start::<Request, Response, Router>(
    "8080",
    router,
    Some(jwt_secret),
    Some(redis_url),
    Some(Arc::new(|client_info| { /* on disconnect */ })),
).await?;

// Or build routes to combine with HTTP
let ws_routes = CnctdSocket::build_routes::<Request, Response, Router>(config);

// Client management
CnctdClient::add_client(client_id, user_id, auth_token).await?;
CnctdClient::message_client(client_id, &message).await?;
CnctdSocket::broadcast_message(&message).await?;
```

## Usage by cnctd.world

cnctd.world imports cnctd_server throughout its codebase:

```rust
use cnctd_server::{
    router::{HttpMethod, error::ErrorResponse, response::SuccessResponse},
    socket::{CnctdSocket, client::CnctdClient},
    bad_request, not_found, unauthorized, internal_server_error,
    success_data, success_msg,
};
```

**Integration Points:**
- `router/resources/*.rs` - All REST endpoints use response macros
- `router/socket.rs` - WebSocket handling via CnctdSocket
- `router/mod.rs` - Implements RestRouterFunction trait

## Dependencies

- `warp` - HTTP server framework
- `tokio` - Async runtime
- `serde` / `serde_json` - Serialization
- `cnctd_redis` - Redis client wrapper
- `jwt` / `hmac` / `sha2` - JWT authentication

## Development

```bash
# Build
cargo build

# Test
cargo test

# Publish to crates.io
cargo publish
```

## Version

Current version: **0.4.10**

---

*Part of the cnctd monorepo. See `../../../CLAUDE.md` for ecosystem context.*
