//! REST behavior of cnctd_server 0.6.x, as cnctd.world depends on it.
//!
//! Server here: no client dir, no socket, no heartbeat, CORS restricted to
//! one origin. See tests/common/mod.rs for the recording router.

mod common;

use std::sync::OnceLock;

use cnctd_server::server::{CnctdServer, ServerConfig};
use common::{base, http, RecordingRouter, RecordingSocketRouter, TestMsg};
use reqwest::StatusCode;
use serde_json::{json, Value};

const PORT: u16 = 48110;
const ALLOWED_ORIGIN: &str = "http://allowed.test";

static ROUTER: OnceLock<RecordingRouter> = OnceLock::new();

fn server() -> (&'static RecordingRouter, String) {
    let router = ROUTER.get_or_init(|| {
        let router = RecordingRouter::default();
        let for_server = router.clone();
        common::spawn_server(PORT, move || async move {
            let config = ServerConfig::new(
                "rest-test",
                &PORT.to_string(),
                None,
                for_server,
                None,
                Some(vec![ALLOWED_ORIGIN.to_string()]),
                None,
            );
            CnctdServer::start::<RecordingRouter, TestMsg, Value, RecordingSocketRouter>(config, None).await
        });
        router
    });
    (router, base(PORT))
}

async fn envelope(resp: reqwest::Response) -> (StatusCode, Value) {
    let status = resp.status();
    let body: Value = resp.json().await.expect("JSON envelope");
    (status, body)
}

// ---------------------------------------------------------------- envelope

#[tokio::test]
async fn post_json_body_reaches_router_with_full_context() {
    let (router, base) = server();
    let resp = http()
        .post(format!("{}/api/echo/post-ctx", base))
        .header("Authorization", "tok-123")
        .header("Client-Id", "conn-abc")
        .json(&json!({"name": "dot", "n": 2}))
        .send()
        .await
        .unwrap();
    let (status, body) = envelope(resp).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["success"], json!(true));
    assert_eq!(body["status"], json!("OK"));
    assert_eq!(body["data"]["method"], json!("POST"));
    // The path handed to the router is the FULL path, prefix included.
    assert_eq!(body["data"]["path"], json!("/api/echo/post-ctx"));
    assert_eq!(body["data"]["data"], json!({"name": "dot", "n": 2}));
    assert_eq!(body["data"]["auth_token"], json!("tok-123"));
    assert_eq!(body["data"]["connection_id"], json!("conn-abc"));
    // No X-Forwarded-For: falls back to the peer address.
    assert_eq!(body["data"]["ip_address"], json!("127.0.0.1"));

    let calls = router.calls_for("/api/echo/post-ctx");
    let stages: Vec<_> = calls.iter().map(|c| c.stage).collect();
    assert_eq!(stages, vec!["raw", "binary", "route"], "POST dispatch order");
}

#[tokio::test]
async fn get_query_string_becomes_data_of_strings() {
    let (_, base) = server();
    let resp = http()
        .get(format!("{}/api/echo/get-q?a=1&b=two&c=", base))
        .send()
        .await
        .unwrap();
    let (status, body) = envelope(resp).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["method"], json!("GET"));
    // Query values are strings, never coerced.
    assert_eq!(body["data"]["data"], json!({"a": "1", "b": "two", "c": ""}));
    assert_eq!(body["data"]["auth_token"], Value::Null);
    assert_eq!(body["data"]["connection_id"], Value::Null);
}

#[tokio::test]
async fn get_without_query_gives_empty_object() {
    let (_, base) = server();
    let (_, body) = envelope(http().get(format!("{}/api/echo/get-empty", base)).send().await.unwrap()).await;
    assert_eq!(body["data"]["data"], json!({}));
}

#[tokio::test]
async fn delete_uses_query_like_get() {
    let (_, base) = server();
    let (status, body) = envelope(
        http().delete(format!("{}/api/echo/del?id=9", base)).send().await.unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["method"], json!("DELETE"));
    assert_eq!(body["data"]["data"], json!({"id": "9"}));
}

