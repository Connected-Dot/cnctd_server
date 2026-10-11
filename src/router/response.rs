//! What a route returns: the JSON success envelope (and the macros that
//! build it), raw bytes, a redirect, a byte stream or server-sent events.

use std::{fmt, pin::Pin};

use axum::http::StatusCode;
use bytes::Bytes;
use futures_util::Stream;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum SuccessCode {
    OK = 200,
    Created = 201,
    Accepted = 202,
    NoContent = 204,
}

impl SuccessCode {
    pub fn as_u16(&self) -> u16 {
        *self as u16
    }

    pub fn status_code(&self) -> StatusCode {
        StatusCode::from_u16(self.as_u16()).expect("success codes are valid")
    }

    /// 0.6 name for [`Self::status_code`]; kept so consumers compile.
    #[deprecated(since = "1.0.0", note = "use status_code()")]
    pub fn to_warp_status_code(&self) -> StatusCode {
        self.status_code()
    }
}

/// The JSON envelope every successful API call returns:
/// `{"success": true, "status": "OK", "msg": ..., "data": ...}`.
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SuccessResponse {
    pub success: bool,
    pub status: SuccessCode,
    pub msg: Option<String>,
    pub data: Option<Value>,
}

impl SuccessResponse {
    pub fn new(status: Option<SuccessCode>, msg: Option<String>, data: Option<Value>) -> Self {
        Self {
            success: true,
            status: status.unwrap_or(SuccessCode::OK),
            msg,
            data,
        }
    }
}

// The macros expand to bare `SuccessResponse` / `SuccessCode` names, so the
// caller needs those in scope. That has been the contract since 0.1 and
// every call site in cnctd.world relies on it.

#[macro_export]
macro_rules! success_response {
    () => {
        SuccessResponse::new(None, None, None)
    };
    ($status:expr) => {
        SuccessResponse::new(Some($status), None, None)
    };
    ($status:expr, $msg:expr) => {
        SuccessResponse::new(Some($status), Some($msg.to_string()), None)
    };
    ($status:expr, $msg:expr, $data:expr) => {
        SuccessResponse::new(Some($status), Some($msg.to_string()), Some($data))
    };
}

#[macro_export]
macro_rules! success_data {
    ($data:expr) => {
        SuccessResponse::new(None, None, Some($data))
    };
}

#[macro_export]
macro_rules! success_msg {
    ($msg:expr) => {
        SuccessResponse::new(None, Some($msg.to_string()), None)
    };
}

#[macro_export]
macro_rules! created {
    ($msg:expr, $data:expr) => {
        SuccessResponse::new(Some(SuccessCode::Created), Some($msg.to_string()), Some($data))
    };
}

/// A non-envelope body: whatever bytes with whatever content type. For
/// protocols that bring their own framing (MCP JSON-RPC, files, images).
#[derive(Debug, Clone)]
pub struct BinaryResponse {
    pub data: Vec<u8>,
    pub content_type: String,
    /// HTTP status; `None` is 200. MCP streamable-HTTP wants a 202 for
    /// notifications, for example.
    pub status: Option<u16>,
}

impl BinaryResponse {
    pub fn new(data: Vec<u8>, content_type: &str) -> Self {
        Self {
            data,
            content_type: content_type.to_string(),
            status: None,
        }
    }

    pub fn with_status(mut self, status: u16) -> Self {
        self.status = Some(status);
        self
    }

    pub fn octet_stream(data: Vec<u8>) -> Self {
        Self::new(data, "application/octet-stream")
    }

    pub fn json<T: Serialize>(value: &T) -> Self {
        Self::new(serde_json::to_vec(value).unwrap_or_default(), "application/json")
    }
}

/// One server-sent event. Framework-neutral so protocol types stay free of
/// the HTTP library.
#[derive(Debug, Clone, Default)]
pub struct SseEvent {
    pub event: Option<String>,
    pub id: Option<String>,
    pub data: String,
}

impl SseEvent {
    pub fn data(data: impl Into<String>) -> Self {
        Self {
            data: data.into(),
            ..Default::default()
        }
    }

    pub fn named(event: impl Into<String>, data: impl Into<String>) -> Self {
        Self {
            event: Some(event.into()),
            data: data.into(),
            id: None,
        }
    }

    pub fn json<T: Serialize>(event: Option<&str>, value: &T) -> Self {
        Self {
            event: event.map(str::to_string),
            id: None,
            data: serde_json::to_string(value).unwrap_or_default(),
        }
    }
}

pub type ByteStream = Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>>;
pub type EventStream = Pin<Box<dyn Stream<Item = SseEvent> + Send>>;

/// What a [`crate::RestRouter`] returns on success.
pub enum Response {
    /// The JSON envelope, with its status from `SuccessCode`.
    Json(SuccessResponse),
    /// Raw bytes with a content type and optional status.
    Binary(BinaryResponse),
    /// `302 Found` to this URL. An unparseable URL becomes a 404 envelope.
    Redirect(String),
    /// A chunked body, sent as it is produced.
    Stream {
        content_type: String,
        /// `None` is 200.
        status: Option<u16>,
        body: ByteStream,
    },
    /// Server-sent events, kept open until the stream ends.
    Sse(EventStream),
}

impl Response {
    pub fn stream(content_type: &str, body: ByteStream) -> Self {
        Self::Stream {
            content_type: content_type.to_string(),
            status: None,
            body,
        }
    }

    pub fn sse<S>(events: S) -> Self
    where
        S: Stream<Item = SseEvent> + Send + 'static,
    {
        Self::Sse(Box::pin(events))
    }
}

impl From<SuccessResponse> for Response {
    fn from(r: SuccessResponse) -> Self {
        Self::Json(r)
    }
}

impl From<BinaryResponse> for Response {
    fn from(r: BinaryResponse) -> Self {
        Self::Binary(r)
    }
}

impl fmt::Debug for Response {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(r) => f.debug_tuple("Json").field(r).finish(),
            Self::Binary(r) => f.debug_tuple("Binary").field(r).finish(),
            Self::Redirect(u) => f.debug_tuple("Redirect").field(u).finish(),
            Self::Stream { content_type, status, .. } => f
                .debug_struct("Stream")
                .field("content_type", content_type)
                .field("status", status)
                .finish_non_exhaustive(),
            Self::Sse(_) => f.write_str("Sse(..)"),
        }
    }
}

/// Reply shape for request/response over the WebSocket, keyed by the
/// caller's `response_channel`.
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SocketResponse {
    pub success: bool,
    pub msg: Option<String>,
    pub data: Option<Value>,
    pub response_channel: String,
}

impl SocketResponse {
    pub fn success(msg: Option<String>, data: Option<Value>, response_channel: String) -> Self {
        Self {
            success: true,
            msg,
            data,
            response_channel,
        }
    }

    pub fn failure(msg: Option<String>, data: Option<Value>, response_channel: String) -> Self {
        Self {
            success: false,
            msg,
            data,
            response_channel,
        }
    }
}
