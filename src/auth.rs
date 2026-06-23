use std::collections::HashMap;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};
use base64::Engine;

pub trait AccountStore: Send + Sync {
    fn verify(&self, username: &str, password: &str) -> bool;
}

pub struct TokenIssuer {
    secret: [u8; 32],
    ttl: Duration,
}

impl TokenIssuer {
    pub fn new(secret: [u8; 32], ttl: Duration) -> Self {
        TokenIssuer { secret, ttl }
    }

    fn mac(&self, payload: &str) -> String {
        let mut hasher = blake3::Hasher::new_keyed(&self.secret);
        hasher.update(payload.as_bytes());
        hasher.finalize().to_hex().to_string()
    }

    pub fn issue(&self, username: &str) -> String {
        let expiry = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + self.ttl.as_secs();
        let payload = format!("{username}|{expiry}");
        let mac = self.mac(&payload);
        let full = format!("{payload}|{mac}");
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(full)
    }

    pub fn verify(&self, token: &str) -> Option<String> {
        let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(token)
            .ok()?;
        let s = String::from_utf8(decoded).ok()?;
        let parts: Vec<&str> = s.splitn(3, '|').collect();
        if parts.len() != 3 {
            return None;
        }
        let (username, expiry_s, mac) = (parts[0], parts[1], parts[2]);
        let payload = format!("{username}|{expiry_s}");
        if self.mac(&payload) != mac {
            return None;
        }
        let expiry: u64 = expiry_s.parse().ok()?;
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        if now >= expiry {
            return None;
        }
        Some(username.to_string())
    }
}

pub struct FakeAccountStore {
    pub creds: HashMap<String, String>,
}

impl AccountStore for FakeAccountStore {
    fn verify(&self, username: &str, password: &str) -> bool {
        self.creds.get(username).map(|p| p == password).unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn issuer() -> TokenIssuer {
        TokenIssuer::new([9u8; 32], Duration::from_secs(3600))
    }

    #[test]
    fn fake_store_verifies_known_creds() {
        let mut creds = HashMap::new();
        creds.insert("claude".to_string(), "claudepw".to_string());
        let store = FakeAccountStore { creds };
        assert!(store.verify("claude", "claudepw"));
        assert!(!store.verify("claude", "wrong"));
        assert!(!store.verify("nobody", "x"));
    }

    #[test]
    fn issued_token_verifies_and_returns_username() {
        let iss = issuer();
        let token = iss.issue("claude");
        assert_eq!(iss.verify(&token).as_deref(), Some("claude"));
    }

    #[test]
    fn tampered_token_is_rejected() {
        let iss = issuer();
        let mut token = iss.issue("claude");
        token.push('x');
        assert!(iss.verify(&token).is_none());
    }

    #[test]
    fn expired_token_is_rejected() {
        let iss = TokenIssuer::new([9u8; 32], Duration::from_secs(0));
        let token = iss.issue("claude");
        std::thread::sleep(Duration::from_millis(10));
        assert!(iss.verify(&token).is_none());
    }

    #[test]
    fn different_secret_rejects_token() {
        let token = TokenIssuer::new([1u8; 32], Duration::from_secs(3600)).issue("claude");
        let other = TokenIssuer::new([2u8; 32], Duration::from_secs(3600));
        assert!(other.verify(&token).is_none());
    }
}
