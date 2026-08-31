//! Diagnostic: for a named HierarchicalSpriteDef, print its `dm_sprites`
//! attached-mesh list (resolved to mesh names via the DmSprite(0x2D) wrapper
//! chain) plus per-mesh bounding box / scale / skin_assignment_groups so we
//! can see how additional meshes (eyes, etc.) are positioned relative to the
//! main body mesh and which skeleton pieces they bind to.
//! Usage: skelmeshes <archive.s3d> <wld_name> <skeleton_name>
use libeq_wld::parser::{DmSprite, DmSpriteDef2, HierarchicalSpriteDef, StringReference, WldDoc};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (arch, wld_name, skel_name) = (&args[1], &args[2], &args[3]);
    let file = std::fs::File::open(arch).expect("open archive");
    let mut pfs = libeq_pfs::PfsReader::open(file).expect("pfs open");
    let wld_bytes = pfs.get(wld_name).expect("get wld").expect("wld present");
    let doc = WldDoc::parse(&wld_bytes).expect("parse wld");

    let skel = doc
        .fragment_iter::<HierarchicalSpriteDef>()
        .find(|s| doc.get_string(s.name_reference).unwrap_or("") == skel_name)
        .expect("skeleton not found");

    println!(
        "skeleton '{skel_name}': flags={:?} num_dags={} num_attached_skins={:?}",
        skel.flags, skel.num_dags, skel.num_attached_skins
    );

    let dag_names: Vec<String> = skel
        .dags
        .iter()
        .map(|d| doc.get_string(StringReference(d.name_reference)).unwrap_or("").to_string())
        .collect();

    // print every dag's mesh_or_sprite_reference (unfiltered)
    println!("\n== ALL dags with nonzero mesh_or_sprite_reference ==");
    for (i, d) in skel.dags.iter().enumerate() {
        if d.mesh_or_sprite_reference != 0 {
            println!("  [{i}] {} mesh_or_sprite_reference={}", dag_names[i], d.mesh_or_sprite_reference);
        }
    }

    if let Some(dm_sprites) = &skel.dm_sprites {
        println!("\n== dm_sprites (attached mesh list, {} entries) ==", dm_sprites.len());
        for (idx, s) in dm_sprites.iter().enumerate() {
            // These are raw fragment indices (1-based per WLD convention) into the
            // overall fragment table; try resolving as DmSprite (0x2D) wrapper first.
            println!("  [{idx}] raw={s}");
        }
    } else {
        println!("\n(no dm_sprites field present)");
    }

    // Now list every DmSpriteDef2 mesh in the doc with bbox/scale/skin groups summary,
    // and try to find which one is referenced by which mechanism.
    println!("\n== every DmSpriteDef2 in this WLD ==");
    for m in doc.fragment_iter::<DmSpriteDef2>() {
        let name = doc.get_string(m.name_reference).unwrap_or("");
        println!(
            "  {name}: verts={} faces={} scale={} center={:?} min={:?} max={:?} max_distance={}",
            m.position_count, m.face_count, m.scale, m.center, m.min, m.max, m.max_distance
        );
        print!("    skin_assignment_groups: ");
        for (c, p) in &m.skin_assignment_groups {
            let n = dag_names.get(*p as usize).map(|s| s.as_str()).unwrap_or("<oor>");
            print!("({c},{p}={n}) ");
        }
        println!();
    }

    println!("\n== every DmSprite (0x2D wrapper) in this WLD ==");
    for (i, s) in doc.fragment_iter::<DmSprite>().enumerate() {
        let name = doc.get_string(s.name_reference).unwrap_or("<unnamed>");
        println!("  #{i} name={name:?} reference={:?}", s.reference);
    }
}
