use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};

fn ensure_sodium() -> bool {
    static SODIUM: OnceLock<bool> = OnceLock::new();
    *SODIUM.get_or_init(|| sodiumoxide::init().is_ok())
}

use md5::Md5;
use sha1::Sha1;
use sha2::{Digest, Sha512};
use sqlx::mysql::MySqlPoolOptions;
use sqlx::{MySqlPool, Row};

use crate::auth::AccountStore;

fn hex_digest<D: Digest>(input: &str) -> String {
    let mut hasher = D::new();
    hasher.update(input.as_bytes());
    hex::encode(hasher.finalize())
}

/// Verify a provided password against an EQEmu `login_accounts.account_password`
/// digest. EQEmu hashes by its loginserver EncryptionMode; we detect the stored
/// format per-account: `$7$` = libsodium scrypt (mode 14), `$argon2` = libsodium
/// argon2 (mode 13), else a hex digest by length (128→SHA512, 40→SHA1, 32→MD5).
pub fn verify_password(stored: &str, provided: &str) -> bool {
    let stored = stored.trim();
    if stored.starts_with("$7$") {
        return scrypt_verify(stored, provided);
    }
    if stored.starts_with("$argon2") {
        return argon2_verify(stored, provided);
    }
    if stored.starts_with('$') {
        tracing::warn!("unsupported password hash format (prefix '$')");
        return false;
    }
    let computed = match stored.len() {
        128 => hex_digest::<Sha512>(provided),
        40 => hex_digest::<Sha1>(provided),
        32 => hex_digest::<Md5>(provided),
        _ => return false,
    };
    computed.eq_ignore_ascii_case(stored)
}

fn scrypt_verify(stored: &str, provided: &str) -> bool {
    use sodiumoxide::crypto::pwhash::scryptsalsa208sha256 as sc;
    if !ensure_sodium() {
        tracing::error!("libsodium init failed");
        return false;
    }
    let mut buf = [0u8; sc::HASHEDPASSWORDBYTES];
    let b = stored.as_bytes();
    if b.len() >= buf.len() {
        tracing::warn!("stored password hash too long / missing NUL terminator slot");
        return false;
    }
    buf[..b.len()].copy_from_slice(b);
    sc::pwhash_verify(&sc::HashedPassword(buf), provided.as_bytes())
}

fn argon2_verify(stored: &str, provided: &str) -> bool {
    if !ensure_sodium() {
        tracing::error!("libsodium init failed");
        return false;
    }
    let b = stored.as_bytes();
    // Dispatch by prefix to the correct argon2 variant module.
    // EQEmu mode 13 uses libsodium's generic crypto_pwhash_str_verify which
    // accepts both argon2id ($argon2id$) and argon2i ($argon2i$) hashes.
    if stored.starts_with("$argon2id$") {
        use sodiumoxide::crypto::pwhash::argon2id13 as pw;
        let mut buf = [0u8; pw::HASHEDPASSWORDBYTES];
        if b.len() >= buf.len() {
            tracing::warn!("stored password hash too long / missing NUL terminator slot");
            return false;
        }
        buf[..b.len()].copy_from_slice(b);
        pw::pwhash_verify(&pw::HashedPassword(buf), provided.as_bytes())
    } else if stored.starts_with("$argon2i$") {
        use sodiumoxide::crypto::pwhash::argon2i13 as pw;
        let mut buf = [0u8; pw::HASHEDPASSWORDBYTES];
        if b.len() >= buf.len() {
            tracing::warn!("stored password hash too long / missing NUL terminator slot");
            return false;
        }
        buf[..b.len()].copy_from_slice(b);
        pw::pwhash_verify(&pw::HashedPassword(buf), provided.as_bytes())
    } else {
        tracing::warn!("unsupported argon2 variant in hash");
        false
    }
}

pub struct MariaAccountStore {
    creds: RwLock<HashMap<String, String>>,
    pool: MySqlPool,
}

impl MariaAccountStore {
    pub async fn connect(url: &str) -> anyhow::Result<Self> {
        let pool = MySqlPoolOptions::new()
            .max_connections(4)
            .connect(url)
            .await?;
        Ok(Self { creds: RwLock::new(HashMap::new()), pool })
    }

    async fn fetch_password(&self, username: &str) -> anyhow::Result<Option<String>> {
        let row = sqlx::query("SELECT account_password FROM login_accounts WHERE account_name = ? LIMIT 1")
            .bind(username)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|r| r.get::<String, _>("account_password")))
    }
}

impl AccountStore for MariaAccountStore {
    fn verify(&self, username: &str, password: &str) -> bool {
        // Cheap cache hit path.
        if let Some(stored) = self.creds.read().unwrap().get(username) {
            return verify_password(stored, password);
        }
        // Miss: block on a DB fetch. `verify` is sync (called from an async
        // handler); use a current-thread block to query, then cache.
        // requires the multi-thread runtime (guaranteed by #[tokio::main] full features); a current_thread runtime would panic here
        let stored = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(self.fetch_password(username))
        });
        match stored {
            Ok(Some(pw)) => {
                self.creds.write().unwrap().insert(username.to_string(), pw.clone());
                verify_password(&pw, password)
            }
            Ok(None) => false, // unknown account
            Err(e) => {
                tracing::warn!("account lookup failed for {username}: {e}");
                false
            }
        }
    }
}
