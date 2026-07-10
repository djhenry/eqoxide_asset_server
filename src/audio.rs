//! Zone audio extraction: EQ `_sndbnk.eff` / `_sounds.eff` parsing and the wav/mp3
//! source lookup that feeds the `sound/<zone>` and `music/<zone>` asset sets.
//!
//! Three EQ source pieces make up a zone's audio (issue #32):
//!   * `<zone>_sndbnk.eff` — a TEXT sound bank: an `EMIT` list then a `LOOP` list of
//!     sound-effect base names (CRLF-separated). Emitter records index into these.
//!   * `<zone>_sounds.eff` — a BINARY list of fixed 84-byte emitter placement records
//!     (position, radius, day/night sound ids + type). See [`parse_sounds_eff`].
//!   * the referenced `.wav` files, which live either loose in `sounds/` or inside the
//!     `snd*.pfs` archives (2128 files across 17 archives), plus per-zone `.mp3` music.
//!
//! This module only PARSES + RESOLVES; `build.rs` turns the result into CAS sets.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

/// A parsed `_sndbnk.eff` sound bank: two ordered name lists the emitter records
/// index into. Names are base names (no extension); the wav is `<name>.wav`.
#[derive(Debug, Default, Clone, PartialEq, Serialize)]
pub struct SoundBank {
    /// One-shot ("EMIT") sound-effect names, in file order.
    pub emit: Vec<String>,
    /// Looping ("LOOP") sound-effect names, in file order.
    pub loops: Vec<String>,
}

/// Parse a `_sndbnk.eff` text sound bank.
///
/// Layout is line-oriented (CRLF or LF), with two section headers:
/// ```text
/// EMIT
/// torch3d
/// torch3d
/// LOOP
/// wind_lp4
/// ```
/// Everything after `EMIT` (until `LOOP`) is the emit list; everything after `LOOP`
/// is the loop list. A leading `EMIT` header is expected but tolerated if absent
/// (older banks). Blank lines are skipped.
pub fn parse_sndbnk(bytes: &[u8]) -> SoundBank {
    let text = String::from_utf8_lossy(bytes);
    let mut bank = SoundBank::default();
    // Before any header we default to the emit list (some banks omit the leading EMIT).
    let mut in_loops = false;
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        match line.to_ascii_uppercase().as_str() {
            "EMIT" => in_loops = false,
            "LOOP" => in_loops = true,
            _ => {
                if in_loops {
                    bank.loops.push(line.to_string());
                } else {
                    bank.emit.push(line.to_string());
                }
            }
        }
    }
    bank
}

/// A single emitter placement from `_sounds.eff` (84-byte record), decoded per the
/// RoF2 client's own parser (`eqgame.exe FUN_004aac30`). Fields are raw values;
/// sound-name resolution against a [`SoundBank`] happens in [`resolve_emitters`].
///
/// Byte map (little-endian; offsets in the 84-byte record):
///   0x00–0x0F opaque tool metadata (client ignores; 0x0C is an ascending id)
///   0x10/0x14/0x18 f32 X/Y/Z — Z is negated at runtime by the client
///   0x1C f32 radius (< 0 ⇒ engine default)
///   0x2C 4-byte unread gap
///   0x30/0x34 i32 SIGNED sound-id / music-selector (day/night) — see `type_`
///   0x38/0x39 u8 type (day/night); only the DAY byte drives resolution
///   0x44/0x48 i32 repeat-delay MAX ms (day/night; TYPE 1, clamped 20000)
///   0x4C/0x50 i32 repeat-delay MIN ms (day/night; TYPE 1)
#[derive(Debug, Clone, PartialEq)]
pub struct RawEmitter {
    /// Ascending record id (record 0x0C) — informational; the client ignores it.
    pub id: i32,
    /// Position, EQ-native file coordinates (records 0x10/0x14/0x18). The client
    /// negates Z at load; consumers should apply their usual EQ coord transform.
    pub x: f32,
    pub y: f32,
    pub z: f32,
    /// Trigger / full-volume radius (record 0x1C); `< 0` means "engine default".
    pub radius: f32,
    /// Type discriminator (record 0x38, day byte): 0/2/3 index the zone sndbnk,
    /// 1 selects the global SFX table (id<0) or zone `.xmi` music (id>=0).
    pub type_: u8,
    /// Signed sound-id / music-selector, day and night (records 0x30/0x34).
    pub sound_day: i32,
    pub sound_night: i32,
    /// Looping repeat-delay bounds, ms (records 0x4C min, 0x44 max; TYPE 1 only).
    pub repeat_min_ms: i32,
    pub repeat_max_ms: i32,
}

