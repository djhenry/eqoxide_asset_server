use crate::cas::Cas;
use crate::chunker::chunk_into;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
pub struct FileEntry {
    pub path: String,
    pub size: u64,
    pub blake3: String,
    pub chunks: Vec<String>,
}

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
pub struct Manifest {
    pub set: String,
    /// Content identity of the set: blake3 over the sorted (path, file-blake3) list. Same content
    /// yields the same digest on any server, so the client can skip an unchanged set and never
    /// cross-contaminate between servers with diverging custom assets.
    pub digest: String,
    pub files: Vec<FileEntry>,
}

pub struct ManifestStore {
    root: PathBuf,
    allow_shrink: bool,
}

impl ManifestStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        ManifestStore { root: root.into(), allow_shrink: false }
    }

    /// Permit a set to publish fewer files than its current `latest`. Off by default: a
    /// shrinking set is nearly always a failed bake (#45), so `build_and_write` refuses it
    /// rather than repointing `latest` at a degraded manifest. Intentional removals — the
    /// -22 classic `ske*` textures of eqoxide#704, say — opt in with `--allow-shrink`.
    pub fn allow_shrink(mut self, yes: bool) -> Self {
        self.allow_shrink = yes;
        self
    }

    fn set_dir(&self, set: &str) -> PathBuf {
        self.root.join("manifests").join(set)
    }

    /// The set's content identity: blake3 over the files sorted by path, each contributing
    /// `"{path}\0{blake3}\n"`. Deterministic, build-order-independent, server-independent. MUST stay
    /// byte-identical to the client's `eqoxide::asset_sync::set_digest`.
    pub fn set_digest(files: &[FileEntry]) -> String {
        let mut sorted: Vec<&FileEntry> = files.iter().collect();
        sorted.sort_by(|a, b| a.path.cmp(&b.path));
        let mut h = blake3::Hasher::new();
        for f in sorted {
            h.update(f.path.as_bytes());
            h.update(b"\0");
            h.update(f.blake3.as_bytes());
            h.update(b"\n");
        }
        h.finalize().to_hex().to_string()
    }

    pub fn latest_digest(&self, set: &str) -> Option<String> {
        let p = self.set_dir(set).join("latest");
        std::fs::read_to_string(p).ok().map(|s| s.trim().to_string())
    }

    /// Fail if publishing `incoming` files to `set` would leave it smaller than its current
    /// `latest`.
    ///
    /// Only a genuinely absent `latest` pointer is a benign "nothing to compare against"
    /// (the first publish of a set). Every other failure — an unreadable pointer, a dangling
    /// one, a corrupt manifest — is fatal rather than a silent pass. Swallowing those with
    /// `if let Ok(..)` would disable this guard exactly when the store is in the damaged
    /// state it exists to protect, which is the same error-collapsing mistake as #45 itself.
    fn check_would_not_shrink(&self, set: &str, incoming: usize) -> anyhow::Result<()> {
        let p = self.set_dir(set).join("latest");
        let digest = match std::fs::read_to_string(&p) {
            Ok(d) => d.trim().to_string(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => anyhow::bail!(
                "refusing to publish '{set}': cannot read {} to check for a shrinking set: {e}",
                p.display(),
            ),
        };
        let prev = self.load(set, &digest).map_err(|e| {
            anyhow::anyhow!(
                "refusing to publish '{set}': its current latest ({}) could not be loaded to \
                 check for a shrinking set: {e}",
                &digest[..12.min(digest.len())],
            )
        })?;
        if incoming < prev.files.len() {
            anyhow::bail!(
                "refusing to publish '{set}': {} file(s), down from {} in the current \
                 latest ({}). A shrinking set is nearly always a failed bake; re-run \
                 with --allow-shrink if the removal is intentional.",
                incoming,
                prev.files.len(),
                &digest[..12.min(digest.len())],
            );
        }
        Ok(())
    }

    pub fn build_and_write(
        &self,
        cas: &Cas,
        set: &str,
        files: &[(String, Vec<u8>)],
    ) -> anyhow::Result<Manifest> {
        // Guard the one place `latest` repoints. A set that loses files between bakes is
        // almost always a degraded build (a skipped conversion, an unreadable archive); the
        // store is append-only so the old manifest survives, but `latest` is what the client
        // follows, and repointing it is what shipped bad assets in #45.
        //
        // This runs before the chunking loop below, so a refused publish writes nothing at
        // all. Chunking first would strand its chunks in the CAS, which has no GC.
        if !self.allow_shrink {
            self.check_would_not_shrink(set, files.len())?;
        }

        let mut entries = Vec::new();
        for (path, bytes) in files {
            let chunks = chunk_into(cas, bytes)?;
            entries.push(FileEntry {
                path: path.clone(),
                size: bytes.len() as u64,
                blake3: Cas::hash(bytes),
                chunks,
            });
        }
        let digest = Self::set_digest(&entries);

        let manifest = Manifest { set: set.to_string(), digest: digest.clone(), files: entries };

        // Content-addressed store: identical content overwrites the same `<digest>.json` (no-op,
        // no counter churn); changed content writes a new digest and `latest` repoints.
        let dir = self.set_dir(set);
        std::fs::create_dir_all(&dir)?;
        let json = serde_json::to_vec_pretty(&manifest)?;
        std::fs::write(dir.join(format!("{digest}.json")), json)?;
        std::fs::write(dir.join("latest"), &digest)?;
        Ok(manifest)
    }

    pub fn load(&self, set: &str, digest: &str) -> anyhow::Result<Manifest> {
        let p = self.set_dir(set).join(format!("{digest}.json"));
        let bytes = std::fs::read(p)?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    pub fn load_latest(&self, set: &str) -> anyhow::Result<Manifest> {
        let d = self
            .latest_digest(set)
            .ok_or_else(|| anyhow::anyhow!("no manifest for set {set}"))?;
        self.load(set, &d)
    }

    /// Every set in the store (a directory under `manifests/` that has a `latest` pointer).
    /// Nested set names like `zone/qeynos` are returned with `/` separators.
    pub fn all_sets(&self) -> Vec<String> {
        fn walk(dir: &Path, base: &Path, out: &mut Vec<String>) {
            if dir.join("latest").is_file() {
                if let Ok(rel) = dir.strip_prefix(base) {
                    out.push(rel.to_string_lossy().replace('\\', "/"));
                }
            }
            if let Ok(rd) = std::fs::read_dir(dir) {
                for e in rd.flatten() {
                    if e.path().is_dir() {
                        walk(&e.path(), base, out);
                    }
                }
            }
        }
        let base = self.root.join("manifests");
        let mut sets = Vec::new();
        if base.is_dir() {
            walk(&base, &base, &mut sets);
        }
        sets.sort();
        sets
    }

    /// Migrate a set's `latest` manifest from the legacy version-keyed format (`<version>.json`,
    /// `latest`=version) to the content-digest store (`<digest>.json`, `latest`=digest). Idempotent:
    /// returns `Ok(None)` when `latest` is already a digest. Reuses the existing chunks (the file
    /// list is unchanged) — no re-derivation, so clients with the content cached re-download nothing.
    pub fn migrate_to_digest(&self, set: &str) -> anyhow::Result<Option<String>> {
        let dir = self.set_dir(set);
        let latest = std::fs::read_to_string(dir.join("latest"))?.trim().to_string();
        if latest.len() == 64 && latest.chars().all(|c| c.is_ascii_hexdigit()) {
            return Ok(None); // already content-addressed
        }
        // Legacy manifest: read it ignoring its `version` field (serde skips unknown fields).
        #[derive(Deserialize)]
        struct Legacy {
            #[serde(default)]
            set: String,
            files: Vec<FileEntry>,
        }
        let bytes = std::fs::read(dir.join(format!("{latest}.json")))?;
        let legacy: Legacy = serde_json::from_slice(&bytes)?;
        let set_name = if legacy.set.is_empty() { set.to_string() } else { legacy.set };
        let digest = Self::set_digest(&legacy.files);
        let manifest = Manifest { set: set_name, digest: digest.clone(), files: legacy.files };
        std::fs::write(dir.join(format!("{digest}.json")), serde_json::to_vec_pretty(&manifest)?)?;
        std::fs::write(dir.join("latest"), &digest)?;
        Ok(Some(digest))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files() -> Vec<(String, Vec<u8>)> {
        vec![
            ("humanoid.glb".to_string(), vec![1u8; 50_000]),
            ("textures/skin.png".to_string(), vec![2u8; 5_000]),
        ]
    }

    fn fe(path: &str, blake3: &str) -> FileEntry {
        FileEntry { path: path.into(), size: 1, blake3: blake3.into(), chunks: vec![blake3.into()] }
    }

    #[test]
    fn digest_is_deterministic_and_order_independent() {
        let a = vec![fe("b.bin", "22"), fe("a.bin", "11")];
        let b = vec![fe("a.bin", "11"), fe("b.bin", "22")];
        assert_eq!(ManifestStore::set_digest(&a), ManifestStore::set_digest(&b));
        assert_eq!(ManifestStore::set_digest(&a).len(), 64);
    }

    #[test]
    fn digest_changes_when_a_file_changes() {
        let a = vec![fe("a.bin", "11")];
        let b = vec![fe("a.bin", "99")];
        assert_ne!(ManifestStore::set_digest(&a), ManifestStore::set_digest(&b));
    }

    #[test]
    fn build_writes_digest_named_manifest_and_latest() {
        let dir = tempfile::tempdir().unwrap();
        let cas = Cas::new(dir.path());
        let store = ManifestStore::new(dir.path());

        let m = store.build_and_write(&cas, "common", &files()).unwrap();
        assert_eq!(m.set, "common");
        assert_eq!(m.files.len(), 2);
        assert_eq!(m.digest.len(), 64);
        assert!(store.set_dir("common").join(format!("{}.json", m.digest)).exists());
        assert_eq!(
            std::fs::read_to_string(store.set_dir("common").join("latest")).unwrap(),
            m.digest
        );
        assert_eq!(store.latest_digest("common").as_deref(), Some(m.digest.as_str()));
    }

    #[test]
    fn identical_rebuild_dedups_no_new_file() {
        let dir = tempfile::tempdir().unwrap();
        let cas = Cas::new(dir.path());
        let store = ManifestStore::new(dir.path());
        let m1 = store.build_and_write(&cas, "common", &files()).unwrap();
        let count1 = std::fs::read_dir(store.set_dir("common")).unwrap().count();
        let m2 = store.build_and_write(&cas, "common", &files()).unwrap();
        let count2 = std::fs::read_dir(store.set_dir("common")).unwrap().count();
        assert_eq!(m1.digest, m2.digest);
        assert_eq!(count1, count2); // <digest>.json + latest, no churn
    }

    #[test]
    fn migrate_legacy_to_digest_idempotent_and_loadable() {
        let dir = tempfile::tempdir().unwrap();
        let store = ManifestStore::new(dir.path());
        let entries = vec![fe("b.bin", "22"), fe("a.bin", "11")];
        // hand-write a legacy version-keyed manifest
        let sd = store.set_dir("common");
        std::fs::create_dir_all(&sd).unwrap();
        let legacy = serde_json::json!({ "set": "common", "version": 7, "files": entries });
        std::fs::write(sd.join("7.json"), serde_json::to_vec_pretty(&legacy).unwrap()).unwrap();
        std::fs::write(sd.join("latest"), "7").unwrap();

        let d = store.migrate_to_digest("common").unwrap().unwrap();
        assert_eq!(d, ManifestStore::set_digest(&entries));
        assert_eq!(store.latest_digest("common").as_deref(), Some(d.as_str()));
        // the new loader can now read it
        let m = store.load_latest("common").unwrap();
        assert_eq!(m.digest, d);
        assert_eq!(m.files.len(), 2);
        // idempotent + discoverable
        assert!(store.migrate_to_digest("common").unwrap().is_none());
        assert!(store.all_sets().contains(&"common".to_string()));
    }

    #[test]
    fn file_entry_chunks_reassemble_to_original() {
        let dir = tempfile::tempdir().unwrap();
        let cas = Cas::new(dir.path());
        let store = ManifestStore::new(dir.path());
        let input = files();
        let m = store.build_and_write(&cas, "common", &input).unwrap();
        let entry = m.files.iter().find(|f| f.path == "humanoid.glb").unwrap();
        let reassembled: Vec<u8> =
            entry.chunks.iter().flat_map(|h| cas.get(h).unwrap()).collect();
        assert_eq!(reassembled, input[0].1);
        assert_eq!(entry.blake3, Cas::hash(&input[0].1));
        assert_eq!(entry.size, input[0].1.len() as u64);
    }

    #[test]
    fn load_latest_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let cas = Cas::new(dir.path());
        let store = ManifestStore::new(dir.path());
        let written = store.build_and_write(&cas, "zone/qeynos", &files()).unwrap();
        let loaded = store.load_latest("zone/qeynos").unwrap();
        assert_eq!(written, loaded);
    }

    #[test]
    fn unchanged_rebuild_reuses_chunks() {
        let dir = tempfile::tempdir().unwrap();
        let cas = Cas::new(dir.path());
        let store = ManifestStore::new(dir.path());
        let m1 = store.build_and_write(&cas, "common", &files()).unwrap();
        let m2 = store.build_and_write(&cas, "common", &files()).unwrap();
        // identical inputs => identical chunk hash lists (content-addressed dedup)
        assert_eq!(m1.files[0].chunks, m2.files[0].chunks);
    }

    /// #45: `latest` must not repoint at a set that lost files. The store is append-only so
    /// the old manifest survives, but `latest` is what the client follows.
    #[test]
    fn shrinking_set_is_refused_by_default() {
        let dir = tempfile::tempdir().unwrap();
        let cas = Cas::new(dir.path());
        let store = ManifestStore::new(dir.path());

        let two = vec![
            ("a.glb".to_string(), vec![1u8; 1000]),
            ("b.glb".to_string(), vec![2u8; 1000]),
        ];
        let before = store.build_and_write(&cas, "common", &two).unwrap();

        let one = vec![("a.glb".to_string(), vec![1u8; 1000])];
        let err = store.build_and_write(&cas, "common", &one).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("refusing to publish"), "unexpected error: {msg}");
        assert!(msg.contains("--allow-shrink"), "error should name the escape hatch: {msg}");

        // and `latest` still points at the full set
        assert_eq!(store.latest_digest("common").unwrap(), before.digest);
    }

    /// Intentional removals opt in — eqoxide#704's -22 classic `ske*` textures are a real one.
    #[test]
    fn shrinking_set_is_allowed_with_the_flag() {
        let dir = tempfile::tempdir().unwrap();
        let cas = Cas::new(dir.path());
        let store = ManifestStore::new(dir.path()).allow_shrink(true);

        let two = vec![
            ("a.glb".to_string(), vec![1u8; 1000]),
            ("b.glb".to_string(), vec![2u8; 1000]),
        ];
        store.build_and_write(&cas, "common", &two).unwrap();

        let one = vec![("a.glb".to_string(), vec![1u8; 1000])];
        let after = store.build_and_write(&cas, "common", &one).unwrap();
        assert_eq!(after.files.len(), 1);
        assert_eq!(store.latest_digest("common").unwrap(), after.digest);
    }

    /// Growing and same-size republishes are ordinary and must not be blocked.
    #[test]
    fn same_size_and_growing_sets_still_publish() {
        let dir = tempfile::tempdir().unwrap();
        let cas = Cas::new(dir.path());
        let store = ManifestStore::new(dir.path());

        let one = vec![("a.glb".to_string(), vec![1u8; 1000])];
        store.build_and_write(&cas, "common", &one).unwrap();

        // same count, changed content
        let changed = vec![("a.glb".to_string(), vec![9u8; 1000])];
        store.build_and_write(&cas, "common", &changed).unwrap();

        let two = vec![
            ("a.glb".to_string(), vec![9u8; 1000]),
            ("b.glb".to_string(), vec![2u8; 1000]),
        ];
        let grown = store.build_and_write(&cas, "common", &two).unwrap();
        assert_eq!(grown.files.len(), 2);
        assert_eq!(store.latest_digest("common").unwrap(), grown.digest);
    }

    fn cas_chunk_count(dir: &std::path::Path) -> usize {
        std::fs::read_dir(dir.join("cas")).map(|d| d.count()).unwrap_or(0)
    }

    /// #45 review F5: the guard must run before any chunk is written. The CAS has no GC, so
    /// chunking first would strand the refused set's chunks on disk permanently.
    #[test]
    fn a_refused_publish_writes_nothing_to_the_cas() {
        let dir = tempfile::tempdir().unwrap();
        let cas = Cas::new(dir.path());
        let store = ManifestStore::new(dir.path());

        let two = vec![
            ("a.glb".to_string(), vec![1u8; 4096]),
            ("b.glb".to_string(), vec![2u8; 4096]),
        ];
        store.build_and_write(&cas, "common", &two).unwrap();
        let before = cas_chunk_count(dir.path());

        let one = vec![("c.glb".to_string(), vec![3u8; 4096])];
        store.build_and_write(&cas, "common", &one).unwrap_err();

        assert_eq!(
            cas_chunk_count(dir.path()),
            before,
            "a refused publish must not leave orphan chunks in the CAS"
        );
    }

    /// #45 review F3: `latest` present but its manifest missing (a crash between the two
    /// non-atomic writes). Swallowing this with `if let Ok(..)` silently disabled the guard
    /// in exactly the damaged state it exists to protect.
    #[test]
    fn a_dangling_latest_pointer_is_fatal_not_a_silent_pass() {
        let dir = tempfile::tempdir().unwrap();
        let cas = Cas::new(dir.path());
        let store = ManifestStore::new(dir.path());

        let two = vec![
            ("a.glb".to_string(), vec![1u8; 1000]),
            ("b.glb".to_string(), vec![2u8; 1000]),
        ];
        let before = store.build_and_write(&cas, "common", &two).unwrap();
        std::fs::remove_file(
            dir.path().join(format!("manifests/common/{}.json", before.digest)),
        )
        .unwrap();

        let one = vec![("a.glb".to_string(), vec![1u8; 1000])];
        let err = store.build_and_write(&cas, "common", &one).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("refusing to publish"), "unexpected error: {msg}");
        assert!(msg.contains("could not be loaded"), "unexpected error: {msg}");
        assert_eq!(
            store.latest_digest("common").unwrap(),
            before.digest,
            "the damaged set's latest must not be repointed"
        );
    }

    /// A corrupt manifest must be just as fatal as a dangling one.
    #[test]
    fn a_corrupt_latest_manifest_is_fatal() {
        let dir = tempfile::tempdir().unwrap();
        let cas = Cas::new(dir.path());
        let store = ManifestStore::new(dir.path());

        let two = vec![
            ("a.glb".to_string(), vec![1u8; 1000]),
            ("b.glb".to_string(), vec![2u8; 1000]),
        ];
        let before = store.build_and_write(&cas, "common", &two).unwrap();
        std::fs::write(
            dir.path().join(format!("manifests/common/{}.json", before.digest)),
            b"{ truncated",
        )
        .unwrap();

        let one = vec![("a.glb".to_string(), vec![1u8; 1000])];
        let err = store.build_and_write(&cas, "common", &one).unwrap_err();
        assert!(
            err.to_string().contains("refusing to publish"),
            "unexpected error: {err}"
        );
    }

    /// --allow-shrink is an explicit override, so it bypasses the damaged-store checks too
    /// rather than wedging an operator who is deliberately repairing a set.
    #[test]
    fn allow_shrink_still_publishes_over_a_dangling_latest() {
        let dir = tempfile::tempdir().unwrap();
        let cas = Cas::new(dir.path());
        let store = ManifestStore::new(dir.path()).allow_shrink(true);

        let two = vec![
            ("a.glb".to_string(), vec![1u8; 1000]),
            ("b.glb".to_string(), vec![2u8; 1000]),
        ];
        let before = store.build_and_write(&cas, "common", &two).unwrap();
        std::fs::remove_file(
            dir.path().join(format!("manifests/common/{}.json", before.digest)),
        )
        .unwrap();

        let one = vec![("a.glb".to_string(), vec![1u8; 1000])];
        let after = store.build_and_write(&cas, "common", &one).unwrap();
        assert_eq!(store.latest_digest("common").unwrap(), after.digest);
    }

}
