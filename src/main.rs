use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use clap::{Parser, Subcommand};
use eqoxide_asset_server::auth::TokenIssuer;
use eqoxide_asset_server::build::ingest_dir;
use eqoxide_asset_server::cas::Cas;
use eqoxide_asset_server::db::MariaAccountStore;
use eqoxide_asset_server::manifest::ManifestStore;
use eqoxide_asset_server::server::{serve, AppState};

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
    /// Run the HTTP asset server.
    Serve {
        #[arg(long)] data: PathBuf,
        #[arg(long, default_value = "0.0.0.0:8088")] addr: SocketAddr,
        #[arg(long, env = "EQEMU_DB_URL")] db: String,
        #[arg(long)] secret_file: PathBuf,
    },
}

fn load_secret(path: &PathBuf) -> [u8; 32] {
    let raw = std::fs::read(path).expect("read secret file");
    *blake3::hash(&raw).as_bytes()
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    match Cli::parse().cmd {
        Cmd::Build { set, from, out } => {
            let cas = Cas::new(&out);
            let store = ManifestStore::new(&out);
            let m = ingest_dir(&cas, &store, &set, &from)?;
            println!("built set '{}' version {} ({} files)", m.set, m.version, m.files.len());
            Ok(())
        }
        Cmd::Serve { data, addr, db, secret_file } => {
            let accounts = MariaAccountStore::connect(&db).await?;
            let state = AppState {
                cas: Arc::new(Cas::new(&data)),
                manifests: Arc::new(ManifestStore::new(&data)),
                accounts: Arc::new(accounts),
                tokens: Arc::new(TokenIssuer::new(load_secret(&secret_file), Duration::from_secs(3600))),
            };
            serve(state, addr).await
        }
    }
}
