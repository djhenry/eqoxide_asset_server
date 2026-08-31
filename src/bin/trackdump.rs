use libeq_wld::parser::{Track, WldDoc};

fn main() {
    let path = std::env::args().nth(1).expect("usage: trackdump <archive.s3d>");
    let filter = std::env::args().nth(2);
    let file = std::fs::File::open(&path).expect("open archive");
    let mut pfs = libeq_pfs::PfsReader::open(file).expect("pfs open");
    let names = pfs.filenames().expect("filenames");
    println!("archive: {path}  ({} files)", names.len());
    for n in &names { println!("  file: {n}"); }

    for wld_name in names.iter().filter(|f| f.to_lowercase().ends_with(".wld")) {
        let wld_bytes = match pfs.get(wld_name) { Ok(Some(b)) => b, _ => continue };
        let doc = match WldDoc::parse(&wld_bytes) { Ok(d) => d, Err(e) => { println!("\n{wld_name}: parse err {e:?}"); continue } };
        println!("\n===== WLD {wld_name} =====");
        let mut tracks: Vec<String> = doc.fragment_iter::<Track>()
            .map(|f| doc.get_string(f.name_reference).unwrap_or("").to_string())
            .collect();
        tracks.sort();
        println!("Track (0x13) fragment count: {}", tracks.len());
        for t in &tracks {
            if let Some(f) = &filter {
                if !t.to_uppercase().contains(f.as_str()) { continue; }
            }
            println!("   {t}");
        }
    }
}
