//! Portable authoring packaging for the common static visual profile.
//!
//! This exporter preserves visual content and reference bytes. Its editable glTF
//! deliberately has no engine activation header; reimport and package pairing are
//! separate work. Authoring IDs are scoped to one reference, not universal asset IDs.
use std::{
    collections::BTreeMap,
    io::{self, Write},
};

use anyhow::{ensure, Context, Result};
use eqoxide_assets::{decode_static_visual, DecodeLimits};
use serde::Serialize;
use serde_json::{
    json,
    value::{to_raw_value, RawValue},
    Value,
};
use sha2::{Digest, Sha256};

/// Logical work/output limits in addition to the pinned common reader's defaults.
/// These bounds are not a peak-process-memory or hostile-filesystem guarantee.
#[derive(Debug, Clone)]
pub struct EditableVisualLimits {
    pub max_glb_bytes: usize,
    pub max_json_bytes: usize,
    /// Also limits the common reader's other resource collections and PNG chunks.
    pub max_resources: usize,
    pub max_single_png_bytes: usize,
    pub max_png_bytes: usize,
    /// Includes the retained reference, binary, images, JSON and sidecar.
    pub max_output_bytes: usize,
}
impl Default for EditableVisualLimits {
    fn default() -> Self {
        Self {
            max_glb_bytes: 128 << 20,
            max_json_bytes: 8 << 20,
            max_resources: 100_000,
            max_single_png_bytes: 64 << 20,
            max_png_bytes: 128 << 20,
            max_output_bytes: 400 << 20,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthoringIdentity {
    pub kind: String,
    pub id: String,
    pub original_ordinal: usize,
    pub name: Option<String>,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthoringFile {
    pub bytes: usize,
    pub sha256: String,
}
/// The sidecar intentionally has no hash of itself or of later edited content.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditableVisualReport {
    pub schema_version: u32,
    pub purpose: String,
    pub reference_sha256: String,
    pub reference_header: Value,
    pub identities: Vec<AuthoringIdentity>,
    /// Every generated file except this report's own authoring.json serialization.
    pub files: BTreeMap<String, AuthoringFile>,
    /// Sum of `files` sizes; authoring.json is additionally charged to the output limit.
    pub payload_bytes: usize,
    /// Optional root collections actually removed because they were empty.
    pub omitted_empty_collections: Vec<String>,
    pub notes: Vec<String>,
}
#[derive(Debug)]
pub struct EditableVisualBundle {
    /// Generated safe relative paths. Content is deterministic for identical input bytes.
    pub files: BTreeMap<String, Vec<u8>>,
    pub report: EditableVisualReport,
}

fn sha256(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
fn charge(total: &mut usize, length: usize, limit: usize) -> Result<()> {
    let next = total
        .checked_add(length)
        .context("editable output byte count overflow")?;
    ensure!(next <= limit, "editable output byte limit exceeded");
    *total = next;
    Ok(())
}
fn copy(bytes: &[u8]) -> Result<Vec<u8>> {
    let mut result = Vec::new();
    result
        .try_reserve_exact(bytes.len())
        .context("cannot reserve editable payload")?;
    result.extend_from_slice(bytes);
    Ok(result)
}
struct BoundedJson {
    bytes: Vec<u8>,
    limit: usize,
}
impl Write for BoundedJson {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let next = self
            .bytes
            .len()
            .checked_add(bytes.len())
            .filter(|&n| n <= self.limit)
            .ok_or_else(|| io::Error::other("editable JSON/output byte limit exceeded"))?;
        self.bytes
            .try_reserve(next - self.bytes.len())
            .map_err(io::Error::other)?;
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn serialize(value: &impl Serialize, limit: usize) -> Result<Vec<u8>> {
    let mut output = BoundedJson {
        bytes: Vec::new(),
        limit,
    };
    serde_json::to_writer_pretty(&mut output, value)?;
    output.write_all(b"\n")?;
    Ok(output.bytes)
}

type RawObject = BTreeMap<String, Box<RawValue>>;

fn bounded_raw(value: &impl Serialize, limit: usize) -> Result<Box<RawValue>> {
    let bytes = serialize(value, limit)?;
    Ok(RawValue::from_string(String::from_utf8(bytes)?)?)
}

fn entries(root: &RawObject, key: &str) -> Result<Vec<RawObject>> {
    root.get(key)
        .map(|v| serde_json::from_str(v.get()).map_err(Into::into))
        .unwrap_or_else(|| Ok(Vec::new()))
}

/// Export an admitted modern static visual GLB without rebuilding its geometry.
///
/// Legacy inspection artifacts rely on a client unlit override rather than standard
/// material declarations and are refused. Portable export supports eight-bit PNGs;
/// higher-precision reference images are never silently quantized. No URI is fetched.
pub fn export_editable_visual(
    glb: &[u8],
    limits: &EditableVisualLimits,
) -> Result<EditableVisualBundle> {
    ensure!(
        glb.len() <= limits.max_glb_bytes,
        "editable input GLB byte limit exceeded"
    );
    let mut decode_limits = DecodeLimits::default();
    decode_limits.max_glb_bytes = decode_limits.max_glb_bytes.min(limits.max_glb_bytes);
    decode_limits.max_json_bytes = decode_limits.max_json_bytes.min(limits.max_json_bytes);
    decode_limits.max_resources = decode_limits.max_resources.min(limits.max_resources);
    decode_limits.max_png_bytes = decode_limits.max_png_bytes.min(limits.max_png_bytes);
    decode_limits.max_single_png_bytes = decode_limits
        .max_single_png_bytes
        .min(limits.max_single_png_bytes);
    let visual =
        decode_static_visual(glb, &decode_limits).context("reference static visual refused")?;
    ensure!(visual.header.requirements.capabilities.iter().any(|c| c == eqoxide_assets::static_visual::PBR_FACTORS_CAPABILITY),
        "legacy inspection lighting is not a portable declared material; rebake with a modern material policy");
    for (index, image) in visual.images.iter().enumerate() {
        // The common reader already validated PNG signature, IHDR, CRCs and pixels.
        ensure!(
            image.png_bytes.get(24) == Some(&8),
            "image {index} requires an eight-bit portable PNG; original preserved"
        );
    }
    let binary = gltf::binary::Glb::from_slice(glb).context("reference GLB packaging")?;
    // Raw values preserve unchanged numeric tokens. Parsing and reserializing a
    // floating-point matrix can change its decimal value even without editing it.
    let mut root: RawObject = serde_json::from_slice(&binary.json)?;
    let mut extras: RawObject = serde_json::from_str(
        root.get("extras")
            .context("reference extras missing")?
            .get(),
    )?;
    let header: Value = serde_json::from_str(
        extras
            .remove("eqoxideAsset")
            .context("reference activation header missing")?
            .get(),
    )?;
    root.insert(
        "extras".into(),
        bounded_raw(&extras, limits.max_json_bytes)?,
    );
    let reference_sha256 = sha256(glb);
    let mut identities = Vec::new();
    let identity_count = [
        visual.materials.len(),
        visual.meshes.len(),
        visual.instances.len(),
    ]
    .into_iter()
    .try_fold(0usize, |sum, n| sum.checked_add(n))
    .context("authoring identity count overflow")?;
    identities
        .try_reserve_exact(identity_count)
        .context("cannot reserve authoring identities")?;
    for (collection, kind) in [
        ("materials", "material"),
        ("meshes", "mesh"),
        ("nodes", "node"),
    ] {
        let mut values = entries(&root, collection)?;
        for (ordinal, entry) in values.iter_mut().enumerate() {
            let id = format!("{kind}-{ordinal:06}");
            identities.push(AuthoringIdentity {
                kind: kind.into(),
                id: id.clone(),
                original_ordinal: ordinal,
                name: entry
                    .get("name")
                    .map(|name| serde_json::from_str(name.get()))
                    .transpose()?,
            });
            entry.insert(
                "extras".into(),
                to_raw_value(&json!({"eqoxideAuthoring": {
                    "referenceSha256": reference_sha256, "kind": kind, "id": id
                }}))?,
            );
        }
        root.insert(
            collection.into(),
            bounded_raw(&values, limits.max_json_bytes)?,
        );
    }
    let mut buffers = entries(&root, "buffers")?;
    let buffer = buffers.first_mut().context("reference buffer missing")?;
    let declared_bytes = usize::try_from(serde_json::from_str::<u64>(
        buffer
            .get("byteLength")
            .context("reference buffer length")?
            .get(),
    )?)
    .context("reference buffer length exceeds platform size")?;
    buffer.insert("uri".into(), to_raw_value("scene.bin")?);
    root.insert(
        "buffers".into(),
        bounded_raw(&buffers, limits.max_json_bytes)?,
    );
    let bin = binary
        .bin
        .as_ref()
        .context("reference BIN missing")?
        .get(..declared_bytes)
        .context("reference buffer exceeds BIN")?;
    let mut payload_bytes = 0;
    charge(&mut payload_bytes, glb.len(), limits.max_output_bytes)?;
    charge(&mut payload_bytes, bin.len(), limits.max_output_bytes)?;
    for image in &visual.images {
        charge(
            &mut payload_bytes,
            image.png_bytes.len(),
            limits.max_output_bytes,
        )?;
    }
    let mut files = BTreeMap::new();
    files.insert("reference.glb".into(), copy(glb)?);
    files.insert("scene.bin".into(), copy(bin)?);
    let mut images = entries(&root, "images")?;
    ensure!(
        images.len() == visual.images.len(),
        "reference image count mismatch"
    );
    for (index, (entry, image)) in images.iter_mut().zip(visual.images).enumerate() {
        let path = format!("images/image-{index:06}.png");
        entry.remove("bufferView");
        entry.remove("mimeType");
        entry.insert("uri".into(), to_raw_value(&path)?);
        files.insert(path, image.png_bytes);
    }
    if root.contains_key("images") || !images.is_empty() {
        root.insert(
            "images".into(),
            bounded_raw(&images, limits.max_json_bytes)?,
        );
    }
    // Optional glTF collections have minItems=1 when present; absent and empty
    // are equivalent to the common reader but only absence is portable here.
    let omitted_empty_collections: Vec<String> = root
        .iter()
        .filter_map(|(key, value)| {
            let value = value.get().trim();
            (value.starts_with('[')
                && serde_json::from_str::<Vec<&RawValue>>(value).is_ok_and(|a| a.is_empty()))
            .then(|| key.clone())
        })
        .collect();
    for key in &omitted_empty_collections {
        root.remove(key);
    }
    let json = serialize(
        &root,
        limits
            .max_json_bytes
            .min(limits.max_output_bytes - payload_bytes),
    )?;
    charge(&mut payload_bytes, json.len(), limits.max_output_bytes)?;
    files.insert("scene.gltf".into(), json);
    let report = EditableVisualReport {
        schema_version: 1,
        purpose: "portable-visual-authoring-unactivated".into(),
        reference_sha256,
        reference_header: header,
        identities,
        files: files.iter().map(|(path, bytes)| (path.clone(), AuthoringFile { bytes: bytes.len(), sha256: sha256(bytes) })).collect(),
        payload_bytes,
        omitted_empty_collections,
        notes: vec![
            "Engine activation header omitted from scene.gltf; reference.glb is unchanged input, not edited output.".into(),
            "Authoring IDs are original ordinals scoped to this reference SHA256; names and exported indices are not universal identities.".into(),
            "Original BIN ranges and buffer views retained, including now-unused embedded image ranges; BIN alignment padding outside buffer.byteLength is not copied to scene.bin.".into(),
            "Reimport, collision pairing and edited package activation are not implemented by this exporter.".into(),
        ],
    };
    let sidecar = serialize(&report, limits.max_output_bytes - payload_bytes)?;
    charge(&mut payload_bytes, sidecar.len(), limits.max_output_bytes)?;
    files.insert("authoring.json".into(), sidecar);
    Ok(EditableVisualBundle { files, report })
}
