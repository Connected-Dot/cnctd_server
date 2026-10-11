# CLAUDE.md - cnctd_server

> Context for Claude about the cnctd_server crate. 1.0 line (axum). The 0.6
> line (warp) lives on `main` until cnctd.world has moved.

## What it is

The HTTP + WebSocket framework behind cnctd.world's server and hub. The app
implements two traits and hands them to `CnctdServer`:

- `RestRouter` — one `route(Request) -> Result<Response, ErrorResponse>` call
  per HTTP request under the API prefix (`/api` by default) and under
  `/redirect`. The framework does no routing beyond that: `req.path` is the
  full path and the app dispatches on `req.resource_and_operation()`.
- `SocketRouter<Req, Resp>` — one `route(msg, connection_id) -> Option<Resp>`
  call per inbound text frame that parses as `Req`.

The framework provides: JSON envelopes built by the house macros, raw /
binary / streaming / SSE responses, redirects, lenient body parsing, a
single-page-app fallback with per-path `<head>` injection, CORS, a
connection table with pub/sub push (channel, user, connection, key/value),
inline WebSocket registration, JWT at upgrade, a `server-info` heartbeat,
and graceful shutdown. Nothing in the public API names axum.

## Layout

```
src/
  lib.rs              re-exports: Request, Response, RestRouter, SocketRouter,
                      CnctdServer, ServerConfig, RunningServer, SocketConfig,
                      Connections, CnctdConnection, Fanout, async_trait, Bytes
  router/
    mod.rs            HttpMethod, RestRouter, SocketRouter, NoSocketRouter
    request.rs        Request (method, path, prefix, query, body, data, headers,
                      auth_token, connection_id, ip_address) + bearer(),
                      resource_and_operation()
    response.rs       SuccessCode/SuccessResponse + success_* macros,
                      BinaryResponse, SseEvent, Response enum
                      (Json | Binary | Redirect | Stream | Sse), SocketResponse
    error.rs          ErrorCode/ErrorResponse + bad_request!, not_found!, ...
    message.rs        Message (channel/instruction/data) + broadcast()/to_user()
  server/
    mod.rs            ServerConfig (+ builders), CnctdServer::bind/start/
                      start_with_shutdown/start_without_socket, RunningServer,
                      heartbeat task
    handlers.rs       the one API handler: build Request, call router, map Response
    spa.rs            client dir -> files, else index.html with page_meta tags
    cors.rs           allowed origins; Client-Id is in the allow-list
    server_info.rs    ServerInfo (heartbeat payload)
  socket/
    mod.rs            SocketConfig, /ws upgrade rules, per-connection loop,
                      CnctdSocket::broadcast_message (compat)
    connection.rs     Connections (the table), CnctdConnection, ConnectionInfo,
                      ConnectionFormat; CnctdConnection::* associated fns over
                      the global table (0.6-compatible API)
    fanout.rs         Fanout trait (cross-instance push), LocalOnly
    redis_fanout.rs   RedisFanout (feature `redis`): pub/sub on one channel per server id
  auth/mod.rs         CnctdAuth: HS256 JWT sign/verify with a `sub` claim
  utils/mod.rs        client_ip (first X-Forwarded-For hop), lowercase_headers,
                      inject_head
tests/                behavioral suite, see below
MIGRATION.md          0.6 -> 1.0 for consumers
```

## Rules that hold the design together

- **One table, no panics.** `Connections` is a plain cloneable handle. The
  server creates one, hands it to the socket layer and heartbeat, and
  installs it as the process default so `CnctdConnection::*`, `Message::
  broadcast` and `CnctdSocket::broadcast_message` keep working. Those return
  an error (not a panic) when no server runs. Multiple servers per process
  are fine; the static API talks to the most recently started one.
- **Never hold the table lock across a send or another lock.** Collect under
  the guard, drop it, then act. 0.6 nested read guards on tokio's
  write-preferring `RwLock` and froze the whole table whenever a writer
  queued behind a heartbeat.
