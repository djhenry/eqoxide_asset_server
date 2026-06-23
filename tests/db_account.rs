//! Requires a live MariaDB seeded with an `account` row.
//! Run with: EQEMU_DB_URL=mysql://peq:peqpass@127.0.0.1:3306/peq cargo test --test db_account -- --ignored

use eqoxide_asset_server::auth::AccountStore;
use eqoxide_asset_server::db::{verify_password, MariaAccountStore};

#[test]
fn plaintext_password_matches() {
    assert!(verify_password("claudepw", "claudepw"));
    assert!(!verify_password("claudepw", "nope"));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn maria_store_verifies_seeded_account() {
    let url = std::env::var("EQEMU_DB_URL").unwrap();
    let store = MariaAccountStore::connect(&url).await.unwrap();
    // Seed expected: INSERT INTO account (name, password) VALUES ('claude','claudepw');
    assert!(store.verify("claude", "claudepw"));
    assert!(!store.verify("claude", "wrong"));
}
