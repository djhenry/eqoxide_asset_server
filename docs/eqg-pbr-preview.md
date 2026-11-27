# Binary EQG static PBR preview

The explicit offline `eqg_pbr_zone` example resolves a binary descriptor version 2
and its selected EQG archive into the common editable glTF PBR profile. It includes
terrain and selected static placements, diffuse alpha cutouts, and a static water
approximation. It does not publish an asset-server route or certify gameplay,
dynamic actors, animated effects, or native appearance parity.

```sh
cargo run --locked --offline --example eqg_pbr_zone -- \
  --static-preview --descriptor-file zone.zon zone.eqg preview.glb
cargo run --locked --offline --example eqg_pbr_zone -- \
  --static-preview --inspect --descriptor-member zone.zon zone.eqg
```

Exactly one explicit descriptor selector is required. `--inspect` performs CPU
source inspection and emits JSON without an output artifact. Duplicate, unknown,
conflicting, missing-value, and extra arguments refuse. `--` delimits positional
filenames. Bake output uses the common `ScalarPbrHdrBlendV1` writer and requires a
consumer supporting `pbr-metallic-roughness-blend-v1` and the shared HDR reference
lighting profile.

The library entry point is `eqg::pbr_visual::load_eqg_pbr_preview`. Its owned result
contains the scene, recommended scalar material policy, conversion report,
unchanged parsed source, and unique referenced raw texture resources. Materials
are keyed by source mesh and material table ordinal, independently of declared
IDs. All selected nonsentinel geometry uses the existing geometry bridge; record
zero supplies terrain lighting metadata, while unassigned and repeated terrain
placements are reported as exclusions.

The exact supported shader names are `Opaque_MPLFull2UV.fx`, `Opaque_MPLBump.fx`,
`Opaque_MPLBasic.fx`, `Opaque_MPLSB.fx`, `Opaque_MaxCB1.fx`,
`Opaque_MaxCBSGE1.fx`, `Opaque_MaxC1.fx`, `Opaque_MaxCBSG1.fx`,
`Opaque_MaxCBS1.fx`, `Chroma_MaxC1.fx`, `Chroma_MaxCB1.fx`, and
`Opaque_MaxWater.fx`, `Opaque_OpaqueSModelCB1Max.fx`, and
`Opaque_OpaqueSModelCBGG1Max.fx`. Each has a mandatory exact typed property set. Missing,
extra, duplicate, wrong-kind, and unknown required shader/property semantics
refuse. Unknown unused material semantics remain retained and uninterpreted;
structural source validity is still required globally.

Ordinary materials use white RGBA factors, metallic 0, roughness 1, OPAQUE, and
single sided rendering. The two Chroma families use MASK at 192/255 with static
tint alpha 1. Water uses preserved diffuse alpha multiplied once by a material
alpha factor of 0.55, metallic 0, roughness 0.2, BLEND, and double sided rendering.
These editable water scalars are authored appearance defaults, not physical
measurements or a claim of 55 percent effective pixel opacity. Water motion,
Fresnel, refraction, and environment arithmetic are omitted.

The two additional opaque spellings use the existing opaque scalar PBR defaults,
with source interpretation verified against the native RoF2 client. The CB1
spelling requires diffuse and normal texture properties; the CBGG1 spelling also
requires kind-0 bumpiness and shininess. Those scalar words stay raw and unapplied.

After validating a known used material's exact typed schema, the exact
case-sensitive string `None` on `e_TextureNormal0` means an absent optional normal
resource. Its raw source string/value remains unchanged, `resource_index` stays
null, and `normal_source_absence: "ExactNone"` plus a decision records why no image
is requested. Even a present archive member named `None` is ignored for that
normal property. Other strings (including `none`, `NONE`, and `NoneExtra.PNG`)
remain ordinary required image names. Diffuse, coverage, and environment
properties receive no absence exemption. Unknown unused materials remain
uninterpreted. This policy adds no descriptor-v1 admission or native appearance
claim.

All required diffuse, normal, coverage, and environment resources resolve only
against the selected archive, using ASCII case folding with ambiguous member
names refused. Selected path-bearing names and missing resources refuse. Every
unique required image passes the existing static RGBA decoder; only normalized
diffuse PNG enters the scene. Source RGBA sample depth, row orientation, and UV0
are preserved. Packed vertex RGB/alpha, secondary UVs, source parameters, raw
auxiliary bytes, lights, regions, and prelighting remain separately retained and
unapplied. No `COLOR_0` or native RGB bake is emitted. A prelighting count match
only describes the current vertex pool; it does not identify a native consumer.

`EqgPbrLimits` allows reducing fixed implementation maxima. Regular archive and
descriptor bytes, directory metadata, member count/name bytes, and every archive
member's advertised uncompressed bytes are checked before the existing source
assembler. Unused archive members count too. Logical source-record caps apply
immediately after assembly, not before its vector allocations. Required raw image
bytes are charged before retaining them. Decode work and normalized PNG bytes
are charged sequentially after one bounded static-decoder scratch image at a
time; auxiliary normalized images are discarded. These logical limits do not
bound peak process memory. In particular the pinned PFS dependency still inflates
a block before checking its final size, so this entry point requires trusted,
stable local files.

