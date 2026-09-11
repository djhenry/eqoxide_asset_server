//! Source-level binary EQG zone assembly, without render or collision semantics.
pub mod report;
mod zon;
pub use zon::{BinaryZoneScene, DescriptorSource, ZoneMesh, ZonePlacement, load_binary_zone};
