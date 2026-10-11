//! Static-file / single-page-app serving with per-path `<head>` injection.

mod common;

use std::{
    fs,
    sync::{Arc, Mutex},
};

use cnctd_server::RunningServer;
use common::{config, http, start_rest, RecordingRouter};
use reqwest::StatusCode;
use serde_json::{json, Value};
use tempfile::TempDir;

const INDEX: &str = "<html>\n<head>\n<meta charset=\"UTF-8\">\n<title>Old</title>\n<meta property=\"og:title\" content=\"Old\" />\n<meta name=\"twitter:card\" content=\"summary\" />\n</head>\n<body>app shell</body>\n</html>";
const APP_JS: &str = "console.log('app');\n";

struct Spa {
    server: RunningServer,
    dir: TempDir,
    meta_paths: Arc<Mutex<Vec<String>>>,
}

async fn spa() -> Spa {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("index.html"), INDEX).unwrap();
    fs::create_dir(dir.path().join("assets")).unwrap();
    fs::write(dir.path().join("assets/app.js"), APP_JS).unwrap();

    let meta_paths: Arc<Mutex<Vec<String>>> = Default::default();
    let seen = meta_paths.clone();
    let mut cfg = config("spa-test", RecordingRouter::default());
    cfg.client_dir = Some(dir.path().to_string_lossy().to_string());
    let cfg = cfg.with_page_meta(Arc::new(move |path: String| {
        seen.lock().unwrap().push(path.clone());
        Box::pin(async move {
            // Share pages get tags; a real asset path asking for tags proves
            // files win over the page route.
            if let Some(code) = path.strip_prefix("/s/") {
                Some(format!(
                    "<title>Share {}</title>\n<meta property=\"og:title\" content=\"Share {}\" />\n",
                    code, code
                ))
            } else if path == "/assets/app.js" {
                Some("<title>should never be used</title>\n".to_string())
            } else {
                None
            }
        })
    }));
    let server = start_rest(cfg).await;
    Spa { server, dir, meta_paths }
}

#[tokio::test]
async fn static_file_is_served_as_is_and_wins_over_page_meta() {
    let s = spa().await;
    let resp = http().get(format!("{}/assets/app.js", s.server.url())).send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(resp.headers()["content-type"].to_str().unwrap().contains("javascript"));
    assert_eq!(resp.text().await.unwrap(), APP_JS);
    assert!(!s.meta_paths.lock().unwrap().iter().any(|p| p == "/assets/app.js"), "meta never consulted for a real file");
}

#[tokio::test]
async fn root_serves_index_unchanged() {
    let s = spa().await;
    let resp = http().get(format!("{}/", s.server.url())).send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.text().await.unwrap();
    assert!(body.contains("<title>Old</title>"));
    assert!(body.contains("og:title"));
}

#[tokio::test]
async fn unknown_route_serves_index_with_no_cache_when_meta_is_none() {
    let s = spa().await;
    let resp = http().get(format!("{}/some/deep/route?x=1", s.server.url())).send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(resp.headers()["content-type"].to_str().unwrap().starts_with("text/html"));
    assert_eq!(resp.headers()["cache-control"], "no-cache");
    assert_eq!(resp.text().await.unwrap(), INDEX, "served verbatim when meta returns None");
    assert!(s.meta_paths.lock().unwrap().iter().any(|p| p == "/some/deep/route"), "meta sees the path without the query");
}

#[tokio::test]
async fn page_meta_replaces_title_and_social_tags_at_top_of_head() {
    let s = spa().await;
    let resp = http().get(format!("{}/s/abc", s.server.url())).send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.headers()["cache-control"], "no-cache");
    let body = resp.text().await.unwrap();
    assert!(body.contains("<title>Share abc</title>"));
    assert!(body.contains("content=\"Share abc\""));
    assert!(!body.contains("Old"), "document's own title/og tags removed:\n{}", body);
    assert!(!body.contains("twitter:card"), "twitter tags removed");
    assert!(body.contains("<meta charset=\"UTF-8\">"), "other head lines kept");
    assert!(body.find("<title>Share abc").unwrap() < body.find("charset").unwrap(), "injected first");
    assert!(body.contains("app shell"));
}

#[tokio::test]
async fn index_is_read_from_disk_on_every_page_request() {
    let s = spa().await;
    let before = http().get(format!("{}/live/check", s.server.url())).send().await.unwrap().text().await.unwrap();
    assert!(!before.contains("REBUILT"));

    fs::write(s.dir.path().join("index.html"), INDEX.replace("app shell", "app shell REBUILT")).unwrap();

    let after = http().get(format!("{}/live/check", s.server.url())).send().await.unwrap().text().await.unwrap();
    assert!(after.contains("REBUILT"), "new client build picked up without restart");
}

#[tokio::test]
async fn api_routes_still_work_alongside_the_spa() {
    let s = spa().await;
    let resp = http().get(format!("{}/api/echo/spa?x=1", s.server.url())).send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["data"]["path"], json!("/api/echo/spa"));
}

#[tokio::test]
async fn non_get_on_a_page_path_is_405_json() {
    let s = spa().await;
    let resp = http().post(format!("{}/some/page", s.server.url())).send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["status"], json!("MethodNotAllowed"));
}

#[tokio::test]
async fn missing_index_is_404_json() {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = config("spa-empty", RecordingRouter::default());
    cfg.client_dir = Some(dir.path().to_string_lossy().to_string());
    let server = start_rest(cfg).await;
    let resp = http().get(format!("{}/anything", server.url())).send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["success"], json!(false));
}
