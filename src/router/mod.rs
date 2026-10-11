//! The two traits an app implements, and the request / response types that
//! cross them.

pub mod error;
pub mod message;
pub mod request;
pub mod response;

use std::fmt;

use serde::{de::DeserializeOwned, Deserialize, Serialize};

pub use request::Request;
pub use response::Response;

use self::error::ErrorResponse;

#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HttpMethod {
    GET,
    POST,
    PUT,
    DELETE,
}

impl HttpMethod {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::GET => "GET",
            Self::POST => "POST",
            Self::PUT => "PUT",
            Self::DELETE => "DELETE",
        }
    }

    /// The methods the API prefix answers. Anything else is a 405.
    pub fn from_http(method: &axum::http::Method) -> Option<Self> {
        use axum::http::Method;
        match *method {
            Method::GET => Some(Self::GET),
            Method::POST => Some(Self::POST),
            Method::PUT => Some(Self::PUT),
            Method::DELETE => Some(Self::DELETE),
            _ => None,
        }
    }

    /// GET and DELETE carry their data in the query string; POST and PUT in
    /// the body.
    pub fn has_body(&self) -> bool {
        matches!(self, Self::POST | Self::PUT)
    }
}

impl fmt::Display for HttpMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The app's HTTP router. One call per request under the API prefix (default
/// `/api`) and under `/redirect`; the framework does no routing of its own
/// beyond that — `req.path` is the full path and the app dispatches on it
/// (see [`Request::resource_and_operation`]).
///
/// Return a [`Response`] variant for the body you want — the JSON envelope the
/// macros build, raw bytes, a redirect, a byte stream or server-sent events —
/// or an [`ErrorResponse`] (also built by macros) for a JSON error envelope
/// with its status code.
///
/// ```ignore
/// #[cnctd_server::async_trait]
/// impl RestRouter for MyRouter {
///     async fn route(&self, req: Request) -> Result<Response, ErrorResponse> {
///         let (resource, op) = req.resource_and_operation();
///         match resource.as_str() {
///             "dots" => Ok(dots::route(req.method, op, req.data, req.auth_token).await?.into()),
///             _ => Err(not_found!("no such resource")),
///         }
///     }
/// }
/// ```
#[async_trait::async_trait]
pub trait RestRouter: Send + Sync + 'static {
    async fn route(&self, req: Request) -> Result<Response, ErrorResponse>;
}

/// The app's WebSocket router. Text frames are parsed as `Req`; a `Some`
/// reply is serialized and sent back on the same connection. Frames that do
/// not parse as `Req` are dropped. Binary frames never reach this router —
/// see [`crate::socket::SocketConfig::with_on_binary`].
#[async_trait::async_trait]
pub trait SocketRouter<Req, Resp>: Send + Sync + 'static
where
    Req: DeserializeOwned + Send + 'static,
    Resp: Serialize + Send + 'static,
{
    async fn route(&self, msg: Req, connection_id: String) -> Option<Resp>;
}

/// For servers without a WebSocket. `CnctdServer::start_without_socket` uses
/// it so callers never spell out the socket type parameters.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoSocketRouter;

#[async_trait::async_trait]
impl SocketRouter<serde_json::Value, serde_json::Value> for NoSocketRouter {
    async fn route(&self, _msg: serde_json::Value, _connection_id: String) -> Option<serde_json::Value> {
        None
    }
}