- **Local delivery is always done by `Connections`.** A `Fanout` only tells
  other instances. `LocalOnly` is the default. `RedisFanout` (feature
  `redis`) publishes each `Envelope` to `cnctd_server:fanout:<server id>`
  and runs a subscriber (started by `Fanout::attach`) that calls
  `Connections::deliver_local` for envelopes whose `origin` is not its own
  session id. Turn it on with `ServerConfig::with_fanout(Arc::new(
  RedisFanout::connect(url, id).await?))`, or just set
  `SocketConfig.redis_url` with the feature enabled. Publishes during a Redis
  outage fail and are logged; the subscriber reconnects with backoff.
- **Lenient bodies.** POST/PUT bodies that are empty or not JSON route with
  `data = {}`; the raw bytes are always in `req.body`. GET/DELETE take their
  data from the query string (values are strings).
- **Macros expand to bare names.** `success_data!` etc. need
  `SuccessResponse`, `SuccessCode`, `ErrorResponse`, `ErrorCode` in scope at
  the call site. Thousands of call sites rely on this; do not change it.
- **Logging is `tracing`.** No `println!`. Consumers pick the subscriber.
- **No network at boot.** `public_ip` is whatever the app sets with
  `with_public_ip`; the crate never looks it up.

## Upgrade rules for `/ws`

| Query | Result |
|---|---|
| `connection_id=<registered>` | upgrade |
| `connection_id=<unknown>` | 404 envelope, no upgrade |
| `subscriptions=a,b[&format=binary]` | inline registration, upgrade |
| neither | 400 envelope |
| `&token=<jwt>` with a socket secret | sets `user_id` + `authenticated`; invalid token is 401 |

A registered connection that never opens is dropped after 10 s.

## Development

```bash
cargo test                        # the behavioral suite + units (~12 s; one test waits out the 10 s grace)
cargo test --features redis       # also tests/redis.rs against REDIS_URL (default redis://127.0.0.1:6379; skips if down)
cargo clippy --all-targets --all-features
scripts/export-ts.sh [out-dir]    # TypeScript definitions of the protocol types (feature `ts`, ts-rs)
```

### Protocol types and the TypeScript client

The wire types are defined once, here, and tagged `#[cfg_attr(feature = "ts",
derive(ts_rs::TS), ts(export))]`: `HttpMethod`, `SuccessCode`,
`SuccessResponse`, `ErrorCode`, `ErrorResponse`, `SocketResponse`, `Message`,
`SocketMessage`, `UserMessage`, `UserSocketMessage`, `ConnectionFormat`,
`ConnectionInfo`, `ServerInfo`, `ServerStatus`. `scripts/export-ts.sh` writes
one `.ts` file per type (default `./bindings`, gitignored); the TypeScript
client package (`modules/typescript/cnctd-server`) points it at its own
`src/generated`. A new wire type gets the attribute; a renamed field shows up
as a TS diff in the client. Serde attributes are honoured (`ConnectionFormat`
is `"json" | "binary"`).

### Behavioral test suite (`tests/`)

Every test starts its own server on an ephemeral loopback port
(`CnctdServer::bind` with port `"0"`), so tests run in parallel with no
shared state. The suite is the spec a consumer migration is checked against.

- `tests/rest.rs` — the Request the router receives, every Response variant,
  envelopes and status codes, lenient bodies, prefix, body limit, CORS,
  405/404 envelopes, graceful shutdown, several servers per process
- `tests/socket.rs` — upgrade rules, inline registration, token auth, routing,
  push (channel / user / connection / key-value, exclude), binary frames,
  disconnect hook, heartbeat, grace expiry, custom path, table isolation
- `tests/redis.rs` (feature `redis`) — two instances on one channel: channel /
  user / connection / key-value pushes cross, exclude holds, no self-redelivery,
  `redis_url` alone enables it, shutdown stops the subscriber
- `tests/global.rs` — the `CnctdConnection::*` / `Message::broadcast` /
  `CnctdSocket::broadcast_message` static API (one server per binary)
- `tests/spa.rs` — files win, index fallback, `page_meta` injection, live
  index reload, 405 on non-GET pages, 404 without an index
- `tests/units.rs` — path splitting, `Request::bearer`, JWT, macros, builders
- `tests/common/mod.rs` — recording routers and `config()` / `start()` helpers

## Version

**1.0.0-alpha.1** on branch `feat/1.0-axum`. Consumers: see `MIGRATION.md`.

---

*Part of the cnctd monorepo. See `../../../CLAUDE.md` for ecosystem context.*
