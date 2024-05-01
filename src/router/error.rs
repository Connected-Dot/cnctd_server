use std::fmt;

use serde::{Deserialize, Serialize};
use warp::http::StatusCode;

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq)]
pub enum ErrorCode {
    BadRequest = 400,
    Unauthorized = 401,
    Forbidden = 403,
    NotFound = 404,
    MethodNotAllowed = 405,
    RequestTimeout = 408,
    TooManyRequests = 429,
    InternalServerError = 500,
    BadGateway = 502,
    ServiceUnavailable = 503,
    GatewayTimeout = 504,
}

impl ErrorCode {
    pub fn to_warp_status_code(&self) -> StatusCode {
        match self {
            Self::BadRequest => StatusCode::BAD_REQUEST,
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::Forbidden => StatusCode::FORBIDDEN,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::MethodNotAllowed => StatusCode::METHOD_NOT_ALLOWED,
            Self::RequestTimeout => StatusCode::REQUEST_TIMEOUT,
            Self::TooManyRequests => StatusCode::TOO_MANY_REQUESTS,
            Self::InternalServerError => StatusCode::INTERNAL_SERVER_ERROR,
            Self::BadGateway => StatusCode::BAD_GATEWAY,
            Self::ServiceUnavailable => StatusCode::SERVICE_UNAVAILABLE,
            Self::GatewayTimeout => StatusCode::GATEWAY_TIMEOUT,
        }
    }

    pub fn from_status(status: StatusCode) -> Self {
        match status {
            StatusCode::BAD_REQUEST => Self::BadRequest,
            StatusCode::UNAUTHORIZED => Self::Unauthorized,
            StatusCode::FORBIDDEN => Self::Forbidden,
            StatusCode::NOT_FOUND => Self::NotFound,
            StatusCode::METHOD_NOT_ALLOWED => Self::MethodNotAllowed,
            StatusCode::REQUEST_TIMEOUT => Self::RequestTimeout,
            StatusCode::TOO_MANY_REQUESTS => Self::TooManyRequests,
            StatusCode::INTERNAL_SERVER_ERROR => Self::InternalServerError,
            StatusCode::BAD_GATEWAY => Self::BadGateway,
            StatusCode::SERVICE_UNAVAILABLE => Self::ServiceUnavailable,
            StatusCode::GATEWAY_TIMEOUT => Self::GatewayTimeout,
            _ => Self::InternalServerError,
        }
    }
}


#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ErrorResponse {
    pub success: bool,
    pub status: ErrorCode,
    pub msg: Option<String>,
}

impl ErrorResponse {
    pub fn new(error_code: Option<ErrorCode>, msg: Option<String>) -> Self {
        let status = error_code.unwrap_or(ErrorCode::NotFound);
        Self { success: false, status, msg }
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