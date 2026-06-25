use std::path::{Path, PathBuf};

use rayon::prelude::*;

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
    // Every playable race+gender has its own `global<code>_chr.s3d` skeletal model in
    // the Titanium client (the client loads a distinct archive per race; there is no
    // model sharing). The client maps each race to its own model with no fallback.
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
    ("globalerm_chr.s3d",     None, "race_erm.glb"), ("globalerf_chr.s3d", None, "race_erf.glb"),
    ("globalhim_chr.s3d",     None, "race_him.glb"), ("globalhif_chr.s3d", None, "race_hif.glb"),
    ("globaldam_chr.s3d",     None, "race_dam.glb"), ("globaldaf_chr.s3d", None, "race_daf.glb"),
    ("globalham_chr.s3d",     None, "race_ham.glb"), ("globalhaf_chr.s3d", None, "race_haf.glb"),
    ("globalpcfroglok_chr.s3d", None, "race_pcfroglok.glb"),
];

/// Resolve the worker-thread count for a build. An explicit `--jobs N` (already
/// validated `>= 1` by clap) is used as-is; otherwise default to all-but-one core,
/// floored at 1, falling back to 1 if the core count can't be determined.
pub fn resolve_jobs(requested: Option<usize>) -> usize {
    match requested {
        Some(n) => n,
        None => std::thread::available_parallelism()
            .map(|n| n.get().saturating_sub(1))
            .unwrap_or(1)
            .max(1),
    }
}

