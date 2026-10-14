# Binary EQG Common Collision Adapter Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox syntax for tracking.

**Goal:** Convert explicitly owned binary-zone collision candidates into the existing source-independent positions-only input, with bounded work and honest extraction diagnostics.

**Architecture:** A pure in-memory adapter borrows `BinaryZoneScene`, explicit owner bindings and independent limits. It reuses the common source coordinate mechanics and emits shared indexed meshes plus owner instances; the common writer remains the artifact validator. Caller-selected exclusions and unconsumed source features are reported, never upgraded to world completeness.

**Tech Stack:** Existing Rust, glam, libeq source records, serde diagnostics and common collision writer; no new dependency.

**Spec:** [Shared collision design](shared-static-collision-design.md), [geometry contract](static-collision-contract.md), and [issue #77](https://github.com/djhenry/eqoxide_asset_server/issues/77).

## Global Constraints

- Emit geometry capability `static-collision-triangles-v1`; no query operation advertisement or gameplay integration.
- Common geometry uses `declared-static-triangle-candidates-v1`, reader contract 2 and `(sx,sy,sz) -> (sx,sz,-sy)` at the writer boundary.
- Source to server swaps X/Y and reverses triangle order once; placement is the existing `T*Rz*Ry*Rx*S` conjugation. No altitude exclusion or actor height offset.
- Terrain appears once at identity; descriptor-v2 placement 0 is lighting metadata, unassigned and repeated-terrain placements are accounted-for source exclusions.
- Every other actual placement has an explicit include owner ID or caller-declared exclusion. Never derive stable identity from a source ordinal/name.
- Unknown required geometry/placement behavior and admitted local/world collapse refuse; no silent degenerate omission. The common writer's widened f32-world validation stays unchanged.
- Limits bound logical payload/work, not total memory. The input is already assembled; archive ingestion is outside this API's bounds.
- No native assets, proprietary source citations or local machine details in tracked/public text. Human review precedes merge.

## Review Focus

- A source face with no visual material can be an eligible collision face; malformed indices are rejected even on excluded/unused faces.
- Missing/extraneous owner bindings cannot silently remove an owner or fabricate known-empty; included zero-candidate owners still have validated identity/placement and explicit reports.
- Per-instance f32 placement can collapse one shared face; refusal must preserve the other source records instead of filtering the shared mesh globally.
- Preflight must cover unused source collections, material/property metadata, strings/suffixes, report copies and distinct flag groups before their scans/allocations.
- Declared exclusion evidence is caller input, not independently verified static/dynamic coverage; known-empty means empty selected candidates only.

---

### Task 1: Pure bounded source adapter and discriminating tests

**Files:** Create `src/eqg/static_collision.rs` and `tests/eqg_static_collision.rs`; export the module in `src/eqg/mod.rs`. Permit narrowly scoped crate-private reuse of triangle validation in `src/static_collision.rs` and coordinate helpers in `src/eqg/static_scene.rs` if needed, preserving their behavior.

**Interfaces:**

```rust
pub struct CollisionOwnerBindings {
    pub terrain_owner_id: String,
    pub placements: std::collections::BTreeMap<usize, PlacementOwner>,
}
pub enum PlacementOwner {
    Include { owner_id: String },
    Exclude { reason: String },
}
pub struct BridgedStaticCollision {
    pub scene: crate::static_collision::StaticCollisionScene,
    pub report: CollisionBridgeReport,
}
pub fn build_static_collision(
    source: &super::BinaryZoneScene,
    owners: &CollisionOwnerBindings,
    limits: &CollisionBridgeLimits,
) -> anyhow::Result<BridgedStaticCollision>;
```

`CollisionBridgeLimits` uses checked usize counters: input/output logical bytes (128MiB each), vertices (5,000,000), indices (15,000,000), resources (100,000), expanded eligible triangles (5,000,000), and world vertices (50,000,000). Count three paired server/asset evaluations per expanded triangle, matching common writer terminology. Distinct flag/report entries and string copies consume resource/output limits before insertion/copy.

Bound diagnostic work independently of eligible geometry: build a per-source-mesh flag histogram once, charging each new `(mesh, flag)` entry to resource and `max_output_bytes` limits. The latter conservatively charges all adapter-created logical arrays/map records/string copies, including temporary histograms and multiplicity tables; do not reclaim the counter when a temporary is dropped. Count included owner multiplicities per mesh once; checked-multiply each histogram group by that multiplicity and merge it into global expanded counts once per source group. Do not rescan excluded faces or flag groups for every owner. This keeps many zero-candidate instances from bypassing work bounds through report updates.

Owner IDs use the existing common writer token grammar and must be unique across every include binding, including zero-candidate owners. Exclusion reasons use that bounded token grammar. Bindings for lighting/unassigned/repeated-terrain records or absent placements are extraneous and refuse. Exactly one distinct terrain mesh is required. Included placement extensions and descriptor/mesh suffixes refuse; excluded records' bounded extension lengths are reported without interpreting behavior. Descriptor-v1 extension words remain invalid; incomplete words always refuse.

Structural validation covers versions, model/name slot agreement, valid string-offset termination, null and out-of-range references, finite positions/placement fields, indices on all source triangles, and supported non-skeletal geometry. It deliberately does not validate visual materials, normals, UV/color/lighting values for collision. Account for those collections' logical size before ignoring their semantics. Included scales/matrices must satisfy the existing common positive uniform-transform validator even if eligibility produces zero triangles.

Required string-offset termination applies to interpreted descriptor model, placement, region and light names. Visual material/property name, shader and value offsets, and mesh visual string-table contents, are uninterpreted collision metadata; account for their storage without imposing visual reference validity.

Matching null model-table slots are legal. Validate all records' string/finite/framing and model-slot bounds first, then classify v2 lighting metadata and unassigned records before dereferencing any geometry target. Actual placements must resolve a non-null target even when the caller excludes that owner. Repeated terrain is classified after resolution. Thus a legitimate v2 lighting record targeting a null slot remains metadata, while a v1 first actual placement targeting it refuses.

The report includes source-to-collision mesh association, source owner handles paired with supplied include IDs or exclusion reasons, explicit zero-candidate owners, automatic source exclusions, all-source flag counts versus included-placement-expanded counts, work counts, and unconsumed region/light counts. It explicitly disclaims verified ownership/world completeness. Source references are association handles only. Keep raw source records borrowed and unchanged.

- [ ] Write synthetic source constructors with valid model/string tables and asymmetric indexed points. Write public API tests asserting the desired owner/mesh/winding/placement and refusal behavior.
- [ ] Run the focused suite with a stub returning an error; record assertion failure for valid conversion before implementing conversion.
- [ ] Preflight all borrowed collections and metadata using checked arithmetic before scanning payload or allocating output/report collections. Validate structure separately from visual eligibility.
- [ ] Classify placements and require exact owner-binding membership before building output. Calculate bounded selected/expanded work before world evaluation.
- [ ] Filter only by `flags & 1 == 0`, retain complete flag values in diagnostics, normalize indexed positions and reverse order once. Share reusable meshes; omit zero-candidate geometry with explicit owner reports.
- [ ] Validate selected local and paired f32 server/asset world triangles using the common widened predicate. Refuse overflow/collapse without skipping a face. Return explicit `AvailableEmpty` only after all selected extraction obligations and source structure pass.
- [ ] Run tests for noncommuting rotations/scale/translation, slot indirection/nulls, v1 first placement versus v2 metadata, underground placements, shared geometry, invisible solids, flag1/flag2/unknown flags, irrelevant visual attributes, all invalid references, missing/extraneous/duplicate/bad owners, unknown suffix/extensions, local/world collapse, empty versus refused, every preflight/output/world budget and overflow.
- [ ] Verify mutations of axis swap, winding, face eligibility, owner membership and budget charging fail assertions, then restore exact source and rerun the suite.

A minimum valid-conversion assertion should inspect normalized input directly before any GLB parser:

```rust
let bridged = build_static_collision(&source, &owners, &limits).unwrap();
let StaticCollisionScene::Available { meshes, instances } = bridged.scene else {
    panic!("eligible source is not known-empty");
};
assert_eq!(meshes[0].positions[0], [2.0, 1.0, 3.0]);
assert_eq!(meshes[0].primitives[0].indices, [0, 2, 1]);
assert_eq!(instances[0].owner_id, "terrain-owner");
assert_eq!(instances[0].matrix, glam::Mat4::IDENTITY.to_cols_array_2d());
```

### Task 2: Observable common artifact probe and independent acceptance

**Files:** Create `examples/eqg_collision_fixture.rs` and `docs/eqg-static-collision.md`; finish the focused tests without changing production dispatch, loader or descriptor schema.

**Interfaces:** The fixture constructs authored source records and explicit identities, calls the adapter and actual `write_static_collision`, then exposes ordinary common collision bytes. The existing client `static_collision_probe` is the consumer; package probing additionally uses an actual producer descriptor/manifest fixture rather than relabeling an unpaired GLB.

- [ ] Add a bounded synthetic fixture example with nonempty/asymmetric instanced and known-empty modes, fixed authored bake revision, caller-selected output and explicit front/both reserved metadata. No source discovery or native file reads.
- [ ] Assert common writer header, geometry-only capabilities, owner/node records, no visual materials/attributes and legal JSON-only empty form. Writer errors must leave previous output intact.
- [ ] Build/run the actual example and actual client decoder against both emitted artifacts. Record independently checked counts, indexed world bounds, owner IDs and declared policy. A successful decoder is CPU format acceptance, not a segment query or gameplay result.
- [ ] Run the ordinary server suite, named ignored tests separately only where applicable, and the focused suite after restoration. Record any unrelated failure without relabeling it as a pass.
- [ ] Document API limits, source coordinate/winding mechanics, conservative refusals, supplied-identity/coverage limitations, empty semantics and bounded in-memory scope.
- [ ] Complete a scoped public privacy scan, formatting and diff checks. Commit exact author-validated bytes before independent acceptance.
- [ ] A separate agent reviews and tests the committed source with its own adversarial cases, actual writer-to-consumer probe and meaningful mutations. Mechanical doc/subject/body review remains separate.
- [ ] Publish a ready PR only after acceptance, leaving merge to human review. No query operation, source-corpus completeness or online publication claim.

## Verification ceiling

The deliverable is CPU source normalization through the real common writer and decoder. Representative real-source coverage, stable importer registries, dynamic exclusions, native query semantics, server landmarks, online transport and gameplay remain separate acceptance units. The numerical backend choice in client PR #1166 does not block this source adapter.
