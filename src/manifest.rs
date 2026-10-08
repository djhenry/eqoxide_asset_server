use crate::cas::Cas;
use crate::compatibility::{ReaderRequirements, valid_hash, valid_path};
use anyhow::ensure;
use std::collections::BTreeSet;
use std::io::Write;
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
    pub schema_version: u32,
    pub revision: String,
    pub requirements: ReaderRequirements,
    pub set: String,
    /// Content identity of the set: blake3 over the sorted (path, file-blake3) list. Same content
    /// yields the same digest on any server, so the client can skip an unchanged set and never
    /// cross-contaminate between servers with diverging custom assets.
    pub digest: String,
    pub files: Vec<FileEntry>,
}

impl Manifest {
    pub fn canonical_revision(&self) -> anyhow::Result<String> {
        let capabilities: BTreeSet<_> = self.requirements.capabilities.iter().collect();
        let mut files: Vec<_> = self.files.iter().collect();
        files.sort_by(|a,b| a.path.cmp(&b.path));
        let files: Vec<_> = files.iter().map(|f| (&f.path, f.size, &f.blake3, &f.chunks)).collect();
        let bytes = serde_json::to_vec(&(self.schema_version, &self.set, &self.digest,
            self.requirements.reader_version, capabilities, files))?;
        let mut h = blake3::Hasher::new();
        h.update(b"eqoxide-manifest-v1\0"); h.update(&bytes);
        Ok(h.finalize().to_hex().to_string())
    }
    pub fn validate(&self, set: &str) -> anyhow::Result<()> {
        ensure!(self.schema_version == 1, "unsupported manifest schema");
        ensure!(valid_path(&self.set) && self.set == set, "invalid or mismatched manifest set");
        self.requirements.validate()?;
        let mut paths = BTreeSet::new();
        for f in &self.files {
            ensure!(valid_path(&f.path) && paths.insert(&f.path), "invalid or duplicate file path");
            ensure!(valid_hash(&f.blake3) && f.chunks.iter().all(|s| valid_hash(s)), "invalid content reference");
        }
        ensure!(valid_hash(&self.digest) && self.digest == ManifestStore::set_digest(&self.files), "manifest content digest mismatch");
        ensure!(valid_hash(&self.revision) && self.revision == self.canonical_revision()?, "manifest revision mismatch");
        Ok(())
    }
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

    /// Current immutable manifest revision (historical method name retained).
    pub fn latest_digest(&self, set: &str) -> Option<String> {
        if !valid_path(set) { return None; }
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
        requirements: ReaderRequirements,
    ) -> anyhow::Result<Manifest> {
        requirements.validate()?;
        ensure!(valid_path(set), "invalid set path");
        let mut paths = BTreeSet::new();
        ensure!(files.iter().all(|(path,_)| valid_path(path) && paths.insert(path)), "invalid or duplicate file path");
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

        let mut manifest = Manifest { schema_version: 1, revision: String::new(), requirements,
            set: set.to_string(), digest, files: entries };
        manifest.revision = manifest.canonical_revision()?;
        self.publish(&manifest)?;
        Ok(manifest)
    }

    fn publish(&self, manifest: &Manifest) -> anyhow::Result<()> {
        manifest.validate(&manifest.set)?;
        let dir = self.set_dir(&manifest.set);
        std::fs::create_dir_all(&dir)?;
        fn atomic(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
            let mut temp = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
            temp.write_all(bytes)?; temp.as_file().sync_all()?;
            temp.persist(path).map_err(|e| e.error)?;
            Ok(())
        }
        atomic(&dir.join(format!("{}.json", manifest.revision)), &serde_json::to_vec_pretty(manifest)?)?;
        atomic(&dir.join("latest"), manifest.revision.as_bytes())?;
        Ok(())
    }

