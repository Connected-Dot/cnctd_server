//! Pure helpers: path splitting, auth tokens, envelope constructors, macros.
//! No server needed.

use std::collections::HashMap;

use cnctd_server::{
    auth::CnctdAuth,
    bad_request, conflict, created, forbidden, internal_server_error, not_found,
    router::{
        error::{ErrorCode, ErrorResponse},
        message::Message,
        response::{BinaryResponse, SuccessCode, SuccessResponse},
    },
    server::CnctdServer,
    socket::connection::ConnectionFormat,
    success_data, success_msg, unauthorized, Bytes, HttpMethod, Request,
};
use serde_json::json;

#[test]
fn path_splits_into_resource_and_operation() {
    assert_eq!(CnctdServer::path_to_resource_and_operation("/api/dots/create"), ("dots".into(), Some("create".into())));
    assert_eq!(CnctdServer::path_to_resource_and_operation("/api/dots"), ("dots".into(), None));
    assert_eq!(CnctdServer::path_to_resource_and_operation("/api/dots/"), ("dots".into(), Some("".into())));
    // Only the first two segments matter; the rest is dropped.
    assert_eq!(CnctdServer::path_to_resource_and_operation("/api/a/b/c"), ("a".into(), Some("b".into())));
    // Without the /api/ prefix the leading slash lands in the resource (as it always has).
    assert_eq!(CnctdServer::path_to_resource_and_operation("/dots/x"), ("".into(), Some("dots".into())));
}

fn request(path: &str, prefix: &str, auth: Option<&str>) -> Request {
    Request {
        method: HttpMethod::GET,
        path: path.into(),
        prefix: prefix.into(),
        query: json!({}),
        body: Bytes::new(),
        data: json!({}),
        headers: HashMap::new(),
        auth_token: auth.map(str::to_string),
        connection_id: None,
        ip_address: None,
    }
}

#[test]
fn request_knows_its_own_prefix_and_bearer() {
    let r = request("/v2/dots/create", "v2", Some("Bearer  abc"));
    assert_eq!(r.resource_and_operation(), ("dots".into(), Some("create".into())));
    assert_eq!(r.bearer(), Some("abc"));
    let r = request("/api/dots", "api", Some("rawtoken"));
    assert_eq!(r.resource_and_operation(), ("dots".into(), None));
    assert_eq!(r.bearer(), Some("rawtoken"), "no prefix: passed through");
    assert_eq!(request("/api/x", "api", Some("BEARER t")).bearer(), Some("t"), "case-insensitive");
    assert_eq!(request("/api/x", "api", None).bearer(), None);
}

#[test]
fn jwt_round_trips_the_subject() {
    let secret = b"a-secret".to_vec();
    let token = CnctdAuth::get_jwt(secret.clone(), "user-1").unwrap();
    assert_eq!(CnctdAuth::verify_auth_token(secret.clone(), &token).unwrap(), "user-1");
    assert!(CnctdAuth::verify_auth_token(b"other".to_vec(), &token).is_err());
    assert!(CnctdAuth::verify_auth_token(secret, "not.a.token").is_err());
}

#[test]
fn envelope_defaults() {
    let ok = SuccessResponse::new(None, None, None);
    assert!(ok.success);
    assert_eq!(ok.status, SuccessCode::OK);
    assert_eq!(serde_json::to_value(&ok).unwrap(), json!({"success": true, "status": "OK", "msg": null, "data": null}));

    let err = ErrorResponse::new(None, None);
    assert!(!err.success);
    assert_eq!(err.status, ErrorCode::NotFound);
    assert_eq!(serde_json::to_value(&err).unwrap(), json!({"success": false, "status": "NotFound", "msg": null}));
    assert_eq!(err.to_string(), ": NotFound");
}

