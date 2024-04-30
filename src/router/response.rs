use serde::{Deserialize, Serialize};
use serde_json::Value;
use warp::{http::StatusCode, reply::Json};

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq)]
pub enum SuccessCode {
    OK = 200,
    Created = 201,
    Accepted = 202,
    NoContent = 204,
}

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

    pub fn from_status_code(status_code: StatusCode) -> Self {
        match status_code {
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

impl SuccessCode {
    pub fn to_warp_status_code(&self) -> StatusCode {
        match self {
            Self::OK => StatusCode::OK,
            Self::Created => StatusCode::CREATED,
            Self::Accepted => StatusCode::ACCEPTED,
            Self::NoContent => StatusCode::NO_CONTENT,
        }
    
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SuccessResponse {
    pub success: bool,
    pub status_code: SuccessCode,
    pub msg: Option<String>,
    pub data: Option<Value>,
}

impl SuccessResponse {
    pub fn new(status_code: Option<SuccessCode>, msg: Option<String>, data: Option<Value>) -> Self {
        let status_code = status_code.unwrap_or(SuccessCode::OK);
        Self { success: true, status_code,  msg, data }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ErrorResponse {
    pub success: bool,
    pub status_code: ErrorCode,
    pub msg: Option<String>,
}

impl ErrorResponse {
    pub fn new(error_code: Option<ErrorCode>, msg: Option<String>) -> Self {
        let status_code = error_code.unwrap_or(ErrorCode::NotFound);
        Self { success: false, status_code, msg }
    }
}

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
            response_channel
        }
    }

    pub fn failure(msg: Option<String>, data: Option<Value>, response_channel: String) -> Self {
        Self {
            success: false,
            msg,
            data,
            response_channel
        }
    }
}