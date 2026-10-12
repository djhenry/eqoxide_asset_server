# Static Collision Writer Implementation Plan

> **For agentic workers:** Use the repository's canonical agent-fleet process for implementation and independent acceptance. Implement the tasks below in this existing isolated worktree; do not merge without human review.

**Goal:** Produce bounded source-independent positions-only collision GLBs for subsequent collision inspection work.

**Architecture:** A normalized collision input has an explicit nonempty or known-empty state and reusable geometry with stable owner identities. A dedicated serializer writes only positions, indices and flat instances, reusing the visual coordinate/transform and atomic publication behavior. Existing source exporters, visual rendering and production collision remain unchanged.

**Tech Stack:** Rust, glam, serde_json, anyhow, tempfile and existing glTF test tooling; no new dependencies.

**Spec:** [Producer contract](static-collision-contract.md), implementing the producer portion of [shared collision design](shared-static-collision-design.md). Tracks [issue #72](https://github.com/djhenry/eqoxide_asset_server/issues/72).

## Global constraints

- Schema 1, reader contract 2, role `collision`, capability `static-collision-triangles-v1`; production reader advertisements remain unchanged.
- Coordinate profile `eqoxide-static-y-up-v1`, unit scale 1; server-to-asset `(x,y,z) -> (x,z,-y)`; CCW order preserved, no actor-origin offset.
- Only proper rotation, positive uniform scale and affine translation; reuse existing `1e-5` transform validation tolerances.
- Explicit known-empty JSON-only GLB; missing source data never becomes known-empty.
- Positions/index data and ownership are independent of visuals, native flags and material choices.
- Use the exact default limits and accounting table in the producer contract; checked arithmetic before copies/work.
- Keep native provenance and local environment details out of tracked/public artifacts. Preserve prior output on any failure.
- Author self-validation and separate acceptance must include meaningful fault discrimination. This writer has no live client consumer yet; serializer/importer checks are the observable boundary, not gameplay acceptance.

## Review focus

1. Geometry sharing must not undercount instance-expanded work or duplicate owner identities.
2. Tiny positive scales or huge translations can collapse otherwise valid triangles in f32 world coordinates and must fail.
3. Unused malformed resources must fail rather than hide behind scene selection.
4. Known-empty must satisfy glTF core array/buffer rules and remain distinguishable from missing data.
5. JSON/output exhaustion and invalid input must preserve an existing destination and its permissions.

## Task 1: Normalized input, validation and positions-only serializer

**Files:** Create `src/static_collision.rs`, `tests/static_collision.rs`; modify `src/lib.rs` to export the module. Make the existing `src/static_scene.rs` coordinate conversion helpers crate-visible, or extract them unchanged into `src/static_common.rs` if necessary for shared atomic publication. Do not route through a visual serializer or change its output.

**Interfaces:**

```rust
pub enum StaticCollisionScene {
    Available {
        meshes: Vec<StaticCollisionMesh>,
        instances: Vec<StaticCollisionInstance>,
    },
    AvailableEmpty,
}
pub struct StaticCollisionMesh {
    pub positions: Vec<[f32; 3]>,
    pub primitives: Vec<StaticCollisionPrimitive>,
}
pub struct StaticCollisionPrimitive { pub indices: Vec<u32> }
pub struct StaticCollisionInstance {
    pub owner_id: String,
    pub mesh_index: usize,
    pub matrix: [[f32; 4]; 4],
}
pub enum InspectionQueryPolicy { FrontFaces, BothFaces }
pub struct CollisionWriteLimits {
    pub max_glb_bytes: usize,
    pub max_json_bytes: usize,
    pub max_geometry_bytes: usize,
    pub max_resources: usize,
    pub max_vertices: usize,
    pub max_indices: usize,
    pub max_expanded_triangles: usize,
    pub max_world_vertices: usize,
}
pub fn write_static_collision(
    scene: &StaticCollisionScene,
    bake_revision: &str,
    policy: InspectionQueryPolicy,
    output: &std::path::Path,
    limits: &CollisionWriteLimits,
) -> anyhow::Result<crate::compatibility::ReaderRequirements>;
```

- [ ] Write a failing fixture test using the new interface before adding the module:

```rust
use eqoxide_asset_server::static_collision::*;
use glam::Mat4;
fn fixture() -> StaticCollisionScene {
    StaticCollisionScene::Available {
        meshes: vec![StaticCollisionMesh {
            positions: vec![[1., 2., 3.], [4., 2., 3.], [1., 6., 3.]],
            primitives: vec![StaticCollisionPrimitive { indices: vec![0, 1, 2] }],
        }],
        instances: vec![StaticCollisionInstance {
            owner_id: "authored-floor".into(), mesh_index: 0,
            matrix: Mat4::IDENTITY.to_cols_array_2d(),
        }],
    }
}
#[test]
fn position_only_output_preserves_orientation() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("collision.glb");
    let req = write_static_collision(&fixture(), &"a".repeat(64),
        InspectionQueryPolicy::FrontFaces, &out, &CollisionWriteLimits::default()).unwrap();
    assert_eq!(req.capabilities, ["static-collision-triangles-v1"]);
    let (doc, buffers, images) = gltf::import(&out).unwrap();
    assert!(images.is_empty());
    let primitive = doc.meshes().next().unwrap().primitives().next().unwrap();
    assert_eq!(primitive.attributes().count(), 1);
    let reader = primitive.reader(|b| Some(&buffers[b.index()]));
    let positions: Vec<_> = reader.read_positions().unwrap().collect();
    assert_eq!(positions, [[1.,3.,-2.], [4.,3.,-2.], [1.,3.,-6.]]);
    assert_eq!(reader.read_indices().unwrap().into_u32().collect::<Vec<_>>(), [0,1,2]);
    let n = (glam::Vec3::from_array(positions[1]) - glam::Vec3::from_array(positions[0]))
        .cross(glam::Vec3::from_array(positions[2]) - glam::Vec3::from_array(positions[0]));
    assert!(n.y > 0.);
}
```

- [ ] Run `cargo test --locked --offline --test static_collision position_only_output_preserves_orientation` with one local build job; record unresolved-module RED privately.
- [ ] Implement the types, exact input/state/ownership validation and checked counters. Charge aggregate geometry/resources before preparation, expanded counts before any world loop; validate all pools and ordered triangles using widened cross products, including both world frames.
- [ ] Serialize one POSITION pool per mesh and u32 indices per primitive with strict root/node records, proper converted matrices, bounded JSON output and exact GLB lengths/padding. Publish only after success using the shared atomic behavior.
- [ ] Run the same positive test; then extend it with two differently placed instances of one mesh, combined rotation/uniform scale/translation, and both policy encodings. Check metadata and byte-level accessor/view counts independently of glTF import.

## Task 2: Refusals, empty state and resource boundaries

**Files:** Continue `tests/static_collision.rs`; internal checked-counter tests may live in `src/static_collision.rs`.

- [ ] Add the known-empty fixture and assert core resource arrays are omitted and its total length ends after the JSON chunk. Read the application record and assert explicit `available-empty` and an empty selection. Independently validate official glTF schemas: the pinned `gltf-json` 1.4.1 deserializer incorrectly requires `Scene.nodes`, tracked in [issue #73](https://github.com/djhenry/eqoxide_asset_server/issues/73). Nonempty fixtures continue through the Rust importer; never emit an illegal empty core array to appease it.

```rust
#[test]
fn explicit_empty_has_no_binary_payload() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("empty.glb");
    write_static_collision(&StaticCollisionScene::AvailableEmpty, &"a".repeat(64),
        InspectionQueryPolicy::BothFaces, &out, &CollisionWriteLimits::default()).unwrap();
    let bytes = std::fs::read(out).unwrap();
    let json_len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    assert_eq!(bytes.len(), 20 + json_len);
    let doc: serde_json::Value = serde_json::from_slice(&bytes[20..]).unwrap();
    assert_eq!(doc["scene"], 0);
    assert_eq!(doc["scenes"], serde_json::json!([{}]));
    assert_eq!(doc["extras"]["eqoxideCollision"]["availability"], "available-empty");
    assert_eq!(doc["extras"]["eqoxideCollision"]["selectedNodes"], serde_json::json!([]));
    for resource in ["buffers", "bufferViews", "accessors", "meshes", "nodes"] {
        assert!(doc.get(resource).is_none());
    }
}
```

- [ ] Exercise nonempty state with no meshes/instances, invalid owner grammar/duplicate owners, invalid revision, out-of-range/partial indices, nonfinite unused vertices, zero-area unused triangles and bad mesh references; each must refuse.
- [ ] Exercise nonuniform/sheared/reflected/projective/zero-scale matrices, transformed overflow and f32 collapse from huge translation/tiny scale. Keep a small positive uniform-scale success case where the resulting triangle is nonzero.
- [ ] Lower every public limit below the minimal fixture charge, assert refusal and unchanged existing sentinel bytes/permissions. Exercise exact-boundary success for JSON/GLB and independent expanded-triangle/world-visit limits using shared instances. Internal helper tests check multiplication/addition overflow with `usize::MAX` without allocating enormous inputs.
- [ ] Run targeted tests, then the existing static visual tests and the whole writer/library suite. Record actual counts and any unrelated limitations; no broadening of claims from a passing serializer suite.

## Task 3: Author evidence, independent acceptance and human PR

**Files:** Add `examples/static_collision_fixture.rs` for a synthetic shared-mesh artifact and explicit empty artifact. Update only relevant contract/plan completion facts. Keep evidence outside tracked source.

- [ ] Generate artifacts through the example and independently inspect them with glTF tooling. Check owner/scene/header records and transformed coordinates; produce no native asset fixtures.
- [ ] Deliberately remove axis conversion, known-empty state metadata and an expanded-work guard one at a time. Run their discriminating tests expecting RED, restore exact bytes and run GREEN. Record hashes/commands privately.
- [ ] Mechanically verify all new prose/code references and privacy, then give a separate acceptance agent the exact diff. Ask it to refute coordinates, degeneracy/limits, empty legality, publication preservation and misleading readiness claims; require its own suite, mutation probes and artifact generation.
- [ ] Repair real findings, rerun relevant checks and obtain acceptance of the final content. Commit, push an isolated stacked PR based on `codex/shared-collision-design`, attach it to this chat and mark it ready for human review. Do not merge.

## Remaining inspection milestone units

This plan completes the producer subproject only. The approved milestone still needs a bounded role-specific CPU decoder sharing safe GLB/accessor helpers; a separately frozen and accepted segment-query implementation; hashed `scene.json` descriptor validation and snapshot identity binding; normalized source adapters with explicit coverage. Each receives its own issue and detailed plan before implementation. Only their combined end-to-end inspection acceptance completes the milestone; movement, volumes and dynamic gameplay policy remain a separate decision.
