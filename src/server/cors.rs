use axum::http::{header, HeaderName, HeaderValue, Method};
use tower_http::cors::{Any, CorsLayer};

/// The headers our clients send, including `Client-Id` (the WebSocket
/// connection id a browser attaches to every REST call).
pub(crate) fn layer(allowed_origins: Option<Vec<String>>) -> CorsLayer {
    let cors = CorsLayer::new()
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE, Method::OPTIONS])
        .allow_headers([
            header::USER_AGENT,
            header::REFERER,
            header::ORIGIN,
            header::CONTENT_TYPE,
            header::AUTHORIZATION,
            header::ACCESS_CONTROL_REQUEST_METHOD,
            header::ACCESS_CONTROL_REQUEST_HEADERS,
            HeaderName::from_static("client-id"),
            HeaderName::from_static("sec-fetch-mode"),
        ]);

    match allowed_origins {
        Some(origins) => {
            let values: Vec<HeaderValue> = origins.iter().filter_map(|o| o.parse().ok()).collect();
            cors.allow_origin(values)
        }
        None => cors.allow_origin(Any),
    }
}
