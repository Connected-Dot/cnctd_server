//! What a route returns: the JSON success envelope (from `cnctd_protocol`,
//! with the macros that build it), raw bytes, a redirect, a byte stream or
//! server-sent events.

use std::{fmt, pin::Pin};

use bytes::Bytes;
use futures_util::Stream;
use serde::Serialize;

pub use cnctd_protocol::response::{SocketResponse, SuccessCode, SuccessResponse};

/// A non-envelope body: bytes with a content type. For
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
