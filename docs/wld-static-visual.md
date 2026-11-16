# Offline WLD static visual conversion

`wld_visual::load_wld_visual` resolves explicitly supplied WLD archives into the
existing `StaticScene` input. The standard material-policy writer owns GLB
serialization and reader requirements. The client receives the common visual
format; it needs no WLD archive or source-specific material dispatch.

```sh
cargo run --locked --offline --example wld_pbr_zone -- \
  MAIN.s3d OUTPUT.glb OBJECTS.s3d
```

Use `wld_pbr_zone --inspect MAIN.s3d [OBJECTS.s3d]` for CPU source inspection
without writing a GLB or asserting writer/renderer compatibility.

Omit the final argument when the main archive has no placed objects and no
required texture depends on that archive. Inputs must be regular local files.
The example is offline: it neither publishes a manifest nor activates gameplay.
It writes the GLB only after complete conversion and existing writer validation;
source-conversion refusal preserves a prior output. The JSON report goes to
standard output after successful writing.

## Selected inputs and geometry

The terrain document is the main archive basename plus `.wld`. If present,
`objects.wld` in that archive supplies placement records. The optional object
archive basename plus `.wld` supplies static model definitions and additional
textures. Archive lookup is ASCII case-insensitive; case-equivalent duplicate
member names refuse. Other WLD members are reported rather than implicitly
selected. This source selection is an explicit importer policy, not proof of
native archive/provider precedence or WorldTree visibility membership.

The importer accepts modern mesh records in old `0x00015500` or new
`0x1000c800` WLD documents. Fragment headers and complete material groups are
checked before geometry traversal. Missing references, malformed visual fragment
bodies, invalid indices and unsupported mesh forms refuse. Positive fragment
ordinals resolve directly, even if unused fragments share names. A named
reference requires one matching typed target. Placed model names similarly need
one ActorDef; the importer follows its DmSprite mesh reference rather than
matching a convenient mesh basename.

Each selected ActorDef has one static sprite. An optional sole LOD record must
have one finite nonnegative distance and no unknown action control. The offline
scene includes that sprite without applying its distance-culling threshold;
there is no alternate mesh to select. Multiple sprite/LOD alternatives, animated
or skinned meshes, ActorDef-local transforms and unsupported current actions
refuse. Dynamic actor activation, doors and runtime visibility are not certified
by this inspection export.

Raw positions and mesh centers convert once from source `(x,y,z)` into server
`(y,x,z)` geometry axes. WLD's source clockwise face order becomes CCW under
that reflection; indices retain their source face/corner order. Packed nonzero
vertex normals are explicitly normalized in those same axes. Object instances
reuse geometry, retain positive scale (including values below `0.01`) and use
the existing analytic three-axis WLD placement composition. Missing, zero,
negative and nonfinite transforms refuse instead of using radius, one or origin.
The common writer then converts server axes into its documented glTF basis.

Invisible render faces are excluded and counted; this says nothing about their
collision eligibility. Faces with coincident source positions are also excluded
with a count, preserving other faces. Other zero-area faces refuse. Unsupported
required content is never replaced by a white texture or dropped placement.

## Materials and source preservation

Supported diffuse and solid standard materials become editable rough nonmetal
PBR defaults. Masked materials retain a cutoff of `0.5`; palette-index-zero BMP
keying and preserved DDS/PNG alpha use the existing bounded texture normalizer.
Ordinary 25%, 50% and 75% source blend modes retain `BLEND` intent and put opacity
only in the base-color alpha factor. Texture pixels and RGB are not multiplied
by that opacity. The earlier scalar-PBR writer still refuses lit BLEND; source
normalization alone does not enable an HDR renderer or advertise another
capability. The example therefore refuses a blend-containing scene at its
historical scalar-PBR writer boundary until a compatible profile is selected.

Standard materials declaring no texture use an explicit white base-color
starting point, not an invented bitmap or a claim to recover source reflectance.
Source pen, brightness and ambient words remain report data. Textured materials
require their exact declared image. The supplied-provider policy prefers main
for terrain and object for object materials, falling back to the other supplied
archive only when the exact member is absent. The same chosen resource determines
both decoded pixels and UV orientation: upright BMP/PNG uses `1-v`, while the
source's mirrored DDS row convention keeps V. Conflicting provider containers
cannot make UV selection disagree with embedded bytes. No filename-extension
substitution or corrupt-image fallback occurs.

Texture sequences, additive materials, unsupported standard texture/draw modes,
unknown coverage, texture layers and material UV motion refuse. Alpha and
sidedness are separate from PBR roughness/metalness choices. This API does not
infer physical properties from dark pixels, apply native prelighting, or place
lighting weights into material color or alpha.

Original files remain untouched. Reports bind actual selected WLD and texture
bytes by BLAKE3, identify selected providers and material references, retain
lighting-word digests/counts and raw material lighting words, and account for
source/retained/excluded faces and instance work. Source vertex lighting and
Actor lighting references are not applied as `COLOR_0`. The bake revision binds
selected input bytes and conversion/writer policy sources plus the dependency
lock. These identities are scoped to this bake, not stable Blender material
indices or proof of native/world completeness.

## Limits and acceptance boundary

Limits may be lowered by callers; values above these supported maxima refuse:

| Logical resource | Maximum |
| --- | ---: |
| Each encoded archive | 128 MiB |
| Each decoded member | 64 MiB |
| Archive directory | 4 MiB |
| Aggregate read member bytes | 256 MiB |
| Aggregate WLD fragments | 100,000 |
| Output vertices | 5,000,000 |
| Output indices | 15,000,000 |
| Instances | 100,000 |
| Output resources | 100,000 |
| Transformed vertex visits | 50,000,000 |
| Geometry/image/instance payload estimate | 128 MiB |

Member names and WLD symbol names are bounded at 256 bytes. Inputs admit no
trailing document content except an empty suffix or the exact four-byte all-ones
WLD terminator. Declared and returned member sizes must agree. Logical budgets
cover managed payload/work, not total RSS, parser/decoder internals or driver
memory. File selection and hashing do not establish a hostile-filesystem or
concurrent-edit security contract. The parser boundary isolates ordinary panics;
that does not turn allocation exhaustion into a recoverable guarantee.

Synthetic PFS/WLD tests exercise actual raw references, material pixels,
independent transformed points, winding/normals, mask and blend intent, repeated
instances, tiny scales, same-provider UV orientation and explicit negative
controls. Successful CPU conversion is a prerequisite, not proof of client GPU
rendering, native-zone completeness, package validity, collision or gameplay.
Blender edits require validation and rebuilt engine/package identity before use;
arbitrary index ordering and metadata round trips are not promised.
