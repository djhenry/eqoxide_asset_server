use std::path::{Path, PathBuf};

use crate::cas::Cas;
use crate::convert::s3d_to_glb_model;
use crate::manifest::{Manifest, ManifestStore};
use crate::zone::bake_zone;

/// The `common` model set: (source archive, model code, output `.glb`). A `None`
/// model code converts the whole archive (one model per `global*_chr.s3d`); a
/// `Some("XXX")` extracts that single 3-char EQ model out of a multi-model archive.
/// Reproducible from raw EQ files alone (no curated/hand-built artifacts) — other
/// EQEmu operators get the same set from their own `EQ_Files`. Missing/unparseable
/// archives are skipped with a warning, not fatal.
///
/// Two groups:
///  1. The client's render archetypes (`humanoid`, `elf`, … — the names
///     `models::archetype_to_chr_s3d` loads). These must keep their exact names +
///     source archives so the client renders unchanged.
///  2. Every playable race+gender as `race_<archivecode>.glb` (the widest base-race
///     set, for the client to map spawns onto). Named by raw archive code rather
///     than race name to avoid mislabeling (EQ's `hom`/`ham` half-elf/halfling
///     codes are easy to swap).
const COMMON_MODELS: &[(&str, Option<&str>, &str)] = &[
    // --- client render archetypes (match models::archetype_to_chr_s3d) ---
    ("globalhum_chr.s3d",     None,        "humanoid.glb"),  // human male
    ("globalelf_chr.s3d",     None,        "elf.glb"),       // wood elf
    ("globaldwf_chr.s3d",     None,        "dwarf.glb"),     // dwarf
    ("globalgnm_chr.s3d",     None,        "gnoll.glb"),     // gnome (placeholder for gnoll)
    ("globalfroglok_chr.s3d", None,        "frog.glb"),      // froglok
    ("global_chr.s3d",        Some("SKE"), "skeleton.glb"),
    ("befallen_chr.s3d",      Some("ZOM"), "zombie.glb"),
    ("acrylia_chr.s3d",       Some("SPI"), "creature.glb"),  // spider
    ("global2_chr.s3d",       Some("BEA"), "bear.glb"),
    ("global6_chr.s3d",       Some("WOL"), "wolf.glb"),
    ("akanon_chr.s3d",        Some("RAT"), "rat.glb"),
    ("acrylia_chr.s3d",       Some("SNA"), "snake.glb"),
    ("befallen_chr.s3d",      Some("BAT"), "bat.glb"),
    ("airplane_chr.s3d",      Some("WAS"), "wasp.glb"),
    ("burningwood_chr.s3d",   Some("WUR"), "worm.glb"),      // wurm (serpentine)
    ("airplane_chr.s3d",      Some("AVI"), "bird.glb"),      // aviak
    // --- playable races with a standalone model in their own archive, both genders ---
    // Omitted: dark elf (dam/daf), erudite (erm/erf), halfling (ham/haf), high elf (him/hif).
    // Their `_chr` archives carry only texture/variation data and share another race's
    // skeleton, so converting them standalone yields a ~350 KB partial ("blob"). EQ/the
    // client renders them from a base race anyway (DKE/HEF→elf, ERU→humanoid).
    ("globalhum_chr.s3d",     None, "race_hum.glb"), ("globalhuf_chr.s3d", None, "race_huf.glb"),
    ("globalbam_chr.s3d",     None, "race_bam.glb"), ("globalbaf_chr.s3d", None, "race_baf.glb"),
    ("globalelm_chr.s3d",     None, "race_elm.glb"), ("globalelf_chr.s3d", None, "race_elf.glb"),
    ("globalhom_chr.s3d",     None, "race_hom.glb"), ("globalhof_chr.s3d", None, "race_hof.glb"),
    ("globaldwm_chr.s3d",     None, "race_dwm.glb"), ("globaldwf_chr.s3d", None, "race_dwf.glb"),
    ("globalgnm_chr.s3d",     None, "race_gnm.glb"), ("globalgnf_chr.s3d", None, "race_gnf.glb"),
    ("globalikm_chr.s3d",     None, "race_ikm.glb"), ("globalikf_chr.s3d", None, "race_ikf.glb"),
    ("globalkem_chr.s3d",     None, "race_kem.glb"), ("globalkef_chr.s3d", None, "race_kef.glb"),
    ("globalogm_chr.s3d",     None, "race_ogm.glb"), ("globalogf_chr.s3d", None, "race_ogf.glb"),
    ("globaltrm_chr.s3d",     None, "race_trm.glb"), ("globaltrf_chr.s3d", None, "race_trf.glb"),
    ("globalpcfroglok_chr.s3d", None, "race_pcfroglok.glb"),
];