pub fn build_from_raw(
    cas: &Cas,
    store: &ManifestStore,
    raw_dir: &Path,
    work_dir: &Path,
    pool: &rayon::ThreadPool,
) -> anyhow::Result<Vec<Manifest>> {
    let common_out = work_dir.join("common");
    std::fs::create_dir_all(&common_out)?;
    pool.install(|| {
        COMMON_MODELS.par_iter().for_each(|(archive, model_code, out_name)| {
            let src = raw_dir.join(archive);
            if !src.exists() {
                tracing::warn!("skip missing archive {archive} (for {out_name})");
                return;
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
        });
    });
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

/// Build the "zonedoors/<short>" set: the zone's object archive (`<short>_obj.s3d`) so the client
/// renders clickable-door 3D models from the asset-server cache instead of ~/eq_assets. Door object
/// models live in `_obj.s3d`; the client's load_object_models skips the main `.s3d` if it's absent,
/// so we don't ship the (large, GLB-redundant) terrain archive. Returns None if the zone has no obj.
pub fn build_zonedoors_from_raw(cas: &Cas, store: &ManifestStore, raw_dir: &Path, short: &str)
    -> anyhow::Result<Option<Manifest>>
{
    let obj = raw_dir.join(format!("{short}_obj.s3d"));
    if !obj.exists() { return Ok(None); }
    let files = vec![(format!("{short}_obj.s3d"), std::fs::read(&obj)?)];
    Ok(Some(store.build_and_write(cas, &format!("zonedoors/{short}"), &files)?))
}

pub fn build_zones_from_raw(
    cas: &Cas,
    store: &ManifestStore,
    raw_dir: &Path,
    work_dir: &Path,
    pool: &rayon::ThreadPool,
) -> anyhow::Result<Vec<String>> {
    // libeq panics (not Errs) on some malformed WLDs; we catch_unwind each zone and
    // log a clean WARN, so silence the default hook's verbose backtrace dump. Set once
    // before the parallel region (the hook is process-global).
    std::panic::set_hook(Box::new(|_| {}));

    // Collect zone archive paths first, then fan out the per-zone conversion.
    let mut zone_paths = Vec::new();
    for entry in std::fs::read_dir(raw_dir)? {
        let path = entry?.path();
        let fname = match path.file_name().and_then(|s| s.to_str()) { Some(f) => f.to_string(), None => continue };
        if !is_zone_archive(&fname) { continue; }
        zone_paths.push(path);
    }

    let baked: anyhow::Result<Vec<Option<String>>> = pool.install(|| {
        zone_paths
            .par_iter()
            .map(|path| -> anyhow::Result<Option<String>> {
                let short = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
                let obj = raw_dir.join(format!("{short}_obj.s3d"));
                let zdir = work_dir.join("zone").join(&short);
                std::fs::create_dir_all(&zdir)?;
                let glb = zdir.join(format!("{short}.glb"));
                let result = std::panic::catch_unwind(|| {
                    bake_zone(path, obj.exists().then_some(obj.as_path()), &glb)
                });
                match result {
                    Ok(Ok(())) => {
                        ingest_dir(cas, store, &format!("zone/{short}"), &zdir)?;
                        if let Err(e) = build_zonedoors_from_raw(cas, store, raw_dir, &short) {
                            tracing::warn!("zonedoors {short}: {}", short_err(&e));
                        }
                        Ok(Some(short))
                    }
                    Ok(Err(e)) => {
                        tracing::warn!("skip zone {short}: {}", short_err(&e));
                        Ok(None)
                    }
                    Err(payload) => {
                        let msg = payload
                            .downcast_ref::<&str>()
                            .copied()
                            .or_else(|| payload.downcast_ref::<String>().map(|s| s.as_str()))
                            .unwrap_or("<non-string panic>");
                        tracing::warn!("skip zone {short}: bake_zone panicked: {}", short_err(&msg));
                        Ok(None)
                    }
                }
            })
            .collect()
    });

    let mut baked: Vec<String> = baked?.into_iter().flatten().collect();
    baked.sort();
    Ok(baked)
}

/// The equipment/weapon S3D archives the client needs to texture worn armor and render held
/// weapons. Kept in sync with eq_client_lite renderer.rs (index_s3d_textures globals + ARCHETYPES /
/// archetype_to_chr_s3d) and assets.rs load_weapon_model (gequip*). Served raw (the client parses
/// S3D); this removes the client's direct dependency on the ~/eq_assets folder.
const GAMEEQUIP_ARCHIVES: &[&str] = &[
    // armor texture sets (global17_amr.s3d .. global23_amr.s3d)
    "global17_amr.s3d", "global18_amr.s3d", "global19_amr.s3d", "global20_amr.s3d",
    "global21_amr.s3d", "global22_amr.s3d", "global23_amr.s3d",
    // combined all-races base body textures
    "global_chr.s3d",
    // per-archetype character archives (body textures for the mapped player races)
    "globalhum_chr.s3d", "globalhum_chr2.s3d",
    "globalelf_chr.s3d", "globalelf_chr2.s3d",
    "globaldwf_chr.s3d", "globaldwf_chr2.s3d",
    "globalgnm_chr.s3d", "globalgnm_chr2.s3d",
    "globalfroglok_chr.s3d",
    // held weapon models
    "gequip.s3d", "gequip2.s3d", "gequip3.s3d", "gequip4.s3d",
    "gequip5.s3d", "gequip6.s3d", "gequip7.s3d", "gequip8.s3d",
];

/// Build the "gameequip" set: the equipment/weapon S3D archives (served raw, see
/// GAMEEQUIP_ARCHIVES) so the client loads worn-armor textures + held-weapon models from the asset
/// server cache instead of reading ~/eq_assets at runtime.
pub fn build_gameequip_from_raw(
    cas: &Cas,
    store: &ManifestStore,
    raw_dir: &Path,
) -> anyhow::Result<Manifest> {
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    for name in GAMEEQUIP_ARCHIVES {
        let p = raw_dir.join(name);
        if p.exists() {
            files.push((name.to_string(), std::fs::read(&p)?));
        } else {
            tracing::warn!("gameequip: missing {name} in {}", raw_dir.display());
        }
    }
    store.build_and_write(cas, "gameequip", &files)
}

/// Build the "gamedata" set: the runtime TEXT game data the client needs but shouldn't read from
/// ~/eq_assets at runtime — the string table (eqstr_us.txt), spell DB (spells_us.txt), and the zone
/// maps tree (maps/, including the water-region maps/water/*.wtr). Files keep their relative paths
/// ("eqstr_us.txt", "maps/qcat.txt", "maps/water/qcat.wtr") so the client finds them in its cache.
pub fn build_gamedata_from_raw(
    cas: &Cas,
    store: &ManifestStore,
    raw_dir: &Path,
) -> anyhow::Result<Manifest> {
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    for name in ["eqstr_us.txt", "spells_us.txt"] {
        let p = raw_dir.join(name);
        if p.exists() {
            files.push((name.to_string(), std::fs::read(&p)?));
        } else {
            tracing::warn!("gamedata: missing {name} in {}", raw_dir.display());
        }
    }
    let maps = raw_dir.join("maps");
    if maps.is_dir() {
        let mut paths = Vec::new();
        collect_files(&maps, &mut paths)?;
        paths.sort();
        for p in paths {
            let rel = format!("maps/{}", p.strip_prefix(&maps)?.to_string_lossy().replace('\\', "/"));
            files.push((rel, std::fs::read(&p)?));
        }
    } else {
        tracing::warn!("gamedata: no maps/ dir in {}", raw_dir.display());
    }
    store.build_and_write(cas, "gamedata", &files)
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
    use super::resolve_jobs;
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

    #[test]
    fn resolve_jobs_honors_explicit_request() {
        assert_eq!(resolve_jobs(Some(3)), 3);
        assert_eq!(resolve_jobs(Some(1)), 1);
    }

    #[test]
    fn resolve_jobs_default_is_at_least_one() {
        assert!(resolve_jobs(None) >= 1);
    }
}
