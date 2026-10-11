//! The JSON error envelope and the macros that build it.

use std::fmt;

use axum::http::StatusCode;
use serde::{Deserialize, Serialize};

#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    BadRequest = 400,
    Unauthorized = 401,
    Forbidden = 403,
    NotFound = 404,
    MethodNotAllowed = 405,
    RequestTimeout = 408,
    Conflict = 409,
    PayloadTooLarge = 413,
    UnprocessableEntity = 422,
    TooManyRequests = 429,
    InternalServerError = 500,
    BadGateway = 502,
    ServiceUnavailable = 503,
    GatewayTimeout = 504,
}

impl ErrorCode {
    pub fn as_u16(&self) -> u16 {
        *self as u16
    }

    pub fn status_code(&self) -> StatusCode {
        StatusCode::from_u16(self.as_u16()).expect("error codes are valid")
    }

    pub fn from_status(status: StatusCode) -> Self {
        Self::from_u16(status.as_u16())
    }

    /// Unknown codes become 500.
    pub fn from_u16(code: u16) -> Self {
        match code {
            400 => Self::BadRequest,
            401 => Self::Unauthorized,
            403 => Self::Forbidden,
            404 => Self::NotFound,
            405 => Self::MethodNotAllowed,
            408 => Self::RequestTimeout,
            409 => Self::Conflict,
            413 => Self::PayloadTooLarge,
            422 => Self::UnprocessableEntity,
            429 => Self::TooManyRequests,
            500 => Self::InternalServerError,
            502 => Self::BadGateway,
            503 => Self::ServiceUnavailable,
            504 => Self::GatewayTimeout,
            _ => Self::InternalServerError,
        }
    }
}

/// `{"success": false, "status": "BadRequest", "msg": ...}` with the HTTP
/// status to match. Also a plain `std::error::Error`, so `?` works in routes.
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ErrorResponse {
    pub success: bool,
    pub status: ErrorCode,
    pub msg: Option<String>,
}

impl ErrorResponse {
    /// No code means 404 — `not_found!` is the bare macro.
    pub fn new(error_code: Option<ErrorCode>, msg: Option<String>) -> Self {
        Self {
            success: false,
            status: error_code.unwrap_or(ErrorCode::NotFound),
            msg,
        }
    }
}

impl fmt::Display for ErrorResponse {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}: {:?}", self.msg.clone().unwrap_or_default(), self.status)
    }
}

impl std::error::Error for ErrorResponse {}

#[macro_export]
macro_rules! error_response {
    () => {
        ErrorResponse::new(None, None)
    };
    ($error_code:expr, $msg:expr) => {
        ErrorResponse::new(Some($error_code), Some($msg.to_string()))
    };
}

#[macro_export]
macro_rules! not_found {
    ($msg:expr) => {
        ErrorResponse::new(None, Some($msg.to_string()))
    };
}

#[macro_export]
macro_rules! unauthorized {
    ($msg:expr) => {
        ErrorResponse::new(Some(ErrorCode::Unauthorized), Some($msg.to_string()))
    };
}

#[macro_export]
macro_rules! forbidden {
    ($msg:expr) => {
        ErrorResponse::new(Some(ErrorCode::Forbidden), Some($msg.to_string()))
    };
}

#[macro_export]
macro_rules! bad_request {
    ($msg:expr) => {
        ErrorResponse::new(Some(ErrorCode::BadRequest), Some($msg.to_string()))
    };
}

#[macro_export]
macro_rules! method_not_allowed {
    ($msg:expr) => {
        ErrorResponse::new(Some(ErrorCode::MethodNotAllowed), Some($msg.to_string()))
    };
}

#[macro_export]
macro_rules! request_timeout {
    ($msg:expr) => {
        ErrorResponse::new(Some(ErrorCode::RequestTimeout), Some($msg.to_string()))
    };
}

#[macro_export]
macro_rules! conflict {
    ($msg:expr) => {
        ErrorResponse::new(Some(ErrorCode::Conflict), Some($msg.to_string()))
    };
}

#[macro_export]
macro_rules! too_many_requests {
    ($msg:expr) => {
        ErrorResponse::new(Some(ErrorCode::TooManyRequests), Some($msg.to_string()))
    };
}

#[macro_export]
macro_rules! internal_server_error {
    ($msg:expr) => {
        ErrorResponse::new(Some(ErrorCode::InternalServerError), Some($msg.to_string()))
    };
}

#[macro_export]
macro_rules! bad_gateway {
    ($msg:expr) => {
        ErrorResponse::new(Some(ErrorCode::BadGateway), Some($msg.to_string()))
    };
}

#[macro_export]
macro_rules! service_unavailable {
    ($msg:expr) => {
        ErrorResponse::new(Some(ErrorCode::ServiceUnavailable), Some($msg.to_string()))
    };
}
