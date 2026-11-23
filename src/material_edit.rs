//! Selected standard material edits applied to immutable original geometry.
use crate::editable_visual::{export_editable_visual, EditableVisualLimits};
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{
    json,
    value::{to_raw_value, RawValue},
    Map, Value,
};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
    path::Path,
};

const POLICY: &str = "selected-material-original-geometry-single-mip-v1";
type RawObject = BTreeMap<String, Box<RawValue>>;

#[derive(Debug, Clone)]
pub struct MaterialEditLimits {
    pub reference: EditableVisualLimits,
    pub max_edited_glb_bytes: usize,
    pub max_edited_json_bytes: usize,
    pub max_resources: usize,
    pub max_single_png_bytes: usize,
    pub max_single_decoded_png_bytes: usize,
    pub max_selected_png_bytes: usize,
    pub max_selected_decoded_png_bytes: usize,
    pub max_dimension: u32,
    pub max_output_glb_bytes: usize,
    pub max_report_bytes: usize,
}
impl Default for MaterialEditLimits {
    fn default() -> Self {
        Self {
            reference: EditableVisualLimits::default(),
            max_edited_glb_bytes: 128 << 20,
            max_edited_json_bytes: 8 << 20,
            max_resources: 100_000,
            max_single_png_bytes: 64 << 20,
            max_single_decoded_png_bytes: 64 << 20,
            max_selected_png_bytes: 128 << 20,
            max_selected_decoded_png_bytes: 128 << 20,
            max_dimension: 8192,
            max_output_glb_bytes: 128 << 20,
            max_report_bytes: 16 << 20,
        }
    }
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MaterialEditReport {
    pub schema_version: u32,
    pub policy: String,
    pub reference_sha256: String,
    pub edited_sha256: String,
    pub bake_revision: String,
    pub selected_materials: Vec<AppliedMaterialEdit>,
    pub unselected_editor_material_count: usize,
    pub ignored_editor_geometry: bool,
    pub notes: Vec<String>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppliedMaterialEdit {
    pub id: String,
    pub original_ordinal: usize,
    pub before: Value,
    pub after: Value,
    pub sampler_normalization: Option<Value>,
}
#[derive(Debug)]
pub struct ReimportedMaterialEdits {
    pub glb: Vec<u8>,
    pub report: MaterialEditReport,
}
fn sha(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
struct BoundedJson {
    bytes: Vec<u8>,
    limit: usize,
}
impl Write for BoundedJson {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let next = self
            .bytes
            .len()
            .checked_add(bytes.len())
            .filter(|n| *n <= self.limit)
            .ok_or_else(|| std::io::Error::other("JSON/report byte limit exceeded"))?;
        self.bytes
            .try_reserve(next - self.bytes.len())
            .map_err(std::io::Error::other)?;
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn serialize_bounded(value: &impl Serialize, pretty: bool, limit: usize) -> Result<Vec<u8>> {
    let mut out = BoundedJson {
        bytes: Vec::new(),
        limit,
    };
    if pretty {
        serde_json::to_writer_pretty(&mut out, value)?;
    } else {
        serde_json::to_writer(&mut out, value)?;
    }
    Ok(out.bytes)
}
fn bounded_raw(value: &impl Serialize) -> Result<Box<RawValue>> {
    Ok(RawValue::from_string(String::from_utf8(
        serialize_bounded(value, false, 8 << 20)?,
    )?)?)
}
/// Serialize the complete audit report with its limit enforced during growth.
pub fn material_edit_report_json(report: &MaterialEditReport, limit: usize) -> Result<Vec<u8>> {
    let mut bytes = serialize_bounded(report, true, limit.saturating_sub(1))?;
    ensure!(
        bytes.len() < limit,
        "material edit report byte limit exceeded"
    );
    bytes.push(b'\n');
    Ok(bytes)
}
fn object<'a>(v: &'a Value, keys: &[&str], what: &str) -> Result<&'a Map<String, Value>> {
    let m = v
        .as_object()
        .with_context(|| format!("{what} must be an object"))?;
    for k in m.keys() {
        ensure!(keys.contains(&k.as_str()), "unsupported {what} field {k}");
    }
    Ok(m)
}
fn index(v: &Value, what: &str) -> Result<usize> {
    usize::try_from(
        v.as_u64()
            .with_context(|| format!("{what} must be an unsigned integer"))?,
    )
    .context("index exceeds platform size")
}
fn required<'a>(m: &'a Map<String, Value>, k: &str) -> Result<&'a Value> {
    m.get(k).with_context(|| format!("missing {k}"))
}
fn scalar(m: &Map<String, Value>, k: &str, default: f64) -> Result<f64> {
    let x = match m.get(k) {
        Some(v) => v.as_f64().with_context(|| format!("{k} must be numeric"))?,
        None => default,
    };
    ensure!(
        x.is_finite() && (0.0..=1.0).contains(&x),
        "{k} must be normalized"
    );
    Ok(x)
}
fn array<'a>(root: &'a Value, key: &str) -> Result<&'a [Value]> {
    match root.get(key) {
        Some(v) => Ok(v
            .as_array()
            .with_context(|| format!("{key} must be an array"))?),
        None => Ok(&[]),
    }
}
fn at<'a>(root: &'a Value, key: &str, i: usize) -> Result<&'a Value> {
    array(root, key)?
        .get(i)
        .with_context(|| format!("{key} index out of bounds"))
}
fn raw_entries(root: &RawObject, k: &str) -> Result<Vec<Box<RawValue>>> {
    root.get(k)
        .map(|v| serde_json::from_str(v.get()).map_err(Into::into))
        .unwrap_or_else(|| Ok(Vec::new()))
}
fn charge(total: &mut usize, n: usize, limit: usize, what: &str) -> Result<()> {
    *total = total.checked_add(n).context("resource byte sum overflow")?;
    ensure!(*total <= limit, "{what} limit exceeded");
    Ok(())
}

