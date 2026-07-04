//! EQEMUWATER v1 (`.wtr`) generation from a zone's WLD BSP tree.
//!
//! The zone WLD already IS the authority on water: its 0x21 WorldTree carries the
//! zone's BSP, and 0x29 Zone fragments flag leaf regions by name prefix (`WT…` =
//! water, `LA…` = lava, `DRNTP…` = zone line, `DRP…` = PvP, `SL…` = slime, `VW…` =
//! icy water). EQEmu's classic `azone`/`awater` tools serialized exactly that tree
//! as `EQEMUWATER` v1; regenerating it here keeps the water maps' provenance in
//! lockstep with the zone geometry we bake (RoF2), instead of shipping `.wtr`
//! files derived from some other client's files.
//!
//! v1 layout (matches EQEmu `water_map.h` / eqoxide `water_map.rs`):
//! `"EQEMUWATER"` + u32 version(1) + u32 node_count + node_count × 36-byte
//! `ZBSP_Node { i32 node_number; f32 normal[3]; f32 split; i32 region; i32
//! special; i32 left; i32 right }`, node references 1-based, 0 = none, leaves
//! carry the region type in `special`. Consumers query with server (y, x, z) —
//! the WLD's native axis order — so the tree is copied verbatim.

use anyhow::Context;
use std::path::Path;

use libeq_wld::parser::{FragmentRef, Region, WldDoc, WorldNode, WorldTree, Zone};

/// EQEmu region types (water_map.h). Only the ones WLD 0x29 fragments encode.
fn region_type_for_zone_name(name: &str) -> i32 {
    let n = name.to_uppercase();
    if n.starts_with("WT") { 1 }        // water
    else if n.starts_with("VW") { 7 }   // icy water (still swimmable)
    else if n.starts_with("LA") { 2 }   // lava
    else if n.starts_with("DRNTP") { 3 } // zone line
    else if n.starts_with("DRP") { 4 }  // PvP
    else if n.starts_with("SL") { 5 }   // slime
    else { 0 }
}

/// Serialize the zone WLD's BSP as an EQEMUWATER v1 blob. Returns `None` when the
/// WLD has no world tree or no flagged (non-normal) region at all — writing a
/// tree that classifies everything as 0 would only waste space.
pub fn wtr_from_wld(wld: &WldDoc) -> Option<Vec<u8>> {
    let tree = wld.fragment_iter::<WorldTree>().next()?;

    // Region ordinal = order of 0x22 fragments in the file. Both the 0x29 Zone
    // region lists (0-based) and the 0x21 leaf-node region field (1-based) count
    // in that same order — the node field is a region NUMBER, not a fragment ref.
    let region_count = wld.fragment_iter::<Region>().count();

    let mut region_types = vec![0i32; region_count];
    let mut any_flagged = false;
    for z in wld.fragment_iter::<Zone>() {
        // Old-era WLDs name the fragment itself "WT_ZONE…"; newer ones (RoF2) use a
        // generic "Z####_ZONE" name and put the code in user_data ("WTN__…").
        let name = wld.get_string(z.name_reference).unwrap_or("");
        let t = match region_type_for_zone_name(&z.user_data) {
            0 => region_type_for_zone_name(name),
            t => t,
        };
        if t == 0 { continue; }
        for &ord in &z.regions {
            if let Some(slot) = region_types.get_mut(ord as usize) {
                *slot = t;
                any_flagged = true;
            }
        }
    }
    if !any_flagged {
        return None;
    }

    fn ref_index(r: &FragmentRef<WorldNode>) -> u32 {
        match r {
            FragmentRef::Index(i, _) => *i,
            FragmentRef::Name(..) => 0,
        }
    }
    let region_ref_ordinal = |r: &FragmentRef<Region>| -> Option<usize> {
        match r {
            FragmentRef::Index(i, _) if *i > 0 => Some(*i as usize - 1),
            _ => None,
        }
    };

    let mut out = Vec::with_capacity(18 + tree.world_nodes.len() * 36);
    out.extend_from_slice(b"EQEMUWATER");
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&(tree.world_nodes.len() as u32).to_le_bytes());
    for (i, node) in tree.world_nodes.iter().enumerate() {
        let left = ref_index(&node.front_tree);
        let right = ref_index(&node.back_tree);
        let ordinal = region_ref_ordinal(&node.region);
        // Leaves carry the region's type; internal nodes carry 0.
        let special = if left == 0 && right == 0 {
            ordinal.and_then(|o| region_types.get(o).copied()).unwrap_or(0)
        } else {
            0
        };
        out.extend_from_slice(&(i as i32).to_le_bytes()); // azone wrote 0-based node numbers
        out.extend_from_slice(&node.normal.0.to_le_bytes());
        out.extend_from_slice(&node.normal.1.to_le_bytes());
        out.extend_from_slice(&node.normal.2.to_le_bytes());
        out.extend_from_slice(&node.split_distance.to_le_bytes());
        out.extend_from_slice(&(ordinal.map(|o| o as i32 + 1).unwrap_or(0)).to_le_bytes());
        out.extend_from_slice(&special.to_le_bytes());
        out.extend_from_slice(&(left as i32).to_le_bytes());
        out.extend_from_slice(&(right as i32).to_le_bytes());
    }
    Some(out)
}

