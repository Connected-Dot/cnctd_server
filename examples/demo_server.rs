//! A small server for the TypeScript client's integration test
//! (modules/typescript/cnctd-server/scripts/integration.sh).
//!
//!     cargo run --example demo_server -- <port>
//!
//! REST: `/api/echo/*` answers with everything the router received,
//! `/api/fail/<code>` answers with that error, `/api/connections/register`
//! registers a connection and returns its id. Socket: `echo` echoes as a
//! push on the same channel, `rpc` answers the caller's response_channel.
//! Heartbeat every second; connected announcement on.

use std::net::{IpAddr, Ipv4Addr};

use cnctd_server::{
    async_trait, bad_request, not_found, success_data, unauthorized,
    router::{
        error::{ErrorCode, ErrorResponse},
        message::SocketMessage,
        response::{BinaryResponse, SuccessResponse},
    },
    CnctdServer, Connections, Request, Response, RestRouter, ServerConfig, SocketConfig, SocketRouter,
};
use serde_json::{json, Value};

struct Rest;

#[async_trait]
impl RestRouter for Rest {
    async fn route(&self, req: Request) -> Result<Response, ErrorResponse> {
        let (resource, operation) = req.resource_and_operation();
        match (resource.as_str(), operation.as_deref()) {
            ("echo", _) => Ok(success_data!(json!({
                "method": req.method.as_str(),
                "path": req.path,
                "data": req.data,
                "auth_token": req.auth_token,
                "connection_id": req.connection_id,
                "ip_address": req.ip_address,
            }))
            .into()),
            ("fail", Some("bad-request")) => Err(bad_request!("requested failure")),
            ("fail", Some("unauthorized")) => Err(unauthorized!("no token")),
            ("bin", _) => Ok(BinaryResponse::octet_stream(b"BIN".to_vec()).into()),
            ("connections", Some("register")) => {
                let subs: Vec<String> = req
                    .data
                    .get("subscriptions")
                    .and_then(Value::as_array)
                    .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
                    .unwrap_or_default();
                let id = Connections::global()
                    .map_err(|e| ErrorResponse::new(Some(ErrorCode::InternalServerError), Some(e.to_string())))?
                    .register(subs, req.ip_address, Default::default())
                    .await;
                Ok(success_data!(json!(id)).into())
            }
            _ => Err(not_found!(format!("no {}", req.path))),
        }
    }
}

struct Socket;

#[async_trait]
impl SocketRouter<SocketMessage, Value> for Socket {
    async fn route(&self, msg: SocketMessage, connection_id: String) -> Option<Value> {
        match msg.channel.as_str() {
            "echo" => Some(json!({"channel": "echo", "instruction": msg.instruction, "data": msg.data})),
            "rpc" => Some(json!({
                "success": true,
                "msg": null,
                "data": {"answered": msg.data, "from": connection_id},
                "response_channel": msg.response_channel,
            })),
            _ => None,
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let port = std::env::args().nth(1).unwrap_or_else(|| "48180".to_string());
    let server = ServerConfig::new("demo", &port, None, Rest, Some(1), None, None)
        .with_bind_addr(IpAddr::V4(Ipv4Addr::LOCALHOST));
    let socket = SocketConfig::new(Socket, None, None, None).with_connected_announcement(true);
    eprintln!("demo server on http://127.0.0.1:{}", port);
    CnctdServer::start(server, Some(socket)).await
}
