//! eqoxide_asset_server — derived-asset delivery for EQEmu.

pub mod cas;
pub mod chunker;
pub mod manifest;

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