/// Generate a zone's `.wtr` from its main `.s3d` archive (the WLD holding the
/// world tree). `Ok(None)` = zone has no flagged regions (dry zone) — normal.
pub fn wtr_from_zone_s3d(main_s3d: &Path) -> anyhow::Result<Option<Vec<u8>>> {
    let file = std::fs::File::open(main_s3d)
        .with_context(|| format!("open {}", main_s3d.display()))?;
    let mut pfs = libeq_pfs::PfsReader::open(file)
        .with_context(|| format!("parse PFS {}", main_s3d.display()))?;
    let names: Vec<String> = pfs.filenames()?;
    for wn in names.iter().filter(|f| f.to_lowercase().ends_with(".wld")) {
        let Ok(Some(bytes)) = pfs.get(wn) else { continue };
        let Ok(wld) = WldDoc::parse(&bytes) else { continue };
        if let Some(out) = wtr_from_wld(&wld) {
            return Ok(Some(out));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reference query matching EQEmu WaterMapV1 / eqoxide water_map.rs exactly:
    /// walk from node 1, `dist = loc·normal + split`, >0 → left else right, and
    /// the caller passes server coords swapped to (y, x, z).
    fn region_at(wtr: &[u8], sx: f32, sy: f32, sz: f32) -> i32 {
        assert_eq!(&wtr[..10], b"EQEMUWATER");
        assert_eq!(u32::from_le_bytes(wtr[10..14].try_into().unwrap()), 1);
        let count = u32::from_le_bytes(wtr[14..18].try_into().unwrap()) as usize;
        let node = |n: usize| {
            let o = 18 + (n - 1) * 36;
            let f = |k: usize| f32::from_le_bytes(wtr[o + k..o + k + 4].try_into().unwrap());
            let i = |k: usize| i32::from_le_bytes(wtr[o + k..o + k + 4].try_into().unwrap());
            ([f(4), f(8), f(12)], f(16), i(24), i(28), i(32))
        };
        let (lx, ly, lz) = (sy, sx, sz);
        let mut nn = 1usize;
        for _ in 0..256 {
            if nn == 0 || nn > count { return 0; }
            let (normal, split, special, left, right) = node(nn);
            if left == 0 && right == 0 { return special; }
            let dist = lx * normal[0] + ly * normal[1] + lz * normal[2] + split;
            if dist == 0.0 { return 0; }
            nn = if dist > 0.0 { left as usize } else { right as usize };
        }
        0
    }

    #[test]
    #[ignore = "requires ~/eq_assets/everquest_rof2/qeynos2.s3d"]
    fn qeynos2_wtr_from_rof2_classifies_the_moat_as_water() {
        let home = std::env::var("HOME").unwrap();
        let s3d = std::path::PathBuf::from(format!("{home}/eq_assets/everquest_rof2/qeynos2.s3d"));
        if !s3d.exists() { eprintln!("skip: {s3d:?} missing"); return; }
        let wtr = wtr_from_zone_s3d(&s3d).unwrap().expect("qeynos2 has water regions");

        // The North Qeynos moat column (server coords): water through the column,
        // dry on the street and above the surface.
        assert_eq!(region_at(&wtr, -502.3, -141.3, -12.0), 1, "moat @-12 is water");
        assert_eq!(region_at(&wtr, -502.3, -141.3, -9.0), 1, "moat @-9 is water");
        assert_eq!(region_at(&wtr, -502.3, -141.3, 20.0), 0, "air above moat is dry");
        assert_eq!(region_at(&wtr, -560.0, -141.0, -10.0), 0, "street is dry");
    }
}
