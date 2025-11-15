# Phase 2: WebSocket Migration - Complete Guide

## Context

Phase 1 (REST API migration) is **COMPLETE** on branch `feat/axum-migration`.

This document contains everything needed to complete Phase 2: WebSocket migration from Warp to Axum.

## What Phase 1 Completed

✅ REST API fully migrated to Axum
✅ Custom extractors (ClientIp, FullPath)
✅ Status code conversions (SuccessCode, ErrorCode)
✅ Dual handler implementation (Warp + Axum)
✅ CORS, static files, redirects
✅ Server info & heartbeat
✅ 100% backward compatible

**Key Files Created:**
- `src/server/extractors.rs`
- `src/server/axum_server.rs`
- `src/server/handlers.rs` (updated)
- `src/router/response.rs` (updated)
- `src/router/error.rs` (updated)

## What Phase 2 Must Do

### Primary Goal

Migrate WebSocket functionality from Warp to Axum while maintaining 100% compatibility with the `SocketRouterFunction` trait.

### Files to Examine First

```
src/socket/
├── mod.rs              # Main WebSocket implementation (Warp)
├── client.rs           # Client management
└── [other files]       # Any additional socket-related files
```

### Integration Point

In `src/server/axum_server.rs`, line ~175, there's this stub:

```rust
// Add WebSocket routes if configured
if let Some(config) = socket_config {
    let ws_router = CnctdSocket::build_axum_routes(config);
    app = app.merge(ws_router);
}
```

**This method does not exist yet.** Phase 2 must implement `CnctdSocket::build_axum_routes()`.

## Implementation Steps

### Step 1: Examine Current Implementation

Read these files to understand the current Warp implementation:

1. **`src/socket/mod.rs`** - How `build_routes()` works with Warp
2. **`src/socket/client.rs`** - How `CnctdClient` is structured
3. **`SocketConfig`** - What configuration is needed
4. **`SocketRouterFunction` trait** - The routing contract

Key things to identify:
- How WebSocket connections are established
- How messages are routed
- How clients are stored/managed
- Redis integration (if any)
- Disconnect handling

### Step 2: Create Axum WebSocket Implementation

Create `src/socket/axum_socket.rs` with:

#### Core Structure

```rust
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Query, State,
    },
    response::IntoResponse,
    routing::get,
    Router,
};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;

pub struct CnctdSocket;

impl CnctdSocket {
    pub fn build_axum_routes<M, Resp, R>(config: SocketConfig<R>) -> Router
    where
        M: Serialize + DeserializeOwned + Send + Sync + Debug + Clone + 'static,
        Resp: Serialize + DeserializeOwned + Send + Sync + Debug + Clone + 'static,
        R: SocketRouterFunction<M, Resp> + 'static,
    {
        // TODO: Implement this
    }
}
```

#### Key Components Needed

1. **WebSocket Handler Function**
   - Accept WebSocketUpgrade
   - Extract query parameters (client_id, etc.)
   - Upgrade connection
   - Handle messages

2. **Client State Management**
   - Maintain CLIENTS map (same as Warp version)
   - Store sender channels per client
   - Handle subscriptions

3. **Message Routing**
   - Parse incoming messages
   - Route through `SocketRouterFunction`
   - Send responses back to client

4. **Redis Integration** (if enabled)
   - Push client info to Redis
   - Remove on disconnect
   - Same as Warp version

5. **Disconnect Handling**
   - Call optional `on_disconnect` callback
   - Clean up client state
   - Remove from Redis

### Step 3: Update Client Management

In `src/socket/client.rs`, handle Axum message types:

**Current Issue:** The client sender likely uses Warp's message type.

**Solution:** Make it generic or use Axum's message type:

```rust
use axum::extract::ws::Message as AxumMessage;

pub struct CnctdClient {
    pub user_id: Option<String>,
    pub subscriptions: Vec<String>,
    pub sender: Option<mpsc::UnboundedSender<Result<AxumMessage, axum::Error>>>,
}
```

Or create a wrapper type if you need framework independence.

### Step 4: Update Module Exports

