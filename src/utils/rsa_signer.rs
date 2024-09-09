use ring::{signature, rand};
use anyhow::{Result, anyhow};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde_json;

use super::signed_cookie::SignedCookie;

pub struct RSASigner {
    private_key: Vec<u8>,
}

impl RSASigner {
    pub fn new(private_key: Vec<u8>) -> Self {
        RSASigner { private_key }
    }

    pub fn sign_cookie(&self, cookie: &SignedCookie) -> Result<String> {
        // Serialize the cookie into a JSON string
        let serialized_cookie = serde_json::to_string(cookie)
            .map_err(|e| anyhow!("Failed to serialize cookie: {}", e))?;

        // Generate a random number generator
        let rng = rand::SystemRandom::new();

        // Load the private key for signing
        let key_pair = signature::RsaKeyPair::from_pkcs8(&self.private_key)
            .map_err(|e| anyhow!("Failed to load RSA key pair: {}", e))?;

        // Create a signature buffer
        let mut signature = vec![0; key_pair.public_modulus_len()];
        
        // Sign the serialized cookie
        key_pair.sign(
            &signature::RSA_PKCS1_SHA256,
            &rng,
            serialized_cookie.as_bytes(),
            &mut signature,
        ).map_err(|e| anyhow!("Failed to sign cookie: {}", e))?;

        // Base64 encode the signature using URL_SAFE_NO_PAD engine
        let signed_cookie = format!(
            "{}.{}",
            serialized_cookie,
            URL_SAFE_NO_PAD.encode(&signature)
        );
        
        Ok(signed_cookie)
    }

    pub fn verify_cookie(&self, signed_cookie: &str, public_key: &[u8]) -> Result<SignedCookie> {
        let parts: Vec<&str> = signed_cookie.split('.').collect();
        if parts.len() != 2 {
            return Err(anyhow!("Invalid cookie format"));
        }

        let serialized_cookie = parts[0];
        let signature = URL_SAFE_NO_PAD.decode(parts[1])
            .map_err(|e| anyhow!("Failed to decode base64 signature: {}", e))?;

        // Verify the signature
        let public_key = signature::UnparsedPublicKey::new(&signature::RSA_PKCS1_2048_8192_SHA256, public_key);
        public_key.verify(serialized_cookie.as_bytes(), &signature)
            .map_err(|_| anyhow!("Failed to verify signature"))?;

        // Deserialize the cookie
        let cookie: SignedCookie = serde_json::from_str(serialized_cookie)
            .map_err(|e| anyhow!("Failed to deserialize cookie: {}", e))?;

        Ok(cookie)
    }
}