/// Size of one `_sounds.eff` emitter record.
pub const SOUNDS_EFF_RECORD: usize = 84;

/// Parse a `_sounds.eff` binary into raw emitter records. Trailing bytes that don't
/// fill a whole record are ignored (all real files are exact multiples of 84).
pub fn parse_sounds_eff(bytes: &[u8]) -> Vec<RawEmitter> {
    let mut out = Vec::with_capacity(bytes.len() / SOUNDS_EFF_RECORD);
    for rec in bytes.chunks_exact(SOUNDS_EFF_RECORD) {
        let i32_at = |o: usize| i32::from_le_bytes(rec[o..o + 4].try_into().unwrap());
        let f32_at = |o: usize| f32::from_le_bytes(rec[o..o + 4].try_into().unwrap());
        out.push(RawEmitter {
            id: i32_at(0x0C),
            x: f32_at(0x10),
            y: f32_at(0x14),
            z: f32_at(0x18),
            radius: f32_at(0x1C),
            type_: rec[0x38],
            sound_day: i32_at(0x30),
            sound_night: i32_at(0x34),
            repeat_min_ms: i32_at(0x4C),
            repeat_max_ms: i32_at(0x44),
        });
    }
    out
}

/// First sndbnk index reserved for the LOOP list. EMIT entries occupy 1..=161;
/// the client always parks the LOOP list at index 162 (`0xA2`) regardless of how
/// many EMIT entries precede it (`eqgame.exe FUN … :119324-119354`).
pub const LOOP_INDEX_BASE: i32 = 162;

/// A resolved sound reference for one day-or-night slot of an emitter.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SoundRef {
    /// No sound in this slot (id 0, or out-of-range index).
    None,
    /// A zone sndbnk sound effect; `wav` is `<name>.wav`. `looping` = came from the
    /// LOOP list (index >= 162) vs the one-shot EMIT list.
    Sound { name: String, looping: bool },
    /// TYPE 1, negative id: the |id|-th entry (1-based) of the client's global,
    /// exe-baked SFX table. That table isn't in the shipped assets, so we surface the
    /// index for the client to resolve.
    GlobalSfx { index: i32 },
    /// TYPE 1, non-negative id: the zone's background music (`<zone>.xmi`), with `id`
    /// as the track/variation parameter.
    Music { track: i32 },
}

/// Resolve one (type, sound-id) slot against a zone sound bank per the client's rules.
pub fn resolve_ref(bank: &SoundBank, type_: u8, id: i32) -> SoundRef {
    if type_ == 1 {
        return if id < 0 {
            SoundRef::GlobalSfx { index: -id }
        } else {
            SoundRef::Music { track: id }
        };
    }
    // TYPE 0/2/3: 1-based combined sndbnk index (1..=161 EMIT, >=162 LOOP).
    if id <= 0 {
        SoundRef::None
    } else if (id as usize) <= bank.emit.len() {
        SoundRef::Sound { name: bank.emit[(id - 1) as usize].clone(), looping: false }
    } else if id >= LOOP_INDEX_BASE && ((id - LOOP_INDEX_BASE) as usize) < bank.loops.len() {
        SoundRef::Sound { name: bank.loops[(id - LOOP_INDEX_BASE) as usize].clone(), looping: true }
    } else {
        SoundRef::None
    }
}

/// A fully-resolved emitter for the `sound/<zone>` manifest.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Emitter {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub radius: f32,
    #[serde(rename = "type")]
    pub type_: u8,
    pub day: SoundRef,
    pub night: SoundRef,
    #[serde(skip_serializing_if = "is_zero")]
    pub repeat_min_ms: i32,
    #[serde(skip_serializing_if = "is_zero")]
    pub repeat_max_ms: i32,
}

fn is_zero(v: &i32) -> bool {
    *v == 0
}

/// The `sound/<zone>` emitter manifest: the parsed bank plus every emitter resolved.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SoundManifest {
    pub zone: String,
    pub bank: SoundBank,
    pub emitters: Vec<Emitter>,
}

/// Resolve all emitters against a bank into the serializable manifest form.
pub fn resolve_emitters(zone: &str, bank: &SoundBank, raw: &[RawEmitter]) -> SoundManifest {
    let emitters = raw
        .iter()
        .map(|e| Emitter {
            x: e.x,
            y: e.y,
            z: e.z,
            radius: e.radius,
            type_: e.type_,
            day: resolve_ref(bank, e.type_, e.sound_day),
            night: resolve_ref(bank, e.type_, e.sound_night),
            repeat_min_ms: e.repeat_min_ms,
            repeat_max_ms: e.repeat_max_ms,
        })
        .collect();
    SoundManifest { zone: zone.to_string(), bank: bank.clone(), emitters }
}

