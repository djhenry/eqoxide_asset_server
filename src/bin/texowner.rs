//! Diagnostic: find zone textures where the main `.s3d` and the sibling `_obj.s3d`
//! both contain a file of the same name but with DIFFERENT bytes, and the object
//! WLD is the one that references it. `bake_zone` searches [main, obj] in that
//! order, so those resolve to the main archive's (stale) copy.
//!
//! Usage:
//!   texowner scan <dir>              — sweep every zone, report conflicts
//!   texowner one <dir> <zone> [tex…] — detail for a single zone

use std::collections::{HashMap, HashSet};

/// Distinct base-color texture names referenced by every WLD in an archive.
fn wld_texture_refs(pfs: &mut libeq_pfs::PfsReader<std::fs::File>) -> HashSet<String> {
    let mut out = HashSet::new();
    let names: Vec<String> = match pfs.filenames() {
        Ok(n) => n,
        Err(_) => return out,
    };
    for wn in names.iter().filter(|f| f.to_lowercase().ends_with(".wld")) {
        let bytes = match pfs.get(wn) {
            Ok(Some(b)) => b,
            _ => continue,
        };
        let wld = match libeq_wld::load(&bytes) {
            Ok(w) => w,
            Err(_) => continue,
        };
        for mesh in wld.meshes() {
            for prim in mesh.primitives() {
                if let Some(src) = prim
                    .material()
                    .base_color_texture()
                    .as_ref()
                    .and_then(|t| t.source())
                {
                    out.insert(src.to_lowercase());
                }
            }
        }
    }
    out
}

fn file_map(pfs: &mut libeq_pfs::PfsReader<std::fs::File>) -> HashMap<String, Vec<u8>> {
    let mut m = HashMap::new();
    let names: Vec<String> = pfs.filenames().unwrap_or_default();
    for n in names {
        let lower = n.to_lowercase();
        if lower.ends_with(".wld") {
            continue;
        }
        if let Ok(Some(b)) = pfs.get(&n) {
            m.insert(lower, b);
        }
    }
    m
}

fn fourcc(d: &[u8]) -> String {
    if d.len() >= 128 && &d[0..4] == b"DDS " {
        String::from_utf8_lossy(&d[84..88]).to_string()
    } else if d.len() >= 2 && &d[0..2] == b"BM" {
        "BMP".to_string()
    } else {
        "?".to_string()
    }
}

fn open(path: &std::path::Path) -> Option<libeq_pfs::PfsReader<std::fs::File>> {
    libeq_pfs::PfsReader::open(std::fs::File::open(path).ok()?).ok()
}

