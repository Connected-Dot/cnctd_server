//! Static-file / single-page-app serving with per-path `<head>` injection
//! (ServerConfig::with_page_meta), as cnctd.world uses for share links.

mod common;

use std::{
    fs,
    sync::{Arc, Mutex, OnceLock},
};

use cnctd_server::server::{CnctdServer, ServerConfig};
use common::{base, http, RecordingRouter, RecordingSocketRouter, TestMsg};
use reqwest::StatusCode;
use serde_json::Value;
use tempfile::TempDir;

const PORT: u16 = 48130;

const INDEX: &str = "<html>\n<head>\n<meta charset=\"UTF-8\">\n<title>Old</title>\n<meta property=\"og:title\" content=\"Old\" />\n<meta name=\"twitter:card\" content=\"summary\" />\n</head>\n<body>app shell</body>\n</html>";
const APP_JS: &str = "console.log('app');\n";

struct Harness {
    dir: TempDir,
    meta_paths: Arc<Mutex<Vec<String>>>,
}

static HARNESS: OnceLock<Harness> = OnceLock::new();

fn server() -> (&'static Harness, String) {
    let h = HARNESS.get_or_init(|| {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("index.html"), INDEX).unwrap();
        fs::create_dir(dir.path().join("assets")).unwrap();
        fs::write(dir.path().join("assets/app.js"), APP_JS).unwrap();

        let meta_paths: Arc<Mutex<Vec<String>>> = Default::default();
        let seen = meta_paths.clone();
        let client_dir = dir.path().to_string_lossy().to_string();
        common::spawn_server(PORT, move || async move {
            let config = ServerConfig::new("spa-test", &PORT.to_string(), Some(client_dir), RecordingRouter::default(), None, None, None)
                .with_page_meta(Arc::new(move |path: String| {
                    seen.lock().unwrap().push(path.clone());
                    Box::pin(async move {
                        // Share pages get tags; a real asset path asking for
                        // tags proves files win over the page route.
                        if let Some(code) = path.strip_prefix("/s/") {
                            Some(format!("<title>Share {}</title>\n<meta property=\"og:title\" content=\"Share {}\" />\n", code, code))
                        } else if path == "/assets/app.js" {
                            Some("<title>should never be used</title>\n".to_string())
                        } else {
                            None
                        }
                    })
                }));
            CnctdServer::start::<RecordingRouter, TestMsg, Value, RecordingSocketRouter>(config, None).await
        });
        Harness { dir, meta_paths }
    });
    (h, base(PORT))
}

#[tokio::test]
async fn static_file_is_served_as_is_and_wins_over_page_meta() {
    let (h, base) = server();
    let resp = http().get(format!("{}/assets/app.js", base)).send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(resp.headers()["content-type"].to_str().unwrap().contains("javascript"));
    assert_eq!(resp.text().await.unwrap(), APP_JS);
    assert!(!h.meta_paths.lock().unwrap().iter().any(|p| p == "/assets/app.js"), "meta never consulted for a real file");
}

#[tokio::test]
async fn root_serves_index_unchanged() {
    let (_, base) = server();
    let resp = http().get(format!("{}/", base)).send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.text().await.unwrap();
    assert!(body.contains("<title>Old</title>"));
    assert!(body.contains("og:title"));
}

#[tokio::test]
async fn unknown_route_serves_index_with_no_cache_when_meta_is_none() {
    let (h, base) = server();
    let resp = http().get(format!("{}/some/deep/route?x=1", base)).send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(resp.headers()["content-type"].to_str().unwrap().starts_with("text/html"));
    assert_eq!(resp.headers()["cache-control"], "no-cache");
    let body = resp.text().await.unwrap();
    // Not compared byte-for-byte: the live-reload test rewrites the body text
    // concurrently. The head is what matters here — untouched.
    assert!(body.starts_with("<html>\n<head>\n<meta charset=\"UTF-8\">\n<title>Old</title>"), "{}", body);
    assert!(body.contains("og:title") && body.contains("twitter:card"), "head served verbatim when meta returns None");
    // The meta fn sees the path without the query string.
    assert!(h.meta_paths.lock().unwrap().iter().any(|p| p == "/some/deep/route"));
}

#[tokio::test]
async fn page_meta_replaces_title_and_social_tags_at_top_of_head() {
    let (_, base) = server();
    let resp = http().get(format!("{}/s/abc", base)).send().await.unwrap();
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
    let (h, base) = server();
    // First request: original.
    let before = http().get(format!("{}/live/check", base)).send().await.unwrap().text().await.unwrap();
    assert!(!before.contains("REBUILT"));

    let path = h.dir.path().join("index.html");
    let original = fs::read_to_string(&path).unwrap();
    fs::write(&path, original.replace("app shell", "app shell REBUILT")).unwrap();

    let after = http().get(format!("{}/live/check", base)).send().await.unwrap().text().await.unwrap();
    assert!(after.contains("REBUILT"), "new client build picked up without restart");

    fs::write(&path, original).unwrap();
}

#[tokio::test]
async fn api_routes_still_work_alongside_the_spa() {
    let (_, base) = server();
    let resp = http().get(format!("{}/api/echo/spa?x=1", base)).send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["data"]["path"], serde_json::json!("/api/echo/spa"));
}

#[tokio::test]
async fn non_get_on_a_page_path_is_method_not_allowed() {
    let (_, base) = server();
    let resp = http().post(format!("{}/some/page", base)).send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
}
