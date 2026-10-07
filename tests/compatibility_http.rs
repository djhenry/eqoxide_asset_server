use std::{sync::{Arc,atomic::{AtomicUsize,Ordering}},collections::HashMap,time::Duration};
use eqoxide_asset_server::{cas::Cas,compatibility::ReaderRequirements,manifest::ManifestStore,server::{router,AppState},auth::{FakeAccountStore,TokenIssuer},sync_client::SyncClient};
async fn server() -> (String,tempfile::TempDir,eqoxide_asset_server::manifest::Manifest) {
    let dir=tempfile::tempdir().unwrap();let cas=Cas::new(dir.path());let store=ManifestStore::new(dir.path());
    let m=store.build_and_write(&cas,"common",&[("a".into(),vec![1,2,3])],ReaderRequirements::legacy()).unwrap();
    let state=AppState {cas:Arc::new(cas),manifests:Arc::new(store),accounts:Arc::new(FakeAccountStore{creds:HashMap::new()}),tokens:Arc::new(TokenIssuer::new([5;32],Duration::from_secs(3600))),no_auth:true};
    let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();let addr=listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener,router(state)).await.unwrap(); });
    (format!("http://{addr}"),dir,m)
}
#[tokio::test]
async fn compatibility_gate_precedes_etag_and_no_auth_does_not_bypass_it() {
    let(base,dir,m)=server().await;let http=reqwest::Client::new();
    let request=||http.get(format!("{base}/manifest/common")).header("If-None-Match",format!("\"{}\"",m.revision));
    assert_eq!(request().send().await.unwrap().status(),409);
    for (version,cap,status) in [("2","legacy-assets-v1",409),("1","other",409),("1,","legacy-assets-v1",400),("1","bad cap",400),("1","legacy-assets-v1",304)] {
        let response=request().header("X-Eqoxide-Asset-Readers",version).header("X-Eqoxide-Asset-Capabilities",cap).send().await.unwrap();
        assert_eq!(response.status(),status,"{version} {cap}");
    }
    let compatible=||http.get(format!("{base}/manifest/common")).header("X-Eqoxide-Asset-Readers","1").header("X-Eqoxide-Asset-Capabilities","legacy-assets-v1");
    let response=compatible().header("If-None-Match",&m.digest).send().await.unwrap();assert_eq!(response.status(),200);
    assert_eq!(response.headers()["etag"],format!("\"{}\"",m.revision));
    let mut req=ReaderRequirements::legacy();req.reader_version=2;
    let next=ManifestStore::new(dir.path()).build_and_write(&Cas::new(dir.path()),"common",&[("a".into(),vec![1,2,3])],req).unwrap();
    assert_eq!(next.digest,m.digest);assert_ne!(next.revision,m.revision);
    assert_eq!(compatible().header("If-None-Match",&next.revision).send().await.unwrap().status(),409);
    assert_eq!(request().header("X-Eqoxide-Asset-Readers","2").header("X-Eqoxide-Asset-Capabilities","legacy-assets-v1").send().await.unwrap().status(),200);
    let path=dir.path().join(format!("manifests/common/{}.json",next.revision));
    std::fs::write(&path,b"broken").unwrap();
    assert_ne!(compatible().header("If-None-Match",&next.revision).send().await.unwrap().status(),304);
    std::fs::remove_file(&path).unwrap();
    assert_ne!(compatible().header("If-None-Match",&next.revision).send().await.unwrap().status(),304);
}
#[tokio::test]
async fn sync_client_rejects_incompatible_success_response_before_chunks() {
    let dir=tempfile::tempdir().unwrap();let mut req=ReaderRequirements::legacy();req.reader_version=2;
    let m=ManifestStore::new(dir.path()).build_and_write(&Cas::new(dir.path()),"common",&[("a".into(),vec![1,2,3])],req).unwrap();
    let chunks=Arc::new(AtomicUsize::new(0));let count=chunks.clone();
    let app=axum::Router::new().route("/auth",axum::routing::post(||async {axum::Json(serde_json::json!({"token":"test"}))}))
        .route("/manifest/common",axum::routing::get(move ||{let m=m.clone();async move {axum::Json(m)}}))
        .route("/chunk/:hash",axum::routing::get(move ||{count.fetch_add(1,Ordering::SeqCst);async {"unexpected"}}));
    let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();let addr=listener.local_addr().unwrap();
    tokio::spawn(async move {axum::serve(listener,app).await.unwrap();});
    let client=SyncClient::login(&format!("http://{addr}"),"test","test").await.unwrap();let local=tempfile::tempdir().unwrap();
    assert!(client.sync_set("common",&Cas::new(local.path())).await.unwrap_err().to_string().contains("asset_reader_incompatible"));
    assert_eq!(chunks.load(Ordering::SeqCst),0);assert!(!local.path().join("cas").exists());
}
