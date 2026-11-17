//! Strict offline import of selected WLD static geometry into the common visual scene.
//! Source lighting stays in the original inputs and is reported, never applied as albedo.
use crate::{
    source_material::{
        decode_static_texture, normalize_material, AlphaMode, SourceMaterial, TextureDecodePolicy,
    },
    static_scene::{self, StaticInstance, StaticMesh, StaticPrimitive, StaticScene},
};
use anyhow::{bail, ensure, Context, Result};
use glam::{DVec3, Mat4};
use libeq_pfs::PfsReader;
use libeq_wld::parser::{
    Actor, ActorDef, DmSprite, DmSpriteDef2, DrawStyle, Fragment, FragmentParser, FragmentRef,
    FragmentType, MaterialDef, MaterialType, RenderMethod, TextureCoordinates, TextureStyle,
    WldDoc,
};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    fs::File,
    io::{Read, Seek, SeekFrom},
    ops::Deref,
    panic::{catch_unwind, AssertUnwindSafe},
    path::Path,
};

mod snapshot;
pub use snapshot::{WldTextureAnimation, WldTextureFrame, WldVertexSnapshot, WldVisualMode};

/// Logical input, output and traversal limits; these are not peak-process-memory limits.
#[derive(Debug, Clone)]
pub struct WldVisualLimits {
    pub max_archive_bytes: usize,
    pub max_member_bytes: usize,
    pub max_input_bytes: usize,
    pub max_fragments: usize,
    pub max_vertices: usize,
    pub max_indices: usize,
    pub max_instances: usize,
    pub max_resources: usize,
    pub max_transform_visits: usize,
    pub max_output_bytes: usize,
}
impl Default for WldVisualLimits {
    fn default() -> Self {
        Self {
            max_archive_bytes: 128 << 20,
            max_member_bytes: 64 << 20,
            max_input_bytes: 256 << 20,
            max_fragments: 100_000,
            max_vertices: 5_000_000,
            max_indices: 15_000_000,
            max_instances: 100_000,
            max_resources: 100_000,
            max_transform_visits: 50_000_000,
            max_output_bytes: 128 << 20,
        }
    }
}
#[derive(Debug, Serialize)]
pub struct WldVisualSource {
    pub role: &'static str,
    pub archive_blake3: String,
    pub selected_wld: String,
    pub wld_blake3: String,
    pub unselected_wld_members: Vec<String>,
}
#[derive(Debug, Serialize)]
pub struct WldVisualMaterial {
    pub texture_frames: Vec<WldTextureFrame>,
    pub texture_animation: Option<WldTextureAnimation>,
    pub approximation: Option<&'static str>,
    pub material_index: usize,
    pub document_role: &'static str,
    pub palette_reference: String,
    pub source_name: String,
    pub texture_member: Option<String>,
    pub texture_provider: Option<&'static str>,
    pub source_texture_blake3: Option<String>,
    pub png_blake3: Option<String>,
    pub flip_v: bool,
    pub coverage: &'static str,
    pub double_sided: bool,
    pub source_rgb_pen: u32,
    pub source_brightness_bits: u32,
    pub source_scaled_ambient_bits: u32,
}
#[derive(Debug, Serialize)]
pub struct WldVisualMesh {
    pub mesh_index: usize,
    pub document_role: &'static str,
    pub fragment_slot: usize,
    pub source_name: String,
    pub source_vertices: usize,
    pub source_faces: usize,
    pub output_vertices: usize,
    pub output_faces: usize,
    pub source_lighting_words: usize,
    pub source_lighting_blake3: String,
}
#[derive(Debug, Serialize)]
pub struct WldVisualReport {
    pub mode: WldVisualMode,
    pub vertex_snapshots: Vec<WldVertexSnapshot>,
    pub animation_value_visits: usize,
    pub profile: &'static str,
    pub scope: &'static str,
    pub sources: Vec<WldVisualSource>,
    pub materials: Vec<WldVisualMaterial>,
    pub meshes: Vec<WldVisualMesh>,
    pub instances: usize,
    pub source_actor_records: usize,
    pub placements_wld_blake3: Option<String>,
    pub source_mesh_records: usize,
    pub source_faces: usize,
    pub source_meshes_without_visible_faces: usize,
    pub source_actor_lighting_references: usize,
    pub source_single_lod_actor_defs: usize,
    pub invisible_faces_excluded: usize,
    pub coincident_source_faces_excluded: usize,
    pub normalized_vertex_normals: usize,
    pub transformed_vertex_visits: usize,
    pub input_member_bytes: usize,
    pub output_payload_bytes: usize,
    pub bounds: Option<[[f64; 3]; 2]>,
    pub conversion_revision: String,
    pub limitations: Vec<&'static str>,
}
#[derive(Debug)]
pub struct ResolvedWldVisual {
    pub recommended_material_policy: static_scene::StaticMaterialPolicy,
    pub scene: StaticScene,
    pub report: WldVisualReport,
}
fn charge(total: &mut usize, n: usize, max: usize, label: &str) -> Result<()> {
    *total = total
        .checked_add(n)
        .with_context(|| format!("{label} overflow"))?;
    ensure!(*total <= max, "{label} limit exceeded");
    Ok(())
}
struct Archive {
    reader: PfsReader<File>,
    names: BTreeMap<String, String>,
    digest: String,
    role: &'static str,
}
impl Archive {
    fn open(path: &Path, role: &'static str, l: &WldVisualLimits) -> Result<Self> {
        let mut f = File::open(path).with_context(|| format!("open {role} archive"))?;
        ensure!(
            f.metadata()?.is_file() && f.metadata()?.len() <= l.max_archive_bytes as u64,
            "{role} archive byte limit exceeded or not regular"
        );
        let mut hasher = blake3::Hasher::new();
        let mut buf = [0u8; 16384];
        let mut count = 0;
        loop {
            let n = f.read(&mut buf)?;
            if n == 0 {
                break;
            }
            charge(&mut count, n, l.max_archive_bytes, "archive bytes")?;
            hasher.update(&buf[..n]);
        }
        f.seek(SeekFrom::Start(0))?;
        let mut reader = PfsReader::open(f).context("open PFS reader")?;
        ensure!(
            reader.archive_info()?.file_count as usize <= l.max_resources,
            "archive resource limit exceeded"
        );
        ensure!(
            reader.directory_info()?.uncompressed_size as usize <= l.max_member_bytes.min(4 << 20),
            "archive directory byte limit exceeded"
        );
        let names = reader.filenames()?;
        ensure!(
            names.len() <= l.max_resources,
            "archive resource limit exceeded"
        );
        let mut index = BTreeMap::new();
        for name in names {
            ensure!(name.len() <= 256, "archive member name limit exceeded");
            ensure!(
                index.insert(name.to_ascii_lowercase(), name).is_none(),
                "ambiguous case-equivalent archive members"
            );
        }
        Ok(Self {
            reader,
            names: index,
            digest: hasher.finalize().to_hex().to_string(),
            role,
        })
    }
    fn read(
        &mut self,
        requested: &str,
        budget: &mut usize,
        l: &WldVisualLimits,
    ) -> Result<Vec<u8>> {
        let exact = self
            .names
            .get(&requested.to_ascii_lowercase())
            .context("missing selected archive member")?
            .clone();
        let declared = self
            .reader
            .info(&exact)?
            .context("archive member disappeared")?
            .uncompressed_size as usize;
        ensure!(
            declared <= l.max_member_bytes,
            "archive member byte limit exceeded"
        );
        ensure!(
            budget
                .checked_add(declared)
                .is_some_and(|v| v <= l.max_input_bytes),
            "aggregate input member bytes limit exceeded"
        );
        let r = self
            .reader
            .get_reader(&exact)?
            .context("archive member disappeared")?;
        let mut bytes = Vec::new();
        r.take(l.max_member_bytes as u64 + 1)
            .read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() == declared,
            "archive member declared/actual size mismatch"
        );
        charge(
            budget,
            bytes.len(),
            l.max_input_bytes,
            "aggregate input member bytes",
        )?;
        Ok(bytes)
    }
}
struct Document {
    doc: WldDoc,
    names: BTreeMap<(u32, String), Vec<usize>>,
    snapshot_layouts: BTreeMap<usize, bool>,
}
impl Deref for Document {
    type Target = WldDoc;
    fn deref(&self) -> &Self::Target {
        &self.doc
    }
}
impl Document {
    fn get<T: Fragment + FragmentParser + 'static>(
        &self,
        reference: &FragmentRef<T>,
    ) -> Result<&T> {
        match reference {
            FragmentRef::Index(_, _) => self
                .doc
                .get(reference)
                .context("unresolved indexed WLD reference"),
            FragmentRef::Name(name, _) => {
                ensure!(name.0 < 0, "null WLD visual reference");
                let name = self
                    .doc
                    .get_string(*name)
                    .context("missing WLD reference name")?;
                let slots = self
                    .names
                    .get(&(T::TYPE_ID, name.to_ascii_lowercase()))
                    .context("unresolved named WLD reference")?;
                let mut matches = slots
                    .iter()
                    .filter_map(|i| self.doc.at(*i)?.as_any().downcast_ref::<T>());
                let target = matches
                    .next()
                    .context("WLD named reference has wrong type")?;
                ensure!(
                    matches.next().is_none(),
                    "ambiguous named WLD visual reference {name}"
                );
                Ok(target)
            }
        }
    }
}
fn parse_document(bytes: &[u8], total: &mut usize, l: &WldVisualLimits) -> Result<Document> {
    ensure!(bytes.len() >= 28, "truncated WLD header");
    let word = |i: usize| u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap());
    ensure!(
        word(0) == 0x54503d02 && matches!(word(4), 0x15500 | 0x1000c800),
        "unsupported WLD header"
    );
    let n = word(8) as usize;
    charge(total, n, l.max_fragments, "WLD fragments")?;
    let mut p = 28usize
        .checked_add(word(20) as usize)
        .context("WLD string size overflow")?;
    ensure!(p <= bytes.len(), "truncated WLD strings");
    let mut layouts = Vec::with_capacity(n);
    for _ in 0..n {
        ensure!(
            p.checked_add(8).is_some_and(|x| x <= bytes.len()),
            "truncated WLD fragment header"
        );
        let size = word(p) as usize;
        let kind = word(p + 4);
        let body_start = p + 8;
        p = p
            .checked_add(8)
            .and_then(|x| x.checked_add(size))
            .context("WLD fragment size overflow")?;
        ensure!(p <= bytes.len(), "truncated WLD fragment body");
        layouts.push(snapshot::valid_frame_layout(kind, &bytes[body_start..p]));
    }
    ensure!(
        bytes[p..].is_empty() || bytes[p..] == [255; 4],
        "unsupported trailing WLD bytes"
    );
    let doc = WldDoc::parse(bytes).map_err(|e| anyhow::anyhow!("WLD parse failed: {e:?}"))?;
    let mut names: BTreeMap<(u32, String), Vec<usize>> = BTreeMap::new();
    for (slot, f) in doc.iter().enumerate() {
        ensure!(
            f.type_id() != 0x2c,
            "legacy WLD mesh representation requires a separate profile"
        );
        if let FragmentType::Unknown(u) = f {
            ensure!(
                !matches!(
                    u.type_id,
                    0x03 | 0x04 | 0x05 | 0x14 | 0x15 | 0x2c | 0x2d | 0x30 | 0x31 | 0x36
                ),
                "unavailable WLD visual fragment body type {}",
                u.type_id
            );
        }
        if f.name_ref().0 < 0 {
            if let Some(n) = doc.get_string(*f.name_ref()).filter(|n| !n.is_empty()) {
                ensure!(n.len() <= 256, "source symbol name limit exceeded");
                names
                    .entry((f.type_id(), n.to_ascii_lowercase()))
                    .or_default()
                    .push(slot);
            }
        }
    }
    let mut snapshot_layouts = BTreeMap::new();
    for (slot, f) in doc.iter().enumerate() {
        if let Some(v) = f.as_any().downcast_ref::<libeq_wld::parser::BmInfo>() {
            snapshot_layouts.insert(v as *const _ as usize, layouts[slot]);
        }
        if let Some(v) = f
            .as_any()
            .downcast_ref::<libeq_wld::parser::SimpleSpriteDef>()
        {
            snapshot_layouts.insert(v as *const _ as usize, layouts[slot]);
        }
        if let Some(v) = f.as_any().downcast_ref::<libeq_wld::parser::DmTrack>() {
            snapshot_layouts.insert(v as *const _ as usize, layouts[slot]);
        }
        if let Some(v) = f.as_any().downcast_ref::<libeq_wld::parser::DmTrackDef2>() {
            snapshot_layouts.insert(v as *const _ as usize, layouts[slot]);
        }
    }
    Ok(Document {
        doc,
        names,
        snapshot_layouts,
    })
}

