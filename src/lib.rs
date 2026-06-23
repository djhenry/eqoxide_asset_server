//! eqoxide_asset_server — derived-asset delivery for EQEmu.

pub mod auth;
pub mod build;
pub mod cas;
pub mod chunker;
pub mod convert;
pub mod db;
pub mod manifest;
pub mod server;
pub mod sync_client;

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    #[test]
    fn version_is_nonempty() {
        assert!(!super::version().is_empty());
    }
}
