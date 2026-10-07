//! Publish a small synthetic set for cross-repository manifest acceptance tests.
//! Use a disposable data directory, never the running asset server's store.
use eqoxide_asset_server::{cas::Cas, compatibility::ReaderRequirements, manifest::ManifestStore};

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    anyhow::ensure!(args.len() == 2, "usage: manifest_fixture DISPOSABLE_DATA_DIRECTORY READER_VERSION");
    let mut requirements = ReaderRequirements::legacy();
    requirements.reader_version = args[1].parse()?;
    let manifest = ManifestStore::new(&args[0]).build_and_write(
        &Cas::new(&args[0]), "fixture/demo",
        &[("b.txt".into(), b"world\n".to_vec()), ("a.txt".into(), b"hello\n".to_vec())],
        requirements,
    )?;
    println!("{}", serde_json::to_string_pretty(&manifest)?);
    Ok(())
}
