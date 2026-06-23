use std::path::{Path, PathBuf};

use crate::cas::Cas;
use crate::manifest::{Manifest, ManifestStore};

fn collect_files(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_files(root, &path, out)?;
        } else {
            out.push(path);
        }
    }
    Ok(())
}

pub fn ingest_dir(
    cas: &Cas,
    store: &ManifestStore,
    set: &str,
    dir: &Path,
) -> anyhow::Result<Manifest> {
    let mut paths = Vec::new();
    collect_files(dir, dir, &mut paths)?;
    paths.sort();

    let mut files = Vec::new();
    for p in paths {
        let rel = p
            .strip_prefix(dir)?
            .to_string_lossy()
            .replace('\\', "/");
        let bytes = std::fs::read(&p)?;
        files.push((rel, bytes));
    }
    store.build_and_write(cas, set, &files)
}
