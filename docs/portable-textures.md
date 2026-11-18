# Explicit portable PNG8 preparation

`source_material::portable::prepare_portable_textures(&scene, &limits)` borrows a
`StaticScene` and returns owned, same-order textures plus a serializable report.
It applies to the current base-color texture slots. Future normal, metallic or
roughness data maps need their own semantics before using this policy.

The WLD example opts in explicitly:

```sh
cargo run --locked --offline --example wld_pbr_zone -- \
  --preview-snapshot --portable-png8 MAIN.s3d OUTPUT.glb OBJECTS.s3d
```

`--portable-png8` also works with strict mode and `--inspect`. Inspection actually
prepares and validates the textures, reports `cpu-portable-texture-inspection`,
and writes no GLB. It does not certify whole-scene writer or renderer validity.
Duplicate flags, unknown options and incorrect positional argument counts refuse.
Without the flag, texture precision is preserved as before.

## Precision and coverage

Policy `portable-png8-nearest-v1` maps each 16-bit RGBA channel independently:
`q = (u32(v) + 128) / 257`. It emits RGBA8 PNG, appending
`:png8-nearest-v1` to the texture name. RGB stays encoded sRGB; alpha stays linear
and straight. There is no gamma conversion, premultiplication or dithering.
PNG8 images retain their exact names and encoded bytes after full validation.

The source-code error `abs(v - 257*q)` is at most 128, or `128/65535` normalized.
Endpoints are exact, as are all 8-bit values expanded by 257. Some nonzero BLEND
alpha samples become zero. These are measured precision losses, with no promise
of exact rendered appearance or native-client parity.

If any material references a texture as MASK, every decoded 16-bit alpha sample
must satisfy `v % 257 == 0`; otherwise the whole preparation refuses. This also
checks unused materials and shared MASK/BLEND textures. Exact alpha preserves the
texel field used by filtering, factors and cutoff. RGB may still quantize.
Material factors, alpha modes, cutoffs, sidedness, geometry, normals, UVs, colors
and transforms are unchanged. Preparation does not copy mesh geometry.

## Inputs, reports and identity

The API leaves its borrowed source scene and texture bytes unchanged. The CLI
writes a derived GLB and leaves native archives unchanged. The preserve-depth
invocation can regenerate the high-precision snapshot. A second on-disk bundle of
intermediate PNGs is not automatically created; callers needing those bytes keep
the borrowed source textures.

`conversion` remains the original importer report, marked `source-import`.
`texture_preparation` describes `emitted-texture-preparation` from
`source-import-png`: each ordinal records original/emitted names, dimensions,
input/output bit depths, encoded BLAKE3 hashes, maximum channel error, changed
sample counts and nonzero alpha samples rounded to zero. The new report is
serialized before committing a GLB. A standard-output failure after the GLB has
been committed cannot roll that write back.

The bake revision binds importer conversion, material choices, writer profile,
explicit precision policy, source/emitted image names and hashes, and converter
and writer implementation identity. Directory and timestamp are not identities.
Choosing portable policy changes the revision even when all inputs are PNG8.
The existing common writer remains the sole GLB serializer and validates the
whole scene; preparation success alone is not publication or gameplay readiness.

## Bounds and filesystem safety

Default logical limits are:

| Resource | Limit |
| --- | ---: |
| Combined textures and materials | 100,000 |
| Single encoded input/output PNG | 64 MiB |
| Aggregate encoded input PNGs | 128 MiB |
| Aggregate encoded output PNGs | 128 MiB |
| Single charged RGBA decode | 64 MiB |
| Aggregate charged RGBA decode | 256 MiB |
| Maximum image axis | 8,192 |
| Copied texture and report names | 8 MiB |
| Serialized compact preparation report | 16 MiB |

Preflight checks dimensions, checked sums/products and all input/name/work budgets
before pixel allocation. Decode charges four bytes per PNG8 pixel and eight per
PNG16 pixel, including RGB/grayscale expansion. The shared static decoder also
retains its own 64 MiB and 8,192-axis ceilings when callers raise these limits.
Complete PNG pixels, CRCs and trailers are validated even for byte-preserving
PNG8. Header depths other than 8 and 16 refuse. Encoded conversion output and
serialized report growth are capped.

These are logical caps, not hard process-memory guarantees. A decoder may hold
native-channel and expanded RGBA buffers transiently. Conversion can hold RGBA16,
RGBA8 and encoded output together; the caller also retains its source scene.

Before parsing archives, the CLI refuses an existing output that resolves to
either supplied archive through direct, relative or symbolic-link paths. On Unix
it additionally compares device/inode identity, covering hard links. Other
platforms provide canonical-path checks. Refusals preserve existing sources and
previous output. This assumes a stable, trusted local filesystem and makes no
concurrent path-substitution guarantee. The common writer's validation and
atomic output replacement still apply to ordinary destinations.

Portable authoring export is distinct from full edited-scene reimport
qualification. Precision preparation does not waive RGB, alpha, normal or other
roundtrip acceptance thresholds.
