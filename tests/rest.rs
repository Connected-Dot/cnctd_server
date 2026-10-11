//! REST behavior: what the app's router receives, and what the client gets
//! back for every `Response` variant and error.

mod common;

use common::{config, http, start_rest, RecordingRouter};
use reqwest::StatusCode;
use serde_json::{json, Value};

async fn envelope(resp: reqwest::Response) -> (StatusCode, Value) {
    let status = resp.status();
    let body: Value = resp.json().await.expect("JSON envelope");
    (status, body)
}

// ---------------------------------------------------------------- request

#[tokio::test]
async fn post_json_body_reaches_router_with_full_context() {
    let router = RecordingRouter::default();
    let s = start_rest(config("rest", router.clone())).await;
    let resp = http()
        .post(format!("{}/api/echo/post-ctx", s.url()))
        .header("Authorization", "Bearer tok-123")
        .header("Client-Id", "conn-abc")
        .json(&json!({"name": "dot", "n": 2}))
        .send()
        .await
        .unwrap();
    let (status, body) = envelope(resp).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["success"], json!(true));
    assert_eq!(body["status"], json!("OK"));
    let d = &body["data"];
    assert_eq!(d["method"], json!("POST"));
    assert_eq!(d["path"], json!("/api/echo/post-ctx"), "full path, prefix included");
    assert_eq!(d["prefix"], json!("api"));
    assert_eq!(d["resource"], json!("echo"));
    assert_eq!(d["operation"], json!("post-ctx"));
    assert_eq!(d["data"], json!({"name": "dot", "n": 2}));
    assert_eq!(d["query"], json!({}));
    assert_eq!(d["auth_token"], json!("Bearer tok-123"), "verbatim");
    assert_eq!(d["bearer"], json!("tok-123"), "stripped on request");
    assert_eq!(d["connection_id"], json!("conn-abc"));
    assert_eq!(d["ip_address"], json!("127.0.0.1"), "peer address without X-Forwarded-For");
    assert_eq!(router.calls_for("/api/echo/post-ctx").len(), 1, "exactly one route call per request");
}

#[tokio::test]
async fn get_query_string_becomes_data_of_strings() {
    let s = start_rest(config("rest", RecordingRouter::default())).await;
    let (status, body) = envelope(http().get(format!("{}/api/echo/get-q?a=1&b=two&c=", s.url())).send().await.unwrap()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["method"], json!("GET"));
    assert_eq!(body["data"]["data"], json!({"a": "1", "b": "two", "c": ""}), "strings, never coerced");
    assert_eq!(body["data"]["query"], body["data"]["data"]);
    assert_eq!(body["data"]["body"], json!(""));
    assert_eq!(body["data"]["auth_token"], Value::Null);
    assert_eq!(body["data"]["connection_id"], Value::Null);
}

#[tokio::test]
async fn get_without_query_gives_empty_object() {
    let s = start_rest(config("rest", RecordingRouter::default())).await;
    let (_, body) = envelope(http().get(format!("{}/api/echo/get-empty", s.url())).send().await.unwrap()).await;
    assert_eq!(body["data"]["data"], json!({}));
}

#[tokio::test]
async fn delete_uses_query_like_get() {
    let s = start_rest(config("rest", RecordingRouter::default())).await;
    let (status, body) = envelope(http().delete(format!("{}/api/echo/del?id=9", s.url())).send().await.unwrap()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["method"], json!("DELETE"));
    assert_eq!(body["data"]["data"], json!({"id": "9"}));
}

#[tokio::test]
async fn put_json_body_reaches_router() {
    let s = start_rest(config("rest", RecordingRouter::default())).await;
    let (status, body) = envelope(
        http().put(format!("{}/api/echo/put-body", s.url())).json(&json!({"x": [1, 2]})).send().await.unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["method"], json!("PUT"));
    assert_eq!(body["data"]["data"], json!({"x": [1, 2]}));
}

#[tokio::test]
async fn post_with_empty_body_and_no_content_type_gives_empty_object() {
    let s = start_rest(config("rest", RecordingRouter::default())).await;
    let (status, body) = envelope(http().post(format!("{}/api/echo/post-empty", s.url())).send().await.unwrap()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["data"], json!({}));
    assert_eq!(body["data"]["body"], json!(""));
}

#[tokio::test]
async fn post_with_unparseable_body_is_lenient() {
    // Sonos-style webhooks post bodies that are not JSON; the request still
    // routes, with data = {} and the raw bytes intact.
    let s = start_rest(config("rest", RecordingRouter::default())).await;
    let (status, body) = envelope(
        http()
            .post(format!("{}/api/echo/post-garbage", s.url()))
            .header("Content-Type", "text/plain")
            .body("this is <not> json")
            .send()
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["data"], json!({}));
    assert_eq!(body["data"]["body"], json!("this is <not> json"));
}