In `src/socket/mod.rs`:

```rust
pub mod client;
pub mod axum_socket;

// Re-export for backward compatibility
pub use axum_socket::CnctdSocket;
```

Comment out or remove the old Warp implementation after verifying Axum version works.

### Step 5: Test Integration

The WebSocket routes should integrate seamlessly in `axum_server.rs` since the integration point is already there.

## Key Differences: Warp vs Axum WebSockets

### Connection Upgrade

**Warp:**
```rust
warp::ws()
    .map(|ws: warp::ws::Ws| {
        ws.on_upgrade(|websocket| handle_connection(websocket))
    })
```

**Axum:**
```rust
get(|ws: WebSocketUpgrade, Query(params): Query<QueryParams>| {
    ws.on_upgrade(|socket| handle_connection(socket, params))
})
```

### Message Types

**Warp:** `warp::ws::Message`
**Axum:** `axum::extract::ws::Message`

Both have Text, Binary, Close, etc. The API is very similar.

### Sending Messages

**Warp:**
```rust
let (mut tx, mut rx) = websocket.split();
tx.send(Message::text("hello")).await?;
```

**Axum:**
```rust
let (mut tx, mut rx) = websocket.split();
tx.send(Message::Text("hello".to_string())).await?;
```

Note: Axum uses `Text(String)` vs Warp's `text(str)`.

### State Management

**Axum uses the same State pattern as REST routes:**

```rust
#[derive(Clone)]
struct WsState<R> {
    router: R,
    redis: bool,
    on_disconnect: Option<Arc<dyn Fn(ClientInfo) + Send + Sync>>,
}

Router::new()
    .route("/ws", get(ws_handler::<M, Resp, R>))
    .with_state(state)
```

## Critical Implementation Details

### 1. Client Channel Type

The `UnboundedSender` must match the message type:

```rust
mpsc::UnboundedSender<Result<AxumMessage, axum::Error>>
```

### 2. Message Serialization

Incoming text messages must be deserialized from JSON:

```rust
if let Message::Text(text) = msg {
    if let Ok(message) = serde_json::from_str::<M>(&text) {
        // Route message
    }
}
```

### 3. Response Serialization

Outgoing messages must be serialized to JSON:

```rust
if let Ok(response_str) = serde_json::to_string(&response) {
    let _ = resp_tx.send(Ok(Message::Text(response_str)));
}
```

### 4. Client ID Extraction

Must come from query parameters:

```rust
#[derive(Deserialize)]
struct QueryParams {
    client_id: Option<String>,
    // other params
}
```

### 5. Concurrent Message Handling

Use `tokio::select!` to handle both incoming and outgoing messages:

```rust
tokio::select! {
    _ = process_incoming => {},
    _ = send_responses => {},
}
```

## Testing Checklist

After implementation, verify:

- [ ] WebSocket connections establish
- [ ] Client ID is extracted from query params
- [ ] Messages route through `SocketRouterFunction`
- [ ] Responses are sent back to clients
- [ ] Multiple clients can connect simultaneously
- [ ] Subscriptions work correctly
- [ ] `broadcast_message()` works
- [ ] Redis integration works (if enabled)
- [ ] Disconnect callbacks fire
- [ ] Clients are cleaned up on disconnect
- [ ] Server info updates with socket status

## Common Pitfalls

### Pitfall 1: Message Type Mismatch
**Issue:** Using wrong message enum variant
**Solution:** Axum uses `Message::Text(String)` not `message::text(&str)`

### Pitfall 2: Sender Channel Type
**Issue:** CnctdClient sender type doesn't match
**Solution:** Update sender type to use Axum's message type

### Pitfall 3: Query Parameter Extraction
**Issue:** client_id not found
**Solution:** Use Axum's Query extractor with proper struct

### Pitfall 4: State Cloning
**Issue:** Router state not Clone
**Solution:** Wrap non-Clone types in Arc

### Pitfall 5: Error Handling
**Issue:** WebSocket errors not handled
**Solution:** Wrap sends in Result, handle disconnects gracefully