fn null<T>(r: &FragmentRef<T>) -> bool {
    r.to_bytes() == [0, 0, 0, 0]
}
type TextureCacheKey = (&'static str, String, bool, bool);
type TextureCacheValue = (usize, bool, String, String);
struct Builder<'a> {
    mode: WldVisualMode,
    policy: static_scene::StaticMaterialPolicy,
    raw_textures: BTreeMap<(&'static str, String), (Vec<u8>, String)>,
    main: Archive,
    objects: Option<Archive>,
    scene: StaticScene,
    report: WldVisualReport,
    l: &'a WldVisualLimits,
    material_cache: BTreeMap<(&'static str, Vec<u8>), (usize, bool)>,
    texture_cache: BTreeMap<TextureCacheKey, TextureCacheValue>,
    vertices: usize,
    indices: usize,
    resources: usize,
    mesh_slots: BTreeMap<(&'static str, usize), usize>,
}
impl Builder<'_> {
    fn material(
        &mut self,
        doc: &Document,
        r: &FragmentRef<MaterialDef>,
        role: &'static str,
    ) -> Result<(usize, bool)> {
        let key = (role, r.to_bytes());
        if let Some(v) = self.material_cache.get(&key) {
            return Ok(*v);
        }
        let mat = doc.get(r).context("unresolved WLD material reference")?;
        ensure!(
            mat.pair.is_none_or(|(a, b)| a == 0 && b == 0.),
            "unsupported material UV motion"
        );
        let fire = self.mode == WldVisualMode::PreviewSnapshotV1
            && matches!(
                mat.render_method,
                RenderMethod::UserDefined {
                    material_type: MaterialType::TransparentAdditiveUnlit
                }
            );
        let (alpha, keyed, coverage, textured) = match mat.render_method {
            RenderMethod::UserDefined {
                material_type: MaterialType::TransparentAdditiveUnlit,
            } if fire => (
                AlphaMode::Blend(1000),
                false,
                "snapshot-additive-unlit-alpha-approximation",
                true,
            ),
            RenderMethod::UserDefined {
                material_type:
                    MaterialType::Diffuse
                    | MaterialType::Diffuse2
                    | MaterialType::Diffuse3
                    | MaterialType::Diffuse4
                    | MaterialType::Diffuse5
                    | MaterialType::Diffuse6
                    | MaterialType::Diffuse7
                    | MaterialType::Diffuse8,
            } => (AlphaMode::Opaque, false, "opaque", true),
            RenderMethod::UserDefined {
                material_type:
                    MaterialType::TransparentMasked | MaterialType::TransparentMaskedPassable,
            } => (AlphaMode::Masked, true, "mask-0.5", true),
            RenderMethod::UserDefined {
                material_type: MaterialType::Transparent25,
            } => (AlphaMode::Blend(250), false, "blend-0.25", true),
            RenderMethod::UserDefined {
                material_type: MaterialType::Transparent50,
            } => (AlphaMode::Blend(500), false, "blend-0.5", true),
            RenderMethod::UserDefined {
                material_type: MaterialType::Transparent75,
            } => (AlphaMode::Blend(750), false, "blend-0.75", true),
            RenderMethod::Standard {
                draw_style: DrawStyle::Solid,
                texture_style: TextureStyle::None,
                unknown_bits: 0,
                ..
            } => (
                AlphaMode::Opaque,
                false,
                "opaque-untextured-modern-default",
                false,
            ),
            RenderMethod::Standard {
                draw_style: DrawStyle::Solid,
                texture_style:
                    TextureStyle::Texture1
                    | TextureStyle::Texture2
                    | TextureStyle::Texture3
                    | TextureStyle::Texture4
                    | TextureStyle::Texture5,
                unknown_bits: 0,
                ..
            } => (AlphaMode::Opaque, false, "opaque", true),
            _ => bail!("unsupported WLD material coverage {:?}", mat.render_method),
        };
        let mut texture_index = None;
        let mut flip = false;
        let mut texture_member = None;
        let mut texture_provider = None;
        let mut source_texture_blake3 = None;
        let mut png_blake3 = None;
        let mut texture_frames = Vec::new();
        let mut texture_animation = None;
        if textured {
            let sprite = doc
                .get(&mat.reference)
                .context("visible material missing sprite reference")?;
            let texture = doc
                .get(&sprite.reference)
                .context("unresolved texture definition")?;
            let (frames, animation) = self.texture_frames(doc, texture, role)?;
            let selected = &frames[0];
            let provider_role = selected.provider;
            let requested = selected.member.clone();
            let texture_key = (provider_role, requested.clone(), keyed, fire);
            let (ti, orientation, raw_hash, png_hash) =
                if let Some(v) = self.texture_cache.get(&texture_key) {
                    v.clone()
                } else {
                    let (bytes, raw_hash) = &self.raw_textures[&(provider_role, requested.clone())];
                    let orientation = !bytes.starts_with(b"DDS ");
                    let mut image = decode_static_texture(
                        requested.clone(),
                        bytes,
                        if keyed {
                            TextureDecodePolicy::PaletteIndexZero
                        } else {
                            TextureDecodePolicy::PreserveRgba
                        },
                    )?;
                    if fire {
                        image = snapshot::derive_fire(image)?;
                    }
                    charge(
                        &mut self.report.output_payload_bytes,
                        image.png_bytes.len(),
                        self.l.max_output_bytes,
                        "output payload bytes",
                    )?;
                    charge(
                        &mut self.resources,
                        1,
                        self.l.max_resources,
                        "output resources",
                    )?;
                    let png_hash = blake3::hash(&image.png_bytes).to_hex().to_string();
                    let ti = self.scene.textures.len();
                    self.scene.textures.push(image);
                    let v = (ti, orientation, raw_hash.clone(), png_hash);
                    self.texture_cache.insert(texture_key, v.clone());
                    v
                };
            texture_index = Some(ti);
            flip = orientation;
            texture_member = Some(requested);
            texture_provider = Some(provider_role);
            source_texture_blake3 = Some(raw_hash);
            png_blake3 = Some(png_hash);
            texture_frames = frames;
            texture_animation = animation;
        }
        charge(
            &mut self.resources,
            1,
            self.l.max_resources,
            "output resources",
        )?;
        let mi = self.scene.materials.len();
        let name = doc.get_string(mat.name_reference).unwrap_or("").to_string();
        self.scene
            .materials
            .push(normalize_material(&SourceMaterial {
                name: format!("{role}:{name}"),
                base_color: [1.; 4],
                texture_index,
                alpha_mode: alpha,
                double_sided: mat.flags.is_two_sided(),
                texture_sequence: None,
            })?);
        if fire {
            self.policy
                .overrides
                .insert(mi, static_scene::StaticSurfaceModel::Unlit);
        }
        self.report.materials.push(WldVisualMaterial {
            texture_frames, texture_animation,
            approximation: fire.then_some("Linear normalized RGB with alpha=max(RGB)*source alpha; alpha-over approximates addition only over black, filtering differs"),
            material_index: mi,
            document_role: role,
            palette_reference: format!("{:02x?}", r.to_bytes()),
            source_name: name,
            texture_member,
            texture_provider,
            source_texture_blake3,
            png_blake3,
            flip_v: flip,
            coverage,
            double_sided: mat.flags.is_two_sided(),
            source_rgb_pen: mat.rgb_pen,
            source_brightness_bits: mat.brightness.to_bits(),
            source_scaled_ambient_bits: mat.scaled_ambient.to_bits(),
        });
        self.material_cache.insert(key, (mi, flip));
        Ok((mi, flip))
    }
    fn mesh(
        &mut self,
        doc: &Document,
        mesh: &DmSpriteDef2,
        role: &'static str,
    ) -> Result<Option<usize>> {
        self.report.source_mesh_records += 1;
        self.report.source_faces = self
            .report
            .source_faces
            .checked_add(mesh.faces.len())
            .context("source face count overflow")?;
        ensure!(
            mesh.skin_assignment_groups.is_empty() && mesh.skin_assignment_groups_count == 0,
            "animated or skinned WLD mesh requires a separate profile"
        );
        if !null(&mesh.animation_ref) {
            self.vertex_snapshot(doc, mesh, role)?;
        }
        ensure!(mesh.scale <= 31, "unsupported WLD position scale");
        ensure!(
            mesh.center.0.is_finite() && mesh.center.1.is_finite() && mesh.center.2.is_finite(),
            "nonfinite mesh center"
        );
        let sum = mesh
            .face_material_groups
            .iter()
            .try_fold(0usize, |a, (n, _)| a.checked_add(*n as usize))
            .context("face material groups overflow")?;
        ensure!(
            sum == mesh.faces.len(),
            "face material groups must cover every face exactly"
        );
        let palette = doc
            .get(&mesh.material_list_ref)
            .context("unresolved material palette")?;
        let uvs: Vec<[f32; 2]> = match &mesh.texture_coordinates {
            TextureCoordinates::Old(v) => v
                .iter()
                .map(|(u, v)| [f32::from(*u) / 256., f32::from(*v) / 256.])
                .collect(),
            TextureCoordinates::New(v) => v.iter().map(|(u, v)| [*u, *v]).collect(),
        };
        let scale = 2f64.powi(-(mesh.scale as i32));
        let mut out = StaticMesh {
            name: doc.get_string(mesh.name_reference).unwrap_or("").into(),
            positions: vec![],
            normals: vec![],
            uvs: vec![],
            primitives: vec![],
        };
        let mut converted = BTreeMap::new();
        let mut offset = 0;
        for &(count, ordinal) in &mesh.face_material_groups {
            let reference = palette
                .fragments
                .get(ordinal as usize)
                .context("material palette ordinal out of range")?;
            let material = doc
                .get(reference)
                .context("unresolved WLD material reference")?;
            let faces = &mesh.faces[offset..offset + count as usize];
            offset += count as usize;
            for face in faces {
                ensure!(
                    [
                        face.vertex_indexes.0,
                        face.vertex_indexes.1,
                        face.vertex_indexes.2
                    ]
                    .iter()
                    .all(|i| (*i as usize) < mesh.positions.len()),
                    "WLD face index out of bounds"
                );
            }
            if crate::convert::is_invisible_render_method(&material.render_method) {
                self.report.invisible_faces_excluded += faces.len();
                continue;
            }
            let (material_index, flip) = self.material(doc, reference, role)?;
            let mut indices = vec![];
            for face in faces {
                let source = [
                    face.vertex_indexes.0,
                    face.vertex_indexes.1,
                    face.vertex_indexes.2,
                ];
                let p = source.map(|i| mesh.positions[i as usize]);
                if p[0] == p[1] || p[0] == p[2] || p[1] == p[2] {
                    self.report.coincident_source_faces_excluded += 1;
                    continue;
                }
                let d =
                    |v: (i16, i16, i16)| DVec3::new(f64::from(v.0), f64::from(v.1), f64::from(v.2));
                ensure!(
                    (d(p[1]) - d(p[0]))
                        .cross(d(p[2]) - d(p[0]))
                        .length_squared()
                        > 0.,
                    "other zero-area WLD face is unsupported"
                );
                let rounded = p.map(|v| {
                    DVec3::new(
                        f64::from((f64::from(v.1) * scale + f64::from(mesh.center.1)) as f32),
                        f64::from((f64::from(v.0) * scale + f64::from(mesh.center.0)) as f32),
                        f64::from((f64::from(v.2) * scale + f64::from(mesh.center.2)) as f32),
                    )
                });
                ensure!(
                    (rounded[1] - rounded[0])
                        .cross(rounded[2] - rounded[0])
                        .length_squared()
                        > 0.,
                    "WLD face collapses during conversion to float positions"
                );
                for i in source {
                    let key = (i, flip);
                    let index = if let Some(index) = converted.get(&key) {
                        *index
                    } else {
                        let at = i as usize;
                        let p = mesh.positions[at];
                        let normal = mesh
                            .vertex_normals
                            .get(at)
                            .context("missing WLD vertex normal")?;
                        let normal = DVec3::new(
                            f64::from(normal.1),
                            f64::from(normal.0),
                            f64::from(normal.2),
                        );
                        ensure!(normal.length_squared() > 0., "zero WLD vertex normal");
                        let n = normal.normalize().as_vec3().to_array();
                        let uv = *uvs.get(at).context("missing WLD primary UV")?;
                        ensure!(
                            uv.into_iter().all(f32::is_finite),
                            "nonfinite WLD primary UV"
                        );
                        let position = [
                            (f64::from(p.1) * scale + f64::from(mesh.center.1)) as f32,
                            (f64::from(p.0) * scale + f64::from(mesh.center.0)) as f32,
                            (f64::from(p.2) * scale + f64::from(mesh.center.2)) as f32,
                        ];
                        ensure!(
                            position.into_iter().all(f32::is_finite),
                            "WLD position overflow"
                        );
                        charge(
                            &mut self.vertices,
                            1,
                            self.l.max_vertices,
                            "output vertices",
                        )?;
                        charge(
                            &mut self.report.output_payload_bytes,
                            32,
                            self.l.max_output_bytes,
                            "output payload bytes",
                        )?;
                        let index = out.positions.len() as u32;
                        out.positions.push(position);
                        out.normals.push(n);
                        out.uvs.push([uv[0], if flip { 1. - uv[1] } else { uv[1] }]);
                        converted.insert(key, index);
                        self.report.normalized_vertex_normals += 1;
                        index
                    };
                    indices.push(index);
                }
            }
            if !indices.is_empty() {
                charge(
                    &mut self.indices,
                    indices.len(),
                    self.l.max_indices,
                    "output indices",
                )?;
                charge(
                    &mut self.report.output_payload_bytes,
                    indices
                        .len()
                        .checked_mul(4)
                        .context("indices bytes overflow")?,
                    self.l.max_output_bytes,
                    "output payload bytes",
                )?;
                out.primitives.push(StaticPrimitive {
                    indices,
                    material_index,
                    colors: None,
                });
            }
        }
        if out.primitives.is_empty() {
            self.report.source_meshes_without_visible_faces += 1;
            return Ok(None);
        }
        charge(
            &mut self.resources,
            1,
            self.l.max_resources,
            "output resources",
        )?;
        let mi = self.scene.meshes.len();
        let mut light_hash = blake3::Hasher::new();
        for word in &mesh.vertex_colors {
            light_hash.update(&word.to_le_bytes());
        }
        self.report.meshes.push(WldVisualMesh {
            mesh_index: mi,
            document_role: role,
            fragment_slot: *self
                .mesh_slots
                .get(&(role, mesh as *const DmSpriteDef2 as usize))
                .context("source mesh slot not indexed")?,
            source_name: out.name.clone(),
            source_vertices: mesh.positions.len(),
            source_faces: mesh.faces.len(),
            output_vertices: out.positions.len(),
            output_faces: out.primitives.iter().map(|p| p.indices.len() / 3).sum(),
            source_lighting_words: mesh.vertex_colors.len(),
            source_lighting_blake3: light_hash.finalize().to_hex().to_string(),
        });
        self.scene.meshes.push(out);
        Ok(Some(mi))
    }
    fn instance(&mut self, mesh_index: usize, matrix: [[f32; 4]; 4]) -> Result<()> {
        ensure!(
            self.scene.instances.len() < self.l.max_instances,
            "instance limit exceeded"
        );
        static_scene::validate_matrix(matrix)?;
        charge(
            &mut self.report.transformed_vertex_visits,
            self.scene.meshes[mesh_index].positions.len(),
            self.l
                .max_transform_visits
                .saturating_sub(self.report.animation_value_visits),
            "transformed vertex visits",
        )?;
        charge(
            &mut self.report.output_payload_bytes,
            64,
            self.l.max_output_bytes,
            "output payload bytes",
        )?;
        self.scene
            .instances
            .push(StaticInstance { mesh_index, matrix });
        Ok(())
    }
}
fn actor_matrix(actor: &Actor) -> Result<[[f32; 4]; 4]> {
    let p = actor.location.as_ref().context("missing Actor location")?;
    let s = actor.scale_factor.context("missing Actor scale")?;
    ensure!(
        s.is_finite() && s > 0.,
        "Actor scale must be finite and positive"
    );
    ensure!(
        [p.x, p.y, p.z, p.rotate_x, p.rotate_y, p.rotate_z]
            .into_iter()
            .all(f32::is_finite),
        "nonfinite Actor transform"
    );
    // Reuse the legacy analytic three-axis transform, then conjugate its (x,z,y)
    // basis into server (y,x,z). Source angles are turns in 512 units.
    let old = crate::zone::wld_placement_matrix(
        (p.x, p.z, p.y),
        (
            p.rotate_y / 512. * -360.,
            p.rotate_z / 512. * -360.,
            p.rotate_x / 512. * -360.,
        ),
        s,
    )?;
    let permutation = [2, 0, 1, 3];
    let mut out = [[0.; 4]; 4];
    for c in 0..4 {
        for r in 0..4 {
            out[c][r] = old[permutation[c]][permutation[r]];
        }
    }
    Ok(out)
}
/// Import the main archive's basename WLD plus optional `objects.wld` placements.
/// Object ActorDefs are resolved in the explicitly supplied object archive's basename WLD.
/// This is offline static inspection, not a complete native-world or gameplay certificate.
pub fn load_wld_visual(
    main_archive: &Path,
    object_archive: Option<&Path>,
    limits: &WldVisualLimits,
) -> Result<ResolvedWldVisual> {
    load_wld_visual_with_mode(
        main_archive,
        object_archive,
        limits,
        WldVisualMode::StrictStatic,
    )
}
/// Select strict import or the explicit offline preview snapshot approximation.
pub fn load_wld_visual_with_mode(
    main_archive: &Path,
    object_archive: Option<&Path>,
    limits: &WldVisualLimits,
    mode: WldVisualMode,
) -> Result<ResolvedWldVisual> {
    catch_unwind(AssertUnwindSafe(|| {
        load_inner(main_archive, object_archive, limits, mode)
    }))
    .map_err(|_| anyhow::anyhow!("WLD visual source parser panicked"))?
}
fn load_inner(
    main_archive: &Path,
    object_archive: Option<&Path>,
    l: &WldVisualLimits,
    mode: WldVisualMode,
) -> Result<ResolvedWldVisual> {
    let hard = WldVisualLimits::default();
    for (actual, maximum) in [
        (l.max_archive_bytes, hard.max_archive_bytes),
        (l.max_member_bytes, hard.max_member_bytes),
        (l.max_input_bytes, hard.max_input_bytes),
        (l.max_fragments, hard.max_fragments),
        (l.max_vertices, hard.max_vertices),
        (l.max_indices, hard.max_indices),
        (l.max_instances, hard.max_instances),
        (l.max_resources, hard.max_resources),
        (l.max_transform_visits, hard.max_transform_visits),
        (l.max_output_bytes, hard.max_output_bytes),
    ] {
        ensure!(actual <= maximum, "limits exceed supported maximum");
    }

    let mut main = Archive::open(main_archive, "main", l)?;
    let mut objects = object_archive
        .map(|p| Archive::open(p, "objects", l))
        .transpose()?;
    let member = |p: &Path| -> Result<String> {
        Ok(format!(
            "{}.wld",
            p.file_stem()
                .and_then(|v| v.to_str())
                .context("archive basename is not UTF-8")?
        ))
    };
    let terrain_name = member(main_archive)?;
    let mut bytes = 0;
    let mut fragments = 0;
    let terrain_bytes = main.read(&terrain_name, &mut bytes, l)?;
    let terrain = parse_document(&terrain_bytes, &mut fragments, l)?;
    let object_name = object_archive.map(member).transpose()?;
    let object_bytes = match (&mut objects, &object_name) {
        (Some(a), Some(n)) => Some(a.read(n, &mut bytes, l)?),
        _ => None,
    };
    let object_doc = object_bytes
        .as_ref()
        .map(|b| parse_document(b, &mut fragments, l))
        .transpose()?;
    let placement_bytes = if main.names.contains_key("objects.wld") {
        Some(main.read("objects.wld", &mut bytes, l)?)
    } else {
        None
    };
    let placement_doc = placement_bytes
        .as_ref()
        .map(|b| parse_document(b, &mut fragments, l))
        .transpose()?;
    let source = |a: &Archive, n: String, b: &[u8]| WldVisualSource {
        role: a.role,
        archive_blake3: a.digest.clone(),
        selected_wld: n.clone(),
        wld_blake3: blake3::hash(b).to_hex().to_string(),
        unselected_wld_members: a
            .names
            .keys()
            .filter(|v| {
                v.ends_with(".wld")
                    && **v != n.to_ascii_lowercase()
                    && !(a.role == "main" && **v == "objects.wld")
            })
            .cloned()
            .collect(),
    };
    let mut sources = vec![source(&main, terrain_name, &terrain_bytes)];
    if let (Some(a), Some(n), Some(b)) = (&objects, &object_name, &object_bytes) {
        sources.push(source(a, n.clone(), b));
    }
    let mut mesh_slots = BTreeMap::new();
    for (role, doc) in
        std::iter::once(("main", &terrain)).chain(object_doc.as_ref().map(|d| ("objects", d)))
    {
        for (slot, f) in doc.iter().enumerate() {
            if let Some(m) = f.as_any().downcast_ref::<DmSpriteDef2>() {
                mesh_slots.insert((role, m as *const DmSpriteDef2 as usize), slot);
            }
        }
    }
    let mut b = Builder {
        mode,
        policy: static_scene::StaticMaterialPolicy::default(),
        raw_textures: BTreeMap::new(),
        main,
        objects,
        scene: StaticScene::default(),
        report: WldVisualReport {
            mode,
            vertex_snapshots: vec![],
            animation_value_visits: 0,
            profile: "wld-static-pbr-v1",
            scope: "selected-main-modern-meshes-and-static-object-actor-definitions",
            sources,
            materials: vec![],
            meshes: vec![],
            instances: 0,
            source_actor_records: 0,
            placements_wld_blake3: placement_bytes.as_ref().map(|v| blake3::hash(v).to_hex().to_string()),
            source_mesh_records: 0,
            source_faces: 0,
            source_meshes_without_visible_faces: 0,
            source_actor_lighting_references: 0,
            source_single_lod_actor_defs: 0,
            invisible_faces_excluded: 0,
            coincident_source_faces_excluded: 0,
            normalized_vertex_normals: 0,
            transformed_vertex_visits: 0,
            input_member_bytes: bytes,
            output_payload_bytes: 0,
            bounds: None,
            conversion_revision: String::new(),
            limitations: vec![
                "Offline inspection only; no publication, collision, regions or gameplay activation",
                "Original archive/WLD hashes retain source identity and lighting; prelighting is not applied",
                "Editable rough nonmetal PBR defaults with explicit material overrides; no physical-property recovery",
                "Static named ActorDefs only; animated, additive and layered materials refuse",
                "A sole static sprite ignores its source distance-culling threshold; alternative LODs refuse",
                "Source BLEND intent is retained; writer admission separately checks rendering support",
                "Main and object basename WLDs are explicitly selected; WorldTree/native visibility membership is not certified",
            ],
        },
        l,
        material_cache: BTreeMap::new(),
        texture_cache: BTreeMap::new(),
        vertices: 0,
        indices: 0,
        resources: 0,
        mesh_slots,
    };
    if mode == WldVisualMode::PreviewSnapshotV1 {
        b.report.profile = "wld-preview-snapshot-v1";
        b.report.limitations[3] = "Static named ActorDefs only; explicit texture/rest-vertex snapshots and additive-unlit fire approximation; layered materials refuse";
        b.report.limitations.push("Texture animations select first declared frame, not native time zero; vertex tracks retain source mesh rest positions and normals");
        b.report.limitations.push("Additive-unlit fire uses derived RGBA16 unlit BLEND; nonblack backgrounds attenuate and filtering differs from additive rendering");
    }
    for m in terrain.fragment_iter::<DmSpriteDef2>() {
        if let Some(mi) = b.mesh(&terrain, m, "main")? {
            b.instance(mi, Mat4::IDENTITY.to_cols_array_2d())?;
        }
    }
    ensure!(
        !b.scene.instances.is_empty(),
        "selected main WLD contains no supported visible geometry"
    );
    let mut definitions: BTreeMap<String, Vec<&ActorDef>> = BTreeMap::new();
    if let Some(d) = &object_doc {
        for f in d.fragment_iter::<ActorDef>() {
            if let Some(n) = d
                .get_string(f.name_reference)
                .filter(|n| f.name_reference.0 < 0 && !n.is_empty())
            {
                definitions
                    .entry(n.to_ascii_lowercase())
                    .or_default()
                    .push(f);
            }
        }
    }
    let mut models: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    let mut object_meshes: BTreeMap<u32, Option<usize>> = BTreeMap::new();
    if let Some(placements) = &placement_doc {
        for actor in placements.fragment_iter::<Actor>() {
            b.report.source_actor_records += 1;
            ensure!(
                b.report.source_actor_records <= l.max_instances,
                "Actor record limit exceeded"
            );
            ensure!(
                actor.actor_def_reference.0 < 0,
                "unsupported Actor fragment-reference form"
            );
            let name = placements
                .get_string(actor.actor_def_reference)
                .context("unresolved Actor model name")?
                .to_ascii_lowercase();
            let matrix = actor_matrix(actor)?;
            if actor.vertex_color_reference.is_some() {
                b.report.source_actor_lighting_references += 1;
            }
            if !models.contains_key(&name) {
                let doc = object_doc
                    .as_ref()
                    .context("placed object requires an object archive")?;
                let candidates = definitions.get(&name).context("missing placed ActorDef")?;
                ensure!(candidates.len() == 1, "ambiguous placed ActorDef name");
                let def = candidates[0];
                ensure!(
                    def.location.is_none() && def.current_action.is_none_or(|n| n == 0),
                    "unsupported transformed or animated ActorDef"
                );
                ensure!(
                    def.fragment_references.len() == 1,
                    "ActorDef requires exactly one single static sprite"
                );
                ensure!(
                    def.actions.is_empty()
                        || (def.actions.len() == 1
                            && def.actions[0].levels_of_detail_count == 1
                            && def.actions[0].levels_of_detail_distances.len() == 1
                            && def.actions[0].levels_of_detail_distances[0].is_finite()
                            && def.actions[0].levels_of_detail_distances[0] >= 0.
                            && def.actions[0].unknown == 0),
                    "unsupported ActorDef action or alternate LOD"
                );
                if !def.actions.is_empty() {
                    b.report.source_single_lod_actor_defs += 1;
                }
                let mut list = vec![];
                for &reference in &def.fragment_references {
                    ensure!(
                        reference > 0 && reference <= i32::MAX as u32,
                        "invalid ActorDef sprite index"
                    );
                    let sprite = doc
                        .get(&FragmentRef::<DmSprite>::new(reference as i32))
                        .context("placed ActorDef is not a static DmSprite")?;
                    ensure!(sprite.params == 0, "unsupported DmSprite parameters");
                    let mesh = doc
                        .get(&sprite.reference)
                        .context("missing placed DmSprite mesh")?;
                    let wire = u32::from_le_bytes(sprite.reference.to_bytes().try_into().unwrap());
                    let mi = if let Some(v) = object_meshes.get(&wire) {
                        *v
                    } else {
                        let mi = b.mesh(doc, mesh, "objects")?;
                        object_meshes.insert(wire, mi);
                        mi
                    };
                    if let Some(mi) = mi {
                        list.push(mi)
                    }
                }
                ensure!(
                    !list.is_empty(),
                    "placed ActorDef has no supported visible geometry"
                );
                models.insert(name.clone(), list);
            }
            for &mi in models.get(&name).unwrap() {
                b.instance(mi, matrix)?;
            }
        }
    }
    ensure!(
        !b.scene.meshes.is_empty() && !b.scene.instances.is_empty(),
        "no supported visible static geometry"
    );
    let mut min = DVec3::splat(f64::INFINITY);
    let mut max = DVec3::splat(f64::NEG_INFINITY);
    for i in &b.scene.instances {
        let matrix = glam::DMat4::from_cols_array_2d(&i.matrix.map(|c| c.map(f64::from)));
        for p in &b.scene.meshes[i.mesh_index].positions {
            let p = matrix.transform_point3(DVec3::from_array(p.map(f64::from)));
            ensure!(p.is_finite(), "transformed WLD position overflow");
            min = min.min(p);
            max = max.max(p);
        }
    }
    b.report.bounds = Some([min.to_array(), max.to_array()]);
    b.report.instances = b.scene.instances.len();
    let mut revision = blake3::Hasher::new();
    revision.update(b"wld-source-conversion-v2");
    revision.update(&serde_json::to_vec(&mode)?);
    revision.update(&serde_json::to_vec(&b.report.materials)?);
    revision.update(&serde_json::to_vec(&b.report.vertex_snapshots)?);
    for policy in [
        include_bytes!("wld_visual.rs").as_slice(),
        include_bytes!("wld_visual/snapshot.rs").as_slice(),
        include_bytes!("zone.rs").as_slice(),
        include_bytes!("source_material.rs").as_slice(),
        include_bytes!("convert/mod.rs").as_slice(),
        include_bytes!("convert/texture.rs").as_slice(),
        include_bytes!("convert/dds.rs").as_slice(),
        include_bytes!("static_scene.rs").as_slice(),
        include_bytes!("../Cargo.lock").as_slice(),
    ] {
        revision.update(blake3::hash(policy).as_bytes());
    }
    revision.update(&serde_json::to_vec(&b.report.sources)?);
    for s in &b.report.sources {
        revision.update(s.wld_blake3.as_bytes());
    }
    if let Some(p) = placement_bytes.as_ref() {
        revision.update(blake3::hash(p).as_bytes());
    }
    for m in &b.report.materials {
        if let Some(h) = &m.source_texture_blake3 {
            revision.update(h.as_bytes());
        }
    }
    b.report.conversion_revision = revision.finalize().to_hex().to_string();
    Ok(ResolvedWldVisual {
        recommended_material_policy: b.policy,
        scene: b.scene,
        report: b.report,
    })
}
#[cfg(test)]
mod tests;