// Reject ambiguous JSON keys before building the semantic editor model.
struct Unique(Value);
impl<'de> Deserialize<'de> for Unique {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = Unique;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("unambiguous JSON")
            }
            fn visit_bool<E: serde::de::Error>(self, v: bool) -> std::result::Result<Unique, E> {
                Ok(Unique(v.into()))
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> std::result::Result<Unique, E> {
                Ok(Unique(v.into()))
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> std::result::Result<Unique, E> {
                Ok(Unique(v.into()))
            }
            fn visit_f64<E: serde::de::Error>(self, v: f64) -> std::result::Result<Unique, E> {
                Ok(Unique(json!(v)))
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> std::result::Result<Unique, E> {
                Ok(Unique(v.into()))
            }
            fn visit_string<E: serde::de::Error>(
                self,
                v: String,
            ) -> std::result::Result<Unique, E> {
                Ok(Unique(v.into()))
            }
            fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<Unique, E> {
                Ok(Unique(Value::Null))
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut a: A,
            ) -> std::result::Result<Unique, A::Error> {
                let mut out = Vec::new();
                while let Some(Unique(v)) = a.next_element()? {
                    out.push(v)
                }
                Ok(Unique(Value::Array(out)))
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut a: A,
            ) -> std::result::Result<Unique, A::Error> {
                let mut out = Map::new();
                while let Some((k, Unique(v))) = a.next_entry::<String, Unique>()? {
                    if out.insert(k, v).is_some() {
                        return Err(serde::de::Error::custom("duplicate JSON key"));
                    }
                }
                Ok(Unique(Value::Object(out)))
            }
        }
        d.deserialize_any(Visitor)
    }
}
fn edited_document<'a>(bytes: &'a [u8], limits: &MaterialEditLimits) -> Result<(Value, &'a [u8])> {
    ensure!(
        bytes.len() <= limits.max_edited_glb_bytes.min(128 << 20),
        "edited GLB byte limit exceeded"
    );
    let word = |i: usize| -> Result<usize> {
        Ok(u32::from_le_bytes(bytes.get(i..i + 4).context("truncated GLB")?.try_into()?) as usize)
    };
    ensure!(
        bytes.get(..4) == Some(b"glTF") && word(4)? == 2 && word(8)? == bytes.len(),
        "invalid GLB header/length"
    );
    let n = word(12)?;
    ensure!(
        n % 4 == 0 && n <= limits.max_edited_json_bytes.min(8 << 20),
        "edited JSON byte/alignment limit"
    );
    ensure!(
        bytes.get(16..20) == Some(b"JSON"),
        "GLB JSON chunk required"
    );
    let end = 20usize.checked_add(n).context("JSON range overflow")?;
    let Unique(root) = serde_json::from_slice(bytes.get(20..end).context("JSON range")?)?;
    ensure!(
        root.is_object() && root["asset"]["version"] == "2.0",
        "edited glTF 2.0 document required"
    );
    for v in root.as_object().unwrap().values() {
        if let Some(a) = v.as_array() {
            ensure!(
                a.len() <= limits.max_resources.min(100_000),
                "edited resource collection limit exceeded"
            );
        }
    }
    let bin_len = word(end)?;
    ensure!(bin_len % 4 == 0, "BIN alignment");
    ensure!(
        bytes.get(end + 4..end + 8) == Some(b"BIN\0"),
        "single embedded BIN required"
    );
    let start = end.checked_add(8).context("BIN range overflow")?;
    ensure!(
        start.checked_add(bin_len) == Some(bytes.len()),
        "GLB trailing/truncated chunks"
    );
    let buffers = array(&root, "buffers")?;
    ensure!(buffers.len() == 1, "one embedded buffer required");
    let buffer = object(&buffers[0], &["byteLength", "name", "extras"], "buffer")?;
    let declared = index(required(buffer, "byteLength")?, "buffer byteLength")?;
    ensure!(
        declared <= bin_len && bin_len - declared <= 3,
        "buffer length differs from BIN"
    );
    Ok((root, &bytes[start..start + declared]))
}
struct Selected {
    ordinal: usize,
    material: Value,
    image: Option<usize>,
    sampler: Option<Value>,
    effective: Value,
}
fn extract_material(root: &Value, editor: &Value, ordinal: usize) -> Result<Selected> {
    let m = object(
        editor,
        &[
            "name",
            "extras",
            "pbrMetallicRoughness",
            "alphaMode",
            "alphaCutoff",
            "doubleSided",
            "extensions",
            "emissiveFactor",
        ],
        "selected material",
    )?;
    if let Some(name) = m.get("name") {
        ensure!(name.is_string(), "material name must be a string");
    }
    if let Some(e) = m.get("emissiveFactor") {
        ensure!(
            e.as_array()
                .is_some_and(|a| a.len() == 3 && a.iter().all(|v| v.as_f64() == Some(0.0))),
            "nonzero/invalid emission unsupported"
        );
    }
    let empty = json!({});
    let p = object(
        m.get("pbrMetallicRoughness").unwrap_or(&empty),
        &[
            "baseColorFactor",
            "metallicFactor",
            "roughnessFactor",
            "baseColorTexture",
        ],
        "selected PBR",
    )?;
    let factor = p
        .get("baseColorFactor")
        .cloned()
        .unwrap_or(json!([1, 1, 1, 1]));
    ensure!(
        factor.as_array().is_some_and(|a| a.len() == 4
            && a.iter().all(|v| v
                .as_f64()
                .is_some_and(|x| x.is_finite() && (0.0..=1.0).contains(&x)))),
        "baseColorFactor must be normalized RGBA"
    );
    let factor: Vec<f64> = factor
        .as_array()
        .context("baseColorFactor array")?
        .iter()
        .map(|v| v.as_f64().context("baseColorFactor numeric lane"))
        .collect::<Result<_>>()?;
    let metallic = scalar(p, "metallicFactor", 1.)?;
    let roughness = scalar(p, "roughnessFactor", 1.)?;
    let mode = m
        .get("alphaMode")
        .map(|v| v.as_str().context("alphaMode must be a string"))
        .transpose()?
        .unwrap_or("OPAQUE");
    ensure!(
        ["OPAQUE", "MASK", "BLEND"].contains(&mode),
        "unsupported alpha mode"
    );
    ensure!(
        mode == "MASK" || !m.contains_key("alphaCutoff"),
        "alphaCutoff requires MASK"
    );
    let cutoff = scalar(m, "alphaCutoff", 0.5)?;
    let double = m
        .get("doubleSided")
        .map(|v| v.as_bool().context("doubleSided must be boolean"))
        .transpose()?
        .unwrap_or(false);
    let unlit = if let Some(e) = m.get("extensions") {
        let e = object(e, &["KHR_materials_unlit"], "selected material extensions")?;
        object(required(e, "KHR_materials_unlit")?, &[], "unlit")?;
        true
    } else {
        false
    };
    let mut output = json!({"pbrMetallicRoughness":{"baseColorFactor":factor,"metallicFactor":metallic,"roughnessFactor":roughness},"alphaMode":mode,"doubleSided":double});
    if mode == "MASK" {
        output["alphaCutoff"] = json!(cutoff)
    }
    if unlit {
        output["extensions"] = json!({"KHR_materials_unlit":{}})
    }
    let mut image = None;
    let mut normalization = None;
    if let Some(t) = p.get("baseColorTexture") {
        let t = object(t, &["index", "texCoord"], "selected texture info")?;
        ensure!(
            t.get("texCoord")
                .map(|v| index(v, "texCoord"))
                .transpose()?
                .unwrap_or(0)
                == 0,
            "only UV0 supported"
        );
        let texture = object(
            at(
                root,
                "textures",
                index(required(t, "index")?, "texture index")?,
            )?,
            &["source", "sampler", "name", "extras"],
            "selected texture",
        )?;
        image = Some(index(required(texture, "source")?, "image index")?);
        let supplied = if let Some(s) = texture.get("sampler") {
            at(root, "samplers", index(s, "sampler index")?)?.clone()
        } else {
            json!({})
        };
        let s = object(
            &supplied,
            &["magFilter", "minFilter", "wrapS", "wrapT", "name", "extras"],
            "selected sampler",
        )?;
        let value = |k: &str, default| {
            s.get(k)
                .map(|v| index(v, k))
                .transpose()
                .map(|v| v.unwrap_or(default))
        };
        ensure!(
            value("wrapS", 10497)? == 10497 && value("wrapT", 10497)? == 10497,
            "only repeat wrapping supported"
        );
        ensure!(
            value("magFilter", 9729)? == 9729
                && [9729, 9985, 9987].contains(&value("minFilter", 9729)?),
            "only linear single-mip sampling supported"
        );
        normalization = Some(
            json!({"original":supplied,"resolved":{"wrapS":10497,"wrapT":10497,"magFilter":9729,"minFilter":9729,"mipLevels":1},"policy":"repeat-bilinear-level-zero"}),
        );
    }
    let effective = output.clone();
    Ok(Selected {
        ordinal,
        material: output,
        image,
        sampler: normalization,
        effective,
    })
}
fn image_bytes<'a>(
    root: &Value,
    bin: &'a [u8],
    i: usize,
    limits: &MaterialEditLimits,
) -> Result<(&'a [u8], usize)> {
    let image = object(
        at(root, "images", i)?,
        &["bufferView", "mimeType", "name", "extras"],
        "selected image",
    )?;
    ensure!(
        image.get("mimeType").and_then(Value::as_str) == Some("image/png"),
        "embedded PNG required"
    );
    let view = object(
        at(
            root,
            "bufferViews",
            index(required(image, "bufferView")?, "image bufferView")?,
        )?,
        &["buffer", "byteOffset", "byteLength", "name", "extras"],
        "image buffer view",
    )?;
    ensure!(
        index(required(view, "buffer")?, "image buffer")? == 0,
        "image buffer must be zero"
    );
    let start = view
        .get("byteOffset")
        .map(|v| index(v, "image offset"))
        .transpose()?
        .unwrap_or(0);
    let length = index(required(view, "byteLength")?, "image length")?;
    ensure!(
        length <= limits.max_single_png_bytes.min(64 << 20),
        "selected single PNG encoded limit exceeded"
    );
    let end = start.checked_add(length).context("image range overflow")?;
    let bytes = bin
        .get(start..end)
        .context("image range outside declared BIN")?;
    ensure!(
        bytes.get(..8) == Some(b"\x89PNG\r\n\x1a\n") && bytes.get(24) == Some(&8),
        "eight-bit PNG required"
    );
    let dimension = |o| -> Result<u32> {
        Ok(u32::from_be_bytes(
            bytes
                .get(o..o + 4)
                .context("truncated PNG IHDR")?
                .try_into()?,
        ))
    };
    let w = dimension(16)?;
    let h = dimension(20)?;
    ensure!(
        w > 0
            && h > 0
            && w <= limits.max_dimension.min(8192)
            && h <= limits.max_dimension.min(8192),
        "PNG dimension limit exceeded"
    );
    let decoded =
        usize::try_from(u64::from(w) * u64::from(h) * 4).context("decoded PNG size overflow")?;
    ensure!(
        decoded <= limits.max_single_decoded_png_bytes.min(64 << 20),
        "selected single PNG decoded limit exceeded"
    );
    Ok((bytes, decoded))
}

