use std::path::{Path, PathBuf};

pub struct Cas {
    root: PathBuf,
}

impl Cas {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Cas { root: root.into() }
    }

    pub fn hash(bytes: &[u8]) -> String {
        blake3::hash(bytes).to_hex().to_string()
    }

    fn path_for(&self, hash: &str) -> PathBuf {
        self.root.join("cas").join(hash)
    }

    pub fn put(&self, bytes: &[u8]) -> std::io::Result<String> {
        let hash = Self::hash(bytes);
        let path = self.path_for(&hash);
        if !path.exists() {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            // write to a temp file then rename for atomicity
            let tmp = path.with_extension("tmp");
            std::fs::write(&tmp, bytes)?;
            std::fs::rename(&tmp, &path)?;
        }
        Ok(hash)
    }

    pub fn has(&self, hash: &str) -> bool {
        self.path_for(hash).exists()
    }

    pub fn get(&self, hash: &str) -> std::io::Result<Vec<u8>> {
        std::fs::read(self.path_for(hash))
    }
}

impl AsRef<Path> for Cas {
    fn as_ref(&self) -> &Path {
        &self.root
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_is_idempotent_and_content_addressed() {
        let dir = tempfile::tempdir().unwrap();
        let cas = Cas::new(dir.path());
        let h1 = cas.put(b"hello").unwrap();
        let h2 = cas.put(b"hello").unwrap();
        assert_eq!(h1, h2);
        assert_eq!(h1, Cas::hash(b"hello"));
        assert!(cas.has(&h1));
        assert_eq!(cas.get(&h1).unwrap(), b"hello");
    }

    #[test]
    fn distinct_content_distinct_hash() {
        let dir = tempfile::tempdir().unwrap();
        let cas = Cas::new(dir.path());
        assert_ne!(cas.put(b"a").unwrap(), cas.put(b"b").unwrap());
    }

    #[test]
    fn missing_hash_reports_absent() {
        let dir = tempfile::tempdir().unwrap();
        let cas = Cas::new(dir.path());
        assert!(!cas.has("deadbeef"));
        assert!(cas.get("deadbeef").is_err());
    }
}
