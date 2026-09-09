//! Read-only EQG inventory. Header recognition is not scene validation or source selection.
use anyhow::Context;
use libeq_eqg::{identify, FormatHeader};
use libeq_pfs::PfsReader;
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;

#[derive(Debug, Serialize)]
pub struct InventoryError {
    pub resource: String,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct ZoneDescriptor {
    pub name: String,
    pub location: &'static str,
    pub format: &'static str,
    pub version: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct ZoneCandidate {
    pub archive: String,
    pub zone_name: String,
    pub s3d_twins: Vec<String>,
    pub descriptors: Vec<ZoneDescriptor>,
    pub terrain_members: Vec<String>,
    /// DAT names are candidates only: auxiliary DAT files need not contain terrain.
    pub dat_members: Vec<String>,
    /// Inventory retains competing descriptors without deciding native precedence.
    pub selection: &'static str,
}

#[derive(Debug, Default, Serialize)]
pub struct ZoneInventory {
    pub archives_scanned: usize,
    /// Archives with at least one recognized zone descriptor, not playable zones.
    pub zone_archives: usize,
    pub zone_archives_without_s3d: usize,
    pub candidates: Vec<ZoneCandidate>,
    pub non_zone_archives: Vec<String>,
    pub orphan_descriptors: Vec<String>,
    pub errors: Vec<InventoryError>,
}

impl ZoneInventory {
    pub fn summary(&self) -> String {
        let unresolved = self
            .candidates
            .iter()
            .filter(|c| c.descriptors.is_empty())
            .count();
        format!(
            "{} EQG zone archive(s) unsupported ({} without S3D twins); {} archive(s) examined, {} non-zone, {} unresolved candidate(s), {} orphan descriptor(s), {} inventory error(s)",
            self.zone_archives, self.zone_archives_without_s3d, self.archives_scanned,
            self.non_zone_archives.len(), unresolved, self.orphan_descriptors.len(), self.errors.len()
        )
    }

    fn error(&mut self, resource: String, error: impl std::fmt::Display) {
        self.errors.push(InventoryError {
            resource,
            message: error.to_string(),
        });
    }
}

fn descriptor(
    mut reader: impl Read,
    name: &str,
    location: &'static str,
) -> anyhow::Result<ZoneDescriptor> {
    let mut prefix = Vec::with_capacity(8);
    reader.by_ref().take(8).read_to_end(&mut prefix)?;
    let (format, version) = match identify(&prefix)? {
        Some(FormatHeader::Zone { version }) => ("eqgz", Some(version)),
        Some(FormatHeader::TerrainProject) => ("eqtzp", None),
        _ => anyhow::bail!("unrecognized zone descriptor header"),
    };
    Ok(ZoneDescriptor {
        name: name.to_owned(),
        location,
        format,
        version,
    })
}

/// Inventory root-level EQG archives and associated loose/internal zone descriptors.
///
/// Filesystem access to the root is fatal; individual bad resources are collected
/// as errors so the remaining inventory is still useful. No assets are written.
/// Exact archive entry spelling is retained because PFS lookup uses its bytes.
pub fn inventory_zones(raw: &Path) -> anyhow::Result<ZoneInventory> {
    let mut report = ZoneInventory::default();
    let mut files: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for entry in std::fs::read_dir(raw).context("read inventory directory")? {
        let entry = entry?;
        let path = entry.path();
        let relevant = path
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|ext| {
                ["eqg", "zon", "s3d"]
                    .iter()
                    .any(|known| ext.eq_ignore_ascii_case(known))
            });
        if !relevant {
            continue;
        }
        let name = match entry.file_name().into_string() {
            Ok(name) => name,
            Err(name) => {
                report.error(format!("{name:?}"), "non-UTF-8 resource filename");
                continue;
            }
        };
        match path.metadata() {
            Ok(metadata) if metadata.is_file() => {}
            Ok(_) => continue,
            Err(e) => {
                report.error(name, e);
                continue;
            }
        }
        files
            .entry(name.to_ascii_lowercase())
            .or_default()
            .push(name);
    }
    for names in files.values_mut() {
        names.sort();
    }
    for (key, names) in &files {
        if names.len() > 1
            && [".eqg", ".zon", ".s3d"]
                .iter()
                .any(|ext| key.ends_with(ext))
        {
            report.error(
                names.join(", "),
                "case collision: provider selection not attempted",
            );
        }
        if let Some(stem) = key.strip_suffix(".zon") {
            if !files.contains_key(&format!("{stem}.eqg")) {
                report.orphan_descriptors.extend(names.clone());
            }
        }
    }
    for (key, providers) in files.iter().filter(|(k, _)| k.ends_with(".eqg")) {
        report.archives_scanned += providers.len();
        if providers.len() != 1 {
            continue;
        }
        let name = &providers[0];
        let stem = key.strip_suffix(".eqg").expect("filtered EQG key");
        let mut candidate = ZoneCandidate {
            archive: name.clone(),
            zone_name: stem.to_owned(),
            s3d_twins: files
                .get(&format!("{stem}.s3d"))
                .cloned()
                .unwrap_or_default(),
            descriptors: Vec::new(),
            terrain_members: Vec::new(),
            dat_members: Vec::new(),
            selection: "not_attempted",
        };
        let loose = files.get(&format!("{stem}.zon"));
        if let Some(names) = loose.filter(|names| names.len() == 1) {
            let zon = &names[0];
            match File::open(raw.join(zon))
                .map_err(anyhow::Error::from)
                .and_then(|file| descriptor(file, zon, "loose"))
            {
                Ok(d) => candidate.descriptors.push(d),
                Err(e) => report.error(zon.clone(), e),
            }
        }
        let archive = File::open(raw.join(name))
            .map_err(anyhow::Error::from)
            .and_then(|file| Ok(PfsReader::open(file)?));
        let mut pfs = match archive {
            Ok(pfs) => pfs,
            Err(e) => {
                report.error(name.clone(), e);
                continue;
            }
        };
        let members = match pfs.filenames() {
            Ok(members) => members,
            Err(e) => {
                report.error(name.clone(), e);
                continue;
            }
        };
        let mut index: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for member in members {
            index
                .entry(member.to_ascii_lowercase())
                .or_default()
                .push(member);
        }
        let has_descriptor = loose.is_some() || index.keys().any(|k| k.ends_with(".zon"));
        for (key, mut names) in index {
            names.sort();
            if key.ends_with(".ter") {
                candidate.terrain_members.extend(names.clone());
            }
            if key.ends_with(".dat") {
                candidate.dat_members.extend(names.clone());
            }
            if !key.ends_with(".zon") {
                continue;
            }
            if names.len() != 1 {
                report.error(
                    format!("{name}:{}", names.join(",")),
                    "case collision: descriptor selection not attempted",
                );
                continue;
            }
            let member = &names[0];
            let parsed = (|| -> anyhow::Result<ZoneDescriptor> {
                let reader = pfs
                    .get_reader(member)?
                    .context("descriptor missing from archive index")?;
                descriptor(reader, member, "archive")
            })();
            match parsed {
                Ok(d) => candidate.descriptors.push(d),
                Err(e) => report.error(format!("{name}:{member}"), e),
            }
        }
        if !candidate.descriptors.is_empty() {
            report.zone_archives += 1;
            if candidate.s3d_twins.is_empty() {
                report.zone_archives_without_s3d += 1;
            }
        }
        if has_descriptor
            || !candidate.terrain_members.is_empty()
            || !candidate.dat_members.is_empty()
        {
            report.candidates.push(candidate);
        } else {
            report.non_zone_archives.push(name.clone());
        }
    }
    report
        .errors
        .sort_by(|a, b| (&a.resource, &a.message).cmp(&(&b.resource, &b.message)));
    Ok(report)
}
