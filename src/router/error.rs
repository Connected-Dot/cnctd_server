use std::fmt;

use serde::{Deserialize, Serialize};
use warp::http::StatusCode as WarpStatusCode;

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq)]
pub enum ErrorCode {
    BadRequest = 400,
    Unauthorized = 401,
    Forbidden = 403,
    NotFound = 404,
    UnprocessableEntity = 422,
    MethodNotAllowed = 405,
    RequestTimeout = 408,
    TooManyRequests = 429,
    InternalServerError = 500,
    BadGateway = 502,
    ServiceUnavailable = 503,
    GatewayTimeout = 504,
}

impl ErrorCode {
    pub fn to_warp_status_code(&self) -> WarpStatusCode {
        match self {
            Self::BadRequest => WarpStatusCode::BAD_REQUEST,
            Self::Unauthorized => WarpStatusCode::UNAUTHORIZED,
            Self::Forbidden => WarpStatusCode::FORBIDDEN,
            Self::NotFound => WarpStatusCode::NOT_FOUND,
            Self::UnprocessableEntity => WarpStatusCode::UNPROCESSABLE_ENTITY,
            Self::MethodNotAllowed => WarpStatusCode::METHOD_NOT_ALLOWED,
            Self::RequestTimeout => WarpStatusCode::REQUEST_TIMEOUT,
            Self::TooManyRequests => WarpStatusCode::TOO_MANY_REQUESTS,
            Self::InternalServerError => WarpStatusCode::INTERNAL_SERVER_ERROR,
            Self::BadGateway => WarpStatusCode::BAD_GATEWAY,
            Self::ServiceUnavailable => WarpStatusCode::SERVICE_UNAVAILABLE,
            Self::GatewayTimeout => WarpStatusCode::GATEWAY_TIMEOUT,
        }
    }

    // Add Axum status code conversion
    pub fn to_axum_status_code(&self) -> axum::http::StatusCode {
        match self {
            Self::BadRequest => axum::http::StatusCode::BAD_REQUEST,
            Self::Unauthorized => axum::http::StatusCode::UNAUTHORIZED,
            Self::Forbidden => axum::http::StatusCode::FORBIDDEN,
            Self::NotFound => axum::http::StatusCode::NOT_FOUND,
            Self::UnprocessableEntity => axum::http::StatusCode::UNPROCESSABLE_ENTITY,
            Self::MethodNotAllowed => axum::http::StatusCode::METHOD_NOT_ALLOWED,
            Self::RequestTimeout => axum::http::StatusCode::REQUEST_TIMEOUT,
            Self::TooManyRequests => axum::http::StatusCode::TOO_MANY_REQUESTS,
            Self::InternalServerError => axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Self::BadGateway => axum::http::StatusCode::BAD_GATEWAY,
            Self::ServiceUnavailable => axum::http::StatusCode::SERVICE_UNAVAILABLE,
            Self::GatewayTimeout => axum::http::StatusCode::GATEWAY_TIMEOUT,
        }
    }

    pub fn from_status(status: WarpStatusCode) -> Self {
        match status {
            WarpStatusCode::BAD_REQUEST => Self::BadRequest,
            WarpStatusCode::UNAUTHORIZED => Self::Unauthorized,
            WarpStatusCode::FORBIDDEN => Self::Forbidden,
            WarpStatusCode::NOT_FOUND => Self::NotFound,
            WarpStatusCode::UNPROCESSABLE_ENTITY => Self::UnprocessableEntity,
            WarpStatusCode::METHOD_NOT_ALLOWED => Self::MethodNotAllowed,
            WarpStatusCode::REQUEST_TIMEOUT => Self::RequestTimeout,
            WarpStatusCode::TOO_MANY_REQUESTS => Self::TooManyRequests,
            WarpStatusCode::INTERNAL_SERVER_ERROR => Self::InternalServerError,
            WarpStatusCode::BAD_GATEWAY => Self::BadGateway,
            WarpStatusCode::SERVICE_UNAVAILABLE => Self::ServiceUnavailable,
            WarpStatusCode::GATEWAY_TIMEOUT => Self::GatewayTimeout,
            _ => Self::InternalServerError,
        }
    }

    pub fn from_u16(code: u16) -> Self {
        match code {
            400 => Self::BadRequest,
            401 => Self::Unauthorized,
            403 => Self::Forbidden,
            404 => Self::NotFound,
            405 => Self::MethodNotAllowed,
            408 => Self::RequestTimeout,
            422 => Self::UnprocessableEntity,
            429 => Self::TooManyRequests,
            500 => Self::InternalServerError,
            502 => Self::BadGateway,
            503 => Self::ServiceUnavailable,
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