## Files You'll Modify/Create

### New Files
- [ ] `src/socket/axum_socket.rs` - Main implementation

### Modified Files
- [ ] `src/socket/mod.rs` - Add module, re-export
- [ ] `src/socket/client.rs` - Update message type (maybe)

### Should NOT Need to Change
- ✅ `src/server/axum_server.rs` - Already has integration point
- ✅ `src/router/` - No socket-specific code here
- ✅ `SocketRouterFunction` trait - This stays the same

## Success Criteria

✅ All WebSocket connections work with Axum
✅ Message routing identical to Warp version
✅ Client management works the same
✅ Redis integration functions properly
✅ Broadcast works across all clients
✅ Disconnect handling works
✅ `SocketRouterFunction` trait unchanged
✅ No breaking changes to public API
✅ 100% backward compatible

## Example Implementation Structure

Here's the skeleton of what `axum_socket.rs` should look like:

```rust
use std::{collections::HashMap, fmt::Debug, sync::Arc};
use axum::{
    extract::{ws::{Message as AxumMessage, WebSocket, WebSocketUpgrade}, Query, State},
    response::IntoResponse,
    routing::get,
    Router,
};
use futures_util::{SinkExt, StreamExt};
use serde::{de::DeserializeOwned, Serialize};
use state::InitCell;
use tokio::sync::{mpsc, RwLock};
use anyhow::anyhow;

use crate::router::message::Message;
use crate::router::SocketRouterFunction;
use crate::server::server_info::ServerInfo;
use crate::socket::client::{ClientInfo, CnctdClient, QueryParams};
use super::SocketConfig;

pub static CLIENTS: InitCell<Arc<RwLock<HashMap<String, CnctdClient>>>> = InitCell::new();

pub struct CnctdSocket;

impl CnctdSocket {
    pub fn build_axum_routes<M, Resp, R>(config: SocketConfig<R>) -> Router
    where
        M: Serialize + DeserializeOwned + Send + Sync + Debug + Clone + 'static,
        Resp: Serialize + DeserializeOwned + Send + Sync + Debug + Clone + 'static,
        R: SocketRouterFunction<M, Resp> + 'static,
    {
        CLIENTS.set(Arc::new(RwLock::new(HashMap::new())));

        // Initialize Redis if configured
        let redis = if let Some(ref url) = config.redis_url {
            match cnctd_redis::CnctdRedis::start(url) {
                Ok(_) => {
                    println!("Redis started!");
                    tokio::spawn(async {
                        ServerInfo::set_redis_active(true).await;
                    });
                    true
                }
                Err(e) => {
                    println!("Error starting Redis pool: {:?}", e);
                    false
                }
            }
        } else {
            false
        };

        #[derive(Clone)]
        struct WsState<R> {
            router: R,
            redis: bool,
            on_disconnect: Option<Arc<dyn Fn(ClientInfo) + Send + Sync>>,
        }

        let state = WsState {
            router: config.router,
            redis,
            on_disconnect: config.on_disconnect,
        };

        Router::new()
            .route("/ws", get(ws_handler::<M, Resp, R>))
            .with_state(state)
    }

    async fn handle_connection<M, Resp, R>(
        websocket: WebSocket,
        router: R,
        client_id: String,
        redis: bool,
        on_disconnect: Option<Arc<dyn Fn(ClientInfo) + Send + Sync>>,
    )
    where
        M: Serialize + DeserializeOwned + Send + Sync + Debug + Clone + 'static,
        Resp: Serialize + DeserializeOwned + Send + Sync + Debug + Clone + 'static,
        R: SocketRouterFunction<M, Resp> + 'static,
    {
        // Split the websocket
        let (mut ws_tx, mut ws_rx) = websocket.split();
        let (resp_tx, mut resp_rx) = mpsc::unbounded_channel::<Result<AxumMessage, axum::Error>>();

        // Update client sender in CLIENTS map
        // TODO: Implement this

        let client_id_clone = client_id.clone();

        // Handle incoming messages
        let process_incoming = async move {
            while let Some(result) = ws_rx.next().await {
                match result {
                    Ok(msg) => {
                        if let AxumMessage::Text(text) = msg {
                            if let Ok(message) = serde_json::from_str::<M>(&text) {
                                match router.route(message, client_id_clone.clone()).await {
                                    Some(response) => {
                                        if let Ok(response_str) = serde_json::to_string(&response) {
                                            let _ = resp_tx.send(Ok(AxumMessage::Text(response_str)));
                                        }
                                    }
                                    None => {}
                                }
                            }
                        }
                    }
                    Err(e) => eprintln!("WebSocket receive error: {:?}", e),
                }
            }
        };

        // Handle outgoing messages
        let send_responses = async move {
            while let Some(response) = resp_rx.recv().await {
                if let Ok(msg) = response {
                    if ws_tx.send(msg).await.is_err() {
                        eprintln!("WebSocket send error");
                        break;
                    }
                }
            }
        };

        tokio::select! {
            _ = process_incoming => {},
            _ = send_responses => {},
        };

        // Handle disconnect
        if let Some(callback) = on_disconnect {
            if let Ok(client_info) = CnctdClient::get_client_info(&client_id).await {
                callback(client_info);
            }
        }

        // Cleanup
        match Self::remove_client(&client_id).await {
            Ok(_) => {}
            Err(e) => eprintln!("Error removing client: {:?}", e),
        };

        if redis {
            match Self::remove_client_from_redis(&client_id).await {
                Ok(_) => {}
                Err(e) => eprintln!("Error removing client from Redis: {:?}", e),
            }
        }
    }

    // Keep all existing helper methods from Warp version:
    // - broadcast_message()
    // - remove_client()
    // - push_client_to_redis()
    // - remove_client_from_redis()
}

async fn ws_handler<M, Resp, R>(
    ws: WebSocketUpgrade,
    State(state): State<WsState<R>>,
    Query(params): Query<QueryParams>,
) -> impl IntoResponse
where
    M: Serialize + DeserializeOwned + Send + Sync + Debug + Clone + 'static,
    Resp: Serialize + DeserializeOwned + Send + Sync + Debug + Clone + 'static,
    R: SocketRouterFunction<M, Resp> + 'static,
{
    let client_id = match params.client_id {
        Some(id) => id,
        None => {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                "Missing client_id parameter",
            )
                .into_response();
        }
    };

    ws.on_upgrade(move |socket| {
        CnctdSocket::handle_connection(
            socket,
            state.router,
            client_id,
            state.redis,
            state.on_disconnect,
        )
    })
}

#[derive(Clone)]
struct WsState<R> {
    router: R,
    redis: bool,
    on_disconnect: Option<Arc<dyn Fn(ClientInfo) + Send + Sync>>,
}
```