Before/after content digests cover original file handles and reopened paths, with
observed changes refused. Identity binds actual archive/descriptor/model/resource
bytes, selector kind, raw preserved data, exact material policy, resolver/shared
source fingerprints, dependency lock, and writer profile. Paths and timestamps
are excluded. This is content identity, not filesystem authentication or hostile
concurrent mutate-and-restore detection. Conversion and final GLB revisions are
separate. The CLI owns its load-to-write scope; editing the returned public scene
or a GLB requires a future identity normalization/reimport contract.

Report record and string counters are conservative projection work units, including
generated decisions (including optional-normal absence), lighting/geometry rows,
and duplicated resource-use projections.
The JSON report owns bounded raw-word projections, resource hashes/uses, lighting
record hashes/counts, geometry accounting, actual scalar choices, and omissions.
Raw nonfinite scalar bits serialize as integers. The CLI stages the real common
writer output through an outer closed temporary pathname, checks encoded GLB and
complete report sizes, reopens and syncs the resulting file, and only then
persists it to the destination. Every precommit refusal preserves an existing
output. An stdout failure after persistence cannot roll back that artifact.

## Read-only material inventory

The separate `eqg_pbr_inventory` example accepts explicit binary descriptors v1
and v2 and emits only JSON on stdout:

```sh
cargo run --locked --offline --example eqg_pbr_inventory -- \
  --descriptor-member zone.zon zone.eqg
cargo run --locked --offline --example eqg_pbr_inventory -- \
  --descriptor-file zone.zon zone.eqg
```

Exactly one descriptor selector and one archive positional are required. Unknown,
duplicate, conflicting, missing-value, and extra arguments refuse. There is no
output pathname or artifact writer. Use `--` before positional filenames starting
with a dash, or `--descriptor-file=--zone.zon` for a dash-prefixed selector value.

`eqg::pbr_visual::inspect_eqg_pbr_materials` returns an
`EqgPbrMaterialInventory`. It retains every material keyed by mesh and table
ordinal, including duplicate declared IDs, raw offsets, shader names or nulls,
and ordered properties with exact kind/value words and kind-2 strings. Repeated
model slots refer to one mesh. Required materials are those referenced by
nonsentinel terrain triangles and selected object placements. Descriptor v1
includes placement zero; v2 excludes its terrain-lighting metadata record.

Every required material is checked against the same exact typed schema policy
as the preview. All incompatible rows are reported together, including unknown
and null required shaders. Unused materials remain uninterpreted. Required
kind-2 strings receive safe-name and ASCII-case-folded archive lookup facts,
including missing and unsafe names, even if their shader schema is unsupported.
These facts assign no texture role or meaning to unknown shaders. Ambiguous
archive member names, parse/assembly errors, and invalid required material
references remain hard failures. Invalid shader, property-name, and kind-2 string
offsets are structural parse failures even in unused or unknown-shader rows;
uninterpreted means structurally valid records without assigned semantics.
Exactly one distinct terrain mesh is required; repeated model slots referencing
the same terrain remain valid. Unselected
triangle material references and geometry completeness are not checked.

**Readiness is always not evaluated.** Schema compatibility and advertised member
presence do not validate texture decoding, geometry, lighting, conversion, or
native appearance. No texture bytes are decoded, no geometry is bridged, and no
lighting is evaluated. In particular, v1 inventory does not admit v1 into the
existing v2-only preview.

The inventory uses the preview's archive/snapshot guards and reduced-only
`EqgPbrLimits`. Every advertised archive member counts before assembly, including
unused members. Source-record limits apply after assembler allocations; report
projection rows/strings and final formatted JSON are bounded. Before projecting
material rows, the shared source budget charges every material/property record,
material name, shader, property name and kind-2 string, plus conservative projection
reserves. `max_total_name_bytes` independently bounds archive directory names and
source/report strings; repeated output strings incur repeated charges. Texture
decoding, retained image, geometry bridge, and GLB budgets perform no work in this API.
These logical bounds do not establish peak RAM limits or remove the existing PFS
inflation ceiling; trusted stable regular files are required. Inventory assembles
from the already-selected descriptor bytes and retained bounded archive reader,
using the generic loader's shared assembly logic. Each distinct exact model is
read once; its identity hashes the same bytes passed to the mesh parser. Streaming
model identity rows and their name/hash strings are charged before allocation;
the shared source budget still reserves the complete projection after assembly.
Final original-handle and reopened-path content checks still refuse observed
source changes; hostile concurrent mutate-and-restore detection is not guaranteed.

Archive, explicit descriptor, model content, implementation sources (including
module wiring), and dependency lock contribute to inventory identity. Paths and
timestamps do not. The implementation revision is a conservative source
fingerprint, not executable attestation: formatting production source may change
it. Selected source files are hashed verbatim, including comments and inline
`cfg(test)` code. The CLI example and separate `pbr_visual/tests.rs` source do not
contribute to the library report fingerprint. The standard Cargo package includes the lockfile and CLI
example; manually pruned source distributions are not a supported packaging path.
The existing v2 preview continues to refuse multiple distinct terrain meshes,
as its geometry bridge already required.