    pub fn load(&self, set: &str, digest: &str) -> anyhow::Result<Manifest> {
        ensure!(valid_path(set) && valid_hash(digest), "invalid manifest reference");
        let p = self.set_dir(set).join(format!("{digest}.json"));
        let bytes = std::fs::read(p)?;
        let manifest: Manifest = serde_json::from_slice(&bytes)?;
        manifest.validate(set)?;
        ensure!(manifest.revision == digest, "stored revision mismatch");
        Ok(manifest)
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

    /// Upgrade numeric or content-digest manifests to validated reader envelopes.
    /// Reuse verified chunks and preserve previous manifests for rollback.
    pub fn migrate_to_digest(&self, set: &str) -> anyhow::Result<Option<String>> {
        ensure!(valid_path(set), "invalid set path");
        let dir = self.set_dir(set);
        let latest = std::fs::read_to_string(dir.join("latest"))?.trim().to_string();
        ensure!(valid_hash(&latest) || (!latest.is_empty() && latest.bytes().all(|c| c.is_ascii_digit())), "invalid legacy manifest reference");
        let bytes = std::fs::read(dir.join(format!("{latest}.json")))?;
        let value: serde_json::Value = serde_json::from_slice(&bytes)?;
        if value.get("schema_version").is_some() {
            self.load(set, &latest)?;
            return Ok(None);
        }
        ensure!(value.get("requirements").is_none() && value.get("revision").is_none(), "partial manifest envelope");
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Legacy {
            #[serde(default)] set: String,
            #[serde(default)] digest: Option<String>,
            #[serde(default)] version: Option<u64>,
            files: Vec<FileEntry>,
        }
        let legacy: Legacy = serde_json::from_slice(&bytes)?;
        ensure!(legacy.set.is_empty() || legacy.set == set, "legacy set mismatch");
        let digest = Self::set_digest(&legacy.files);
        if valid_hash(&latest) {
            ensure!(legacy.digest.as_deref() == Some(digest.as_str()) && latest == digest, "legacy digest mismatch");
        } else {
            ensure!(legacy.version == Some(latest.parse()?), "legacy version mismatch");
            ensure!(legacy.digest.as_ref().is_none_or(|d| d == &digest), "legacy digest mismatch");
        }
        let mut manifest = Manifest { schema_version: 1, revision: String::new(), requirements: ReaderRequirements::legacy(), set: set.into(), digest, files: legacy.files };
        manifest.revision = manifest.canonical_revision()?;
        // Validate references before reading CAS; publish also validates the publication boundary.
        manifest.validate(set)?;
        let cas = Cas::new(&self.root);
        for file in &manifest.files {
            let mut hash = blake3::Hasher::new(); let mut size = 0u64;
            for chunk in &file.chunks {
                let bytes = cas.get(chunk)?;
                ensure!(Cas::hash(&bytes) == *chunk, "corrupt migration chunk");
                size = size.checked_add(bytes.len() as u64).ok_or_else(|| anyhow::anyhow!("file size overflow"))?;
                hash.update(&bytes);
            }
            ensure!(size == file.size && hash.finalize().to_hex().as_str() == file.blake3, "migration file mismatch");
        }
        self.publish(&manifest)?;
        Ok(Some(manifest.revision))
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

        let m = store.build_and_write(&cas, "common", &files(), crate::compatibility::ReaderRequirements::legacy()).unwrap();
        assert_eq!(m.set, "common");
        assert_eq!(m.files.len(), 2);
        assert_eq!(m.revision.len(), 64);
        assert!(store.set_dir("common").join(format!("{}.json", m.revision)).exists());
        assert_eq!(
            std::fs::read_to_string(store.set_dir("common").join("latest")).unwrap(),
            m.revision
        );
        assert_eq!(store.latest_digest("common").as_deref(), Some(m.revision.as_str()));
    }

    #[test]
    fn identical_rebuild_dedups_no_new_file() {
        let dir = tempfile::tempdir().unwrap();
        let cas = Cas::new(dir.path());
        let store = ManifestStore::new(dir.path());
        let m1 = store.build_and_write(&cas, "common", &files(), crate::compatibility::ReaderRequirements::legacy()).unwrap();
        let count1 = std::fs::read_dir(store.set_dir("common")).unwrap().count();
        let m2 = store.build_and_write(&cas, "common", &files(), crate::compatibility::ReaderRequirements::legacy()).unwrap();
        let count2 = std::fs::read_dir(store.set_dir("common")).unwrap().count();
        assert_eq!(m1.digest, m2.digest);
        assert_eq!(count1, count2); // <digest>.json + latest, no churn
    }

    #[test]
    fn migrate_legacy_to_digest_idempotent_and_loadable() {
        let dir = tempfile::tempdir().unwrap();
        let store = ManifestStore::new(dir.path());
        let cas = Cas::new(dir.path());
        let entries = vec![fe("b.bin", &cas.put(b"b").unwrap()), fe("a.bin", &cas.put(b"a").unwrap())];
        // hand-write a legacy version-keyed manifest
        let sd = store.set_dir("common");
        std::fs::create_dir_all(&sd).unwrap();
        let legacy = serde_json::json!({ "set": "common", "version": 7, "files": entries });
        std::fs::write(sd.join("7.json"), serde_json::to_vec_pretty(&legacy).unwrap()).unwrap();
        std::fs::write(sd.join("latest"), "7").unwrap();

        let d = store.migrate_to_digest("common").unwrap().unwrap();
        assert_ne!(d, ManifestStore::set_digest(&entries));
        assert_eq!(store.latest_digest("common").as_deref(), Some(d.as_str()));
        // the new loader can now read it
        let m = store.load_latest("common").unwrap();
        assert_eq!(m.revision, d);
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
        let m = store.build_and_write(&cas, "common", &input, crate::compatibility::ReaderRequirements::legacy()).unwrap();
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
        let written = store.build_and_write(&cas, "zone/qeynos", &files(), crate::compatibility::ReaderRequirements::legacy()).unwrap();
        let loaded = store.load_latest("zone/qeynos").unwrap();
        assert_eq!(written, loaded);
    }

    #[test]
    fn unchanged_rebuild_reuses_chunks() {
        let dir = tempfile::tempdir().unwrap();
        let cas = Cas::new(dir.path());
        let store = ManifestStore::new(dir.path());
        let m1 = store.build_and_write(&cas, "common", &files(), crate::compatibility::ReaderRequirements::legacy()).unwrap();
        let m2 = store.build_and_write(&cas, "common", &files(), crate::compatibility::ReaderRequirements::legacy()).unwrap();
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
        let before = store.build_and_write(&cas, "common", &two, crate::compatibility::ReaderRequirements::legacy()).unwrap();

        let one = vec![("a.glb".to_string(), vec![1u8; 1000])];
        let err = store.build_and_write(&cas, "common", &one, crate::compatibility::ReaderRequirements::legacy()).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("refusing to publish"), "unexpected error: {msg}");
        assert!(msg.contains("--allow-shrink"), "error should name the escape hatch: {msg}");

        // and `latest` still points at the full set
        assert_eq!(store.latest_digest("common").unwrap(), before.revision);
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
        store.build_and_write(&cas, "common", &two, crate::compatibility::ReaderRequirements::legacy()).unwrap();

        let one = vec![("a.glb".to_string(), vec![1u8; 1000])];
        let after = store.build_and_write(&cas, "common", &one, crate::compatibility::ReaderRequirements::legacy()).unwrap();
        assert_eq!(after.files.len(), 1);
        assert_eq!(store.latest_digest("common").unwrap(), after.revision);
    }

