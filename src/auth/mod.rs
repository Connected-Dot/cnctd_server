use std::collections::BTreeMap;

use hmac::{Hmac, Mac};
use jwt::{SignWithKey, VerifyWithKey};
use serde_json::Value;
use sha2::Sha256;
use anyhow::anyhow;

pub struct CnctdAuth;

impl CnctdAuth {
    pub fn verify_auth_token(secret: Vec<u8>, auth_token: &str) -> anyhow::Result<String> {
        let key: Hmac<Sha256> = Hmac::new_from_slice(&secret)?;
        let claims: BTreeMap<String, Value> = auth_token.verify_with_key(&key)?;
    
        let sub_claim = claims.get("sub").ok_or(anyhow!("'sub' claim not found"))?;
    
        // Convert the sub claim directly to the expected format (String or Number)
        let user_id = match sub_claim {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            _ => return Err(anyhow!("Unexpected type for 'sub' claim")),
        };
    
        Ok(user_id)
    }
    
    pub fn get_jwt<T: AsRef<str> + std::fmt::Debug>(secret: Vec<u8>, user_id: T) -> anyhow::Result<String> {
        let key: Hmac<Sha256> = Hmac::new_from_slice(&secret)?;
        let mut claims = BTreeMap::new();
        let user_id_ref = user_id.as_ref();
        claims.insert("sub", user_id_ref);
        
        // println!("claims: {:?}, key: {:?}, secret: {:?}", claims, key, secret);
        let token_str = claims.sign_with_key(&key)?;
        
        Ok(token_str)
    }
}