#[tokio::test]
async fn raw_body_bytes_and_lowercase_headers_are_exact() {
    // Spacing that re-serialization would not reproduce proves the bytes are
    // untouched — what a signed webhook (Stripe, GitHub) needs.
    let s = start_rest(config("rest", RecordingRouter::default())).await;
    let wire = "{ \"a\" :  1 ,\n \"b\": \"two\" }";
    let resp = http()
        .post(format!("{}/api/bin-json/sig", s.url()))
        .header("Content-Type", "application/json")
        .header("Stripe-Signature", "t=1,v1=abc")
        .body(wire)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.headers()["content-type"], "application/json");
    let body: Value = resp.json().await.unwrap();
    assert!(body.get("success").is_none(), "not an envelope");
    assert_eq!(body["raw"], json!(wire));
    assert_eq!(body["data"], json!({"a": 1, "b": "two"}));
    assert_eq!(body["headers"]["stripe-signature"], json!("t=1,v1=abc"));
    assert_eq!(body["headers"]["content-type"], json!("application/json"));
    let keys = body["headers"].as_object().unwrap().keys().cloned().collect::<Vec<_>>();
    assert!(keys.iter().all(|k| k == &k.to_lowercase()), "{:?}", keys);
}

#[tokio::test]
async fn x_forwarded_for_first_hop_wins_over_peer_address() {
    let s = start_rest(config("rest", RecordingRouter::default())).await;
    let (_, body) = envelope(
        http()
            .get(format!("{}/api/echo/xff", s.url()))
            .header("X-Forwarded-For", "203.0.113.9, 10.0.0.1")
            .send()
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(body["data"]["ip_address"], json!("203.0.113.9"));
}

#[tokio::test]
async fn custom_api_prefix() {
    let mut cfg = config("rest", RecordingRouter::default());
    cfg.path = Some("v2".into());
    let s = start_rest(cfg).await;
    let (status, body) = envelope(http().get(format!("{}/v2/echo/here", s.url())).send().await.unwrap()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["prefix"], json!("v2"));
    assert_eq!(body["data"]["resource"], json!("echo"));
    assert_eq!(body["data"]["operation"], json!("here"));
    let resp = http().get(format!("{}/api/echo/here", s.url())).send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND, "the old prefix is not served");
}

#[tokio::test]
async fn body_over_the_limit_is_413() {
    let s = start_rest(config("rest", RecordingRouter::default()).with_max_body(16)).await;
    let resp = http().post(format!("{}/api/echo/big", s.url())).body("x".repeat(100)).send().await.unwrap();
    let (status, body) = envelope(resp).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(body["status"], json!("PayloadTooLarge"));
    let resp = http().post(format!("{}/api/echo/small", s.url())).body("tiny").send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

// ---------------------------------------------------------------- envelopes

#[tokio::test]
async fn success_code_sets_http_status() {
    let s = start_rest(config("rest", RecordingRouter::default())).await;
    let (status, body) = envelope(http().post(format!("{}/api/status/created", s.url())).send().await.unwrap()).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body, json!({"success": true, "status": "Created", "msg": "made", "data": {"id": 7}}));
    let (status, _) = envelope(http().post(format!("{}/api/status/accepted", s.url())).send().await.unwrap()).await;
    assert_eq!(status, StatusCode::ACCEPTED);
}

#[tokio::test]
async fn success_msg_envelope_has_null_data() {
    let s = start_rest(config("rest", RecordingRouter::default())).await;
    let (status, body) = envelope(http().get(format!("{}/api/status/msg", s.url())).send().await.unwrap()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({"success": true, "status": "OK", "msg": "just a message", "data": null}));
}

#[tokio::test]
async fn error_envelopes_carry_their_status_code() {
    let s = start_rest(config("rest", RecordingRouter::default())).await;
    for (path, expected, word) in [
        ("bad-request", StatusCode::BAD_REQUEST, "BadRequest"),
        ("unauthorized", StatusCode::UNAUTHORIZED, "Unauthorized"),
        ("conflict", StatusCode::CONFLICT, "Conflict"),
        ("internal", StatusCode::INTERNAL_SERVER_ERROR, "InternalServerError"),
    ] {
        let (status, body) = envelope(http().get(format!("{}/api/fail/{}", s.url(), path)).send().await.unwrap()).await;
        assert_eq!(status, expected, "{}", path);
        assert_eq!(body["success"], json!(false), "{}", path);
        assert_eq!(body["status"], json!(word), "{}", path);
        assert!(body["msg"].is_string(), "{}", path);
        assert!(body.get("data").is_none(), "error envelope has no data key: {}", path);
    }
}

