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

/// For one hex digest family (MD5/SHA1/SHA512), try every EQEmu hashing variant
/// that produces that digest and report whether any reproduces `stored`. EQEmu's
/// `eqcrypt_hash` (loginserver/encryption.cpp) defines, per family:
///   plain    = D(password)
///   PassUser = D(password ":" username)
///   UserPass = D(username ":" password)
///   Triple   = D( hex(D(username)) + hex(D(password)) )
/// A bare hex digest is ambiguous about which variant produced it, so we try all
/// four (the loginserver itself only uses one configured mode, but accounts across
/// the DB were created under different modes).
fn hex_variants_match<D: Digest>(stored: &str, password: &str, username: &str) -> bool {
    let candidates = [
        hex_digest::<D>(password),
        hex_digest::<D>(&format!("{password}:{username}")),
        hex_digest::<D>(&format!("{username}:{password}")),
        hex_digest::<D>(&format!("{}{}", hex_digest::<D>(username), hex_digest::<D>(password))),
    ];
    candidates.iter().any(|c| c.eq_ignore_ascii_case(stored))
}

/// Verify a provided password against an EQEmu `login_accounts.account_password`
/// digest. EQEmu hashes by its loginserver EncryptionMode; we detect the stored
/// format per-account: `$7$` = libsodium scrypt (mode 14), `$argon2` = libsodium
/// argon2 (mode 13), else a hex digest by length (128→SHA512, 40→SHA1, 32→MD5),
/// trying the plain/PassUser/UserPass/Triple variant for that family (`username`
/// is needed for the salted/triple forms).
pub fn verify_password(stored: &str, provided: &str, username: &str) -> bool {
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
    match stored.len() {
        128 => hex_variants_match::<Sha512>(stored, provided, username),
        40 => hex_variants_match::<Sha1>(stored, provided, username),
        32 => hex_variants_match::<Md5>(stored, provided, username),
        _ => false,
    }
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
            return verify_password(stored, password, username);
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
                verify_password(&pw, password, username)
            }
            Ok(None) => false, // unknown account
            Err(e) => {
                tracing::warn!("account lookup failed for {username}: {e}");
                false
            }
        }
    }
}