#[test]
fn error_code_u16_mapping() {
    for (code, expected) in [
        (400, ErrorCode::BadRequest),
        (401, ErrorCode::Unauthorized),
        (403, ErrorCode::Forbidden),
        (404, ErrorCode::NotFound),
        (405, ErrorCode::MethodNotAllowed),
        (408, ErrorCode::RequestTimeout),
        (409, ErrorCode::Conflict),
        (413, ErrorCode::PayloadTooLarge),
        (422, ErrorCode::UnprocessableEntity),
        (429, ErrorCode::TooManyRequests),
        (500, ErrorCode::InternalServerError),
        (502, ErrorCode::BadGateway),
        (503, ErrorCode::ServiceUnavailable),
        (504, ErrorCode::GatewayTimeout),
        (418, ErrorCode::InternalServerError), // anything unknown
    ] {
        assert_eq!(ErrorCode::from_u16(code), expected, "{}", code);
        assert_eq!(expected.status_code().as_u16(), if code == 418 { 500 } else { code });
    }
    assert_eq!(SuccessCode::Created.status_code().as_u16(), 201);
}

/// The macros expand to bare `SuccessResponse` / `ErrorResponse` /
/// `SuccessCode` / `ErrorCode` names, so callers must have those in scope.
/// That is the contract cnctd.world's 2,900 call sites rely on.
#[test]
fn macros_build_envelopes() {
    let r = success_data!(json!({"a": 1}));
    assert_eq!((r.status, r.msg.clone(), r.data.clone()), (SuccessCode::OK, None, Some(json!({"a": 1}))));
    let r = success_msg!("done");
    assert_eq!((r.status, r.msg.clone(), r.data.clone()), (SuccessCode::OK, Some("done".into()), None));
    let r = created!("made", json!(1));
    assert_eq!((r.status, r.msg.clone(), r.data.clone()), (SuccessCode::Created, Some("made".into()), Some(json!(1))));

    let e = bad_request!("x");
    assert_eq!((e.status, e.msg.clone()), (ErrorCode::BadRequest, Some("x".into())));
    let e = unauthorized!(format!("u{}", 1));
    assert_eq!((e.status, e.msg.clone()), (ErrorCode::Unauthorized, Some("u1".into())));
    assert_eq!(forbidden!("f").status, ErrorCode::Forbidden);
    assert_eq!(conflict!("c").status, ErrorCode::Conflict);
    assert_eq!(internal_server_error!("i").status, ErrorCode::InternalServerError);
    let e = not_found!("n");
    assert_eq!((e.status, e.msg.clone()), (ErrorCode::NotFound, Some("n".into())));

    // An envelope converts into the route's return type.
    let as_response: cnctd_server::Response = success_msg!("ok").into();
    assert!(matches!(as_response, cnctd_server::Response::Json(_)));
}

#[test]
fn binary_response_builder() {
    let b = BinaryResponse::octet_stream(vec![1]);
    assert_eq!((b.content_type.as_str(), b.status), ("application/octet-stream", None));
    let b = BinaryResponse::new(vec![], "text/plain").with_status(202);
    assert_eq!(b.status, Some(202));
    let b = BinaryResponse::json(&json!({"k": "v"}));
    assert_eq!((b.content_type.as_str(), b.data.as_slice()), ("application/json", &b"{\"k\":\"v\"}"[..]));
}

#[test]
fn connection_format_parsing_and_message_shape() {
    assert_eq!(ConnectionFormat::from_str_opt(Some("binary")), ConnectionFormat::Binary);
    assert_eq!(ConnectionFormat::from_str_opt(Some("json")), ConnectionFormat::Json);
    assert_eq!(ConnectionFormat::from_str_opt(Some("BINARY")), ConnectionFormat::Json, "case-sensitive");
    assert_eq!(ConnectionFormat::from_str_opt(None), ConnectionFormat::Json);
    assert_eq!(serde_json::to_value(ConnectionFormat::Binary).unwrap(), json!("binary"));

    let m = Message::new("chan", "instr", Some(json!(1)));
    assert_eq!(serde_json::to_value(&m).unwrap(), json!({"channel": "chan", "instruction": "instr", "data": 1}));
}
