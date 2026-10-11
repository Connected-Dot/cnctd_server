# cnctd_server

HTTP + WebSocket server framework for Connected Dot services. Built on axum;
the public API does not expose it.

## Install

```toml
[dependencies]
cnctd_server = "1.0.0-alpha.1"
```

## Quick start

```rust
use cnctd_server::{
    async_trait, bad_request, not_found, success_data,
    router::{error::{ErrorCode, ErrorResponse}, response::{SuccessCode, SuccessResponse}},
    CnctdServer, HttpMethod, Request, Response, RestRouter, ServerConfig,
};
use serde_json::json;

struct MyRouter;

#[async_trait]
impl RestRouter for MyRouter {
    async fn route(&self, req: Request) -> Result<Response, ErrorResponse> {
        let (resource, operation) = req.resource_and_operation();
        match (req.method, resource.as_str(), operation.as_deref()) {
            (HttpMethod::GET, "hello", Some(name)) => Ok(success_data!(json!({"hello": name})).into()),
            (HttpMethod::POST, "echo", _) => Ok(success_data!(req.data).into()),
            (_, "download", _) => Ok(cnctd_server::router::response::BinaryResponse::octet_stream(req.body.to_vec()).into()),
            (_, "", _) => Err(bad_request!("say something")),
            _ => Err(not_found!(format!("no {}", req.path))),
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = ServerConfig::new("my-server", "9000", Some("client/dist".into()), MyRouter, Some(10), None, None);
    CnctdServer::start_without_socket(config).await
}
```

`GET /api/hello/kyle` → `{"success":true,"status":"OK","msg":null,"data":{"hello":"kyle"}}`.
Anything that is not a file under `client/dist` serves `index.html`.

## What you get

- **Envelopes** via macros: `success_data!`, `success_msg!`, `created!`,
  `bad_request!`, `unauthorized!`, `forbidden!`, `not_found!`,
  `internal_server_error!`, and the rest of the 4xx/5xx set.
- **Other bodies**: `Response::Binary`, `Response::Redirect`,
  `Response::Stream`, `Response::Sse`.
- **Request context**: method, full path, prefix, query, raw body, lenient
  parsed data, lowercase headers, auth token (`req.bearer()` strips the
  prefix), the caller's WebSocket `Client-Id`, first-hop client IP.
- **WebSocket** with a connection table: register over REST or inline on the
  upgrade (`/ws?subscriptions=a,b`), optional JWT at upgrade, push by
  channel / user / connection / key-value, binary frames both ways,
  disconnect hook, heartbeat on `server-info`.
- **Operations**: CORS with an origin list, body size limit, graceful
  shutdown (`CnctdServer::bind` returns a `RunningServer`), `tracing` logs.
- **More than one instance**: feature `redis` fans pushes out over Redis
  pub/sub; set `SocketConfig.redis_url` or pass a `RedisFanout`.
- **TypeScript types**: feature `ts` + `scripts/export-ts.sh` emit `.d.ts`-style
  definitions of every wire type for the client package.

Full reference: `CLAUDE.md`. Moving from 0.6: `MIGRATION.md`.

## License

MIT
