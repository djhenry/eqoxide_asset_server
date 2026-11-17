# Static scalar PBR materials

The opt-in `static_scene::write_static_visual_with_material_policy` writer emits
ordinary glTF metallic/roughness factors and explicit `KHR_materials_unlit`
materials. The scene, geometry, base colors, textures, coverage and instance
transforms use the [static visual contract](static-visual-contract.md).
The existing `write_static_visual` API remains the compatibility writer with its
original requirements and scalar values. Its inspection appearance is a consumer
choice; old files do not declare explicit unlit intent.

`StaticMaterialPolicy::default()` chooses `Pbr { metallic: 0, roughness: 1 }` for
all materials: a documented rough nonmetal starting point. A scene-local
`overrides` map replaces that choice by index in `StaticScene::materials`.
`StaticSurfaceModel::Unlit` is an explicit authored choice. These choices do not
infer native shader behavior or claim native visual parity. Uncertain materials
can use the documented default and later receive edited overrides. Indices are
local to the resolved scene, not persistent identities across reimports.

Metallic and roughness must be finite values in `[0, 1]`. The writer refuses
invalid defaults, out-of-range override indices and invalid overrides before
staging output, including unused material entries. The historical scalar profile
also refuses lit PBR `BLEND` materials.
Defaults are validated even if every material is overridden. Explicit unlit
and legacy `BLEND` retain their existing coverage semantics. Neither policy
changes RGBA multiplication, texture sample alpha or opacity normalization.
The returned `StaticVisualPolicyReport` contains the exact reader requirements
and each material's resolved surface and `Default` or `Override` provenance.

Every artifact written through this policy requires reader 2 and both
`static-visual-v1` and `pbr-metallic-roughness-factors-v1`, even when all factors
are `(0, 1)` or all materials are unlit. `vertex-rgba-v1` is additionally required
when vertex colors are present. This atomic profile covers scalar PBR and
explicit unlit; there is no separate unlit capability or custom visual extension.

PBR materials serialize scalar `metallicFactor` and `roughnessFactor` in core
`pbrMetallicRoughness`. Explicit unlit serializes
`extensions.KHR_materials_unlit: {}` and declares `KHR_materials_unlit` in both
root `extensionsUsed` and `extensionsRequired`. Lit-only artifacts omit those
root declarations. The profile does not add metallic/roughness maps, normal,
occlusion or emissive maps, authored light extensions, environment maps or
source-family dispatch. Production reader advertisements and package activation
remain unchanged; isolated inspector support is a separate consumer boundary.

Generate the native-free fixture with:

```sh
cargo run --locked --offline --example static_pbr_fixture -- pbr.glb
```

It contains five equally colored domes facing output +Z, centered at output
`x = -6, -3, 0, 3, 6`, `y = 0`, `z = 0`. Left to right they use default matte
dielectric `(0, 1)`, smooth dielectric `(0, 0.08)`, rough metal `(1, 0.8)`,
smooth metal `(1, 0.08)`, and explicit unlit. Each dome has radius 1.15 and
varying unit normals. Two additional instances share the matte mesh at
`x = -1.5, 1.5`, `y = -3`, with uniform scale 0.65 and output Y rotations
`-0.6, 0.6` radians. Five mesh definitions serve seven nodes. There are no native
assets, textures, vertex colors or authored lights. The example reopens the GLB's
standard JSON and prints requirements, material factors, declarations and policy
provenance. Its bake revision includes the example source and policy identifier.
The older visual and source-material fixtures are retained independently.

## Explicit HDR transparency profile

Call `write_static_visual_with_surface_profile` with
`StaticMaterialProfile::ScalarPbrHdrBlendV1` to admit lit PBR `BLEND`. The existing
`write_static_visual_with_material_policy` entry point selects `ScalarPbrV1` and
keeps its lit-BLEND refusal. Both paths validate every material before staging
output and preserve the previous file if validation fails.

The new profile always adds `pbr-metallic-roughness-blend-v1` to the reader
requirements, including for opaque-only or all-unlit scenes. It also requires
`pbr-metallic-roughness-factors-v1`, `static-visual-v1` and, when present,
`vertex-rgba-v1`. It keeps reader version 2. These requirements identify a
consumer's rendering contract; they do not introduce a custom material extension.
The materials remain editable core glTF PBR plus explicit `KHR_materials_unlit`.

The writer retains straight base-color RGBA, texture pixels, vertex colors,
geometry and transforms. A supporting inspector composites in a linear HDR
intermediate, then applies shared exposure and display mapping once. Its finite
half-float range, sorting and frame-memory limits are consumer policies. Ordinary
glTF viewers can display and edit the materials using their own lighting and
exposure; identical pixels across viewers are not promised. Selecting the HDR
profile does not provide animated textures, animated geometry, refraction or
native effect reproduction, and does not activate production package support.

A caller deriving `bake_revision` must include the selected profile and resolved
material policy in its identity inputs, along with the scene inputs. Switching
profiles changes reader requirements even when the standard material fields are
identical. The writer validates the supplied revision's syntax; it cannot verify
that the caller hashed all inputs.

Generate the authored transparency fixture with:

```sh
cargo run --locked --offline --example static_hdr_fixture -- hdr.glb
```

It contains five quad meshes and six instances: an opaque backdrop, overlapping
red and blue lit layers, a green mask above its vertex-alpha cutoff, an explicit
unlit gold layer, and a smaller rotated instance sharing the red mesh. A white
RGBA texture exercises the texture path. Its revision includes the generator
source, profile identifier and serialized material policy. This fixture uses no
native assets.

Optional `images` and `textures` arrays are omitted when empty, as required by
glTF. Textured outputs retain both arrays and their references. Regenerating an
older untextured fixture therefore changes its GLB bytes without changing its
geometry or materials.
