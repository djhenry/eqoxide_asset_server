//! Diagnostic: dump WLD fragment inventory for a PFS (.s3d) archive, focused on
//! locating Luclin hair/beard geometry. Usage: wlddump <archive.s3d>
use libeq_wld::parser::{DmSpriteDef2, HierarchicalSpriteDef, StringReference, WldDoc};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    // Extract mode: wlddump extract <archive> <filename> <out>
    if args.get(1).map(|s| s.as_str()) == Some("extract") {
        let (arch, fname, out) = (&args[2], &args[3], &args[4]);
        let file = std::fs::File::open(arch).expect("open archive");
        let mut pfs = libeq_pfs::PfsReader::open(file).expect("pfs open");
        match pfs.get(fname) {
            Ok(Some(b)) => { std::fs::write(out, &b).unwrap(); println!("wrote {} ({} bytes)", out, b.len()); }
            _ => println!("{fname} not found"),
        }
        return;
    }
    let path = std::env::args().nth(1).expect("usage: wlddump <archive.s3d>");
    let file = std::fs::File::open(&path).expect("open archive");
    let mut pfs = libeq_pfs::PfsReader::open(file).expect("pfs open");
    let names = pfs.filenames().expect("filenames");
    println!("archive: {path}  ({} files)", names.len());

    let hair_tex: Vec<&String> = names.iter()
        .filter(|n| {
            let l = n.to_lowercase();
            l.contains("hair") || l.contains("hesk") || l.contains("clk") || l.contains("hr0")
        }).collect();
    println!("\n== files matching hair/hesk/clk/hr0 ({}):", hair_tex.len());
    for n in hair_tex.iter().take(60) { println!("   {n}"); }

    for wld_name in names.iter().filter(|f| f.to_lowercase().ends_with(".wld")) {
        let wld_bytes = match pfs.get(wld_name) { Ok(Some(b)) => b, _ => continue };
        let doc = match WldDoc::parse(&wld_bytes) { Ok(d) => d, Err(_) => { println!("\n{wld_name}: parse err"); continue } };
        println!("\n===== WLD {wld_name} =====");

        let meshes: Vec<String> = doc.fragment_iter::<DmSpriteDef2>()
            .map(|f| doc.get_string(f.name_reference).unwrap_or("").to_string())
            .collect();
        println!("DmSpriteDef2 meshes DECODED ({}):", meshes.len());
        for m in &meshes { println!("   {m}"); }

        // Raw fragment histogram (bypasses typed decode) — reveals fragments libeq_wld drops.
        if let Ok((_, headers)) = WldDoc::dump_raw_fragments(&wld_bytes) {
            use std::collections::BTreeMap;
            let mut hist: BTreeMap<String, usize> = BTreeMap::new();
            for h in &headers {
                *hist.entry(format!("{:?}", h.fragment_type)).or_default() += 1;
            }
            println!("RAW fragment count: {}", headers.len());
            for (t, n) in &hist {
                if *n > 1 || t.contains("36") || t.to_lowercase().contains("dmsprite") || t.to_lowercase().contains("mesh") {
                    println!("   {t}: {n}");
                }
            }
        }

        for skel in doc.fragment_iter::<HierarchicalSpriteDef>() {
            let sname = doc.get_string(skel.name_reference).unwrap_or("");
            let interesting: Vec<(String,u32)> = skel.dags.iter()
                .map(|d| (doc.get_string(StringReference(d.name_reference)).unwrap_or("").to_string(), d.mesh_or_sprite_reference))
                .filter(|(n,r)| *r != 0 || n.to_uppercase().contains("HAIR") || n.to_uppercase().contains("HEAD") || n.to_uppercase().contains("HELM"))
                .collect();
            println!("HierarchicalSpriteDef '{sname}': {} dags; hair/head/helm/mesh-bearing:", skel.dags.len());
            for (n,r) in &interesting { println!("   dag {n}  mesh_ref={r}"); }
        }
    }
}
