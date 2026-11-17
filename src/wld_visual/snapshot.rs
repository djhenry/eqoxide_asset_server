//! Explicit bounded offline snapshot choices; source samples and rest geometry remain intact.
use super::*;
use libeq_wld::parser::SimpleSpriteDef;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum WldVisualMode {
    StrictStatic,
    PreviewSnapshotV1,
}
#[derive(Debug, Serialize)]
pub struct WldTextureFrame {
    pub declared_index: usize,
    pub reference: String,
    pub member: String,
    pub provider: &'static str,
    pub source_blake3: String,
}
#[derive(Debug, Serialize)]
pub struct WldTextureAnimation {
    pub flags: u32,
    pub frame_count: u32,
    pub sleep_word: Option<u32>,
    pub selected_declared_index: usize,
    pub choice: &'static str,
}
#[derive(Debug, Serialize)]
pub struct WldVertexSnapshot {
    pub document_role: &'static str,
    pub mesh_fragment_slot: usize,
    pub track_reference: String,
    pub definition_reference: String,
    pub track_flags: u32,
    pub definition_flags: u32,
    pub frame_count: u16,
    pub vertex_count: u16,
    pub scale: u16,
    pub timing_word: u16,
    pub control_word: u16,
    pub track_values_blake3: String,
    pub choice: &'static str,
}
impl Builder<'_> {
    pub(super) fn texture_frames(
        &mut self,
        doc: &Document,
        texture: &SimpleSpriteDef,
        role: &'static str,
    ) -> Result<(Vec<WldTextureFrame>, Option<WldTextureAnimation>)> {
        if self.mode == WldVisualMode::StrictStatic {
            ensure!(
                !texture.flags.is_animated()
                    && texture.frame_references.len() == 1
                    && texture.frame_count == 1,
                "texture animation requires a separate profile"
            );
        } else {
            ensure!(
                doc.snapshot_layouts.get(&(texture as *const _ as usize)) == Some(&true),
                "incoherent snapshot texture frame body layout"
            );
            let flags = u32::from(texture.flags);
            ensure!(
                flags & 0x20 == 0,
                "snapshot current-frame metadata unavailable in pinned parser"
            );
            ensure!(flags & !0x1a == 0, "unsupported snapshot texture flags");
            ensure!(
                texture.frame_count > 0
                    && texture.frame_count as usize == texture.frame_references.len(),
                "incoherent texture frame count"
            );
            ensure!(
                texture.frame_count == 1 || texture.flags.is_animated(),
                "multiple frames without animation flag"
            );
            ensure!(
                texture.sleep.is_some()
                    == (texture.flags.is_animated() && texture.flags.has_sleep()),
                "incoherent texture sleep word"
            );
        }
        let mut frames = Vec::new();
        for (declared_index, reference) in texture.frame_references.iter().enumerate() {
            if self.mode == WldVisualMode::PreviewSnapshotV1 {
                charge(
                    &mut self.resources,
                    1,
                    self.l.max_resources,
                    "texture frame metadata resources",
                )?;
            }
            let bitmap = doc.get(reference).context("unresolved bitmap reference")?;
            ensure!(
                bitmap.entries.len() == 1,
                "layered bitmap requires a separate profile"
            );
            if self.mode == WldVisualMode::PreviewSnapshotV1 {
                ensure!(
                    doc.snapshot_layouts.get(&(bitmap as *const _ as usize)) == Some(&true),
                    "incoherent snapshot bitmap frame body layout"
                );
            }
            let name = &bitmap.entries[0].file_name;
            ensure!(name.len() <= 256, "texture member name limit exceeded");
            let requested = name.to_ascii_lowercase();
            ensure!(!requested.is_empty(), "empty bitmap name");
            let prefer_object = role == "objects"
                && self
                    .objects
                    .as_ref()
                    .is_some_and(|a| a.names.contains_key(&requested));
            let provider = if prefer_object {
                self.objects.as_mut().unwrap()
            } else if self.main.names.contains_key(&requested) {
                &mut self.main
            } else {
                self.objects
                    .as_mut()
                    .filter(|a| a.names.contains_key(&requested))
                    .context("missing required texture")?
            };
            let key = (provider.role, requested.clone());
            if !self.raw_textures.contains_key(&key) {
                if self.mode == WldVisualMode::PreviewSnapshotV1 {
                    charge(
                        &mut self.resources,
                        1,
                        self.l.max_resources,
                        "source texture resources",
                    )?;
                }
                let bytes =
                    provider.read(&requested, &mut self.report.input_member_bytes, self.l)?;
                let hash = blake3::hash(&bytes).to_hex().to_string();
                self.raw_textures.insert(key.clone(), (bytes, hash));
            }
            frames.push(WldTextureFrame {
                declared_index,
                reference: format!("{:02x?}", reference.to_bytes()),
                member: requested,
                provider: key.0,
                source_blake3: self.raw_textures[&key].1.clone(),
            });
        }
        let animation =
            (self.mode == WldVisualMode::PreviewSnapshotV1).then_some(WldTextureAnimation {
                flags: u32::from(texture.flags),
                frame_count: texture.frame_count,
                sleep_word: texture.sleep,
                selected_declared_index: 0,
                choice: "first-declared-frame-not-native-time-zero",
            });
        Ok((frames, animation))
    }
    pub(super) fn vertex_snapshot(
        &mut self,
        doc: &Document,
        mesh: &DmSpriteDef2,
        role: &'static str,
    ) -> Result<()> {
        ensure!(
            self.mode == WldVisualMode::PreviewSnapshotV1,
            "animated WLD mesh requires a separate profile"
        );
        let track = doc
            .get(&mesh.animation_ref)
            .context("unresolved snapshot DmTrack")?;
        let def = doc
            .get(&track.reference)
            .context("unresolved snapshot DmTrackDef2")?;
        ensure!(
            doc.snapshot_layouts.get(&(track as *const _ as usize)) == Some(&true)
                && doc.snapshot_layouts.get(&(def as *const _ as usize)) == Some(&true),
            "incoherent snapshot vertex frame body layout"
        );
        ensure!(
            track.flags == 0 && def.flags == 0 && def.param2 == 0 && def.scale <= 31,
            "unsupported vertex track flags, scale or control"
        );
        ensure!(
            def.frame_count > 0
                && def.vertex_count > 0
                && usize::from(def.vertex_count) == mesh.positions.len()
                && usize::from(def.frame_count) == def.frames.len(),
            "incoherent vertex track counts"
        );
        charge(
            &mut self.resources,
            usize::from(def.frame_count) + 1,
            self.l.max_resources,
            "vertex track metadata resources",
        )?;
        let visits = usize::from(def.frame_count)
            .checked_mul(usize::from(def.vertex_count))
            .context("vertex track visits overflow")?;
        charge(
            &mut self.report.animation_value_visits,
            visits,
            self.l
                .max_transform_visits
                .saturating_sub(self.report.transformed_vertex_visits),
            "animation value visits",
        )?;
        let mut hash = blake3::Hasher::new();
        for frame in &def.frames {
            ensure!(
                frame.len() == usize::from(def.vertex_count),
                "incoherent vertex track frame length"
            );
            for &(x, y, z) in frame {
                for value in [x, y, z] {
                    hash.update(&value.to_le_bytes());
                }
            }
        }
        self.report.vertex_snapshots.push(WldVertexSnapshot {
            document_role: role,
            mesh_fragment_slot: self.mesh_slots[&(role, mesh as *const DmSpriteDef2 as usize)],
            track_reference: format!("{:02x?}", mesh.animation_ref.to_bytes()),
            definition_reference: format!("{:02x?}", track.reference.to_bytes()),
            track_flags: track.flags,
            definition_flags: def.flags,
            frame_count: def.frame_count,
            vertex_count: def.vertex_count,
            scale: def.scale,
            timing_word: def.param1,
            control_word: def.param2,
            track_values_blake3: hash.finalize().to_hex().to_string(),
            choice: "source-mesh-rest-positions-and-normals-not-track-frame-zero",
        });
        Ok(())
    }
}
/// Derived straight-alpha texels approximate additive-unlit color over black only.
pub(super) fn derive_fire(
    mut texture: static_scene::StaticTexture,
) -> Result<static_scene::StaticTexture> {
    let image = crate::convert::decode_texture_rgba(
        &texture.png_bytes,
        TextureDecodePolicy::PreserveRgba,
        &texture.name,
        crate::convert::TextureDecodeProfile::Static,
    )?;
    ensure!(
        u64::from(image.width()) * u64::from(image.height()) * 8
            <= static_scene::MAX_TEXTURE_BYTES as u64,
        "derived fire decoded allocation limit exceeded"
    );
    let mut rgba = image.into_rgba16();
    let linear = |v: u16| {
        let c = f64::from(v) / 65535.;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let srgb = |c: f64| {
        if c <= 0.0031308 {
            c * 12.92
        } else {
            1.055 * c.powf(1. / 2.4) - 0.055
        }
    };
    for p in rgba.pixels_mut() {
        let c = [linear(p[0]), linear(p[1]), linear(p[2])];
        let m = c.into_iter().fold(0f64, f64::max);
        let a = f64::from(p[3]) / 65535.;
        for k in 0..3 {
            p[k] = if m > 0. {
                (srgb(c[k] / m) * 65535.).round().clamp(0., 65535.) as u16
            } else {
                0
            };
        }
        p[3] = (a * m * 65535.).round().clamp(0., 65535.) as u16;
    }
    texture.name.push_str(":snapshot-fire-rgba16");
    texture.png_bytes = crate::convert::encode_texture_rgba_png(
        &image::DynamicImage::ImageRgba16(rgba),
        &texture.name,
    )?;
    ensure!(
        texture.png_bytes.len() <= static_scene::MAX_TEXTURE_BYTES,
        "derived fire encoded byte limit exceeded"
    );
    Ok(texture)
}

// Check the source body before parser-discarded bytes can masquerade as padding.
// Applied only to referenced snapshot records; the strict API retains its admission.
pub(super) fn valid_frame_layout(kind: u32, body: &[u8]) -> bool {
    let u16_at = |at| u16::from_le_bytes([body[at], body[at + 1]]) as usize;
    let u32_at = |at| u32::from_le_bytes(body[at..at + 4].try_into().unwrap()) as usize;
    let expected = match kind {
        3 => return valid_bitmap_layout(body),
        4 if body.len() >= 12 => {
            let flags = u32_at(4);
            12usize
                .checked_add(if flags & 0x20 != 0 { 4 } else { 0 })
                .and_then(|n| n.checked_add(if flags & 0x18 == 0x18 { 4 } else { 0 }))
                .and_then(|n| u32_at(8).checked_mul(4).and_then(|c| n.checked_add(c)))
        }
        0x2f => Some(12),
        0x37 if body.len() >= 18 => u16_at(8)
            .checked_mul(u16_at(10))
            .and_then(|n| n.checked_mul(6))
            .and_then(|n| n.checked_add(18)),
        4 | 0x37 => None,
        _ => return true,
    };
    expected.is_some_and(|n| {
        body.len() >= n && body.len() - n <= (4 - n % 4) % 4 && body[n..].iter().all(|v| *v == 0)
    })
}

fn valid_bitmap_layout(body: &[u8]) -> bool {
    let payload_end = || -> Option<usize> {
        let count = u32::from_le_bytes(body.get(4..8)?.try_into().ok()?).checked_add(1)?;
        // Each declared entry consumes at least its two-byte encoded-length prefix.
        // Bound iteration by actual bytes before trusting a declared count.
        if count as usize > body.len().checked_sub(8)? / 2 {
            return None;
        }
        let mut at = 8usize;
        for _ in 0..count {
            let prefix_end = at.checked_add(2)?;
            let length = u16::from_le_bytes(body.get(at..prefix_end)?.try_into().ok()?);
            at = prefix_end.checked_add(usize::from(length))?;
            if at > body.len() {
                return None;
            }
        }
        Some(at)
    };
    // Bitmap writers use differing final alignment conventions. Preserve up to
    // three zero padding bytes, while rejecting undeclared filename payloads.
    payload_end().is_some_and(|at| body.len() - at <= 3 && body[at..].iter().all(|v| *v == 0))
}
