//! Small helpers shared by the server and socket layers.

use std::{collections::HashMap, net::SocketAddr};

use axum::http::HeaderMap;

/// The caller's address: first hop of `X-Forwarded-For` when a proxy set it,
/// else the peer.
pub fn client_ip(headers: &HeaderMap, peer: Option<SocketAddr>) -> Option<String> {
    headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| peer.map(|p| p.ip().to_string()))
}

/// Every header with a UTF-8 value, keyed by lowercase name.
pub fn lowercase_headers(headers: &HeaderMap) -> HashMap<String, String> {
    let mut out = HashMap::with_capacity(headers.len());
    for (k, v) in headers.iter() {
        if let Ok(s) = v.to_str() {
            out.insert(k.as_str().to_ascii_lowercase(), s.to_string());
        }
    }
    out
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
    use super::*;

    #[test]
    fn page_tags_replace_the_documents_own() {
        let html = "<html>\n<head>\n<meta charset=\"UTF-8\">\n<title>Old</title>\n<meta property=\"og:title\" content=\"Old\" />\n</head>\n<body></body>\n</html>";
        let out = inject_head(html, "<title>New</title>");
        assert!(out.contains("<title>New</title>"));
        assert!(!out.contains("Old"));
        assert!(out.contains("<meta charset=\"UTF-8\">"));
        assert!(out.find("<title>New</title>").unwrap() < out.find("charset").unwrap());
    }

    #[test]
    fn first_forwarded_hop_wins() {
        let mut h = HeaderMap::new();
        h.insert("x-forwarded-for", "203.0.113.9, 10.0.0.1".parse().unwrap());
        let peer: SocketAddr = "127.0.0.1:1".parse().unwrap();
        assert_eq!(client_ip(&h, Some(peer)).as_deref(), Some("203.0.113.9"));
        assert_eq!(client_ip(&HeaderMap::new(), Some(peer)).as_deref(), Some("127.0.0.1"));
        assert_eq!(client_ip(&HeaderMap::new(), None), None);
    }
}
