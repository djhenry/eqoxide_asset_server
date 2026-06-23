use crate::cas::Cas;
use crate::chunker::chunk_into;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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
    pub version: u64,
    pub files: Vec<FileEntry>,
}

pub struct ManifestStore {
    root: PathBuf,
}

impl ManifestStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        ManifestStore { root: root.into() }
    }

    fn set_dir(&self, set: &str) -> PathBuf {
        self.root.join("manifests").join(set)
    }

    pub fn latest_version(&self, set: &str) -> Option<u64> {
        let p = self.set_dir(set).join("latest");
        std::fs::read_to_string(p).ok()?.trim().parse().ok()
    }

    pub fn build_and_write(
        &self,
        cas: &Cas,
        set: &str,
        files: &[(String, Vec<u8>)],
    ) -> anyhow::Result<Manifest> {
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
        let version = self.latest_version(set).unwrap_or(0) + 1;
        let manifest = Manifest { set: set.to_string(), version, files: entries };

        let dir = self.set_dir(set);
        std::fs::create_dir_all(&dir)?;
        let json = serde_json::to_vec_pretty(&manifest)?;
        std::fs::write(dir.join(format!("{version}.json")), json)?;
        std::fs::write(dir.join("latest"), version.to_string())?;
        Ok(manifest)
    }

    pub fn load(&self, set: &str, version: u64) -> anyhow::Result<Manifest> {
        let p = self.set_dir(set).join(format!("{version}.json"));
        let bytes = std::fs::read(p)?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    pub fn load_latest(&self, set: &str) -> anyhow::Result<Manifest> {
        let v = self
            .latest_version(set)
            .ok_or_else(|| anyhow::anyhow!("no manifest for set {set}"))?;
        self.load(set, v)
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

    #[test]
    fn build_writes_manifest_and_increments_version() {
        let dir = tempfile::tempdir().unwrap();
        let cas = Cas::new(dir.path());
        let store = ManifestStore::new(dir.path());

        let m1 = store.build_and_write(&cas, "common", &files()).unwrap();
        assert_eq!(m1.version, 1);
        assert_eq!(m1.set, "common");
        assert_eq!(m1.files.len(), 2);
        assert_eq!(store.latest_version("common"), Some(1));

        let m2 = store.build_and_write(&cas, "common", &files()).unwrap();
        assert_eq!(m2.version, 2);
        assert_eq!(store.latest_version("common"), Some(2));
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
}
