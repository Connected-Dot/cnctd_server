use std::collections::BTreeMap;

use hmac::{Hmac, Mac};
use jwt::{SignWithKey, VerifyWithKey};
use serde_json::Value;
use sha2::Sha256;
use anyhow::anyhow;

pub struct CnctdAuth;

impl CnctdAuth {
    pub fn verify_auth_token<T: AsRef<str> + std::fmt::Debug>(secret: Vec<u8>, auth_token: &str, user_id: T) -> anyhow::Result<()> {
        let key: Hmac<Sha256> = Hmac::new_from_slice(&secret)?;
        let claims: BTreeMap<String, Value> = auth_token.verify_with_key(&key)?;
        
        let sub_claim = claims.get("sub").ok_or(anyhow!("'sub' claim not found"))?;
        let user_id_ref = user_id.as_ref();
    
        // Dynamically check the type of `sub` and compare
        let matched = match sub_claim {
            Value::String(s) => s == user_id_ref,
            Value::Number(n) => n.to_string() == user_id_ref,
            _ => return Err(anyhow!("Unexpected type for 'sub' claim")),
        };
    
        if matched {
            Ok(())
        } else {
            Err(anyhow!("User ID does not match the 'sub' claim"))
        }
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