#[tokio::test]
async fn put_json_body_reaches_router() {
    let (router, base) = server();
    let (status, body) = envelope(
        http()
            .put(format!("{}/api/echo/put-body", base))
            .json(&json!({"x": [1, 2]}))
            .send()
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["method"], json!("PUT"));
    assert_eq!(body["data"]["data"], json!({"x": [1, 2]}));
    // PUT goes straight to `route`: no raw-body or binary stage.
    let stages: Vec<_> = router.calls_for("/api/echo/put-body").iter().map(|c| c.stage).collect();
    assert_eq!(stages, vec!["route"]);
}

#[tokio::test]
async fn post_with_empty_body_and_no_content_type_gives_empty_object() {
    let (router, base) = server();
    let (status, body) = envelope(http().post(format!("{}/api/echo/post-empty", base)).send().await.unwrap()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["data"], json!({}));
    let raw = router.calls_for("/api/echo/post-empty");
    assert_eq!(raw[0].stage, "raw");
    assert_eq!(raw[0].raw_body.as_deref(), Some(&b""[..]));
}

#[tokio::test]
async fn post_with_unparseable_body_is_lenient() {
    // Sonos-style webhooks post bodies that are not JSON; the framework must
    // still route, with data = {} and the raw bytes intact for `raw` handlers.
    let (router, base) = server();
    let (status, body) = envelope(
        http()
            .post(format!("{}/api/echo/post-invalid", base))
            .header("Content-Type", "text/plain")
            .body("this is <not> json")
            .send()
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["data"], json!({}));
    let raw = router.calls_for("/api/echo/post-invalid");
    assert_eq!(raw[0].raw_body.as_deref(), Some(&b"this is <not> json"[..]));
}

#[tokio::test]
async fn success_code_sets_http_status() {
    let (_, base) = server();
    let (status, body) = envelope(http().post(format!("{}/api/status/created", base)).send().await.unwrap()).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body, json!({"success": true, "status": "Created", "msg": "made", "data": {"id": 7}}));
}

#[tokio::test]
async fn success_msg_envelope_has_null_data() {
    let (_, base) = server();
    let (status, body) = envelope(http().get(format!("{}/api/status/msg", base)).send().await.unwrap()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({"success": true, "status": "OK", "msg": "just a message", "data": null}));
}

#[tokio::test]
async fn error_envelopes_carry_their_status_code() {
    let (_, base) = server();
    for (path, expected, word) in [
        ("bad-request", StatusCode::BAD_REQUEST, "BadRequest"),
        ("unauthorized", StatusCode::UNAUTHORIZED, "Unauthorized"),
        ("internal", StatusCode::INTERNAL_SERVER_ERROR, "InternalServerError"),
    ] {
        let (status, body) = envelope(http().get(format!("{}/api/fail/{}", base, path)).send().await.unwrap()).await;
        assert_eq!(status, expected, "{}", path);
        assert_eq!(body["success"], json!(false), "{}", path);
        assert_eq!(body["status"], json!(word), "{}", path);
        assert!(body["msg"].is_string(), "{}", path);
        assert!(body.get("data").is_none(), "error envelope has no data key: {}", path);
    }
}

#[tokio::test]
async fn error_response_with_no_code_defaults_to_not_found() {
    let (_, base) = server();
    let (status, body) = envelope(http().get(format!("{}/api/fail/default", base)).send().await.unwrap()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["status"], json!("NotFound"));
}

#[tokio::test]
async fn errors_are_json_on_every_method() {
    let (_, base) = server();
    let c = http();
    for req in [
        c.post(format!("{}/api/fail/bad-request", base)),
        c.put(format!("{}/api/fail/bad-request", base)),
        c.delete(format!("{}/api/fail/bad-request", base)),
    ] {
        let resp = req.send().await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let ct = resp.headers()["content-type"].to_str().unwrap().to_string();
        assert!(ct.starts_with("application/json"), "{}", ct);
    }
}

// ---------------------------------------------------------------- ip address

