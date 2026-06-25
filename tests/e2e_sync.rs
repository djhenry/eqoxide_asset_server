use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use eqoxide_asset_server::auth::{FakeAccountStore, TokenIssuer};
use eqoxide_asset_server::cas::Cas;
use eqoxide_asset_server::manifest::ManifestStore;
use eqoxide_asset_server::server::{router, AppState};
use eqoxide_asset_server::sync_client::SyncClient;

async fn spawn_server(data: &std::path::Path) -> String {
    let mut creds = HashMap::new();
    creds.insert("claude".to_string(), "claudepw".to_string());
    let state = AppState {
        cas: Arc::new(Cas::new(data)),
        manifests: Arc::new(ManifestStore::new(data)),
        accounts: Arc::new(FakeAccountStore { creds }),
        tokens: Arc::new(TokenIssuer::new([5u8; 32], Duration::from_secs(3600))),
        no_auth: false,
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router(state)).await.unwrap(); });
    format!("http://{addr}")
}

#[tokio::test]
async fn cold_then_warm_resync_transfers_only_changed_chunks() {
    let server_data = tempfile::tempdir().unwrap();
    let server_cas = Cas::new(server_data.path());
    let server_store = ManifestStore::new(server_data.path());
    // initial asset set
    server_store
        .build_and_write(&server_cas, "zone/qeynos",
            &[("qeynos.glb".into(), vec![1u8; 300_000])])
        .unwrap();

    let base = spawn_server(server_data.path()).await;
    let client_data = tempfile::tempdir().unwrap();
    let local = Cas::new(client_data.path());

    // cold sync: downloads everything
    let client = SyncClient::login(&base, "claude", "claudepw").await.unwrap();
    let cold = client.sync_set("zone/qeynos", &local).await.unwrap();
    assert!(cold.chunks_downloaded > 0);
    assert_eq!(cold.chunks_downloaded, cold.chunks_total);

    // warm re-sync of the SAME version: nothing to download
    let warm = client.sync_set("zone/qeynos", &local).await.unwrap();
    assert_eq!(warm.chunks_downloaded, 0);

    // server regenerates the file with a small change at the end
    let mut changed = vec![1u8; 300_000];
    *changed.last_mut().unwrap() = 9;
    server_store
        .build_and_write(&server_cas, "zone/qeynos", &[("qeynos.glb".into(), changed)])
        .unwrap();

    // re-login (new manifest version is 'latest') and re-sync: only changed chunks move
    let client2 = SyncClient::login(&base, "claude", "claudepw").await.unwrap();
    let delta = client2.sync_set("zone/qeynos", &local).await.unwrap();
    assert!(delta.chunks_downloaded > 0, "a changed file should move some chunks");
    assert!(delta.chunks_downloaded < delta.chunks_total,
        "unchanged chunks should be reused: downloaded {} of {}",
        delta.chunks_downloaded, delta.chunks_total);
}