pub fn build_from_raw(
    cas: &Cas,
    store: &ManifestStore,
    raw_dir: &Path,
    work_dir: &Path,
) -> anyhow::Result<Vec<Manifest>> {
    let common_out = work_dir.join("common");
    std::fs::create_dir_all(&common_out)?;
    for (archive, model_code, out_name) in COMMON_MODELS {
        let src = raw_dir.join(archive);
        if !src.exists() {
            tracing::warn!("skip missing archive {archive} (for {out_name})");
            continue;
        }
        // Per-model conversion can panic on malformed archives; isolate each so one
        // bad model doesn't abort the whole common build.
        let out = common_out.join(out_name);
        let result = std::panic::catch_unwind(|| s3d_to_glb_model(&src, &out, true, *model_code));
        match result {
            Ok(Ok(())) => {}
            Ok(Err(e)) => tracing::warn!("skip model {out_name} from {archive}: {}", short_err(&e)),
            Err(_) => tracing::warn!("skip model {out_name} from {archive}: conversion panicked"),
        }
    }
    let common = ingest_dir(cas, store, "common", &common_out)?;
    Ok(vec![common])
}

/// True for archives that hold zone terrain (`<short>.s3d`), excluding the
/// character/object/armor companion archives (`_chr`, `_obj`, `_amr`, with any
/// trailing digits like `_chr2`/`_obj2`) and the global/equipment/sky archives.
fn is_zone_archive(name: &str) -> bool {
    let n = name.to_lowercase();
    let Some(stem) = n.strip_suffix(".s3d") else { return false };
    if stem.starts_with("global") || stem.starts_with("gequip") || stem == "sky" {
        return false;
    }
    // Non-terrain archives that sit in EQ_Files: loading-screen bitmaps (bmpwad*),
    // per-zone lighting (`*_lit`), and the shared grass texture archive.
    if stem.starts_with("bmpwad") || stem.ends_with("_lit") || stem == "grass" {
        return false;
    }
    // Reject companion archives: a `_chr`/`_obj`/`_amr` tag followed only by digits.
    for tag in ["_chr", "_obj", "_amr"] {
        if let Some(pos) = stem.rfind(tag) {
            let rest = &stem[pos + tag.len()..];
            if rest.is_empty() || rest.chars().all(|c| c.is_ascii_digit()) {
                return false;
            }
        }
    }
    true
}

/// Trim a (possibly multi-megabyte) libeq error to a single readable line — its
/// Debug output embeds raw fragment bytes that otherwise flood the log.
fn short_err(e: &impl std::fmt::Display) -> String {
    let mut s = e.to_string();
    if let Some(nl) = s.find('\n') { s.truncate(nl); }
    if s.len() > 200 { s.truncate(200); s.push('…'); }
    s
}

pub fn build_zones_from_raw(cas: &Cas, store: &ManifestStore, raw_dir: &Path, work_dir: &Path)
    -> anyhow::Result<Vec<String>>
{
    // libeq panics (not Errs) on some malformed WLDs; we catch_unwind each zone and
    // log a clean WARN, so silence the default hook's verbose backtrace dump.
    std::panic::set_hook(Box::new(|_| {}));
    let mut baked = Vec::new();
    for entry in std::fs::read_dir(raw_dir)? {
        let path = entry?.path();
        let fname = match path.file_name().and_then(|s| s.to_str()) { Some(f) => f, None => continue };
        if !is_zone_archive(fname) { continue; }
        let short = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
        let obj = raw_dir.join(format!("{short}_obj.s3d"));
        let zdir = work_dir.join("zone").join(&short);
        std::fs::create_dir_all(&zdir)?;
        let glb = zdir.join(format!("{short}.glb"));
        let result = std::panic::catch_unwind(|| {
            bake_zone(&path, obj.exists().then_some(obj.as_path()), &glb)
        });
        match result {
            Ok(Ok(())) => {
                ingest_dir(cas, store, &format!("zone/{short}"), &zdir)?;
                baked.push(short);
            }
            Ok(Err(e)) => tracing::warn!("skip zone {short}: {}", short_err(&e)),
            Err(payload) => {
                let msg = payload
                    .downcast_ref::<&str>()
                    .copied()
                    .or_else(|| payload.downcast_ref::<String>().map(|s| s.as_str()))
                    .unwrap_or("<non-string panic>");
                tracing::warn!("skip zone {short}: bake_zone panicked: {}", short_err(&msg));
            }
        }
    }
    Ok(baked)
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

#[cfg(test)]
mod tests {
    use super::is_zone_archive;
    #[test]
    fn zone_vs_companion_archives() {
        // real zones
        for z in ["qeynos.s3d", "acrylia.s3d", "qey2hh1.s3d", "freporte.s3d", "poair.s3d"] {
            assert!(is_zone_archive(z), "{z} should be a zone");
        }
        // companion / non-zone archives
        for c in [
            "qeynos_obj.s3d", "acrylia_chr2.s3d", "greatdivide_obj2.s3d",
            "lgequip_amr2.s3d", "global_chr.s3d", "gequip.s3d", "sky.s3d",
            "qeynos_chr.s3d", "qeynos_amr.s3d",
        ] {
            assert!(!is_zone_archive(c), "{c} should NOT be a zone");
        }
    }
}
