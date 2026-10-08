# Common static GLB writer implementation plan

> **For agentic workers:** Use the agent-fleet implementation and independent review process. Steps use checkboxes for tracking.

**Goal:** Provide an enforced, source-independent static visual artifact boundary usable by both importers and authored enhancements.

**Architecture:** A typed scene API accepts resolved geometry in server geometry coordinates and resolved glTF material semantics. A validator rejects incomplete or unsupported input before a shared GLB serialization core writes a temporary artifact and atomically replaces the destination. Existing legacy and EQG staging writers keep their current semantics.

**Tech stack:** Rust, glam, serde_json, gltf, image, tempfile.

**Spec:** [Approved unified baked asset design](https://github.com/djhenry/eqoxide/blob/c6697d39/docs/unified-baked-assets-design.md). Tracks #59, #51 and eqoxide#1126.

## Global constraints

- Input positions and instance transforms use server geometry axes; normal vectors and CCW triangle winding refer to those same axes. No actor-origin offset.
- The fixed output mapping is `(sx, sz, -sy)` with unit scale 1; this is an authoring convention, not a calibrated physical measurement. It has positive determinant, so this conversion does not reverse winding.
- Meshes remain reusable across instances. First-slice instance transforms support translation, proper rotation and positive uniform scale only. Unsupported shear, reflection, nonuniform scale or projective matrices fail explicitly.
- Material inputs are already normalized: preserve RGBA factors with and without a texture, MASK cutoff as a float, BLEND, and double-sided choice. Do not apply source-specific alpha overrides or native shader dispatch.
- Preserve full per-primitive RGBA vertex colors. Additive behavior, texture sequences, secondary UVs, skins, animations, collision and regions remain outside this static baseline and need separate capability contracts.
- The artifact header identifies schema 1, visual role, fixed coordinate profile, unit scale, caller-supplied immutable bake revision and reader requirements. Use reader 2 and `static-visual-v1`; add `vertex-rgba-v1` only when present. This reserves producer requirements without advertising client support or publishing packages.
- Production entry points and existing preview/collision GLBs retain their behavior. No merge or deployment without human review. One low-priority local build at a time.

## Review focus

- An asymmetric translated/rotated/scaled instance must decode to the approved coordinate mapping, including transformed normals and unchanged CCW winding.
- A colored MASK primitive sharing vertices/materials with an uncolored primitive must not leak COLOR_0 into its neighbor.
- Textured material tint and nondefault base alpha/cutoff must survive serialization exactly as normalized inputs, rather than inheriting legacy replacement rules.
- NaN/overflow, malformed topology and references, unsupported matrices, and invalid texture/revision input must preserve existing destination bytes.
- Existing legacy and EQG source staging output must not acquire the new header or new material/color semantics.

## Task 1: Typed scene, validation and fixed coordinate profile

**Owner:** implementation worker. Create `src/static_scene.rs`; expose it from `src/lib.rs`.

Public structures describe meshes (positions, normals, primary UVs, primitives), materials (RGBA factor, optional texture index, opaque/mask/blend semantics, double-sided choice), PNG textures, and instances (mesh index, column-major server-axis matrix). Primitives carry triangle indices, material index and optional full-pool RGBA colors. A scene has no source-family field or source-dependent dispatch.

Public entry point:

```rust
pub fn write_static_visual(scene: &StaticScene, bake_revision: &str, output: &std::path::Path)
    -> anyhow::Result<crate::compatibility::ReaderRequirements>;
```

- [x] Add focused failing tests for asymmetric coordinate and transform conversion, vertex color isolation, material factor preservation, and malformed scene rejection with last-good output unchanged.
- [x] Validate before output creation: nonempty renderable scene; finite equal-length vertex attributes; nonzero unit normals; valid triangle/material/texture/node references; index counts divisible by three; nonempty referenced meshes/primitives; normalized material/color factors and cutoffs; valid PNG bytes with bounded decoding; positive proper uniform affine transforms; finite transformed world bounds; lowercase 64-digit bake revision.
- [x] Apply the fixed signed permutation to positions/normals and conjugate each instance matrix. Preserve instance mesh references and input triangle winding.
- [x] Derive the explicit required capability set from scene features. No caller may remove required capabilities.

## Task 2: Shared serialization and atomic artifact writer

**Owner:** same worker. Modify `src/convert/mod.rs` only at the static instanced writer boundary. Create `tests/static_scene.rs` for public API conformance.

- [x] Extract a private serialization core that accepts prepared material JSON and typed RGBA bindings. Build normalized JSON in the typed scene layer. Avoid arbitrary material JSON in the public API.
- [x] Keep existing wrappers converting alpha bytes into white RGBA using the original byte encoding and material helper; prove old empty/alpha paths retain their prior behavior. Do not change `material_to_gltf` globally.
- [x] Serialize generic full RGBA for normalized artifacts and attach `extras.eqoxideAsset` with schema, visual role, fixed coordinate profile/unit scale, bake revision and reader requirements. Use standard glTF alpha/color fields.
- [x] Write via a temporary file in the destination directory and persist only after successful serialization. Reopen the actual GLB in tests and assert header, geometry, transforms, materials and color values.
- [x] Test equivalent authored scenes without provenance-specific behavior and test a legacy-style fixture enhanced with nonwhite RGB and a nondefault mask cutoff.

## Task 3: Acceptance and review request

**Owner:** orchestrator plus independent reviewer.

- [x] Provide a small example that invokes the real public writer and produces an inspectable GLB without native assets.
- [x] Run new conformance tests, existing color/writer and EQG preview tests, then the server suite serially. Demonstrate a meaningful mapping/material/color mutation failing and restored tests passing.
- [x] Independently review the code and public references, repeat relevant tests, and inspect the generated example artifact through the GLB reader. This writer-only milestone has no gameplay behavior to validate; actual artifact decode is its observable boundary.
- [x] Document limits and reserved requirements. Existing clients do not implement reader 2. Do not migrate a production store or change support advertisements.
- [x] Publish a PR linked to #59; leave it ready for human review and do not merge.

## Next units

Adapt WLD and EQG importers into this normalized scene boundary, resolving material approximations explicitly. Then define normalized static collision query semantics and component association, implement the generic client decoder, and independently validate server landmarks and the declared gameplay scope before publication. Static visual artifact inspection alone does not satisfy those gates.

## Acceptance evidence

Author and independent all-target suites each passed 145 tests with 35 ignored. The independent reviewer repeated four production mutations (axis mapping, material factors, RGBA serialization and converted world overflow guard); each caused its targeted regression to fail, and restored conformance tests passed. Independent binary decoding of the actual authored GLB confirmed one mesh shared by two instances, full per-primitive RGBA isolation, textured tint/base alpha/nondefault cutoff, the artifact header, and world bounds `min [-3, 2, -13]`, `max [16.44179, 6.5, 7]`. Restored output was byte-identical. Acceptance applies to the static writer boundary, not source conversion or game runtime.

Human review: [PR #60](https://github.com/djhenry/eqoxide_asset_server/pull/60). No merge or deployment performed.
