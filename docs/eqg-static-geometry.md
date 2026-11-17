# EQG geometry bridge into the common static scene

`eqg::static_scene::build_static_scene` consumes an assembled `BinaryZoneScene` and caller-resolved visual inputs, then returns a `StaticScene` and an exclusion/association report. It works in memory: archive selection and extraction belong to the source assembler; material interpretation belongs to the caller; GLB serialization and final validation belong to `write_static_visual`.

The bridge is a geometry boundary. It does not resolve native shader names, interpret packed vertex colors, infer lighting, choose texture providers or publish asset packages. A successful conversion establishes the supported geometry and placement rules. It does not establish native material fidelity, collision, regions, server landmark alignment or gameplay readiness.

The raw all-ones material shader word is retained as unresolved string data in inspection reports, including its exact wire value. Visual conversion preserves refusal for all parsed materials, including unused records and caller-supplied bindings; it does not imply a default shader. A valid empty string at offset zero remains distinct. Collision extraction does not use shader state to select geometry. The source meaning remains tracked in [libeq #9](https://github.com/djhenry/libeq/issues/9).

## Explicit material decisions

`ResolvedZoneVisual` owns the global common materials and textures. Each `SourceMaterialKey` addresses a deduplicated source mesh and a material **table ordinal**. A material's separate metadata index or name is not that ordinal. Every material referenced by included visual geometry needs a binding to a global output material.

Each `MaterialBinding` declares `ResolvedColors::Unused` or a complete resolved RGBA pool for that source mesh, and declares whether secondary UVs are unused or required. Colors belong to that primitive; a neighboring uncolored primitive stays uncolored even when it shares the vertex pool. Required secondary UVs are unsupported by the first common profile. Explicitly unused secondary UV bits stay in the borrowed source records and need not be interpreted for sampling. The bridge does not synthesize white colors or an opaque material when a decision is missing.

Use `source_material::normalize_material` and `decode_static_texture` to prepare already-resolved material facts and texture samples. These helpers do not resolve native shader behavior. Effects requiring extra texture layers, RGB combining, lighting or animation need a further shared contract and an explicit resolver.

## Geometry and instances

Raw source positions and normals map into server geometry axes as `(a, b, c) -> (b, a, c)`. This reflection reverses handedness, so each raw triangle `[i0, i1, i2]` becomes `[i0, i2, i1]`. Normal components swap X/Y without an additional negation. The common writer subsequently applies its server-to-glTF mapping; callers must not pass geometry already converted by the legacy staging exporter.

A source placement uses `T * Rz(rotation[0]) * Ry(rotation[1]) * Rx(rotation[2]) * S`, with angles in radians and positive uniform scale. The bridge conjugates this transform by the source-to-server axis mapping once. Original descriptor model slots resolve through `model_slots` into deduplicated source meshes, then through the returned source-to-visual mapping. Repeated placements share one output mesh.

Exactly one terrain mesh is emitted at identity. Terrain references in the placement table are excluded and reported instead of instancing terrain repeatedly. Descriptor version 2's first placement is terrain-lighting metadata and is excluded with its opaque extension byte count reported. Version 1 does not unconditionally skip its first placement. Unassigned placements are also reported. All retained placement transform fields must be finite; positive scale is required for actual visual instances.

## Refusals and reporting

The nonvisual material sentinel `u32::MAX` is excluded and counted. Other out-of-range material references fail. Vertex references are checked even on sentinel triangles. Ordinary collision flag bits do not determine visual inclusion.

The first bridge refuses unsupported mesh/descriptor versions, model bones, unknown UV markers, opaque mesh/descriptor suffixes, unexplained extension data on visual placements, required secondary UVs, degenerate triangles and non-unit normals. A placed mesh with no included visual triangles fails. Source resources and metadata remain available to later component adapters through the unchanged source scene.

The result reports skipped placement roles, sentinel triangles, unused source meshes/bindings/resources and unconsumed region/light records. These diagnostics describe exclusions; they are not certificates that those components are implemented.

## Preserving unapplied descriptor-v2 lighting

`build_static_scene_with_unapplied_v2_lighting` is a separate opt-in, in-memory entry point. It returns `BridgedUnappliedLightingScene`, containing the ordinary geometry/report in `geometry` and borrowed `lighting::LightingDiagnostics` in `lighting`. The strict `build_static_scene` keeps its extension refusals and authored-color behavior.

The opt-in path requires descriptor v2, exactly one terrain mesh and record zero. Record zero supplies the terrain array regardless of its model reference and never creates an object instance. Terrain and each included nonterrain placement must have a nonempty, word-aligned array with exactly one packed word per vertex in that target's complete current source pool, including unreferenced vertices. Shared geometry does not merge the distinct per-placement arrays. Descriptor v1 remains available through the strict bridge and separate lighting diagnostics.

On success, every placement's complete extension bytes remain borrowed from the unchanged source, including automatically excluded unassigned and repeated-terrain records. Those excluded records need alignment and ordinary source validity, but need no matching target count. Read `outcome` and `consumer_scope` before interpreting resolved-reference fields: a repeated-terrain diagnostic can retain its mesh name, index and pool count while remaining `Unassigned` with `UnassignedTarget` scope. A non-null model slot resolving to a null mesh still refuses. Refusal returns an error without a partial report and leaves source records unchanged.

`MatchesCurrentPoolOnly` describes shape equality only. It establishes neither native consumer/LOD association, packed channel semantics nor source provenance. No packed words are applied to geometry or material factors. This entry point rejects all `ResolvedColors::Rgba` bindings, including unused bindings and empty vectors, and emits no primitive colors or `COLOR_0`. Caller-owned material and secondary-UV decisions otherwise follow the existing bridge rules.

Bridge payload, resource and transform-work limits pass before diagnostics allocate report arrays. The diagnostic then enforces its independent target, resource, logical-record, name/scan, input-byte and packed-word limits before allocating its report. All extension bytes count, including excluded records. One budget is not credit toward the other. The diagnostic exposes per-record bytes, identities and count outcomes, without aggregate usage counters. These are logical stage bounds, not peak-memory or retrospective archive-parser guarantees.

This API has no production or CLI route. Synthetic source-to-common-writer tests compare it with an otherwise identical extension-free strict input, check borrowed slice identities and unchanged source fields, reopen the GLB, and verify equal geometry/material output without `COLOR_0` or a lighting extension. This CPU boundary proof does not establish a native material bake, rendering equivalence or complete zone PBR readiness.

## Resource bounds and verification

Bridge limits apply before geometry copying, grouping and transformed-world checks. Defaults are 128 MiB each for logical input and conservative output payloads, five million source vertices, fifteen million source indices, 100,000 aggregate resource/record entries and fifty million transformed vertex visits. Input includes source records and caller-owned resolved resources. Source counts include unused meshes and sentinel triangles; output estimates include resolved resources and color pools even when unused. Geometry sharing means output vertex/index counts do not exceed the checked source counts. The resource count combines mesh/slot/name/placement/region/light entries, resolved materials/textures/bindings, source material entries and their properties.

Every binding contributes 32 logical input bytes for its source key, output material reference and color/UV-use decisions, including unused bindings. RGBA values are counted separately once. This is a conservative accounting policy, not a serialized binding format or a measure of map allocator overhead.

These bounds cover this conversion stage; they do not bound total process memory, source archive extraction or the writer's later serialization allocations. The common writer still performs full PNG pixel validation and the final scene validation before producing a GLB. Consumers independently enforce their own resource limits, including triangle-instance render work; passing the bridge limits alone does not establish that every consumer can load the output.

Synthetic fixtures can test this boundary without native asset input: asymmetric geometry, shared rotated/scaled instances, material-table indirection, per-primitive color choices and explicit unsupported-feature failures. The output must pass the common writer and reader. CPU, GPU and isolated preview checks validate the converted synthetic result; real source material/zone equivalence remains a separate gate.

To produce an artifact from synthetic raw records:

```sh
cargo run --example eqg_geometry_fixture -- eqg-geometry.glb
```

The example supplies authored materials, one terrain mesh and three rotated/scaled instances of one object mesh, using duplicate and nonidentity model slots. It also includes repeated-terrain and unassigned records to exercise exclusion reporting. It reopens the GLB and verifies shared geometry and winding. Its supplied materials are test inputs, not native shader translations.
