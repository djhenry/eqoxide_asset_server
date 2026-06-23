//! Requires a live MariaDB seeded with a `login_accounts` row.
//! Run with: EQEMU_DB_URL=mysql://peq:peqpass@127.0.0.1:3306/peq cargo test --test db_account -- --ignored

use eqoxide_asset_server::auth::AccountStore;
use eqoxide_asset_server::db::{verify_password, MariaAccountStore};

#[test]
fn sha512_password_matches_real_vector() {
    // SHA512("claudepw") verified against EQEmu live DB
    let stored = "307cf340cd681b1ea251aaa856d74f47476673cd1ab6fe02124f04759bd85cd40a1f95df40109c212b75f6409885a0597b93679c56c37aeb439192ad39f19bd8";
    assert!(verify_password(stored, "claudepw"));
    assert!(!verify_password(stored, "wrongpw"));
}

#[test]
fn md5_and_sha1_digests_match() {
    // md5("hello") and sha1("hello") known vectors
    assert!(verify_password("5d41402abc4b2a76b9719d911017c592", "hello"));
    assert!(verify_password("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d", "hello"));
}

#[test]
fn scrypt_password_round_trips() {
    use sodiumoxide::crypto::pwhash::scryptsalsa208sha256 as sc;
    sodiumoxide::init().unwrap();
    // Generate a real libsodium $7$ hash for a known password (same function EQEmu uses),
    // then verify through our verify_password.
    let hp = sc::pwhash("keeblerpw".as_bytes(), sc::OPSLIMIT_INTERACTIVE, sc::MEMLIMIT_INTERACTIVE).unwrap();
    let s = std::str::from_utf8(&hp.0).unwrap().trim_end_matches('\0').to_string();
    assert!(s.starts_with("$7$"), "got {s}");
    assert!(verify_password(&s, "keeblerpw"));
    assert!(!verify_password(&s, "wrongpw"));
}

#[test]
fn argon2_password_round_trips() {
    use sodiumoxide::crypto::pwhash::argon2id13 as pw;
    sodiumoxide::init().unwrap();
    let hp = pw::pwhash("argonpw".as_bytes(), pw::OPSLIMIT_INTERACTIVE, pw::MEMLIMIT_INTERACTIVE).unwrap();
    let s = std::str::from_utf8(&hp.0).unwrap().trim_end_matches('\0').to_string();
    assert!(s.starts_with("$argon2"), "got {s}");
    assert!(verify_password(&s, "argonpw"));
    assert!(!verify_password(&s, "nope"));
}

#[test]
fn unsupported_dollar_and_garbage_rejected() {
    assert!(!verify_password("$6$unknownsha512cryptformat", "anything"));
    assert!(!verify_password("tooshort", "x"));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn maria_store_verifies_seeded_account() {
    let url = std::env::var("EQEMU_DB_URL").unwrap();
    let store = MariaAccountStore::connect(&url).await.unwrap();
    // Seed expected: INSERT INTO login_accounts (account_name, account_password)
    // VALUES ('claude', '<SHA512("claudepw")>');
    // SHA512 hash: 307cf340cd681b1ea251aaa856d74f47476673cd1ab6fe02124f04759bd85cd40a1f95df40109c212b75f6409885a0597b93679c56c37aeb439192ad39f19bd8
    assert!(store.verify("claude", "claudepw"));
    assert!(!store.verify("claude", "wrong"));
}
