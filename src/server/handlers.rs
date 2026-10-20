//! One axum handler for the whole API prefix: build the [`Request`], call the
//! app's router, turn its [`Response`] into HTTP.

use std::{convert::Infallible, net::SocketAddr, sync::Arc};

use axum::{
    body::Body,
    extract::{ConnectInfo, Request as HttpRequest, State},
    http::{header, StatusCode, Uri},
    response::{sse, IntoResponse, Response as HttpResponse},
    Json,
};
use futures_util::StreamExt;
use serde_json::{json, Value};

use crate::{
    router::{
        error::{ErrorCode, ErrorResponse},
        request::Request,
        response::Response,
        HttpMethod, RestRouter,
    },
    utils::{client_ip, lowercase_headers},
};

pub(crate) struct AppState<R> {
    pub router: Arc<R>,
    pub prefix: String,
    pub max_body: Option<usize>,
}

impl<R> Clone for AppState<R> {
    fn clone(&self) -> Self {
        Self {
            router: self.router.clone(),
            prefix: self.prefix.clone(),
            max_body: self.max_body,
        }
    }
}

fn error(err: ErrorResponse) -> HttpResponse {
    (err.status.status_code(), Json(err)).into_response()
}

/// `404` JSON envelope for paths nothing claims (servers without a client dir).
pub(crate) async fn not_found(req: HttpRequest) -> HttpResponse {
    error(ErrorResponse::new(None, Some(format!("no route for {} {}", req.method(), req.uri().path()))))
}

pub(crate) async fn api<R: RestRouter>(
    State(state): State<AppState<R>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    req: HttpRequest,
) -> HttpResponse {
    // HEAD is answered like GET (the body is dropped on the way out), so
    // health checks and link previews that use HEAD work.
    let http_method = if req.method() == axum::http::Method::HEAD { axum::http::Method::GET } else { req.method().clone() };
    let Some(method) = HttpMethod::from_http(&http_method) else {
        return error(ErrorResponse::new(
            Some(ErrorCode::MethodNotAllowed),
            Some(format!("{} is not served under /{}", req.method(), state.prefix)),
        ));
    };

    let (parts, body) = req.into_parts();
    let path = parts.uri.path().to_string();
    let query = parse_query(parts.uri.query());
    let headers = lowercase_headers(&parts.headers);
    let ip_address = client_ip(&parts.headers, Some(peer));
    let auth_token = headers.get("authorization").cloned();
    let connection_id = headers.get("client-id").cloned();

    let body = match axum::body::to_bytes(body, state.max_body.unwrap_or(usize::MAX)).await {
        Ok(b) => b,
        Err(e) => {
            return error(ErrorResponse::new(
                Some(ErrorCode::PayloadTooLarge),
                Some(format!("could not read body: {}", e)),
            ))
        }
    };

    // Deliberately lenient: webhooks post bodies that are not JSON, and clients
    // POST without a body for simple operations. Both route with data = {}.
    let data = if method.has_body() {
        if body.is_empty() {
            json!({})
        } else {
            serde_json::from_slice(&body).unwrap_or_else(|_| json!({}))
        }
    } else {
        query.clone()
    };

    tracing::debug!(%method, %path, "routing");

    let request = Request {
        method,
        path,
        prefix: state.prefix.clone(),
        query,
        body,
        data,
        headers,
        auth_token,
        connection_id,
        ip_address,
    };

    match state.router.route(request).await {
        Ok(response) => to_http(response),
        Err(err) => error(err),
    }
}

fn parse_query(query: Option<&str>) -> Value {
    match query.map(serde_urlencoded::from_str::<Value>) {
        Some(Ok(v @ Value::Object(_))) => v,
        _ => json!({}),
    }
}

fn to_http(response: Response) -> HttpResponse {
    match response {
        Response::Json(body) => (body.status.status_code(), Json(body)).into_response(),
        Response::Binary(bin) => {
            let status = bin.status.and_then(|s| StatusCode::from_u16(s).ok()).unwrap_or(StatusCode::OK);
            (status, [(header::CONTENT_TYPE, bin.content_type)], Body::from(bin.data)).into_response()
        }
        Response::Redirect(url) => match url.parse::<Uri>() {
            Ok(uri) => (StatusCode::FOUND, [(header::LOCATION, uri.to_string())]).into_response(),
            Err(_) => error(ErrorResponse::new(None, Some("redirect target is not a URL".into()))),
        },
        Response::Stream {
            content_type,
            status,
            body,
        } => {
            let status = status.and_then(|s| StatusCode::from_u16(s).ok()).unwrap_or(StatusCode::OK);
            (status, [(header::CONTENT_TYPE, content_type)], Body::from_stream(body)).into_response()
        }
        Response::Sse(events) => {
            let stream = events.map(|e| {
                let mut ev = sse::Event::default().data(e.data);
                if let Some(name) = e.event {
                    ev = ev.event(name);
                }
                if let Some(id) = e.id {
                    ev = ev.id(id);
                }
                Ok::<_, Infallible>(ev)
            });
            sse::Sse::new(stream).keep_alive(sse::KeepAlive::default()).into_response()
        }
    }
}