    /// Growing and same-size republishes are ordinary and must not be blocked.
    #[test]
    fn same_size_and_growing_sets_still_publish() {
        let dir = tempfile::tempdir().unwrap();
        let cas = Cas::new(dir.path());
        let store = ManifestStore::new(dir.path());

        let one = vec![("a.glb".to_string(), vec![1u8; 1000])];
        store.build_and_write(&cas, "common", &one, crate::compatibility::ReaderRequirements::legacy()).unwrap();

        // same count, changed content
        let changed = vec![("a.glb".to_string(), vec![9u8; 1000])];
        store.build_and_write(&cas, "common", &changed, crate::compatibility::ReaderRequirements::legacy()).unwrap();

        let two = vec![
            ("a.glb".to_string(), vec![9u8; 1000]),
            ("b.glb".to_string(), vec![2u8; 1000]),
        ];
        let grown = store.build_and_write(&cas, "common", &two, crate::compatibility::ReaderRequirements::legacy()).unwrap();
        assert_eq!(grown.files.len(), 2);
        assert_eq!(store.latest_digest("common").unwrap(), grown.revision);
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
        store.build_and_write(&cas, "common", &two, crate::compatibility::ReaderRequirements::legacy()).unwrap();
        let before = cas_chunk_count(dir.path());

        let one = vec![("c.glb".to_string(), vec![3u8; 4096])];
        store.build_and_write(&cas, "common", &one, crate::compatibility::ReaderRequirements::legacy()).unwrap_err();

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
        let before = store.build_and_write(&cas, "common", &two, crate::compatibility::ReaderRequirements::legacy()).unwrap();
        std::fs::remove_file(
            dir.path().join(format!("manifests/common/{}.json", before.revision)),
        )
        .unwrap();

        let one = vec![("a.glb".to_string(), vec![1u8; 1000])];
        let err = store.build_and_write(&cas, "common", &one, crate::compatibility::ReaderRequirements::legacy()).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("refusing to publish"), "unexpected error: {msg}");
        assert!(msg.contains("could not be loaded"), "unexpected error: {msg}");
        assert_eq!(
            store.latest_digest("common").unwrap(),
            before.revision,
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
        let before = store.build_and_write(&cas, "common", &two, crate::compatibility::ReaderRequirements::legacy()).unwrap();
        std::fs::write(
            dir.path().join(format!("manifests/common/{}.json", before.revision)),
            b"{ truncated",
        )
        .unwrap();

        let one = vec![("a.glb".to_string(), vec![1u8; 1000])];
        let err = store.build_and_write(&cas, "common", &one, crate::compatibility::ReaderRequirements::legacy()).unwrap_err();
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
        let before = store.build_and_write(&cas, "common", &two, crate::compatibility::ReaderRequirements::legacy()).unwrap();
        std::fs::remove_file(
            dir.path().join(format!("manifests/common/{}.json", before.revision)),
        )
        .unwrap();

        let one = vec![("a.glb".to_string(), vec![1u8; 1000])];
        let after = store.build_and_write(&cas, "common", &one, crate::compatibility::ReaderRequirements::legacy()).unwrap();
        assert_eq!(store.latest_digest("common").unwrap(), after.revision);
    }

}
