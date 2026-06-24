//! Requires a live MariaDB seeded with a `login_accounts` row.
//! Run with: EQEMU_DB_URL=mysql://peq:peqpass@127.0.0.1:3306/peq cargo test --test db_account -- --ignored

use eqoxide_asset_server::auth::AccountStore;
use eqoxide_asset_server::db::{verify_password, MariaAccountStore};
use sha2::Digest; // brings the `digest()` assoc fn into scope for Md5/Sha1/Sha512

#[test]
fn sha512_password_matches_real_vector() {
    // SHA512("claudepw") verified against EQEmu live DB (plain, mode 9)
    let stored = "307cf340cd681b1ea251aaa856d74f47476673cd1ab6fe02124f04759bd85cd40a1f95df40109c212b75f6409885a0597b93679c56c37aeb439192ad39f19bd8";
    assert!(verify_password(stored, "claudepw", "claude"));
    assert!(!verify_password(stored, "wrongpw", "claude"));
}

#[test]
fn md5_and_sha1_plain_digests_match() {
    // md5("hello") and sha1("hello") known plain vectors (username irrelevant for plain)
    assert!(verify_password("5d41402abc4b2a76b9719d911017c592", "hello", "anyone"));
    assert!(verify_password("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d", "hello", "anyone"));
}

#[test]
fn salted_hex_variants_match() {
    // Real EQEmu live-DB vector: testuser = SHA1("testpass:testuser") = SHA-PassUser (mode 6).
    let testuser = "a9bbd2e71a55909ab2cc14923e658882b5e18c98";
    assert!(verify_password(testuser, "testpass", "testuser"));
    // PassUser is salted by username — a different username must NOT verify.
    assert!(!verify_password(testuser, "testpass", "someoneelse"));
    assert!(!verify_password(testuser, "wrongpw", "testuser"));

    // MD5 UserPass round-trip: md5("bob:secret") (username ":" password).
    let md5_userpass = format!("{:x}", md5::Md5::digest(b"bob:secret"));
    assert!(verify_password(&md5_userpass, "secret", "bob"));
    // SHA512 PassUser round-trip.
    let sha512_passuser = hex::encode(sha2::Sha512::digest(b"pw:alice"));
    assert!(verify_password(&sha512_passuser, "pw", "alice"));
    // SHA1 Triple round-trip: sha1( hex(sha1(user)) + hex(sha1(pw)) ).
    let inner_u = hex::encode(sha1::Sha1::digest(b"carol"));
    let inner_p = hex::encode(sha1::Sha1::digest(b"hunter2"));
    let sha1_triple = hex::encode(sha1::Sha1::digest(format!("{inner_u}{inner_p}").as_bytes()));
    assert!(verify_password(&sha1_triple, "hunter2", "carol"));
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
    assert!(verify_password(&s, "keeblerpw", "keebler"));
    assert!(!verify_password(&s, "wrongpw", "keebler"));
}

#[test]
fn argon2_password_round_trips() {
    use sodiumoxide::crypto::pwhash::argon2id13 as pw;
    sodiumoxide::init().unwrap();
    let hp = pw::pwhash("argonpw".as_bytes(), pw::OPSLIMIT_INTERACTIVE, pw::MEMLIMIT_INTERACTIVE).unwrap();
    let s = std::str::from_utf8(&hp.0).unwrap().trim_end_matches('\0').to_string();
    assert!(s.starts_with("$argon2"), "got {s}");
    assert!(verify_password(&s, "argonpw", "user"));
    assert!(!verify_password(&s, "nope", "user"));
}

#[test]
fn unsupported_dollar_and_garbage_rejected() {
    assert!(!verify_password("$6$unknownsha512cryptformat", "anything", "user"));
    assert!(!verify_password("tooshort", "x", "user"));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn maria_store_verifies_seeded_account() {
    let url = std::env::var("EQEMU_DB_URL").unwrap();
    let store = MariaAccountStore::connect(&url).await.unwrap();
    // claude uses plain SHA512 (mode 9); testuser uses SHA1-PassUser (mode 6).
    assert!(store.verify("claude", "claudepw"));
    assert!(!store.verify("claude", "wrong"));
    assert!(store.verify("testuser", "testpass"));
}
