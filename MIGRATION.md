# Moving a consumer from cnctd_server 0.6 to 1.0

The wire format is unchanged: same envelopes, same `/api/<resource>/<op>`
shape, same `/ws` query parameters, same push message shape. Clients need no
changes. The server-side changes are mechanical and listed here.

## 1. The REST trait

0.6 had four methods probed in order (`route_with_raw_body`, `route_binary`,
`route`, plus `route_redirect`). 1.0 has one.

```rust
// 0.6
impl RestRouterFunction for RestRouter {
    fn route(&self, method, path, data, auth_token, client_id, ip_address) -> Pin<Box<dyn Future<Output = Result<SuccessResponse, ErrorResponse>> + Send>> { ... }
    fn route_redirect(&self, path, data, auth_token, client_id) -> Pin<Box<dyn Future<Output = String> + Send>> { ... }
    fn route_binary(...)        -> ... Result<Option<BinaryResponse>, ErrorResponse> ...
    fn route_with_raw_body(...) -> ... Result<Option<BinaryResponse>, ErrorResponse> ...
}

// 1.0
#[cnctd_server::async_trait]
impl RestRouter for RestRouter {
    async fn route(&self, req: Request) -> Result<Response, ErrorResponse> {
        if req.path.starts_with("/redirect/") {
            return Ok(Response::Redirect(redirect_url(&req).await));
        }
        if let Some(bin) = try_raw_body_routes(&req).await? {   // webhooks: req.body, req.headers
            return Ok(bin.into());
        }
        if let Some(bin) = try_binary_routes(&req).await? {     // MCP etc
            return Ok(bin.into());
        }
        Ok(route(req.method, req.path, req.data, req.auth_token, req.connection_id, req.ip_address).await?.into())
    }
}
```

Everything below that top-level function is untouched: the existing
positional helpers take the fields out of `Request`. `SuccessResponse` and
`BinaryResponse` convert into `Response` with `.into()`.

`Request` fields: `method: HttpMethod`, `path`, `prefix`, `query: Value`,
`body: Bytes`, `data: Value`, `headers: HashMap<String, String>` (lowercase
keys), `auth_token`, `connection_id`, `ip_address`. Helpers:
`req.resource_and_operation()` (prefix-aware; `CnctdServer::
path_to_resource_and_operation` still exists and assumes `api`),
`req.bearer()`, `req.header("x-thing")`.

## 2. The socket trait

```rust
// 0.6
impl SocketRouterFunction<SocketMessage, SocketResponse> for SocketRouter {
    fn route(&self, msg, client_id) -> Pin<Box<dyn Future<Output = Option<SocketResponse>> + Send>> { Box::pin(async move { ... }) }
}

// 1.0
#[cnctd_server::async_trait]
impl SocketRouter<SocketMessage, SocketResponse> for SocketRouter {
    async fn route(&self, msg: SocketMessage, connection_id: String) -> Option<SocketResponse> { ... }
}
```

## 3. Startup

`ServerConfig::new(id, port, client_dir, router, heartbeat, allowed_origins,
path)`, `.with_page_meta(...)`, `SocketConfig::new(router, secret, redis_url,
on_disconnect)`, `.with_on_binary(...)` and `CnctdServer::start(server_config,
Some(socket_config))` are unchanged. New and optional:

- `.with_max_body(bytes)` — request body limit (unlimited by default, as before)
- `.with_public_ip("...")` — the heartbeat's `public_ip`; the crate no longer looks it up
- `.with_fanout(Arc<dyn Fanout>)` — cross-instance push; default single
  instance. With the `redis` feature, `SocketConfig.redis_url` alone turns on
  Redis pub/sub fan-out between instances that share a server id (0.6
  accepted the url and only wrote connection records with it).
- `.with_bind_addr(ip)` — default all interfaces, as before
- `CnctdServer::start` now returns when the process gets SIGINT/SIGTERM,
  after a graceful shutdown. `start_with_shutdown(cfg, socket, signal)` takes
  your own signal; `bind(cfg, socket)` returns a `RunningServer` handle.
- `CnctdServer::start_without_socket(cfg)` avoids the socket type parameters.
- `SocketConfig.secret` is now used: `/ws?...&token=<jwt>` authenticates the
  connection at upgrade. Leave the query parameter out and nothing changes.

## 4. Connection API

`CnctdConnection::*` associated functions keep their names and signatures
(`register_connection`, `message_user`, `get_connection_infos`, ...), as do
`CnctdSocket::broadcast_message`, `Message::broadcast`, `Message::to_user`,
`ConnectionInfo`, `ConnectionFormat`, `OnBinaryHandler`. They go through the
running server's table. The same methods exist on the `Connections` handle
(`RunningServer::connections()`, or `Connections::global()`) for code that
would rather hold a handle than call statics.

## 5. Behavior changes to know about

| 0.6 | 1.0 |
|---|---|
| A rejected request fell through to the SPA and returned 200 HTML (bad redirect URL, `/ws` without an id, PATCH under `/api`) | 404 / 400 / 405 JSON envelopes |
| Unknown `connection_id` on `/ws` upgraded then dropped | 404 before upgrade |
| `ip_address` was the whole `X-Forwarded-For` value | first hop only |
| `Client-Id` missing from the CORS allow-list | allowed |
| Disallowed CORS origin got a 403 | normal response with no `Access-Control-Allow-Origin` (browsers block it the same way) |
| Non-GET on a page path: 405 empty | 405 JSON envelope |
| Server without a client dir answered unknown paths with placeholder HTML | 404 JSON envelope |
| Public IP looked up over the network at boot | `with_public_ip` or empty |
| `cpu_usage` was the core count, `memory_usage` total RAM | one-minute load average, percent of memory in use |
| `println!` everywhere | `tracing` |
| `ErrorCode::from_u16(504)` was `InternalServerError` | `GatewayTimeout`; `Conflict` (409) and `PayloadTooLarge` (413) added |

## 6. The wire types are their own crate

Since 1.0.0-alpha.2 the envelopes, macros, socket shapes, `ConnectionInfo`,
`ServerInfo` and `HttpMethod` live in `cnctd_protocol`, re-exported by
`cnctd_server` at every 0.6 path, so nothing in a server changes. Code that is
only a client (the hub's world link, the Car Thing) depends on
`cnctd_protocol` or `cnctd_client` and never compiles the server.
`Message::broadcast()` / `Message::to_user()` now come from the
`cnctd_server::router::message::Push` trait; import it where you call them.

## 7. Removed

`RestRouterFunction`, `SocketRouterFunction`, `Handler`, `RedirectHandler`,
`CnctdServer::start_redirect`, `CnctdSocket::start`/`build_routes`,
`utils::{cors, spa, spa_with_meta, serve_files, serve_index_with_range}`,
`SERVER_INFO`/`CONNECTIONS` statics, the `graphql` module, and the
`cnctd_redis` / `public-ip` / `r2d2` dependencies. Nothing in cnctd.world
used any of them.
