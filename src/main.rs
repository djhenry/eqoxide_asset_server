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
        #[arg(long)] set: Option<String>,
        #[arg(long)] from: Option<PathBuf>,
        #[arg(long)] raw: Option<PathBuf>,
        #[arg(long)] out: PathBuf,
        /// With --raw: bake only zones, skip the `common` model set (leaves an
        /// existing common untouched; avoids re-converting character archives).
        #[arg(long)] zones_only: bool,
    },
    /// Convert a single `.s3d` archive to a `.glb` model (skinned by default).
    /// Useful for producing one race/character model without re-baking the whole set.
    Convert {
        /// Path to the input `.s3d` archive.
        #[arg(long)] archive: PathBuf,
        /// Output `.glb` path.
        #[arg(long)] out: PathBuf,
        /// Select a single model by its 3-char EQ code (e.g. "SKE") from a
        /// multi-model archive. Omit for one-model-per-archive `global*_chr.s3d`.
        #[arg(long)] model_code: Option<String>,
        /// Convert as a static (non-skinned) model.
        #[arg(long)] static_model: bool,
    },
    /// Print the skeleton bone names and animation track-name prefixes inside a
    /// character `.s3d` archive (diagnostic for missing animations).
    Analyze {
        #[arg(long)] archive: PathBuf,
    },
    /// Run the HTTP asset server.
    Serve {
        #[arg(long)] data: PathBuf,
        #[arg(long, default_value = "0.0.0.0:8088")] addr: SocketAddr,
        /// EQEmu DB URL for account validation. Not required (and not connected)
        /// when --no-auth-required is set.
        #[arg(long, env = "EQEMU_DB_URL")] db: Option<String>,
        #[arg(long)] secret_file: PathBuf,
        /// DEV ONLY: serve assets without any credential/token check, and skip the
        /// MariaDB connection entirely. Lets tools pull models without the EQEmu
        /// login flow. Do NOT enable on a public/production server.
        #[arg(long)] no_auth_required: bool,
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
        Cmd::Build { set, from, raw, out, zones_only } => {
            let cas = Cas::new(&out);
            let store = ManifestStore::new(&out);
            if let Some(raw_dir) = raw {
                let work = out.join("work");
                if !zones_only {
                    let ms = eqoxide_asset_server::build::build_from_raw(&cas, &store, &raw_dir, &work)?;
                    println!("built {} set(s) from raw archives", ms.len());
                }
                let zones = eqoxide_asset_server::build::build_zones_from_raw(&cas, &store, &raw_dir, &work)?;
                println!("baked {} zone(s): {}", zones.len(), zones.join(", "));
                let gd = eqoxide_asset_server::build::build_gamedata_from_raw(&cas, &store, &raw_dir)?;
                println!("built 'gamedata' set version {} ({} files)", gd.version, gd.files.len());
                let ge = eqoxide_asset_server::build::build_gameequip_from_raw(&cas, &store, &raw_dir)?;
                println!("built 'gameequip' set version {} ({} files)", ge.version, ge.files.len());
            } else {
                let set = set.expect("--set required without --raw");
                let from = from.expect("--from required without --raw");
                let m = ingest_dir(&cas, &store, &set, &from)?;
                println!("built set '{}' version {} ({} files)", m.set, m.version, m.files.len());
            }
            Ok(())
        }
        Cmd::Convert { archive, out, model_code, static_model } => {
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent)?;
            }
            eqoxide_asset_server::convert::s3d_to_glb_model(
                &archive, &out, !static_model, model_code.as_deref(),
            )?;
            let bytes = std::fs::metadata(&out).map(|m| m.len()).unwrap_or(0);
            println!("converted {} -> {} ({} bytes)", archive.display(), out.display(), bytes);
            Ok(())
        }
        Cmd::Analyze { archive } => {
            eqoxide_asset_server::convert::analyze_anims(&archive)
        }
        Cmd::Serve { data, addr, db, secret_file, no_auth_required } => {
            let accounts: Arc<dyn eqoxide_asset_server::auth::AccountStore> = if no_auth_required {
                tracing::warn!("--no-auth-required: serving assets WITHOUT auth (dev mode); skipping MariaDB");
                Arc::new(eqoxide_asset_server::auth::FakeAccountStore { creds: Default::default() })
            } else {
                let url = db.expect("--db (or EQEMU_DB_URL) required unless --no-auth-required");
                Arc::new(MariaAccountStore::connect(&url).await?)
            };
            let state = AppState {
                cas: Arc::new(Cas::new(&data)),
                manifests: Arc::new(ManifestStore::new(&data)),
                accounts,
                tokens: Arc::new(TokenIssuer::new(load_secret(&secret_file), Duration::from_secs(3600))),
                no_auth: no_auth_required,
            };
            serve(state, addr).await
        }
    }
}