/// Every distinct wav base name a bank references (emit ∪ loop), for extraction.
pub fn bank_wav_names(bank: &SoundBank) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    for n in bank.emit.iter().chain(bank.loops.iter()) {
        seen.insert(n.to_lowercase());
    }
    seen.into_iter().collect()
}

/// A wav/mp3 source index: base name (lowercased, no extension) → the archive/loose
/// path it can be read from. Built once per build over `sounds/` + all `snd*.pfs`.
pub struct AudioSources {
    /// Loose files under `sounds/` (and the raw dir): base name → absolute path.
    loose: HashMap<String, PathBuf>,
    /// Files inside `snd*.pfs`: base name → (archive path, entry name).
    archived: HashMap<String, (PathBuf, String)>,
    /// Cache of opened PFS readers keyed by archive path is intentionally omitted —
    /// callers extract in a batch, so we reopen per archive at extract time.
    raw_dir: PathBuf,
}

impl AudioSources {
    /// Index every wav available in the raw client dir: loose `sounds/*.wav` (and any
    /// top-level `*.wav`) plus every entry inside `snd*.pfs`.
    pub fn index(raw_dir: &Path) -> Self {
        let mut loose = HashMap::new();
        let mut archived = HashMap::new();

        let mut add_loose = |dir: &Path| {
            if let Ok(rd) = std::fs::read_dir(dir) {
                for e in rd.flatten() {
                    let p = e.path();
                    if p.extension().and_then(|x| x.to_str()).is_some_and(|x| x.eq_ignore_ascii_case("wav")) {
                        if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                            loose.entry(stem.to_lowercase()).or_insert_with(|| p.clone());
                        }
                    }
                }
            }
        };
        add_loose(&raw_dir.join("sounds"));
        add_loose(raw_dir);