#[tokio::test]
async fn x_forwarded_for_wins_over_peer_address_and_is_passed_whole() {
    // NOTE for 1.0: the whole header value is passed through, not the first
    // hop. cnctd.world stores this as the client ip. Decide whether 1.0
    // should take the first address instead.
    let (_, base) = server();
    let (_, body) = envelope(
        http()
            .get(format!("{}/api/echo/xff", base))
            .header("X-Forwarded-For", "203.0.113.9, 10.0.0.1")
            .send()
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(body["data"]["ip_address"], json!("203.0.113.9, 10.0.0.1"));
}

// ---------------------------------------------------------------- binary

#[tokio::test]
async fn route_binary_short_circuits_post_and_get() {
    let (router, base) = server();
    for (req, method) in [
        (http().post(format!("{}/api/bin/post", base)).json(&json!({"a": 1})), "POST"),
        (http().get(format!("{}/api/bin/get?q=1", base)), "GET"),
    ] {
        let resp = req.send().await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(resp.headers()["content-type"], "application/octet-stream");
        assert_eq!(resp.bytes().await.unwrap(), format!("BIN:{}", method).as_bytes());
    }
    // Neither request reached `route`.
    assert!(router.calls_for("/api/bin/").iter().all(|c| c.stage != "route"));
    // GET has no raw-body stage, only binary then (not reached) route.
    let get_stages: Vec<_> = router.calls_for("/api/bin/get").iter().map(|c| c.stage).collect();
    assert_eq!(get_stages, vec!["binary"]);
}

#[tokio::test]
async fn binary_response_can_override_status() {
    let (_, base) = server();
    let resp = http().post(format!("{}/api/bin-status/x", base)).send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::ACCEPTED);
    assert_eq!(resp.headers()["content-type"], "text/plain");
    assert_eq!(resp.text().await.unwrap(), "accepted");
}

#[tokio::test]
async fn route_binary_error_is_a_json_error_envelope() {
    let (_, base) = server();
    for req in [http().post(format!("{}/api/bin-fail/p", base)), http().get(format!("{}/api/bin-fail/g", base))] {
        let resp = req.send().await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        assert!(resp.headers()["content-type"].to_str().unwrap().starts_with("application/json"));
        let body: Value = resp.json().await.unwrap();
        assert_eq!(body, json!({"success": false, "status": "Forbidden", "msg": "no binary for you"}));
    }
}

// ---------------------------------------------------------------- raw body

#[tokio::test]
async fn raw_body_handler_gets_exact_bytes_parsed_value_and_lowercase_headers() {
    let (router, base) = server();
    // Spacing that re-serialization would not reproduce: proves byte-exactness.
    let wire = "{ \"a\" :  1 ,\n \"b\": \"two\" }";
    let resp = http()
        .post(format!("{}/api/raw/sig", base))
        .header("Content-Type", "application/json")
        .header("Stripe-Signature", "t=1,v1=abc")
        .header("Authorization", "tok")
        .body(wire)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.headers()["content-type"], "application/json");
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["raw"], json!(wire));
    assert_eq!(body["data"], json!({"a": 1, "b": "two"}));
    assert_eq!(body["headers"]["stripe-signature"], json!("t=1,v1=abc"));
    assert_eq!(body["headers"]["content-type"], json!("application/json"));
    assert_eq!(body["headers"]["authorization"], json!("tok"));

    let call = &router.calls_for("/api/raw/sig")[0];
    assert_eq!(call.stage, "raw");
    assert_eq!(call.raw_body.as_deref(), Some(wire.as_bytes()));
    assert_eq!(call.auth_token.as_deref(), Some("tok"));
    assert!(call.headers.as_ref().unwrap().keys().all(|k| k == &k.to_lowercase()));
}

#[tokio::test]
async fn raw_body_error_is_a_json_error_envelope() {
    let (_, base) = server();
    let resp = http().post(format!("{}/api/raw-fail/x", base)).body("{}").send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body, json!({"success": false, "status": "Unauthorized", "msg": "bad signature"}));
}

#[tokio::test]
async fn post_dispatch_order_is_raw_then_binary_then_route() {
    let (router, base) = server();
    let c = http();

    c.post(format!("{}/api/order/all", base)).send().await.unwrap();
    let stages: Vec<_> = router.calls_for("/api/order/all").iter().map(|c| c.stage).collect();
    assert_eq!(stages, vec!["raw"], "raw answered, nothing else ran");

    c.post(format!("{}/api/order/skip-raw", base)).send().await.unwrap();
    let stages: Vec<_> = router.calls_for("/api/order/skip-raw").iter().map(|c| c.stage).collect();
    assert_eq!(stages, vec!["raw", "binary"], "raw passed, binary answered");

    let resp = c.post(format!("{}/api/order/json", base)).send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let stages: Vec<_> = router.calls_for("/api/order/json").iter().map(|c| c.stage).collect();
    assert_eq!(stages, vec!["raw", "binary", "route"], "both passed, route answered");
}

