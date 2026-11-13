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
invalid defaults, out-of-range override indices, invalid overrides and PBR
`BLEND` materials before staging output, including unused material entries.
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
