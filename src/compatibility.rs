//! Reader requirements are independent of asset provenance.
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const READERS_HEADER: &str = "x-eqoxide-asset-readers";
pub const CAPABILITIES_HEADER: &str = "x-eqoxide-asset-capabilities";
pub const READERS: &str = "1";
pub const CAPABILITIES: &str = "legacy-assets-v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReaderRequirements {
    pub reader_version: u32,
    pub capabilities: Vec<String>,
}
impl ReaderRequirements {
    pub fn legacy() -> Self { Self { reader_version: 1, capabilities: vec![CAPABILITIES.into()] } }
    pub fn validate(&self) -> Result<()> {
        ensure!(self.reader_version > 0, "invalid reader version");
        ensure!(self.capabilities.len() <= 128, "too many capabilities");
        ensure!(self.capabilities.iter().all(|s| token(s)), "invalid capability token");
        Ok(())
    }
    pub fn check_supported(&self) -> Result<()> {
        self.validate()?;
        ensure!(self.reader_version == 1 && self.capabilities.iter().all(|c| c == CAPABILITIES),
            "asset_reader_incompatible: requires reader {} and capabilities {:?}", self.reader_version, self.capabilities);
        Ok(())
    }
}
pub fn token(s: &str) -> bool {
    !s.is_empty() && s.len() <= 128 && s.bytes().next().is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && s.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b"._-".contains(&c))
}
pub fn valid_hash(s: &str) -> bool { s.len() == 64 && s.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)) }
pub fn valid_path(s: &str) -> bool {
    !s.is_empty() && s.len() <= 4096 && !s.contains(['\\', '\0', '\n', '\r', ':'])
        && s.split('/').all(|p| !p.is_empty() && p != "." && p != "..")
}

pub struct Advertisement { versions: BTreeSet<u32>, capabilities: BTreeSet<String> }
impl Advertisement {
    pub fn parse(readers: Option<&str>, capabilities: Option<&str>) -> Result<Option<Self>> {
        fn list(s: &str) -> Result<Vec<&str>> {
            ensure!(s.len() <= 4096, "advertisement too long");
            let items: Vec<_> = s.split(',').map(str::trim).collect();
            ensure!(items.len() <= 128 && items.iter().all(|s| !s.is_empty()), "malformed advertisement list");
            Ok(items)
        }
        let versions = readers.map(|s| -> Result<BTreeSet<u32>> {
            list(s)?.into_iter().map(|v| {
                ensure!(v.bytes().all(|c| c.is_ascii_digit()), "invalid reader advertisement");
                let n: u32 = v.parse()?; ensure!(n > 0, "invalid reader advertisement"); Ok(n)
            }).collect()
        }).transpose()?;
        let capabilities = capabilities.map(|s| -> Result<BTreeSet<String>> {
            // An empty capability set is valid; empty elements in a nonempty list are not.
            if s.is_empty() { return Ok(BTreeSet::new()); }
            list(s)?.into_iter().map(|v| { ensure!(token(v), "invalid capability advertisement"); Ok(v.to_owned()) }).collect()
        }).transpose()?;
        Ok(match (versions, capabilities) { (Some(versions),Some(capabilities)) => Some(Self { versions, capabilities }), _ => None })
    }
    pub fn supports(&self, required: &ReaderRequirements) -> bool {
        self.versions.contains(&required.reader_version) && required.capabilities.iter().all(|c| self.capabilities.contains(c))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn advertisement_is_explicit_bounded_and_not_minimum_version() {
        assert!(Advertisement::parse(None, None).unwrap().is_none());
        assert!(!Advertisement::parse(Some("2"), Some(CAPABILITIES)).unwrap().unwrap().supports(&ReaderRequirements::legacy()));
        assert!(Advertisement::parse(Some("2, 1"), Some(CAPABILITIES)).unwrap().unwrap().supports(&ReaderRequirements::legacy()));
        for invalid in ["", "1,", "-1", "0", "4294967296", "one"] {
            assert!(Advertisement::parse(Some(invalid), Some(CAPABILITIES)).is_err(), "{invalid}");
        }
        for invalid in ["A", "a,", ",a", "a/b", "a b"] {
            assert!(Advertisement::parse(Some("1"), Some(invalid)).is_err(), "{invalid}");
        }
        assert!(Advertisement::parse(Some(&"1".repeat(4097)), Some(CAPABILITIES)).is_err());
    }
}
