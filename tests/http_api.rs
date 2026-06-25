use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use eqoxide_asset_server::auth::{FakeAccountStore, TokenIssuer};
use eqoxide_asset_server::cas::Cas;
use eqoxide_asset_server::manifest::ManifestStore;
use eqoxide_asset_server::server::{router, AppState};

async fn spawn() -> (SocketAddr, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let cas = Cas::new(dir.path());
    let store = ManifestStore::new(dir.path());
    // seed one set with one file
    store
        .build_and_write(&cas, "common", &[("humanoid.glb".into(), vec![3u8; 100_000])])
        .unwrap();

    let mut creds = HashMap::new();
    creds.insert("claude".to_string(), "claudepw".to_string());

    let state = AppState {
        cas: Arc::new(Cas::new(dir.path())),
        manifests: Arc::new(ManifestStore::new(dir.path())),
        accounts: Arc::new(FakeAccountStore { creds }),
        tokens: Arc::new(TokenIssuer::new([5u8; 32], Duration::from_secs(3600))),
        no_auth: false,
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router(state)).await.unwrap();
    });
    (addr, dir)
}

#[tokio::test]
async fn auth_then_fetch_manifest_and_chunk() {
    let (addr, _dir) = spawn().await;
    let base = format!("http://{addr}");
    let http = reqwest::Client::new();

    // bad creds => 401
    let bad = http
        .post(format!("{base}/auth"))
        .json(&serde_json::json!({"username":"claude","password":"nope"}))
        .send().await.unwrap();
    assert_eq!(bad.status(), 401);

    // good creds => token
    let resp = http
        .post(format!("{base}/auth"))
        .json(&serde_json::json!({"username":"claude","password":"claudepw"}))
        .send().await.unwrap();
    assert_eq!(resp.status(), 200);
    let token = resp.json::<serde_json::Value>().await.unwrap()["token"]
        .as_str().unwrap().to_string();

    // manifest without token => 401
    let noauth = http.get(format!("{base}/manifest/common")).send().await.unwrap();
    assert_eq!(noauth.status(), 401);

    // chunk without token => 401
    let noauth_chunk = http.get(format!("{base}/chunk/anyvalue")).send().await.unwrap();
    assert_eq!(noauth_chunk.status(), 401);

    // manifest with token => 200 and has our file
    let m = http
        .get(format!("{base}/manifest/common"))
        .bearer_auth(&token)
        .send().await.unwrap();
    assert_eq!(m.status(), 200);
    let manifest = m.json::<serde_json::Value>().await.unwrap();
    let chunk_hash = manifest["files"][0]["chunks"][0].as_str().unwrap().to_string();

    // chunk with token => 200 bytes
    let c = http
        .get(format!("{base}/chunk/{chunk_hash}"))
        .bearer_auth(&token)
        .send().await.unwrap();
    assert_eq!(c.status(), 200);
    assert!(!c.bytes().await.unwrap().is_empty());

    // unknown chunk => 404
    let nf = http
        .get(format!("{base}/chunk/deadbeef"))
        .bearer_auth(&token)
        .send().await.unwrap();
    assert_eq!(nf.status(), 404);
}
