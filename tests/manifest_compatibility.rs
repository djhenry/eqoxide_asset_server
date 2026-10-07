use eqoxide_asset_server::{cas::Cas, compatibility::ReaderRequirements, manifest::{Manifest, ManifestStore}};
fn files() -> Vec<(String, Vec<u8>)> { vec![("a.txt".into(), b"hello".to_vec()), ("b.txt".into(), b"world".to_vec())] }
#[test]
fn requirements_and_layout_change_revision_not_content_identity() {
    let temp = tempfile::tempdir().unwrap(); let cas = Cas::new(temp.path()); let store = ManifestStore::new(temp.path());
    let a = store.build_and_write(&cas,"common",&files(),ReaderRequirements::legacy()).unwrap();
    let mut req = ReaderRequirements::legacy(); req.reader_version = 2;
    let b = store.build_and_write(&cas,"common",&files(),req).unwrap();
    assert_eq!(a.digest,b.digest); assert_ne!(a.revision,b.revision);
    let mut reordered = b.clone(); reordered.files.reverse();
    reordered.requirements.capabilities.push("legacy-assets-v1".into());
    assert_eq!(reordered.canonical_revision().unwrap(), b.revision);
    let mut layout = b.clone(); let hash = layout.files[0].blake3.clone(); layout.files[0].chunks.push(hash);
    assert_eq!(ManifestStore::set_digest(&layout.files),b.digest); assert_ne!(layout.canonical_revision().unwrap(),b.revision);
    layout = b.clone(); layout.files[0].size+=1; assert_ne!(layout.canonical_revision().unwrap(),b.revision);
}
#[test]
fn invalid_publish_writes_nothing_and_invalid_envelopes_fail() {
    let temp=tempfile::tempdir().unwrap();let cas=Cas::new(temp.path());let store=ManifestStore::new(temp.path());
    for input in [vec![("../escape".into(),vec![1])],vec![("a".into(),vec![1]),("a".into(),vec![2])]] {
        assert!(store.build_and_write(&cas,"common",&input,ReaderRequirements::legacy()).is_err());
        assert!(!temp.path().join("cas").exists());
    }
    let a=store.build_and_write(&cas,"common",&files(),ReaderRequirements::legacy()).unwrap();
    for key in ["requirements","schema_version","revision"] {
        let mut value=serde_json::to_value(&a).unwrap();value.as_object_mut().unwrap().remove(key);
        assert!(serde_json::from_value::<Manifest>(value).is_err());
    }
    for mutation in 0..5 {
        let mut bad=a.clone();match mutation {0=>bad.schema_version=2,1=>bad.digest="a".repeat(64),2=>bad.revision="b".repeat(64),3=>bad.files.push(bad.files[0].clone()),_=>bad.files[0].chunks[0]="../bad".into()}
        assert!(bad.validate("common").is_err());
    }
    assert!(a.validate("other").is_err());
}
#[test]
fn migrate_old_digest_verifies_bytes_preserves_history_and_refuses_future() {
    let temp=tempfile::tempdir().unwrap();let cas=Cas::new(temp.path());let store=ManifestStore::new(temp.path());
    let current=store.build_and_write(&cas,"common",&files(),ReaderRequirements::legacy()).unwrap();
    let dir=temp.path().join("manifests/common");
    let old=serde_json::json!({"set":"common","digest":current.digest,"files":current.files});
    let old_path=dir.join(format!("{}.json",current.digest));
    std::fs::write(&old_path,serde_json::to_vec(&old).unwrap()).unwrap();std::fs::write(dir.join("latest"),&current.digest).unwrap();
    assert_eq!(store.migrate_to_digest("common").unwrap(),Some(current.revision.clone()));
    assert!(old_path.exists());assert!(store.migrate_to_digest("common").unwrap().is_none());
    std::fs::write(dir.join("latest"),&current.digest).unwrap();
    std::fs::write(temp.path().join("cas").join(&current.files[0].chunks[0]),b"corrupt").unwrap();
    assert!(store.migrate_to_digest("common").is_err());assert_eq!(store.latest_digest("common").unwrap(),current.digest);
    let mut future=serde_json::to_value(&current).unwrap();future["schema_version"]=2.into();
    std::fs::write(dir.join(format!("{}.json",current.revision)),serde_json::to_vec(&future).unwrap()).unwrap();
    std::fs::write(dir.join("latest"),&current.revision).unwrap();assert!(store.migrate_to_digest("common").is_err());
}
#[test]
fn shared_golden_revision_matches_wire_contract() {
    let one: Manifest = serde_json::from_str(include_str!("fixtures/manifest-v1.json")).unwrap();
    let two: Manifest = serde_json::from_str(include_str!("fixtures/manifest-reader2.json")).unwrap();
    one.validate("fixture/demo").unwrap(); two.validate("fixture/demo").unwrap();
    assert_eq!(one.digest,"4e686a7e59c0ec486ca4668f5b75e848f75c1048adbe191a5dbf7c2b1ee7a864");
    assert_eq!(one.revision,"b21a853d6baa30fb182ac81a5cc5099bfa7002fd3344d2b2e592b74ed0116157");
    assert_eq!(two.revision,"cbc587b05a1010a5c2823ecd0a1cc9fbac4f42c0f849cf74afc052067347f865");
    assert_eq!(one.digest,two.digest); one.requirements.check_supported().unwrap(); assert!(two.requirements.check_supported().is_err());
}
