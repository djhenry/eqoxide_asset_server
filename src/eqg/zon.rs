use anyhow::{Context, Result, bail};
use libeq_eqg::{mesh, zone};
use libeq_pfs::PfsReader;
use std::{
    collections::BTreeMap,
    fs::File,
    path::{Path, PathBuf},
};

/// The caller selects a descriptor explicitly; no source precedence is inferred.
#[derive(Debug, Clone, Copy)]
pub enum DescriptorSource<'a> {
    Loose(&'a Path),
    Archive(&'a str),
}

/// Owned source records. Transforms have not been converted into world matrices.
#[derive(Debug)]
pub struct BinaryZoneScene {
    pub archive_path: PathBuf,
    pub descriptor_name: String,
    pub descriptor_location: &'static str,
    pub version: u32,
    pub string_table: Vec<u8>,
    pub model_name_offsets: Vec<Option<u32>>,
    /// Original model-table slots mapped to deduplicated `meshes` indices.
    pub model_slots: Vec<Option<usize>>,
    pub meshes: Vec<ZoneMesh>,
    pub placements: Vec<ZonePlacement>,
    pub regions: Vec<zone::Region>,
    pub lights: Vec<zone::Light>,
    pub trailing_data: Vec<u8>,
}
#[derive(Debug)]
pub struct ZoneMesh {
    /// Exact archive spelling used to retrieve this resource.
    pub source_name: String,
    pub kind: mesh::MeshKind,
    pub version: u32,
    pub string_table: Vec<u8>,
    pub materials: Vec<mesh::Material>,
    pub vertices: Vec<mesh::Vertex>,
    pub triangles: Vec<mesh::Triangle>,
    pub bone_count: Option<u32>,
    pub uv_marker: Option<u32>,
    pub trailing_data: Vec<u8>,
}
#[derive(Debug)]
pub struct ZonePlacement {
    /// Original model-table slot, not an index into the deduplicated meshes.
    pub model_index: Option<u32>,
    pub name_offset: u32,
    pub position: [f32; 3],
    pub rotation: [f32; 3],
    pub scale: f32,
    pub extension_data: Vec<u8>,
}
impl ZonePlacement {
    pub fn extension_words(&self) -> impl Iterator<Item = u32> + '_ {
        self.extension_data
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
}

fn member<'a>(index: &'a BTreeMap<String, Vec<String>>, requested: &str) -> Result<&'a str> {
    match index
        .get(&requested.to_ascii_lowercase())
        .map(Vec::as_slice)
    {
        Some([name]) => Ok(name),
        Some(_) => bail!("ambiguous archive member {requested:?}"),
        None => bail!("missing archive member {requested:?}"),
    }
}

/// Resolve every non-null model-table entry against the selected archive.
/// Lookup is ASCII case-insensitive, but ambiguous names fail instead of picking
/// a member. Placement transforms and mesh positions, normals, and primary UVs
/// must be finite. Material sentinels, flags, colors, and opaque records remain
/// unchanged. Secondary UV bit patterns are retained even when non-finite;
/// consumers must validate them if used. This does not establish rendering,
/// collision, or skeletal support.
pub fn load_binary_zone(
    archive: &Path,
    descriptor: DescriptorSource<'_>,
) -> Result<BinaryZoneScene> {
    let label = match descriptor {
        DescriptorSource::Loose(path) => path.display().to_string(),
        DescriptorSource::Archive(name) => name.to_owned(),
    };
    load(archive, descriptor).with_context(|| {
        format!(
            "binary zone descriptor {label:?} in archive {}",
            archive.display()
        )
    })
}
fn load(archive: &Path, descriptor: DescriptorSource<'_>) -> Result<BinaryZoneScene> {
    let mut pfs = PfsReader::open(File::open(archive)?)?;
    let mut index: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for name in pfs.filenames()? {
        index
            .entry(name.to_ascii_lowercase())
            .or_default()
            .push(name);
    }
    let (bytes, descriptor_name, descriptor_location) = match descriptor {
        DescriptorSource::Loose(path) => {
            (std::fs::read(path)?, path.display().to_string(), "loose")
        }
        DescriptorSource::Archive(name) => {
            let exact = member(&index, name)?;
            (
                pfs.get(exact)?.context("descriptor member disappeared")?,
                exact.to_owned(),
                "archive",
            )
        }
    };
    let parsed = zone::parse(&bytes).context("parse binary zone descriptor")?;
    let mut placements = Vec::with_capacity(parsed.placements.len());
    for (i, p) in parsed.placements.iter().enumerate() {
        if !p
            .position
            .iter()
            .chain(&p.rotation)
            .chain(std::iter::once(&p.scale))
            .all(|x| x.is_finite())
        {
            bail!("placement {i} contains a non-finite transform");
        }
        placements.push(ZonePlacement {
            model_index: p.model_index,
            name_offset: p.name_offset,
            position: p.position,
            rotation: p.rotation,
            scale: p.scale,
            extension_data: p.extension_data.to_vec(),
        });
    }
    let mut meshes = Vec::new();
    let mut loaded = BTreeMap::new();
    let mut model_slots = Vec::with_capacity(parsed.models.len());
    for (slot, offset) in parsed.models.iter().enumerate() {
        let Some(offset) = offset else {
            model_slots.push(None);
            continue;
        };
        let raw_name = parsed.string(*offset)?;
        let name = std::str::from_utf8(raw_name)
            .with_context(|| format!("model slot {slot} name is not UTF-8"))?;
        let exact = member(&index, name)
            .with_context(|| format!("model slot {slot} requested {name:?}"))?;
        if let Some(&mesh_index) = loaded.get(exact) {
            model_slots.push(Some(mesh_index));
            continue;
        }
        let geometry = (|| -> Result<ZoneMesh> {
            let bytes = pfs.get(exact)?.context("model member disappeared")?;
            let m = mesh::parse(&bytes).context("parse model geometry")?;
            for (i, v) in m.vertices.iter().enumerate() {
                if !v
                    .position
                    .iter()
                    .chain(&v.normal)
                    .chain(&v.uv0)
                    .all(|x| x.is_finite())
                {
                    bail!("vertex {i} contains a non-finite position, normal, or primary UV");
                }
            }
            Ok(ZoneMesh {
                source_name: exact.to_owned(),
                kind: m.kind,
                version: m.version,
                string_table: m.string_table.to_vec(),
                materials: m.materials,
                vertices: m.vertices,
                triangles: m.triangles,
                bone_count: m.bone_count,
                uv_marker: m.uv_marker,
                trailing_data: m.trailing_data.to_vec(),
            })
        })()
        .with_context(|| {
            format!("model slot {slot} requested {name:?} (archive member {exact:?})")
        })?;
        let mesh_index = meshes.len();
        meshes.push(geometry);
        loaded.insert(exact.to_owned(), mesh_index);
        model_slots.push(Some(mesh_index));
    }
    Ok(BinaryZoneScene {
        archive_path: archive.to_owned(),
        descriptor_name,
        descriptor_location,
        version: parsed.version,
        string_table: parsed.string_table.to_vec(),
        model_name_offsets: parsed.models,
        model_slots,
        meshes,
        placements,
        regions: parsed.regions,
        lights: parsed.lights,
        trailing_data: parsed.trailing_data.to_vec(),
    })
}