        // snd*.pfs archives
        if let Ok(rd) = std::fs::read_dir(raw_dir) {
            let mut archives: Vec<PathBuf> = rd
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.file_name().and_then(|s| s.to_str()).is_some_and(|s| {
                        let l = s.to_lowercase();
                        l.starts_with("snd") && l.ends_with(".pfs")
                    })
                })
                .collect();
            archives.sort();
            for arch in archives {
                let Ok(f) = std::fs::File::open(&arch) else { continue };
                let Ok(mut pfs) = libeq_pfs::PfsReader::open(f) else { continue };
                let Ok(names) = pfs.filenames() else { continue };
                for n in names {
                    if !n.to_lowercase().ends_with(".wav") {
                        continue;
                    }
                    let stem = Path::new(&n)
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or(&n)
                        .to_lowercase();
                    // Prefer loose over archived; among archives, first (sorted) wins.
                    archived.entry(stem).or_insert_with(|| (arch.clone(), n.clone()));
                }
            }
        }

        AudioSources { loose, archived, raw_dir: raw_dir.to_path_buf() }
    }

    /// True if a wav with this base name is available anywhere.
    pub fn has(&self, base: &str) -> bool {
        let k = base.to_lowercase();
        self.loose.contains_key(&k) || self.archived.contains_key(&k)
    }

    /// Read the wav bytes for a base name (loose first, then the containing `snd*.pfs`).
    pub fn read_wav(&self, base: &str) -> Option<Vec<u8>> {
        let k = base.to_lowercase();
        if let Some(p) = self.loose.get(&k) {
            return std::fs::read(p).ok();
        }
        if let Some((arch, entry)) = self.archived.get(&k) {
            let f = std::fs::File::open(arch).ok()?;
            let mut pfs = libeq_pfs::PfsReader::open(f).ok()?;
            return pfs.get(entry).ok().flatten();
        }
        None
    }

    /// The raw client dir this index was built over (for locating `<zone>.mp3`, etc.).
    pub fn raw_dir(&self) -> &Path {
        &self.raw_dir
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_sndbnk_splits_emit_and_loop() {
        let bank = parse_sndbnk(b"EMIT\r\ntorch3d\r\ntorch3d\r\nLOOP\r\nwind_lp4\r\n");
        assert_eq!(bank.emit, vec!["torch3d", "torch3d"]);
        assert_eq!(bank.loops, vec!["wind_lp4"]);
    }

    #[test]
    fn parse_sndbnk_tolerates_lf_and_blank_lines() {
        let bank = parse_sndbnk(b"EMIT\nfire_lp\n\nLOOP\nwind_lp2\nwind_lp3\n");
        assert_eq!(bank.emit, vec!["fire_lp"]);
        assert_eq!(bank.loops, vec!["wind_lp2", "wind_lp3"]);
    }

    #[test]
    fn parse_sounds_eff_reads_position_radius_type_id() {
        // One 84-byte record: id=100012 at (840.83, -1860.83, 7.07) radius 40, type 1, sound_day 2.
        let mut rec = vec![0u8; SOUNDS_EFF_RECORD];
        rec[0x0C..0x10].copy_from_slice(&100012i32.to_le_bytes());
        rec[0x10..0x14].copy_from_slice(&840.83f32.to_le_bytes());
        rec[0x14..0x18].copy_from_slice(&(-1860.83f32).to_le_bytes());
        rec[0x18..0x1C].copy_from_slice(&7.07f32.to_le_bytes());
        rec[0x1C..0x20].copy_from_slice(&40.0f32.to_le_bytes());
        rec[0x30..0x34].copy_from_slice(&2i32.to_le_bytes());
        rec[0x38] = 1;
        let ems = parse_sounds_eff(&rec);
        assert_eq!(ems.len(), 1);
        assert_eq!(ems[0].id, 100012);
        assert!((ems[0].radius - 40.0).abs() < 1e-3);
        assert_eq!(ems[0].sound_day, 2);
        assert_eq!(ems[0].type_, 1);
    }

    #[test]
    fn parse_sounds_eff_ignores_trailing_partial_record() {
        let bytes = vec![0u8; SOUNDS_EFF_RECORD + 10];
        assert_eq!(parse_sounds_eff(&bytes).len(), 1);
    }

    fn beholder_bank() -> SoundBank {
        // Real beholder_sndbnk.eff: 2 EMIT (torch3d,torch3d) + 1 LOOP (wind_lp4).
        SoundBank {
            emit: vec!["torch3d".into(), "torch3d".into()],
            loops: vec!["wind_lp4".into()],
        }
    }

    #[test]
    fn resolve_ref_emit_loop_music_and_sfx() {
        let b = beholder_bank();
        // TYPE 0/2/3 with id 0 → none; 1..emit.len() → EMIT; 162.. → LOOP.
        assert_eq!(resolve_ref(&b, 2, 0), SoundRef::None);
        assert_eq!(resolve_ref(&b, 2, 1), SoundRef::Sound { name: "torch3d".into(), looping: false });
        assert_eq!(resolve_ref(&b, 0, 162), SoundRef::Sound { name: "wind_lp4".into(), looping: true });
        // Out-of-range combined index → none (empty middle of the array).
        assert_eq!(resolve_ref(&b, 0, 50), SoundRef::None);
        assert_eq!(resolve_ref(&b, 0, 999), SoundRef::None);
        // TYPE 1: positive → zone music track; negative → global SFX index.
        assert_eq!(resolve_ref(&b, 1, 2), SoundRef::Music { track: 2 });
        assert_eq!(resolve_ref(&b, 1, -5), SoundRef::GlobalSfx { index: 5 });
    }

    #[test]
    fn bank_wav_names_dedups() {
        // torch3d appears twice in EMIT — only one wav to extract.
        assert_eq!(bank_wav_names(&beholder_bank()), vec!["torch3d", "wind_lp4"]);
    }

    #[test]
    #[ignore = "requires ~/eq_assets/everquest_rof2/beholder_{sndbnk,sounds}.eff"]
    fn real_beholder_resolves_torch_wind_and_music() {
        let home = std::env::var("HOME").unwrap();
        let raw = std::path::PathBuf::from(format!("{home}/eq_assets/everquest_rof2"));
        let sndbnk = raw.join("beholder_sndbnk.eff");
        if !sndbnk.exists() {
            eprintln!("skip: no beholder assets");
            return;
        }
        let bank = parse_sndbnk(&std::fs::read(&sndbnk).unwrap());
        assert_eq!(bank.emit, vec!["torch3d", "torch3d"]);
        assert_eq!(bank.loops, vec!["wind_lp4"]);
        let raw_em = parse_sounds_eff(&std::fs::read(raw.join("beholder_sounds.eff")).unwrap());
        let m = resolve_emitters("beholder", &bank, &raw_em);
        assert_eq!(m.emitters.len(), 5);
        // One-shot torch (type 2, day-only).
        assert_eq!(m.emitters[0].day, SoundRef::Sound { name: "torch3d".into(), looping: false });
        assert_eq!(m.emitters[0].night, SoundRef::None);
        // Zone-wide wind loop (type 0, id 162, radius 2000).
        assert!(m.emitters[2].radius > 1000.0);
        assert_eq!(m.emitters[2].day, SoundRef::Sound { name: "wind_lp4".into(), looping: true });
        // Background music triggers (type 1, positive selector).
        assert_eq!(m.emitters[3].day, SoundRef::Music { track: 1 });
        assert_eq!(m.emitters[4].day, SoundRef::Music { track: 2 });
    }
}