/// Fraction of top-mip texels that are transparent (alpha < 128), for DDS DXT1/3/5.
/// Returns `None` for anything we don't decode here.
fn transparent_fraction(d: &[u8]) -> Option<f32> {
    if d.len() < 128 || &d[0..4] != b"DDS " {
        return None;
    }
    let h = u32::from_le_bytes([d[12], d[13], d[14], d[15]]);
    let w = u32::from_le_bytes([d[16], d[17], d[18], d[19]]);
    let blocks = (w.div_ceil(4) as usize) * (h.div_ceil(4) as usize);
    let px = &d[128..];
    let mut clear = 0usize;
    match &d[84..88] {
        b"DXT1" => {
            let n = blocks.min(px.len() / 8);
            for i in 0..n {
                let b = &px[i * 8..i * 8 + 8];
                let c0 = u16::from_le_bytes([b[0], b[1]]);
                let c1 = u16::from_le_bytes([b[2], b[3]]);
                if c0 > c1 {
                    continue;
                }
                let tbl = u32::from_le_bytes([b[4], b[5], b[6], b[7]]);
                for t in 0..16 {
                    if (tbl >> (t * 2)) & 3 == 3 {
                        clear += 1;
                    }
                }
            }
            Some(clear as f32 / (n * 16).max(1) as f32)
        }
        b"DXT3" => {
            let n = blocks.min(px.len() / 16);
            for i in 0..n {
                let b = &px[i * 16..i * 16 + 8];
                for t in 0..16 {
                    let nib = (b[t / 2] >> ((t % 2) * 4)) & 0xF;
                    if nib * 17 < 128 {
                        clear += 1;
                    }
                }
            }
            Some(clear as f32 / (n * 16).max(1) as f32)
        }
        b"DXT5" => {
            let n = blocks.min(px.len() / 16);
            for i in 0..n {
                let b = &px[i * 16..i * 16 + 16];
                let (a0, a1) = (b[0] as u16, b[1] as u16);
                let bits = u64::from_le_bytes([b[2], b[3], b[4], b[5], b[6], b[7], 0, 0]);
                let mut tab = [0u16; 8];
                tab[0] = a0;
                tab[1] = a1;
                if a0 > a1 {
                    for k in 1..7 {
                        tab[k + 1] = ((7 - k as u16) * a0 + k as u16 * a1 + 3) / 7;
                    }
                } else {
                    for k in 1..5 {
                        tab[k + 1] = ((5 - k as u16) * a0 + k as u16 * a1 + 2) / 5;
                    }
                    tab[6] = 0;
                    tab[7] = 255;
                }
                for t in 0..16 {
                    if tab[((bits >> (t * 3)) & 7) as usize] < 128 {
                        clear += 1;
                    }
                }
            }
            Some(clear as f32 / (n * 16).max(1) as f32)
        }
        _ => None,
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mode = args.next().unwrap_or_default();
    let dir = args.next().expect("usage: texowner <scan|one> <dir> [zone] [tex…]");
    let dirp = std::path::PathBuf::from(&dir);

    let zones: Vec<String> = if mode == "one" {
        vec![args.next().expect("zone name")]
    } else {
        let mut z: Vec<String> = std::fs::read_dir(&dirp)
            .expect("read dir")
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let n = e.file_name().to_string_lossy().to_string();
                n.strip_suffix("_obj.s3d").map(|s| s.to_string())
            })
            .collect();
        z.sort();
        z
    };

    let mut total_conflicts = 0usize;
    let mut zones_hit = 0usize;
    let mut also_terrain = 0usize;
    let mut alpha_bugs = 0usize;
    let mut alpha_bugs_obj_only = 0usize;
    let mut details: Vec<String> = Vec::new();
    let mut alpha_details: Vec<String> = Vec::new();

    for zone in &zones {
        let Some(mut main_pfs) = open(&dirp.join(format!("{zone}.s3d"))) else {
            continue;
        };
        let Some(mut obj_pfs) = open(&dirp.join(format!("{zone}_obj.s3d"))) else {
            continue;
        };
        let terrain_refs = wld_texture_refs(&mut main_pfs);
        let obj_refs = wld_texture_refs(&mut obj_pfs);
        let main_files = file_map(&mut main_pfs);
        let obj_files = file_map(&mut obj_pfs);

        let mut hit = 0;
        for name in &obj_refs {
            let (Some(m), Some(o)) = (main_files.get(name), obj_files.get(name)) else {
                continue;
            };
            if m == o {
                continue; // identical bytes — resolution order is harmless
            }
            hit += 1;
            total_conflicts += 1;
            let in_terrain = terrain_refs.contains(name);
            if in_terrain {
                also_terrain += 1;
            }
            // The visible-bug set: object copy has real cutout alpha, main copy has
            // (almost) none, so the bake loses the transparency entirely.
            let (mt, ot) = (transparent_fraction(m), transparent_fraction(o));
            let alpha_lost = matches!((mt, ot), (Some(a), Some(b)) if a < 0.01 && b > 0.05);
            if alpha_lost {
                alpha_bugs += 1;
                if !in_terrain {
                    alpha_bugs_obj_only += 1;
                }
                if alpha_details.len() < 40 {
                    alpha_details.push(format!(
                        "  {zone:14} {name:18} main={:4} {:5.1}% clear -> obj={:4} {:5.1}% clear{}",
                        fourcc(m),
                        100.0 * mt.unwrap_or(0.0),
                        fourcc(o),
                        100.0 * ot.unwrap_or(0.0),
                        if in_terrain { "  ALSO-IN-TERRAIN" } else { "" }
                    ));
                }
            }
            if details.len() < 25 {
                details.push(format!(
                    "  {zone:14} {name:18} main={:4}({:6}B) obj={:4}({:6}B){}",
                    fourcc(m),
                    m.len(),
                    fourcc(o),
                    o.len(),
                    if in_terrain { "  ALSO-IN-TERRAIN" } else { "" }
                ));
            }
        }
        if hit > 0 {
            zones_hit += 1;
            if mode == "one" {
                println!("{zone}: {hit} conflicting textures");
            }
        }
    }

    println!("zones with a main/_obj archive: {}", zones.len());
    println!("zones affected: {zones_hit}");
    println!("object-referenced textures shadowed by a DIFFERENT main-archive copy: {total_conflicts}");
    println!("  ...of which the terrain WLD also references: {also_terrain}");
    println!("  ...of which the object copy has cutout alpha the main copy lacks: {alpha_bugs}");
    println!("      (object-WLD-only, so unambiguous to fix: {alpha_bugs_obj_only})");
    println!("\nALPHA-LOST textures (the visible foliage bug):");
    for d in &alpha_details {
        println!("{d}");
    }
    println!("\nall-conflict samples:");
    for d in &details {
        println!("{d}");
    }
}
