//! Explicit byte-source binding for conditional q32 initial-load diagnostics.
//!
//! This composes the raw project/DAT readers with the ordered geometry model.
//! It does not choose a filesystem/archive provider, convert world coordinates,
//! publish a GLB, interpret materials or establish native/query equivalence.

use super::terrain_initial_load::{InitialLoadError, Q32LoadLimits, Q32PreparedTile};
use libeq_eqg::{terrain_dat, terrain_project};
use sha2::Digest;
/// One explicitly named borrowed byte resource; no IO or provider precedence.
#[derive(Clone, Copy)]
pub struct NamedSource<'a> {
    pub name: &'a [u8],
    pub bytes: &'a [u8],
}
/// Logical source budgets, excluding input ownership, allocator overhead and RSS.
/// Name bytes include descriptor and every provider name; namespace bytes include
/// every provider payload, even unused ones. DAT/preparation limits compose, with
/// the smaller tile limit applied before DAT descriptor allocation.
pub struct SourceLimits {
    pub max_project_bytes: usize,
    pub max_resources: usize,
    pub max_namespace_bytes: usize,
    pub max_name_bytes: usize,
    pub dat: terrain_dat::Limits,
    pub preparation: Q32LoadLimits,
}
impl Default for SourceLimits {
    fn default() -> Self {
        Self {
            max_project_bytes: 1024 * 1024,
            max_resources: 64,
            max_namespace_bytes: 128 * 1024 * 1024,
            max_name_bytes: 64 * 1024,
            dat: terrain_dat::Limits::default(),
            preparation: Q32LoadLimits::default(),
        }
    }
}
/// The value token's original byte offset, or an explicitly supported default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueOrigin {
    Token(usize),
    Default,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Field<T> {
    pub value: T,
    pub origin: ValueOrigin,
}
/// Selected geometry fields. VERSION alone has supported default 1. Other
/// recognized fields are consumed by arity and retained in the raw project,
/// without interpreting their values or claiming full metadata equivalence.
#[derive(Debug)]
pub struct Metadata<'a> {
    pub name: Field<&'a [u8]>,
    pub quads: Field<u32>,
    pub units: Field<f32>,
    pub version: Field<u32>,
    pub tile_width: f32,
    /// Number of consumed value tokens outside NAME/q/units/VERSION.
    pub unmodeled_values: usize,
}
/// Borrowed raw inputs and owned conditional geometry. Content digests are
/// SHA256 over complete bytes. source_id hashes profile bytes
/// `eqoxide/terrain-q32-initial-load/53rz/v1` plus NUL, then descriptor name,
/// descriptor bytes, selected DAT name and DAT bytes, each with LE-u64 length.
/// It identifies supplied content, not archive authenticity, provider
/// completeness, complete-zone coverage or a native arithmetic certificate.
/// Limits and unused namespace entries are not hashed. This is not a cache key
/// for acceptance or prepared output: callers must revalidate their limits and
/// bind any cached result to its preparation policy and implementation revision.
#[derive(Debug)]
pub struct SourceDiagnostic<'a> {
    pub descriptor_name: &'a [u8],
    pub dat_name: &'a [u8],
    pub project: terrain_project::Project<'a>,
    pub dat: terrain_dat::PackedV20<'a>,
    pub metadata: Metadata<'a>,
    pub prepared: Vec<Q32PreparedTile>,
    pub source_id: [u8; 32],
    pub descriptor_hash: [u8; 32],
    pub dat_hash: [u8; 32],
}
/// Typed whole-operation refusal. No input bytes or partial diagnostic change.
#[derive(Debug, thiserror::Error)]
pub enum SourceError {
    #[error("{resource} budget {limit} exceeded by {requested}")]
    Budget {
        resource: &'static str,
        requested: usize,
        limit: usize,
    },
    #[error("size overflow for {resource}")]
    SizeOverflow { resource: &'static str },
    #[error(transparent)]
    Project(#[from] terrain_project::ParseError),
    #[error("missing {0}")]
    MissingField(&'static str),
    #[error("unknown directive at {offset}")]
    UnknownDirective { offset: usize },
    #[error("truncated directive at {offset}")]
    TruncatedField { offset: usize },
    #[error("unsupported {key} at {offset}: {reason}")]
    Metadata {
        offset: usize,
        key: &'static str,
        reason: &'static str,
    },
    #[error("missing exact NAME+.dat provider")]
    MissingDat,
    #[error("ambiguous exact NAME+.dat provider")]
    AmbiguousDat,
    #[error("unsupported {resource} suffix of {bytes} bytes")]
    Suffix {
        resource: &'static str,
        bytes: usize,
    },
    #[error(transparent)]
    Dat(#[from] terrain_dat::ParseError),
    #[error("nonfinite grid0 height in tile {tile}, vertex {vertex}")]
    NonFiniteHeight { tile: usize, vertex: usize },
    #[error("allocation failed")]
    Allocation(#[from] std::collections::TryReserveError),
    #[error(transparent)]
    Load(#[from] InitialLoadError),
}
/// Bind exact NAME+.dat bytes and prepare the complete counted q32 tile set.
/// Source names use byte equality, without case/path normalization or fallback.
/// DAT header naming remains independent of the project NAME.
///
/// Require explicit NAME, q32 and units; effective project VERSION must be 1,
/// using the recorded default when absent. Keys are exact uppercase tokens;
/// recognized arities consume key-looking values. Unknown top-level tokens are
/// refused. Repeated semantic fields use the last assignment, while every
/// assignment must have supported spelling. Other recognized fields only
/// contribute consumed raw tokens, not interpreted scene/constructor behavior.
///
/// Raw tokens are limited to 255 bytes. NAME is 1..=128 ASCII letters, digits,
/// underscores or hyphens. q/VERSION use checked unsigned decimal with optional
/// leading plus. Units use positive integer-valued decimal in 1..=524288, with
/// optional plus and optional nonempty all-zero fraction. This includes 5.00
/// without approximate atof emulation and makes q32 width exactly binary32.
///
/// Both opaque nonempty suffixes are refused. Every grid0 height must be finite;
/// grids1/2 and non-geometry records remain raw. Heights are direct LE f32 bits,
/// without vertical unit scaling. Unique pairs, coordinate bounds, full seam
/// scheduling, fixed controls and conditional 53RZ come from the load model.
/// Fallible allocations and private ownership make late refusal atomic. Empty
/// counted input yields an empty supplied-source diagnostic, not a usable zone.
/// Decoded inputs remain allocated alongside the load model's owned snapshots;
/// tile/triangle limits are not a total-memory or process-RSS bound.
/// No source resolver, world-position conversion, material/GLB/query route or
/// full-content/native-mode/surface-equivalence certificate is enabled.
pub fn prepare_explicit_project_53rz<'a>(
    descriptor: NamedSource<'a>,
    resources: &[NamedSource<'a>],
    limits: &SourceLimits,
) -> Result<SourceDiagnostic<'a>, SourceError> {
    check(
        "project bytes",
        descriptor.bytes.len(),
        limits.max_project_bytes,
    )?;
    check("resources", resources.len(), limits.max_resources)?;
    let mut name_bytes = 0;
    charge(
        "resource name bytes",
        &mut name_bytes,
        descriptor.name.len(),
        limits.max_name_bytes,
    )?;
    let mut namespace_bytes = 0;
    for resource in resources {
        charge(
            "resource name bytes",
            &mut name_bytes,
            resource.name.len(),
            limits.max_name_bytes,
        )?;
        charge(
            "namespace bytes",
            &mut namespace_bytes,
            resource.bytes.len(),
            limits.max_namespace_bytes,
        )?;
    }
    let project = terrain_project::parse(descriptor.bytes)?;
    if let Some(suffix) = project.trailing_data().filter(|suffix| !suffix.is_empty()) {
        return Err(SourceError::Suffix {
            resource: "project",
            bytes: suffix.len(),
        });
    }
    let metadata = metadata(project)?;
    let mut matches = resources
        .iter()
        .filter(|r| r.name.strip_suffix(b".dat") == Some(metadata.name.value));
    let selected = matches.next().ok_or(SourceError::MissingDat)?;
    if matches.next().is_some() {
        return Err(SourceError::AmbiguousDat);
    }
    let mut dat_limits = limits.dat;
    dat_limits.max_tiles = dat_limits.max_tiles.min(limits.preparation.max_tiles);
    let dat = terrain_dat::parse_packed_v20_with_limits(
        selected.bytes,
        metadata.quads.value,
        &dat_limits,
    )?;
    if !dat.trailing_data().is_empty() {
        return Err(SourceError::Suffix {
            resource: "DAT",
            bytes: dat.trailing_data().len(),
        });
    }
    let mut input = Vec::new();
    input.try_reserve_exact(dat.tiles().len())?;
    for (tile, source) in dat.tiles().iter().enumerate() {
        let mut heights = [[0.0; 33]; 33];
        for (vertex, (height, bytes)) in heights
            .iter_mut()
            .flatten()
            .zip(source.grid_bytes[0].as_chunks::<4>().0)
            .enumerate()
        {
            *height = decode_height(bytes);
            if !height.is_finite() {
                return Err(SourceError::NonFiniteHeight { tile, vertex });
            }
        }
        input.push(super::terrain_initial_load::Q32TileInput {
            // Raw words preserve signed two's-complement source coordinates;
            // the load model independently checks its diagnostic admission bound.
            coordinate: source
                .coordinates
                .map(|word| i32::from_le_bytes(word.to_le_bytes())),
            heights,
            flags: std::array::from_fn(|y| {
                source.quad_bytes[32 * y..32 * (y + 1)]
                    .try_into()
                    .expect("fixed q32 row")
            }),
        });
    }
    let prepared =
        super::terrain_initial_load::prepare_initial_load_53rz(&input, &limits.preparation)?;
    let mut identity = sha2::Sha256::new();
    identity.update(b"eqoxide/terrain-q32-initial-load/53rz/v1\0");
    for bytes in [
        descriptor.name,
        descriptor.bytes,
        selected.name,
        selected.bytes,
    ] {
        let length = u64::try_from(bytes.len()).map_err(|_| SourceError::SizeOverflow {
            resource: "identity length",
        })?;
        identity.update(length.to_le_bytes());
        identity.update(bytes);
    }
    Ok(SourceDiagnostic {
        descriptor_name: descriptor.name,
        dat_name: selected.name,
        project,
        dat,
        metadata,
        prepared,
        source_id: identity.finalize().into(),
        descriptor_hash: sha2::Sha256::digest(descriptor.bytes).into(),
        dat_hash: sha2::Sha256::digest(selected.bytes).into(),
    })
}

fn check(resource: &'static str, requested: usize, limit: usize) -> Result<(), SourceError> {
    if requested > limit {
        return Err(SourceError::Budget {
            resource,
            requested,
            limit,
        });
    }
    Ok(())
}
fn charge(
    resource: &'static str,
    total: &mut usize,
    amount: usize,
    limit: usize,
) -> Result<(), SourceError> {
    *total = total
        .checked_add(amount)
        .ok_or(SourceError::SizeOverflow { resource })?;
    check(resource, *total, limit)
}
fn decode_height(bytes: &[u8]) -> f32 {
    f32::from_bits(u32::from_le_bytes(
        bytes.try_into().expect("validated height word"),
    ))
}
fn field<T>(token: terrain_project::Token<'_>, value: T) -> Field<T> {
    Field {
        value,
        origin: ValueOrigin::Token(token.byte_offset),
    }
}
fn invalid(
    token: terrain_project::Token<'_>,
    key: &'static str,
    reason: &'static str,
) -> SourceError {
    SourceError::Metadata {
        offset: token.byte_offset,
        key,
        reason,
    }
}
fn digits(bytes: &[u8]) -> Option<u32> {
    if bytes.is_empty() {
        return None;
    }
    bytes.iter().try_fold(0u32, |n, &byte| {
        if !byte.is_ascii_digit() {
            return None;
        }
        n.checked_mul(10)?.checked_add(u32::from(byte - b'0'))
    })
}
fn integer(token: terrain_project::Token<'_>, key: &'static str) -> Result<u32, SourceError> {
    digits(token.bytes.strip_prefix(b"+").unwrap_or(token.bytes))
        .ok_or_else(|| invalid(token, key, "expected checked unsigned decimal integer"))
}
fn units(token: terrain_project::Token<'_>) -> Result<f32, SourceError> {
    let bytes = token.bytes.strip_prefix(b"+").unwrap_or(token.bytes);
    let mut parts = bytes.split(|&byte| byte == b'.');
    let integer = digits(parts.next().expect("first split part"));
    let fraction = parts.next();
    if parts.next().is_some()
        || fraction.is_some_and(|f| f.is_empty() || f.iter().any(|&b| b != b'0'))
    {
        return Err(invalid(
            token,
            "*UNITSPERVERT",
            "expected exact positive integer-valued decimal units",
        ));
    }
    match integer {
        Some(value @ 1..=524288) => Ok(value as f32),
        _ => Err(invalid(
            token,
            "*UNITSPERVERT",
            "exact units must be within 1..=524288",
        )),
    }
}
fn bounded_token(token: terrain_project::Token<'_>, key: &'static str) -> Result<(), SourceError> {
    if token.raw.len() > 255 {
        return Err(invalid(token, key, "raw token exceeds 255 bytes"));
    }
    Ok(())
}
fn metadata<'a>(project: terrain_project::Project<'a>) -> Result<Metadata<'a>, SourceError> {
    let mut tokens = project.tokens();
    bounded_token(tokens.next().expect("validated project header"), "EQTZP")?;
    let (mut name, mut quads, mut unit) = (None, None, None);
    let mut version = Field {
        value: 1,
        origin: ValueOrigin::Default,
    };
    let mut unmodeled_values = 0;
    while let Some(token) = tokens.next() {
        bounded_token(token, "directive")?;
        let (key, count) = match token.bytes {
            b"*NAME" => ("*NAME", 1),
            b"*QUADSPERTILE" => ("*QUADSPERTILE", 1),
            b"*UNITSPERVERT" => ("*UNITSPERVERT", 1),
            b"*VERSION" => ("*VERSION", 1),
            b"*MINLNG" => ("*MINLNG", 1),
            b"*MAXLNG" => ("*MAXLNG", 1),
            b"*MINLAT" => ("*MINLAT", 1),
            b"*MAXLAT" => ("*MAXLAT", 1),
            b"*COLORMAPINPUTSIZE" => ("*COLORMAPINPUTSIZE", 1),
            b"*COVERMAPINPUTSIZE" => ("*COVERMAPINPUTSIZE", 1),
            b"*LAYERINGMAPINPUTSIZE" => ("*LAYERINGMAPINPUTSIZE", 1),
            b"*COVERAGEMAPINPUTSIZE" => ("*COVERAGEMAPINPUTSIZE", 1),
            b"*MIN_EXTENTS" => ("*MIN_EXTENTS", 3),
            b"*MAX_EXTENTS" => ("*MAX_EXTENTS", 3),
            _ => {
                return Err(SourceError::UnknownDirective {
                    offset: token.byte_offset,
                })
            }
        };
        let first = tokens.next().ok_or(SourceError::TruncatedField {
            offset: token.byte_offset,
        })?;
        bounded_token(first, key)?;
        for _ in 1..count {
            let value = tokens.next().ok_or(SourceError::TruncatedField {
                offset: token.byte_offset,
            })?;
            bounded_token(value, key)?;
        }
        match key {
            "*NAME" => {
                if first.bytes.is_empty()
                    || first.bytes.len() > 128
                    || !first
                        .bytes
                        .iter()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
                {
                    return Err(invalid(
                        first,
                        key,
                        "expected 1..=128 ASCII stem bytes: letters, digits, underscore or hyphen",
                    ));
                }
                name = Some(field(first, first.bytes));
            }
            "*QUADSPERTILE" => quads = Some(field(first, integer(first, key)?)),
            "*UNITSPERVERT" => unit = Some(field(first, units(first)?)),
            "*VERSION" => version = field(first, integer(first, key)?),
            _ => unmodeled_values += count,
        }
    }
    let name = name.ok_or(SourceError::MissingField("*NAME"))?;
    let quads = quads.ok_or(SourceError::MissingField("*QUADSPERTILE"))?;
    let units = unit.ok_or(SourceError::MissingField("*UNITSPERVERT"))?;
    if quads.value != 32 {
        return Err(SourceError::Metadata {
            offset: match quads.origin {
                ValueOrigin::Token(o) => o,
                _ => 0,
            },
            key: "*QUADSPERTILE",
            reason: "only q32 is admitted",
        });
    }
    if version.value != 1 {
        return Err(SourceError::Metadata {
            offset: match version.origin {
                ValueOrigin::Token(o) => o,
                _ => 0,
            },
            key: "*VERSION",
            reason: "only effective project version 1 is admitted",
        });
    }
    Ok(Metadata {
        name,
        quads,
        units,
        version,
        tile_width: 32.0 * units.value,
        unmodeled_values,
    })
}
