//! The single-page-app fallback: real files from the client dir, `index.html`
//! for everything else, with the app's per-path `<head>` tags.

use std::path::PathBuf;

use axum::{
    body::Body,
    extract::{Request, State},
    http::{header, Method, StatusCode},
    response::{IntoResponse, Response},
    Json, Router,
};
use tower::ServiceExt;
use tower_http::services::ServeDir;

use super::PageMetaFn;
use crate::{
    router::error::{ErrorCode, ErrorResponse},
    utils::inject_head,
};

#[derive(Clone)]
struct SpaState {
    dir: PathBuf,
    index: PathBuf,
    meta: Option<PageMetaFn>,
}

pub(crate) fn router(client_dir: &str, meta: Option<PageMetaFn>) -> Router {
    let dir = PathBuf::from(client_dir);
    let state = SpaState {
        index: dir.join("index.html"),
        dir,
        meta,
    };
    Router::new().fallback(serve).with_state(state)
}

async fn serve(State(state): State<SpaState>, req: Request) -> Response {
    let method = req.method().clone();
    let path = req.uri().path().to_string();

    // A real file (or a directory's index.html) wins.
    if method == Method::GET || method == Method::HEAD {
        match ServeDir::new(&state.dir).oneshot(req).await {
            Ok(resp) if resp.status() != StatusCode::NOT_FOUND => return resp.into_response(),
            Ok(_) => {}
            Err(e) => {
                tracing::warn!(%path, "static file service failed: {}", e);
            }
        }
    } else {
        return (
            StatusCode::METHOD_NOT_ALLOWED,
            Json(ErrorResponse::new(Some(ErrorCode::MethodNotAllowed), Some(format!("{} not allowed here", method)))),
        )
            .into_response();
    }

    // The page: index.html, read fresh (a new client build is picked up
    // live), with the app's tags for this path at the top of <head>.
    let Ok(html) = tokio::fs::read_to_string(&state.index).await else {
        return (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new(None, Some("no index.html in the client directory".into()))),
        )
            .into_response();
    };
    let body = match &state.meta {
        Some(meta) => match (meta)(path).await {
            Some(head) => inject_head(&html, &head),
            None => html,
        },
        None => html,
    };
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        Body::from(body),
    )
        .into_response()
}
