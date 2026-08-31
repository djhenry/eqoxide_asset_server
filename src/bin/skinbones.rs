//! Diagnostic: dump skin_assignment_groups (vertex-range -> skeleton DAG bone)
//! and face_material_groups (face-range -> material) for a named DmSpriteDef2
//! mesh, cross-referenced against a named HierarchicalSpriteDef's dag list.
//! Usage: skinbones <archive.s3d> <wld_name> <mesh_name> <skeleton_name>
use libeq_wld::parser::{DmSpriteDef2, HierarchicalSpriteDef, MaterialDef, StringReference, WldDoc};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (arch, wld_name, mesh_name, skel_name) = (&args[1], &args[2], &args[3], &args[4]);
    let file = std::fs::File::open(arch).expect("open archive");
    let mut pfs = libeq_pfs::PfsReader::open(file).expect("pfs open");
    let wld_bytes = pfs.get(wld_name).expect("get wld").expect("wld present");
    let doc = WldDoc::parse(&wld_bytes).expect("parse wld");

    let skel = doc
        .fragment_iter::<HierarchicalSpriteDef>()
        .find(|s| doc.get_string(s.name_reference).unwrap_or("") == skel_name)
        .expect("skeleton not found");
    let dag_names: Vec<String> = skel
        .dags
        .iter()
        .map(|d| doc.get_string(StringReference(d.name_reference)).unwrap_or("").to_string())
        .collect();
    println!("skeleton '{skel_name}' has {} dags (index: name)", dag_names.len());
    for (i, n) in dag_names.iter().enumerate() {
        println!("  [{i}] {n}");
    }

    let mesh = doc
        .fragment_iter::<DmSpriteDef2>()
        .find(|m| doc.get_string(m.name_reference).unwrap_or("") == mesh_name)
        .expect("mesh not found");
    println!(
        "\nmesh '{mesh_name}': {} verts, {} faces, {} skin_assignment_groups, {} face_material_groups",
        mesh.position_count, mesh.face_count, mesh.skin_assignment_groups.len(), mesh.face_material_groups.len()
    );

    println!("\n== skin_assignment_groups (vertex_count, dag_index -> dag_name), vertex range ==");
    let mut vstart: u32 = 0;
    for (count, piece_idx) in &mesh.skin_assignment_groups {
        let name = dag_names.get(*piece_idx as usize).map(|s| s.as_str()).unwrap_or("<out of range>");
        println!(
            "  verts [{}..{}) count={}  piece_idx={}  dag={}",
            vstart, vstart + *count as u32, count, piece_idx, name
        );
        vstart += *count as u32;
    }

    // Resolve material palette for face groups
    let palette = doc.get(&mesh.material_list_ref);
    let mat_names: Vec<String> = if let Some(p) = palette {
        p.fragments
            .iter()
            .map(|fr| {
                doc.get(fr)
                    .map(|m: &MaterialDef| doc.get_string(m.name_reference).unwrap_or("").to_string())
                    .unwrap_or_default()
            })
            .collect()
    } else {
        vec![]
    };
    println!("\n== material palette ({} materials) ==", mat_names.len());
    for (i, n) in mat_names.iter().enumerate() {
        println!("  [{i}] {n}");
    }

    println!("\n== face_material_groups (face_count, material_idx -> name), face range ==");
    let mut fstart: u32 = 0;
    for (count, mat_idx) in &mesh.face_material_groups {
        let name = mat_names.get(*mat_idx as usize).map(|s| s.as_str()).unwrap_or("<oor>");
        println!(
            "  faces [{}..{}) count={}  mat_idx={}  material={}",
            fstart, fstart + *count as u32, count, mat_idx, name
        );
        fstart += *count as u32;
    }
}
