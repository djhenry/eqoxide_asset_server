use std::path::PathBuf;

use clap::{Parser, Subcommand};
use eqoxide_asset_server::build::ingest_dir;
use eqoxide_asset_server::cas::Cas;
use eqoxide_asset_server::manifest::ManifestStore;

#[derive(Parser)]
#[command(name = "eqoxide-assets")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Chunk a directory of derived assets into the CAS + a manifest.
    Build {
        #[arg(long)] set: String,
        #[arg(long)] from: PathBuf,
        #[arg(long)] out: PathBuf,
    },
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().cmd {
        Cmd::Build { set, from, out } => {
            let cas = Cas::new(&out);
            let store = ManifestStore::new(&out);
            let m = ingest_dir(&cas, &store, &set, &from)?;
            println!("built set '{}' version {} ({} files)", m.set, m.version, m.files.len());
            Ok(())
        }
    }
}
