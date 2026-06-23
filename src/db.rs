use std::collections::HashMap;
use std::sync::RwLock;

use sqlx::mysql::MySqlPoolOptions;
use sqlx::{MySqlPool, Row};

use crate::auth::AccountStore;

pub fn verify_password(stored: &str, provided: &str) -> bool {
    // v1: EQEmu local/minilogin accounts store plaintext in account.password.
    // Follow-up: if EncryptionMode is set, hash `provided` the same way before compare.
    stored == provided
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
        let row = sqlx::query("SELECT password FROM account WHERE name = ? LIMIT 1")
            .bind(username)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|r| r.get::<String, _>("password")))
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
        let stored = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(self.fetch_password(username))
        });
        match stored {
            Ok(Some(pw)) => {
                self.creds.write().unwrap().insert(username.to_string(), pw.clone());
                verify_password(&pw, password)
            }
            _ => false,
        }
    }
}
