pub mod signed_cookie;
pub mod rsa_signer;

use std::fs;

use rsa_signer::RSASigner;
use signed_cookie::SignedCookie;
use warp::{filters::cors::Builder, Filter, reply::{with_status, with_header}, http::StatusCode};

pub fn cors(origins: Option<Vec<String>>) -> Builder {
    let mut cors = warp::cors()
        .allow_headers(vec!["User-Agent", "Sec-Fetch-Mode", "Referer", "Origin", "Access-Control-Request-Method", "Access-Control-Request-Headers", "Content-Type", "Authorization"])
        .allow_methods(vec!["POST", "GET", "PUT", "DELETE", "OPTIONS"]);
    if let Some(origins) = origins {
        cors = cors.allow_origins(origins.iter().map(|s| s.as_str()).collect::<Vec<&str>>());
    } else {
        cors = cors.allow_any_origin();
    }
    cors
}

fn serve_index_with_range(client_files_dir: String) -> warp::filters::BoxedFilter<(impl warp::Reply,)> {
    warp::path::end()
        .and(warp::header::optional::<String>("range"))
        .map(move |range_header: Option<String>| {
            let path = format!("{}/index.html", client_files_dir);
            let content = fs::read(&path).expect("Failed to read file");

            if let Some(range) = range_header.and_then(parse_range_header) {
                let start = range.0;
                let end = std::cmp::min(content.len(), range.1 + 1);
                
                let partial_content = &content[start..end];
                let content_range = format!("bytes {}-{}/{}", start, end - 1, content.len());

                with_status(
                    with_header(
                        warp::reply::html(String::from_utf8_lossy(partial_content).into_owned()), 
                        "Content-Range", 
                        content_range
                    ),
                    StatusCode::PARTIAL_CONTENT
                )
            } else {
                with_status(
                    with_header(
                        warp::reply::html(String::from_utf8_lossy(&content).into_owned()), 
                        "Dummy-Header", // Replace with appropriate header or a dummy one.
                        "Dummy-Value"   // Replace with appropriate value or a dummy one.
                    ),
                    StatusCode::OK
                )
            }
        })
        .boxed()
}

pub fn spa(dir: Option<String>) -> warp::filters::BoxedFilter<(Box<dyn warp::Reply>,)> {
    if let Some(directory) = dir {
        let static_files = warp::fs::dir(directory.clone())
            .map(|file: warp::fs::File| {
                let response: Box<dyn warp::Reply> = Box::new(file);
                response
            })
            .boxed();

        let fallback_to_index = warp::fs::file(format!("{}/index.html", directory))
            .map(|file: warp::fs::File| {
                let response: Box<dyn warp::Reply> = Box::new(file);
                response
            })
            .boxed();

        static_files
            .or(fallback_to_index)
            .unify()
            .boxed()
    } else {
        warp::any()
            .map(|| {
                let response: Box<dyn warp::Reply> = Box::new(warp::reply::html("No directory provided or default message here"));
                response
            })
            .boxed()
    }
}





fn serve_files(client_files_dir: String) -> warp::filters::BoxedFilter<(impl warp::Reply,)> {
    warp::fs::dir(client_files_dir)
        .boxed()
}

fn parse_range_header(range: String) -> Option<(usize, usize)> {
    let parts: Vec<&str> = range.trim().split("=").collect();
    if parts.len() != 2 || parts[0] != "bytes" {
        return None;
    }
    let range_parts: Vec<usize> = parts[1]
        .split("-")
        .filter_map(|n| n.parse::<usize>().ok())
        .collect();
    if range_parts.len() == 2 {
        Some((range_parts[0], range_parts[1]))
    } else {
        None
    }
}


pub fn generate_signed_cookie(
    key: &str,
    value: &str,
    private_key: &[u8],
    expiration: u64,
    domain: &str,
    path: &str,
) -> anyhow::Result<String> {
    let cookie = SignedCookie::new(key.to_string(), value.to_string())
        .with_expiration(expiration)
        .with_domain(domain.to_string())
        .with_path(path.to_string());

    let signer = RSASigner::new(private_key.to_vec());
    signer.sign_cookie(&cookie)
}

pub fn verify_signed_cookie(signed_cookie: &str, public_key: &[u8]) -> anyhow::Result<SignedCookie> {
    let signer = RSASigner::new(Vec::new());  // We don't need the private key for verification
    signer.verify_cookie(signed_cookie, public_key)
}
