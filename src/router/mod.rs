pub mod response;
pub mod message;
pub mod request;
pub mod error;


use std::collections::HashMap;
use std::{future::Future, pin::Pin};
use std::fmt::Debug;
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use serde_json::Value;


use self::error::ErrorResponse;
use self::response::{BinaryResponse, SuccessResponse};

// Re-export so consumers can use raw-body routes without depending on `bytes`
// directly: `use cnctd_server::router::Bytes;` (or via the crate root).
pub use bytes::Bytes as ReqBodyBytes;


#[derive(Debug)]
pub enum HttpMethod {
    GET,
    POST,
    PUT,
    DELETE,
}

pub trait RestRouterFunction: Send + Sync + Clone
where
    Self: Send + Sync + Clone,
{
    fn route(&self, method: HttpMethod, path: String, data: Value, auth_token: Option<String>, connection_id: Option<String>, ip_address: Option<String>) -> Pin<Box<dyn Future<Output = Result<SuccessResponse, ErrorResponse>> + Send>>;
    fn route_redirect(&self, path: String, data: Value, auth_token: Option<String>, connection_id: Option<String>) -> Pin<Box<dyn Future<Output = String> + Send>>;

    /// Route a request that returns raw binary data instead of JSON.
    /// Returns `Ok(Some(BinaryResponse))` if the path is handled as binary,
    /// `Ok(None)` if this path should fall through to normal JSON routing,
    /// or `Err(ErrorResponse)` on failure.
    fn route_binary(&self, _method: HttpMethod, _path: String, _data: Value, _auth_token: Option<String>, _connection_id: Option<String>, _ip_address: Option<String>) -> Pin<Box<dyn Future<Output = Result<Option<BinaryResponse>, ErrorResponse>> + Send>> {
        Box::pin(async { Ok(None) })
    }

    /// Route a POST request with access to the **raw request body bytes**,
    /// the **full request headers**, and the parsed Value. Use this for
    /// endpoints that need to verify byte-exact signatures (Stripe webhooks
    /// where the signature lives in `Stripe-Signature`, GitHub webhooks
    /// where it lives in `X-Hub-Signature-256`, etc).
    ///
    /// Headers are passed as a HashMap with lowercase keys (HTTP headers
    /// are case-insensitive) so handlers can do `headers.get("stripe-signature")`
    /// without worrying about case.
    ///
    /// Tried BEFORE `route_binary` and `route` in the POST dispatch chain.
    /// Default impl returns `Ok(None)` so existing routers are unaffected.
    fn route_with_raw_body(
        &self,
        _method: HttpMethod,
        _path: String,
        _body_bytes: Bytes,
        _data: Value,
        _headers: HashMap<String, String>,
        _auth_token: Option<String>,
        _connection_id: Option<String>,
        _ip_address: Option<String>,
    ) -> Pin<Box<dyn Future<Output = Result<Option<BinaryResponse>, ErrorResponse>> + Send>>
    {
        Box::pin(async { Ok(None) })
    }
}

pub trait SocketRouterFunction<Req, Resp>: Send + Sync + Clone
where
    Self: Send + Sync + Clone,
    Req: Serialize + for<'de> Deserialize<'de> + Clone + Debug,
    Resp: Serialize + for<'de> Deserialize<'de> + Clone + Debug,
{
    fn route(&self, msg: Req, connection_id: String) -> Pin<Box<dyn Future<Output = Option<Resp>> + Send>>;
}

