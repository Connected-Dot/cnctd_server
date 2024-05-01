use std::fmt;

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
    pub status: SuccessCode,
    pub msg: Option<String>,
    pub data: Option<Value>,
}

impl SuccessResponse {
    pub fn new(status: Option<SuccessCode>, msg: Option<String>, data: Option<Value>) -> Self {
        let status = status.unwrap_or(SuccessCode::OK);
        Self { success: true, status,  msg, data }
    }
}

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