#[tokio::test]
async fn error_response_with_no_code_defaults_to_not_found() {
    let s = start_rest(config("rest", RecordingRouter::default())).await;
    let (status, body) = envelope(http().get(format!("{}/api/fail/default", s.url())).send().await.unwrap()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["status"], json!("NotFound"));
}

#[tokio::test]
async fn errors_are_json_on_every_method() {
    let s = start_rest(config("rest", RecordingRouter::default())).await;
    let c = http();
    for req in [
        c.post(format!("{}/api/fail/bad-request", s.url())),
        c.put(format!("{}/api/fail/bad-request", s.url())),
        c.delete(format!("{}/api/fail/bad-request", s.url())),
    ] {
        let resp = req.send().await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let ct = resp.headers()["content-type"].to_str().unwrap().to_string();
        assert!(ct.starts_with("application/json"), "{}", ct);
    }
}

// ---------------------------------------------------------------- other response kinds

#[tokio::test]
async fn binary_response_on_any_method() {
    let s = start_rest(config("rest", RecordingRouter::default())).await;
    for (req, method) in [
        (http().post(format!("{}/api/bin/post", s.url())).json(&json!({"a": 1})), "POST"),
        (http().get(format!("{}/api/bin/get?q=1", s.url())), "GET"),
    ] {
        let resp = req.send().await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(resp.headers()["content-type"], "application/octet-stream");
        assert_eq!(resp.bytes().await.unwrap(), format!("BIN:{}", method).as_bytes());
    }
}

#[tokio::test]
async fn binary_response_can_override_status() {
    let s = start_rest(config("rest", RecordingRouter::default())).await;
    let resp = http().post(format!("{}/api/bin-status/x", s.url())).send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::ACCEPTED);
    assert_eq!(resp.headers()["content-type"], "text/plain");
    assert_eq!(resp.text().await.unwrap(), "accepted");
}

#[tokio::test]
async fn stream_response_arrives_whole() {
    let s = start_rest(config("rest", RecordingRouter::default())).await;
    let resp = http().get(format!("{}/api/stream/x", s.url())).send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.headers()["content-type"], "text/plain");
    assert!(resp.headers().get("content-length").is_none(), "streamed, not buffered");
    assert_eq!(resp.text().await.unwrap(), "one,two,three");
}

#[tokio::test]
async fn sse_response_emits_events_then_ends() {
    let s = start_rest(config("rest", RecordingRouter::default())).await;
    let resp = http().get(format!("{}/api/sse/x", s.url())).send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(resp.headers()["content-type"].to_str().unwrap().starts_with("text/event-stream"));
    let text = resp.text().await.unwrap();
    // Events are blank-line separated; field order within one is not fixed.
    let blocks: Vec<&str> = text.split("\n\n").filter(|b| !b.trim().is_empty()).collect();
    assert_eq!(blocks.len(), 3, "{}", text);
    assert_eq!(blocks[0].trim(), "data: a");
    assert!(blocks[1].lines().any(|l| l == "event: tick") && blocks[1].lines().any(|l| l == "data: b"), "{}", blocks[1]);
    assert!(blocks[2].lines().any(|l| l == "event: obj") && blocks[2].lines().any(|l| l == "data: {\"n\":1}"), "{}", blocks[2]);
}

#[tokio::test]
async fn redirect_response_is_302_found() {
    let router = RecordingRouter::default();
    let s = start_rest(config("rest", router.clone())).await;
    let resp = http()
        .get(format!("{}/redirect/ok/cover-abc?size=lg", s.url()))
        .header("Client-Id", "conn-r")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FOUND);
    assert_eq!(resp.headers()["location"], "https://example.com/landed?from=cover-abc");

    let call = &router.calls_for("/redirect/ok/cover-abc")[0];
    assert_eq!(call.path, "/redirect/ok/cover-abc");
    assert_eq!(call.query, json!({"size": "lg"}));
    assert_eq!(call.data, json!({"size": "lg"}));
    assert_eq!(call.connection_id.as_deref(), Some("conn-r"));
}

#[tokio::test]
async fn redirect_to_unparseable_url_is_404_json() {
    let s = start_rest(config("rest", RecordingRouter::default())).await;
    let (status, body) = envelope(http().get(format!("{}/redirect/bad/x", s.url())).send().await.unwrap()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["status"], json!("NotFound"));
}

