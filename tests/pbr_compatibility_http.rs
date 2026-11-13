#[allow(dead_code)]
#[path = "../examples/static_pbr_fixture.rs"]
mod fixture;

use std::{collections::HashMap, sync::Arc, time::Duration};

use eqoxide_asset_server::{
    auth::{FakeAccountStore, TokenIssuer},
    cas::Cas,
    compatibility::{CAPABILITIES, READERS},
    manifest::ManifestStore,
    server::{router, AppState},
    static_scene::write_static_visual_with_material_policy,
};

#[tokio::test]
async fn real_pbr_artifact_requires_new_capability_before_etag() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("visual.glb");
    let (scene, policy) = fixture::fixture_scene();
    let report =
        write_static_visual_with_material_policy(&scene, &policy, &"a".repeat(64), &path).unwrap();
    assert_eq!(READERS, "1");
    assert_eq!(CAPABILITIES, "legacy-assets-v1");
    assert!(report.requirements.check_supported().is_err());
    let cas = Arc::new(Cas::new(dir.path()));
    let manifests = Arc::new(ManifestStore::new(dir.path()));
    let manifest = manifests
        .build_and_write(
            &cas,
            "common",
            &[("visual.glb".into(), std::fs::read(path).unwrap())],
            report.requirements,
        )
        .unwrap();
    let state = AppState {
        cas,
        manifests,
        accounts: Arc::new(FakeAccountStore {
            creds: HashMap::new(),
        }),
        tokens: Arc::new(TokenIssuer::new([5; 32], Duration::from_secs(3600))),
        no_auth: true,
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, router(state)).await.unwrap() });
    let client = reqwest::Client::new();
    for conditional in [false, true] {
        for (readers, capabilities, expected) in [
            ("2", "static-visual-v1", 409),
            ("2", "pbr-metallic-roughness-factors-v1", 409),
            (
                "1",
                "pbr-metallic-roughness-factors-v1,static-visual-v1",
                409,
            ),
            (
                "2",
                "pbr-metallic-roughness-factors-v1,static-visual-v1",
                if conditional { 304 } else { 200 },
            ),
        ] {
            let request = client
                .get(format!("http://{addr}/manifest/common"))
                .header("X-Eqoxide-Asset-Readers", readers)
                .header("X-Eqoxide-Asset-Capabilities", capabilities);
            let request = if conditional {
                request.header("If-None-Match", format!("\"{}\"", manifest.revision))
            } else {
                request
            };
            let response = request.send().await.unwrap();
            assert_eq!(
                response.status(),
                expected,
                "conditional={conditional} {readers} {capabilities}"
            );
        }
    }
    server.abort();
    let _ = server.await;
}
