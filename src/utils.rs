use std::fs;

use warp::{filters::cors::Builder, Filter, reply::{with_status, with_header}, http::StatusCode};

pub fn cors() -> Builder {
    let cors = warp::cors()
        .allow_any_origin()
        .allow_headers(vec!["User-Agent", "Sec-Fetch-Mode", "Referer", "Origin", "Access-Control-Request-Method", "Access-Control-Request-Headers", "Content-Type"])
        .allow_methods(vec!["POST", "GET", "PUT", "DELETE", "OPTIONS"]);
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
        warp::fs::dir(directory)
            .map(|file: warp::fs::File| {
                let response: Box<dyn warp::Reply> = Box::new(file);
                response
            })
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