// ---------------------------------------------------------------- not ours

#[tokio::test]
async fn unsupported_method_under_api_is_405_json() {
    let router = RecordingRouter::default();
    let s = start_rest(config("rest", router.clone())).await;
    let (status, body) = envelope(http().patch(format!("{}/api/echo/patch", s.url())).send().await.unwrap()).await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(body["status"], json!("MethodNotAllowed"));
    assert!(router.calls_for("/api/echo/patch").is_empty(), "router never saw it");
}

#[tokio::test]
async fn unknown_path_without_client_dir_is_404_json() {
    let s = start_rest(config("rest", RecordingRouter::default())).await;
    let (status, body) = envelope(http().get(format!("{}/anything/at/all", s.url())).send().await.unwrap()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["success"], json!(false));
}

// ---------------------------------------------------------------- cors

const ALLOWED_ORIGIN: &str = "http://allowed.test";

fn restricted() -> cnctd_server::ServerConfig<RecordingRouter> {
    let mut cfg = config("rest", RecordingRouter::default());
    cfg.allowed_origins = Some(vec![ALLOWED_ORIGIN.to_string()]);
    cfg
}

#[tokio::test]
async fn preflight_from_allowed_origin_succeeds_including_client_id() {
    let s = start_rest(restricted()).await;
    let resp = http()
        .request(reqwest::Method::OPTIONS, format!("{}/api/echo/cors", s.url()))
        .header("Origin", ALLOWED_ORIGIN)
        .header("Access-Control-Request-Method", "POST")
        .header("Access-Control-Request-Headers", "authorization, content-type, client-id")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.headers()["access-control-allow-origin"], ALLOWED_ORIGIN);
    let methods = resp.headers()["access-control-allow-methods"].to_str().unwrap().to_uppercase();
    for m in ["GET", "POST", "PUT", "DELETE", "OPTIONS"] {
        assert!(methods.contains(m), "{} missing from {}", m, methods);
    }
    let allowed = resp.headers()["access-control-allow-headers"].to_str().unwrap().to_lowercase();
    for h in ["authorization", "content-type", "client-id"] {
        assert!(allowed.contains(h), "{} missing from {}", h, allowed);
    }
}

#[tokio::test]
async fn preflight_from_other_origin_is_not_allowed() {
    let s = start_rest(restricted()).await;
    let resp = http()
        .request(reqwest::Method::OPTIONS, format!("{}/api/echo/cors2", s.url()))
        .header("Origin", "http://evil.test")
        .header("Access-Control-Request-Method", "POST")
        .send()
        .await
        .unwrap();
    assert!(resp.headers().get("access-control-allow-origin").is_none(), "no allow-origin for a stranger");
}

#[tokio::test]
async fn simple_request_from_allowed_origin_gets_allow_origin_header() {
    let s = start_rest(restricted()).await;
    let resp = http().get(format!("{}/api/echo/cors-simple", s.url())).header("Origin", ALLOWED_ORIGIN).send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.headers()["access-control-allow-origin"], ALLOWED_ORIGIN);
}

#[tokio::test]
async fn unconfigured_origins_allow_any() {
    let s = start_rest(config("rest", RecordingRouter::default())).await;
    let resp = http().get(format!("{}/api/echo/cors-any", s.url())).header("Origin", "http://whoever.test").send().await.unwrap();
    assert_eq!(resp.headers()["access-control-allow-origin"], "*");
}

// ---------------------------------------------------------------- lifecycle

#[tokio::test]
async fn graceful_shutdown_stops_accepting() {
    let s = start_rest(config("rest", RecordingRouter::default())).await;
    let url = s.url();
    assert_eq!(http().get(format!("{}/api/status/msg", url)).send().await.unwrap().status(), StatusCode::OK);
    s.shutdown().await.unwrap();
    assert!(http().get(format!("{}/api/status/msg", url)).send().await.is_err(), "nothing listens after shutdown");
}

#[tokio::test]
async fn several_servers_in_one_process() {
    let a = start_rest(config("a", RecordingRouter::default())).await;
    let b = start_rest(config("b", RecordingRouter::default())).await;
    assert_ne!(a.port(), b.port());
    assert_eq!(http().get(format!("{}/api/status/msg", a.url())).send().await.unwrap().status(), StatusCode::OK);
    assert_eq!(http().get(format!("{}/api/status/msg", b.url())).send().await.unwrap().status(), StatusCode::OK);
}