/// Apply explicit reference-scoped material IDs; all editor geometry is ignored.
pub fn reimport_material_edits(
    reference: &[u8],
    authoring_sidecar: &[u8],
    edited_glb: &[u8],
    selected_ids: &[String],
    limits: &MaterialEditLimits,
) -> Result<ReimportedMaterialEdits> {
    ensure!(
        !selected_ids.is_empty() && selected_ids.len() <= limits.max_resources.min(100_000),
        "explicit bounded material selection required"
    );
    ensure!(
        authoring_sidecar.len() <= limits.reference.max_output_bytes,
        "sidecar byte limit exceeded"
    );
    let bundle = export_editable_visual(reference, &limits.reference)?;
    ensure!(
        authoring_sidecar == bundle.files["authoring.json"],
        "immutable authoring sidecar does not match reference exporter report"
    );
    let reference_sha = bundle.report.reference_sha256.clone();
    let canonical: BTreeMap<_, _> = bundle
        .report
        .identities
        .iter()
        .filter(|x| x.kind == "material")
        .map(|x| (x.id.clone(), x.original_ordinal))
        .collect();
    let mut requested = BTreeSet::new();
    for id in selected_ids {
        ensure!(
            requested.insert(id.as_str()),
            "duplicate selected material ID"
        );
        ensure!(
            canonical.contains_key(id),
            "unknown reference-scoped original material ID"
        );
    }
    drop(bundle);
    let (editor, editor_bin) = edited_document(edited_glb, limits)?;
    let materials = array(&editor, "materials")?;
    let mut selected = BTreeMap::new();
    for material in materials {
        let tag = &material["extras"]["eqoxideAuthoring"];
        if let Some(id) = tag
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| requested.contains(id))
        {
            let tag = object(tag, &["id", "kind", "referenceSha256"], "selected identity")?;
            ensure!(
                tag.get("referenceSha256").and_then(Value::as_str) == Some(reference_sha.as_str())
                    && tag.get("kind").and_then(Value::as_str) == Some("material"),
                "foreign reference or wrong-kind selected identity"
            );
            let ordinal = canonical[id];
            ensure!(
                !selected.contains_key(&ordinal),
                "ambiguous duplicate selected identity"
            );
            selected.insert(ordinal, extract_material(&editor, material, ordinal)?);
        }
    }
    ensure!(
        selected.len() == selected_ids.len(),
        "selected material identity missing or malformed"
    );
    let mut imports = BTreeMap::new();
    let mut encoded = 0;
    let mut decoded = 0;
    for material in selected.values() {
        if let Some(i) = material.image {
            if let std::collections::btree_map::Entry::Vacant(e) = imports.entry(i) {
                let (bytes, pixels) = image_bytes(&editor, editor_bin, i, limits)?;
                charge(
                    &mut encoded,
                    bytes.len(),
                    limits.max_selected_png_bytes.min(128 << 20),
                    "selected PNG encoded aggregate",
                )?;
                charge(
                    &mut decoded,
                    pixels,
                    limits.max_selected_decoded_png_bytes.min(128 << 20),
                    "selected PNG decoded aggregate",
                )?;
                e.insert(bytes);
            }
        }
    }
    let mut image_hashes = BTreeMap::new();
    for (&image_index, bytes) in &imports {
        crate::static_scene::validate_png_stream(bytes)?;
        crate::convert::decode_texture_rgba(
            bytes,
            crate::convert::TextureDecodePolicy::PreserveRgba,
            "selected PNG",
            crate::convert::TextureDecodeProfile::Static,
        )?;
        image_hashes.insert(image_index, sha(bytes));
    }
    let packed = gltf::binary::Glb::from_slice(reference)?;
    let mut root: RawObject = serde_json::from_slice(&packed.json)?;
    let mut buffers: Vec<Value> = serde_json::from_str(root["buffers"].get())?;
    let declared = index(&buffers[0]["byteLength"], "reference byteLength")?;
    let mut bin = packed.bin.as_ref().context("reference BIN missing")?[..declared].to_vec();
    let mut original_mats = raw_entries(&root, "materials")?;
    let mut views = raw_entries(&root, "bufferViews")?;
    let mut images = raw_entries(&root, "images")?;
    let mut textures = raw_entries(&root, "textures")?;
    struct ImportedImage<'a> {
        bytes: &'a [u8],
        texture: usize,
    }
    let mut imported: BTreeMap<String, Vec<ImportedImage<'_>>> = BTreeMap::new();
    let mut texture_by_image = BTreeMap::new();
    let mut applied = Vec::new();
    let mut identity_input = Vec::new();
    for s in selected.values_mut() {
        let before: Value = serde_json::from_str(original_mats[s.ordinal].get())?;
        let mut png_identity = None;
        if let Some(i) = s.image {
            let bytes = imports[&i];
            let hash = image_hashes[&i].clone();
            png_identity = Some(hash.clone());
            let bucket = imported.entry(hash).or_default();
            let found = texture_by_image.get(&i).copied().or_else(|| {
                bucket
                    .iter()
                    .find(|old| old.bytes == bytes)
                    .map(|old| old.texture)
            });
            let texture = if let Some(index) = found {
                index
            } else {
                while bin.len() % 4 != 0 {
                    bin.push(0)
                }
                let offset = bin.len();
                let mut projected = bin.len();
                charge(
                    &mut projected,
                    bytes.len(),
                    limits.max_output_glb_bytes.min(128 << 20),
                    "output BIN",
                )?;
                bin.extend_from_slice(bytes);
                let view = views.len();
                views.push(to_raw_value(
                    &json!({"buffer":0,"byteOffset":offset,"byteLength":bytes.len()}),
                )?);
                let image = images.len();
                images.push(to_raw_value(
                    &json!({"bufferView":view,"mimeType":"image/png"}),
                )?);
                let texture = textures.len();
                textures.push(to_raw_value(&json!({"source":image}))?);
                bucket.push(ImportedImage { bytes, texture });
                texture
            };
            texture_by_image.insert(i, texture);
            s.material["pbrMetallicRoughness"]["baseColorTexture"] = json!({"index":texture});
        }
        if let Some(name) = before.get("name") {
            s.material["name"] = name.clone()
        }
        identity_input.push(json!({"id":format!("material-{:06}",s.ordinal),"material":s.effective,"pngSha256":png_identity,"sampling":s.image.map(|_|"repeat-bilinear-level-zero")}));
        applied.push(AppliedMaterialEdit {
            id: format!("material-{:06}", s.ordinal),
            original_ordinal: s.ordinal,
            before,
            after: s.material.clone(),
            sampler_normalization: s.sampler.clone(),
        });
        original_mats[s.ordinal] = to_raw_value(&s.material)?;
    }
    root.insert("materials".into(), bounded_raw(&original_mats)?);
    root.insert("bufferViews".into(), bounded_raw(&views)?);
    if !images.is_empty() {
        root.insert("images".into(), bounded_raw(&images)?);
    }
    if !textures.is_empty() {
        root.insert("textures".into(), bounded_raw(&textures)?);
    }
    buffers[0]["byteLength"] = json!(bin.len());
    root.insert("buffers".into(), to_raw_value(&buffers)?);
    let all: Vec<Value> = original_mats
        .iter()
        .map(|v| serde_json::from_str(v.get()))
        .collect::<std::result::Result<_, _>>()?;
    let uses_unlit = all
        .iter()
        .any(|m| m["extensions"].get("KHR_materials_unlit").is_some());
    for key in ["extensionsUsed", "extensionsRequired"] {
        let mut names: Vec<String> = match root.get(key) {
            Some(raw) => serde_json::from_str(raw.get())?,
            None => Vec::new(),
        };
        names.retain(|n| n != "KHR_materials_unlit");
        if uses_unlit {
            names.push("KHR_materials_unlit".into());
            names.sort();
        }
        if names.is_empty() {
            root.remove(key);
        } else {
            root.insert(key.into(), to_raw_value(&names)?);
        }
    }
    let implementation = sha(include_bytes!("material_edit.rs"));
    let revision = sha(&serde_json::to_vec(
        &json!({"policy":POLICY,"implementation":implementation,"writer":sha(include_bytes!("convert/mod.rs")),"reference":reference_sha,"edits":identity_input}),
    )?);
    let mut extras: RawObject = serde_json::from_str(root["extras"].get())?;
    let mut header: Value = serde_json::from_str(extras["eqoxideAsset"].get())?;
    header["bakeRevision"] = json!(revision);
    let needs_blend = all
        .iter()
        .any(|m| m["alphaMode"] == "BLEND" && m["extensions"].get("KHR_materials_unlit").is_none());
    let caps = header["requirements"]["capabilities"]
        .as_array_mut()
        .context("reference capabilities")?;
    let hdr = json!("pbr-metallic-roughness-blend-v1");
    if needs_blend {
        if !caps.contains(&hdr) {
            caps.push(hdr);
            caps.sort_by_key(|v| v.as_str().unwrap_or_default().to_owned());
        }
    } else {
        caps.retain(|c| c != &hdr);
    }
    extras.insert("eqoxideAsset".into(), to_raw_value(&header)?);
    root.insert("extras".into(), to_raw_value(&extras)?);
    let json = serialize_bounded(&root, false, 8 << 20)?;
    let glb = crate::convert::encode_glb_document(
        &json,
        &bin,
        limits.max_output_glb_bytes.min(128 << 20),
    )?;
    let mut final_limits = eqoxide_assets::DecodeLimits::default();
    final_limits.max_glb_bytes = final_limits.max_glb_bytes.min(limits.max_output_glb_bytes);
    eqoxide_assets::decode_static_visual(&glb, &final_limits)
        .context("final material-edited candidate refused")?;
    let report=MaterialEditReport{schema_version:1,policy:POLICY.into(),reference_sha256:reference_sha,edited_sha256:sha(edited_glb),bake_revision:revision,selected_materials:applied,unselected_editor_material_count:materials.len()-selected.len(),ignored_editor_geometry:true,notes:vec!["Editor meshes, normals, UVs, colors, transforms and assignments were ignored, not validated or roundtrip-qualified.".into(),"Only immutable reference.glb and authoring.json provenance were checked; other original bundle files were not freshly checked.".into(),"Sampler normalization targets the current single-mip repeat bilinear profile; authored mip filtering and cross-viewer pixel equivalence are not preserved claims.".into(),"Standalone visual output: collision/package pairing and production activation are not performed.".into()]};
    material_edit_report_json(&report, limits.max_report_bytes)?;
    Ok(ReimportedMaterialEdits { glb, report })
}

/// Atomically publish a complete reader-admitted visual; earlier errors preserve the old file.
pub fn publish_material_edits(glb: &[u8], destination: &Path) -> Result<()> {
    eqoxide_assets::decode_static_visual(glb, &Default::default())
        .context("publication candidate refused")?;
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temp = tempfile::Builder::new()
        .prefix(".material-edits-")
        .tempfile_in(parent)?;
    temp.write_all(glb)?;
    crate::static_scene::publish_static(temp, destination, "material-edited visual")
}
