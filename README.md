# cnctd_server

HTTP/WebSocket server framework for Rust, built on warp.

## Overview

cnctd_server provides routing abstractions, response helpers, and WebSocket client management for building web servers in Rust. It's the primary server framework used by Connected Dot projects.

## Installation

```toml
[dependencies]
cnctd_server = "0.4.10"
```

## Quick Start

```rust
use cnctd_server::{
    router::{HttpMethod, RestRouterFunction, error::ErrorResponse, response::SuccessResponse},
    success_data, bad_request,
};
use serde_json::{json, Value};
use std::pin::Pin;
use std::future::Future;

#[derive(Clone)]
struct MyRouter;

impl RestRouterFunction for MyRouter {
    fn route(
        &self,
        method: HttpMethod,
        path: String,
        data: Value,
        auth_token: Option<String>,
        client_id: Option<String>,
        ip_address: Option<String>,
    ) -> Pin<Box<dyn Future<Output = Result<SuccessResponse, ErrorResponse>> + Send>> {
        Box::pin(async move {
            match (method, path.as_str()) {
                (HttpMethod::GET, "/health") => Ok(success_data!(json!({"status": "ok"}))),
                _ => Err(bad_request!("Unknown route")),
            }
        })
    }

    fn route_redirect(
        &self,
        path: String,
        data: Value,
        auth_token: Option<String>,
        client_id: Option<String>,
    ) -> Pin<Box<dyn Future<Output = String> + Send>> {
        Box::pin(async move { "/".to_string() })
    }
}
```

## Features

- REST routing with `RestRouterFunction` trait
- WebSocket support with `SocketRouterFunction` trait
- Response macros: `success_data!`, `success_msg!`, `bad_request!`, `unauthorized!`, etc.
- Client management with optional Redis backing
- JWT authentication helpers

## Documentation

See [CLAUDE.md](./CLAUDE.md) for detailed API documentation and usage patterns.

## License

MIT - Connected Dot Inc.
