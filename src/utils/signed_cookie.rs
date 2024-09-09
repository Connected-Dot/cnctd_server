use serde::{Deserialize, Serialize};


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedCookie {
    pub key: String,
    pub value: String,
    pub expiration: Option<u64>,  // Expiration in UNIX timestamp
    pub path: Option<String>,
    pub domain: Option<String>,
    pub secure: bool,
    pub http_only: bool,
}

impl SignedCookie {
    pub fn new(key: String, value: String) -> Self {
        SignedCookie {
            key,
            value,
            expiration: None,
            path: None,
            domain: None,
            secure: true,
            http_only: true,
        }
    }

    pub fn with_expiration(mut self, expiration: u64) -> Self {
        self.expiration = Some(expiration);
        self
    }

    pub fn with_path(mut self, path: String) -> Self {
        self.path = Some(path);
        self
    }

    pub fn with_domain(mut self, domain: String) -> Self {
        self.domain = Some(domain);
        self
    }
}
