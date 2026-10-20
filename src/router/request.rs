//! Everything the framework knows about one HTTP request, in one struct.

use std::collections::HashMap;

use bytes::Bytes;
use serde_json::Value;

use super::HttpMethod;

#[derive(Debug, Clone)]
pub struct Request {
    pub method: HttpMethod,
    /// Full request path, API prefix included, query string excluded
    /// (`/api/dots/create`).
    pub path: String,
    /// The API prefix this server routes under (`api` by default), so
    /// [`Request::resource_and_operation`] can strip it.
    pub prefix: String,
    /// Query string as an object of strings (`?a=1` -> `{"a": "1"}`); `{}`
    /// when absent.
    pub query: Value,
    /// Raw body bytes, untouched — for byte-exact signature checks.
    pub body: Bytes,
    /// The request's data as the app sees it: for POST and PUT the body
    /// parsed as JSON, falling back to `{}` when empty or not JSON (webhooks
    /// post all sorts of things); for GET and DELETE the query object.
    pub data: Value,
    /// Every header with a UTF-8 value, keyed by lowercase name.
    pub headers: HashMap<String, String>,
    /// The `Authorization` header, verbatim. Use [`Request::bearer`] to
    /// strip a `Bearer ` prefix.
    pub auth_token: Option<String>,
    /// The `Client-Id` header: the caller's WebSocket connection id, if it
    /// has one open.
    pub connection_id: Option<String>,
    /// First hop of `X-Forwarded-For` when present, else the peer address.
    pub ip_address: Option<String>,
}

impl Request {
    /// `/api/dots/create` -> `("dots", Some("create"))`. Only the first two
    /// segments after the prefix count.
    pub fn resource_and_operation(&self) -> (String, Option<String>) {
        split_resource_and_operation(&self.path, &self.prefix)
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(&name.to_ascii_lowercase()).map(String::as_str)
    }

    /// The auth token without a leading `Bearer ` (case-insensitive). Our own
    /// clients historically send the raw token; both shapes come out the same.
    pub fn bearer(&self) -> Option<&str> {
        self.auth_token.as_deref().map(|t| {
            let t = t.trim_start();
            if t.len() >= 7 && t[..7].eq_ignore_ascii_case("bearer ") {
                t[7..].trim_start()
            } else {
                t
            }
        })
    }
}

pub(crate) fn split_resource_and_operation(path: &str, prefix: &str) -> (String, Option<String>) {
    let stripped = path
        .strip_prefix('/')
        .unwrap_or(path)
        .strip_prefix(prefix)
        .map(|rest| rest.trim_start_matches('/'))
        .unwrap_or(path);
    let mut parts = stripped.split('/');
    let resource = parts.next().unwrap_or_default().to_string();
    let operation = parts.next().map(|s| s.to_string());
    (resource, operation)
}
