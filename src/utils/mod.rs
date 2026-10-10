use std::fs;

use warp::{filters::{cors::Builder, path::FullPath}, Filter, reply::{with_status, with_header}, http::StatusCode};

use crate::server::PageMetaFn;

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

pub fn serve_index_with_range(client_files_dir: String) -> warp::filters::BoxedFilter<(impl warp::Reply,)> {
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





pub fn serve_files(client_files_dir: String) -> warp::filters::BoxedFilter<(impl warp::Reply,)> {
    warp::fs::dir(client_files_dir)
        .boxed()
}

pub fn parse_range_header(range: String) -> Option<(usize, usize)> {
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


/// `spa`, plus app-chosen `<head>` tags on page requests: static files are
/// served as-is; any other GET gets `index.html` read from disk, with the
/// tags from `meta(path)` put at the top of `<head>` (replacing the page's
/// own `<title>` and og: / twitter: tags) when it returns `Some`.
pub fn spa_with_meta(dir: Option<String>, meta: PageMetaFn) -> warp::filters::BoxedFilter<(Box<dyn warp::Reply>,)> {
    let Some(directory) = dir else { return spa(None) };
    let static_files = warp::fs::dir(directory.clone())
        .map(|file: warp::fs::File| {
            let response: Box<dyn warp::Reply> = Box::new(file);
            response
        })
        .boxed();
    let index_path = format!("{}/index.html", directory);
    let page = warp::get()
        .and(warp::path::full())
        .and_then(move |path: FullPath| {
            let meta = meta.clone();
            let index_path = index_path.clone();
            async move {
                let html = tokio::fs::read_to_string(&index_path)
                    .await
                    .map_err(|_| warp::reject::not_found())?;
                let body = match (meta)(path.as_str().to_string()).await {
                    Some(head) => inject_head(&html, &head),
                    None => html,
                };
                let response: Box<dyn warp::Reply> =
                    Box::new(with_header(warp::reply::html(body), "Cache-Control", "no-cache"));
                Ok::<_, warp::Rejection>(response)
            }
        })
        .boxed();
    static_files.or(page).unify().boxed()
}

/// Put `head` at the top of `<head>`, dropping the document's own `<title>`
/// and og: / twitter: meta lines so the page's tags are the only ones a link
/// preview reads.
pub fn inject_head(html: &str, head: &str) -> String {
    let kept: Vec<&str> = html
        .lines()
        .filter(|line| {
            let t = line.trim_start();
            !(t.starts_with("<title") || t.contains("property=\"og:") || t.contains("name=\"twitter:"))
        })
        .collect();
    let joined = kept.join("\n");
    match joined.find("<head>") {
        Some(i) => {
            let at = i + "<head>".len();
            format!("{}\n{}{}", &joined[..at], head, &joined[at..])
        }
        None => format!("{}{}", head, joined),
    }
}

#[cfg(test)]
mod tests {
    use super::inject_head;

    #[test]
    fn page_tags_replace_the_documents_own() {
        let html = "<html>\n<head>\n<meta charset=\"UTF-8\">\n<title>Old</title>\n<meta property=\"og:title\" content=\"Old\" />\n</head>\n<body></body>\n</html>";
        let out = inject_head(html, "<title>New</title>");
        assert!(out.contains("<title>New</title>"));
        assert!(!out.contains("Old"));
        assert!(out.contains("<meta charset=\"UTF-8\">"));
        assert!(out.find("<title>New</title>").unwrap() < out.find("charset").unwrap());
    }
}
