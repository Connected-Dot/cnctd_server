use axum::{
    extract::FromRequestParts,
    http::{request::Parts, StatusCode},
};
use std::net::SocketAddr;
use async_trait::async_trait;

// Custom extractor for IP address with X-Forwarded-For support
pub struct ClientIp(pub Option<String>);

#[async_trait]
impl<S> FromRequestParts<S> for ClientIp
where
    S: Send + Sync,
{
    type Rejection = (StatusCode, String);

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        // Check X-Forwarded-For header first
        if let Some(forwarded) = parts.headers.get("x-forwarded-for") {
            if let Ok(forwarded_str) = forwarded.to_str() {
                // Take first IP in the chain
                let ip = forwarded_str
                    .split(',')
                    .next()
                    .map(|s| s.trim().to_string());
                return Ok(ClientIp(ip));
            }
        }

        // Fallback to connection remote address
        if let Some(addr) = parts.extensions.get::<SocketAddr>() {
            return Ok(ClientIp(Some(addr.ip().to_string())));
        }

        Ok(ClientIp(None))
    }
}

// Custom extractor for full path (equivalent to warp::path::full())
pub struct FullPath(pub String);

#[async_trait]
impl<S> FromRequestParts<S> for FullPath
where
    S: Send + Sync,
{
    type Rejection = (StatusCode, String);

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(FullPath(parts.uri.path().to_string()))
    }
}
