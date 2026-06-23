use std::path::{Path, PathBuf};

use crate::cas::Cas;
use crate::convert::s3d_to_glb;
use crate::manifest::{Manifest, ManifestStore};

/// (archive filename, output model filename, skinned)
const COMMON_MODELS: &[(&str, &str, bool)] = &[
    ("globalhum_chr.s3d", "humanoid.glb", true),
    ("globalhuf_chr.s3d", "humanoid_f.glb", true),
    ("globalelm_chr.s3d", "elf.glb", true),
    ("globalelf_chr.s3d", "elf_f.glb", true),
    ("globaldwm_chr.s3d", "dwarf.glb", true),
    ("globaldwf_chr.s3d", "dwarf_f.glb", true),
];

pub fn build_from_raw(
    cas: &Cas,
    store: &ManifestStore,
    raw_dir: &Path,
    work_dir: &Path,
) -> anyhow::Result<Vec<Manifest>> {
    let common_out = work_dir.join("common");
    std::fs::create_dir_all(&common_out)?;
    for (archive, out_name, skinned) in COMMON_MODELS {
        let src = raw_dir.join(archive);
        if src.exists() {
            s3d_to_glb(&src, &common_out.join(out_name), *skinned)?;
        } else {
            tracing::warn!("skip missing archive {}", archive);
        }
    }
    let common = ingest_dir(cas, store, "common", &common_out)?;
    Ok(vec![common])
}

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, out)?;
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
    collect_files(dir, &mut paths)?;
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