## Questions to Answer During Implementation

While implementing, you should be able to answer:

1. ✅ How does the client sender channel type work? 
2. ✅ How are query parameters extracted?
3. ✅ How do we maintain the CLIENTS map?
4. ✅ How does message routing work?
5. ✅ How does Redis integration work?
6. ✅ How are disconnects handled?
7. ✅ How does broadcast work?

All of these should match the Warp implementation exactly.

## After Phase 2 is Complete

Once WebSocket migration is done:

### Phase 3: Testing
- Integration tests
- Manual testing with real clients
- Load testing

### Phase 4: Cleanup
- Remove Warp dependency
- Remove old code
- Update documentation

## Getting Started Command

To start Phase 2 in a new chat, say:

```
I'm continuing the Axum migration for cnctd_server. 
Phase 1 (REST API) is complete on branch feat/axum-migration.

Now I need Phase 2: WebSocket migration.

The branch: https://github.com/Connected-Dot/cnctd_server/tree/feat/axum-migration

Please read PHASE2_TODO.md and implement the WebSocket migration.
Start by examining src/socket/ to understand the current Warp implementation.
```

---

**Current Status**: Phase 1 Complete ✅  
**Next**: Phase 2 - WebSocket Migration  
**Branch**: `feat/axum-migration`  
**Repository**: Connected-Dot/cnctd_server
