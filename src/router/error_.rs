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
    pub fn description(&self) -> &str {
        match self {
            Self::BadRequest => "Bad Request: The server cannot or will not process the request due to something that is perceived to be a client error.",
            Self::Unauthorized => "Unauthorized: Authentication is required and has failed or has not yet been provided.",
            Self::Forbidden => "Forbidden: The server understood the request but refuses to authorize it.",
            Self::NotFound => "Not Found: The requested resource could not be found but may be available in the future.",
            Self::MethodNotAllowed => "Method Not Allowed: A request method is not supported for the requested resource.",
            Self::RequestTimeout => "Request Timeout: The server timed out waiting for the request.",
            Self::TooManyRequests => "Too Many Requests: The user has sent too many requests in a given amount of time.",
            Self::InternalServerError => "Internal Server Error: A generic error message, given when no more specific message is suitable.",
            Self::BadGateway => "Bad Gateway: The server was acting as a gateway or proxy and received an invalid response from the upstream server.",
            Self::ServiceUnavailable => "Service Unavailable: The server is currently unable to handle the request due to a temporary overload or scheduled maintenance.",
            Self::GatewayTimeout => "Gateway Timeout: The server was acting as a gateway or proxy and did not receive a timely response from the upstream server.",
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct Error {
    pub code: ErrorCode,
    pub message: Option<String>,
}

impl warp::reject::Reject for ErrorResponse {}

impl Error {
    pub fn new(code: ErrorCode, message: Option<String>) -> Self {
        Self { code, message }
    }

    pub fn from_code(code: ErrorCode) -> Self {
        Self { code, message: Some(code.description().into()) }
    }

    pub fn default() -> Self {
        Self { code: ErrorCode::InternalServerError, message: Some(ErrorCode::InternalServerError.description().into()) }
    }

    pub fn from_anyhow(e: anyhow::Error) -> Self {
        Self { code: ErrorCode::InternalServerError, message: Some(e.to_string()) }
    }

    pub fn to_warp_status_code(&self) -> StatusCode {
        match self.code {
            ErrorCode::BadRequest => StatusCode::BAD_REQUEST,
            ErrorCode::Unauthorized => StatusCode::UNAUTHORIZED,
            ErrorCode::Forbidden => StatusCode::FORBIDDEN,
            ErrorCode::NotFound => StatusCode::NOT_FOUND,
            ErrorCode::MethodNotAllowed => StatusCode::METHOD_NOT_ALLOWED,
            ErrorCode::RequestTimeout => StatusCode::REQUEST_TIMEOUT,
            ErrorCode::TooManyRequests => StatusCode::TOO_MANY_REQUESTS,
            ErrorCode::InternalServerError => StatusCode::INTERNAL_SERVER_ERROR,
            ErrorCode::BadGateway => StatusCode::BAD_GATEWAY,
            ErrorCode::ServiceUnavailable => StatusCode::SERVICE_UNAVAILABLE,
            ErrorCode::GatewayTimeout => StatusCode::GATEWAY_TIMEOUT,
        }
    }

    pub fn to_warp_reply(&self) -> warp::reply::WithStatus<warp::reply::Json> {
        warp::reply::with_status(warp::reply::json(&self), self.to_warp_status_code())
    }
    
}