// ---------------------------------------------------------------- redirect

#[tokio::test]
async fn redirect_route_returns_302_found_to_the_routers_url() {
    let (router, base) = server();
    let resp = http()
        .get(format!("{}/redirect/ok/cover-abc?size=lg", base))
        .header("Client-Id", "conn-r")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FOUND);
    assert_eq!(resp.headers()["location"], "https://example.com/landed?from=cover-abc");

    let call = &router.calls_for("/redirect/ok/cover-abc")[0];
    assert_eq!(call.stage, "redirect");
    assert_eq!(call.path, "/redirect/ok/cover-abc");
    assert_eq!(call.data, json!({"size": "lg"}));
    assert_eq!(call.connection_id.as_deref(), Some("conn-r"));
}

#[tokio::test]
async fn redirect_to_unparseable_url_falls_through_to_the_app_shell() {
    // KNOWN QUIRK, pinned: the handler rejects with not_found, but a warp
    // rejection does not end the request — it falls through the `.or()`
    // chain to the SPA catch-all, which answers 200 HTML (index.html when a
    // client dir is set). 1.0 should answer 404 here.
    let (_, base) = server();
    let resp = http().get(format!("{}/redirect/bad/x", base)).send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(resp.headers()["content-type"].to_str().unwrap().starts_with("text/html"));
}

// ---------------------------------------------------------------- cors

#[tokio::test]
async fn preflight_from_allowed_origin_succeeds() {
    let (_, base) = server();
    let resp = http()
        .request(reqwest::Method::OPTIONS, format!("{}/api/echo/cors", base))
        .header("Origin", ALLOWED_ORIGIN)
        .header("Access-Control-Request-Method", "POST")
        .header("Access-Control-Request-Headers", "authorization, content-type")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.headers()["access-control-allow-origin"], ALLOWED_ORIGIN);
    let methods = resp.headers()["access-control-allow-methods"].to_str().unwrap().to_uppercase();
    for m in ["GET", "POST", "PUT", "DELETE", "OPTIONS"] {
        assert!(methods.contains(m), "{} missing from {}", m, methods);
    }
}

#[tokio::test]
async fn preflight_from_other_origin_is_forbidden() {
    let (_, base) = server();
    let resp = http()
        .request(reqwest::Method::OPTIONS, format!("{}/api/echo/cors2", base))
        .header("Origin", "http://other.test")
        .header("Access-Control-Request-Method", "POST")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn preflight_requesting_client_id_header_is_refused() {
    // KNOWN GAP, pinned on purpose: the server READS `Client-Id` but the CORS
    // allow-list does not include it, so a cross-origin browser client that
    // sends it fails preflight. cnctd.world only works because its web client
    // is same-origin and native goes through CapacitorHttp. 1.0 must allow it.
    let (_, base) = server();
    let resp = http()
        .request(reqwest::Method::OPTIONS, format!("{}/api/echo/cors3", base))
        .header("Origin", ALLOWED_ORIGIN)
        .header("Access-Control-Request-Method", "POST")
        .header("Access-Control-Request-Headers", "client-id")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn simple_request_from_allowed_origin_gets_allow_origin_header() {
    let (_, base) = server();
    let resp = http()
        .get(format!("{}/api/echo/cors-simple", base))
        .header("Origin", ALLOWED_ORIGIN)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.headers()["access-control-allow-origin"], ALLOWED_ORIGIN);
}

// ---------------------------------------------------------------- fallthrough

#[tokio::test]
async fn unmatched_method_under_api_falls_through_to_the_app_shell() {
    // KNOWN QUIRK, pinned: with no client dir, anything the REST filters do
    // not match (here PATCH) lands on the SPA placeholder and returns 200
    // HTML. 1.0 should answer 405 / 404 under the API prefix.
    let (router, base) = server();
    let resp = http().patch(format!("{}/api/echo/patch", base)).send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(resp.headers()["content-type"].to_str().unwrap().starts_with("text/html"));
    assert!(router.calls_for("/api/echo/patch").is_empty(), "router never saw it");
}

#[tokio::test]
async fn non_api_path_without_client_dir_is_placeholder_html() {
    let (_, base) = server();
    let resp = http().get(format!("{}/anything/at/all", base)).send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(resp.headers()["content-type"].to_str().unwrap().starts_with("text/html"));
}
