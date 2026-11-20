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
`Opaque_MaxWater.fx`. Each has a mandatory exact typed property set. Missing,
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
generated decisions, lighting/geometry rows, and duplicated resource-use projections.
The JSON report owns bounded raw-word projections, resource hashes/uses, lighting
record hashes/counts, geometry accounting, actual scalar choices, and omissions.
Raw nonfinite scalar bits serialize as integers. The CLI stages the real common
writer output through an outer closed temporary pathname, checks encoded GLB and
complete report sizes, reopens and syncs the resulting file, and only then
persists it to the destination. Every precommit refusal preserves an existing
output. An stdout failure after persistence cannot roll back that artifact.
