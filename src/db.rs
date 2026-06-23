use std::collections::HashMap;
use std::sync::RwLock;

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

/// Verify a provided password against the EQEmu `login_accounts.account_password`
/// digest. EQEmu's loginserver hashes by its EncryptionMode; we detect the hex
/// digest variant by length. The `$`-prefixed SCrypt/Argon2 modes are not
/// supported in v1 and are rejected with a warning.
pub fn verify_password(stored: &str, provided: &str) -> bool {
    let stored = stored.trim();
    if stored.starts_with('$') {
        tracing::warn!("account uses scrypt/argon2 password hashing; not supported in v1");
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
