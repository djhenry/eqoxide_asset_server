//! Binary EQG source assembly and explicit render-only staging exports.
pub mod report;
mod zon;
pub use zon::{BinaryZoneScene, DescriptorSource, ZoneMesh, ZonePlacement, load_binary_zone};

pub mod export;

pub mod surface;
