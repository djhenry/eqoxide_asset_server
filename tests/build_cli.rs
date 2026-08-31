use eqoxide_asset_server::build::ingest_dir;
use eqoxide_asset_server::cas::Cas;
use eqoxide_asset_server::manifest::ManifestStore;

#[test]
fn jobs_flag_rejects_zero() {
    let exe = env!("CARGO_BIN_EXE_eqoxide-assets");
    let out = std::process::Command::new(exe)
        .args(["build", "--out", "/tmp/unused-eqoxide", "--jobs", "0", "--zones-only"])
        .output()
        .unwrap();
    assert!(!out.status.success(), "expected --jobs 0 to be rejected");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("jobs") || stderr.contains("0"), "stderr was: {stderr}");
}

#[test]
fn ingest_dir_chunks_all_files_with_relative_paths() {
    let src = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(src.path().join("textures")).unwrap();
    std::fs::write(src.path().join("humanoid.glb"), vec![1u8; 80_000]).unwrap();
    std::fs::write(src.path().join("textures/skin.png"), vec![2u8; 4_000]).unwrap();

    let data = tempfile::tempdir().unwrap();
    let cas = Cas::new(data.path());
    let store = ManifestStore::new(data.path());

    let m = ingest_dir(&cas, &store, "common", src.path()).unwrap();
    let mut paths: Vec<_> = m.files.iter().map(|f| f.path.clone()).collect();
    paths.sort();
    assert_eq!(paths, vec!["humanoid.glb", "textures/skin.png"]);

    // re-ingesting identical content reuses chunks and yields the same content digest
    // (the store is content-addressed; there is no version counter to bump)
    let m2 = ingest_dir(&cas, &store, "common", src.path()).unwrap();
    let a = m.files.iter().find(|f| f.path == "humanoid.glb").unwrap();
    let b = m2.files.iter().find(|f| f.path == "humanoid.glb").unwrap();
    assert_eq!(a.chunks, b.chunks);
    assert_eq!(m.digest, m2.digest);
}

// --- #45: a bake that converts nothing must fail loudly instead of publishing a degraded
// set. Each test below reproduces one leg of the incident that shipped bad assets to the
// live server; each passes (exit 0, degraded manifest published) before the fix.

fn run_bake(raw: &std::path::Path, out: &std::path::Path) -> std::process::Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_eqoxide-assets"))
        .args([
            "build",
            "--raw", raw.to_str().unwrap(),
            "--out", out.to_str().unwrap(),
            "--no-zones",
        ])
        .output()
        .unwrap()
}

fn combined(out: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// Every archive looked "missing", so `common` published with 0 files and `latest`
/// repointed at it, exit 0. A fresh store has no previous manifest to shrink from, so the
/// zero-converted check is what has to catch this one.
#[test]
fn empty_raw_dir_fails_instead_of_publishing_empty_common() {
    let raw = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();

    let res = run_bake(raw.path(), out.path());
    let log = combined(&res);

    assert!(!res.status.success(), "expected a non-zero exit; log was:\n{log}");
    assert!(log.contains("converted 0"), "expected a 'converted 0' diagnostic; log was:\n{log}");
    assert!(
        !out.path().join("manifests/common/latest").exists(),
        "a failed bake must not repoint the 'common' latest pointer"
    );
}

/// The ingest read whatever sat in `work/`, which is never cleaned between bakes, so a
/// skipped conversion republished an earlier run's GLB as if freshly built. This is why the
/// live incident served two-month-old models rather than an empty set.
#[test]
fn stale_work_artifact_is_not_republished_as_fresh() {
    let raw = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let work = out.path().join("work/common");
    std::fs::create_dir_all(&work).unwrap();
    std::fs::write(work.join("humanoid.glb"), b"STALE-GLB-FROM-A-PREVIOUS-BAKE").unwrap();

    let res = run_bake(raw.path(), out.path());

    assert!(!res.status.success(), "expected a non-zero exit");
    assert!(
        !out.path().join("manifests/common/latest").exists(),
        "the stale work/ artifact must not be published as this run's output"
    );
}

/// `Path::exists()` is false for *any* stat error, so an unreadable archive was
/// indistinguishable from an absent one. In the incident an SELinux label made all 4253
/// archives unreadable and every model was skipped as "missing".
#[cfg(unix)]
#[test]
fn unreadable_archive_is_fatal_not_a_silent_skip() {
    use std::os::unix::fs::PermissionsExt;

    let raw = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    // Any name from COMMON_MODELS; the contents don't matter, it must never be read.
    let archive = raw.path().join("globalhum_chr.s3d");
    std::fs::write(&archive, b"unreadable").unwrap();
    std::fs::set_permissions(&archive, std::fs::Permissions::from_mode(0o000)).unwrap();

    // root bypasses the permission bits, so the scenario can't be staged there.
    if std::fs::File::open(&archive).is_ok() {
        eprintln!("skip: this process bypasses file permissions (running as root?)");
        return;
    }

    let res = run_bake(raw.path(), out.path());
    let log = combined(&res);

    assert!(!res.status.success(), "expected a non-zero exit; log was:\n{log}");
    assert!(
        log.contains("could not be read"),
        "expected an unreadable-archive diagnostic; log was:\n{log}"
    );
    assert!(
        !out.path().join("manifests/common/latest").exists(),
        "an unreadable archive must not yield a published set"
    );
}
