# Static collision geometry contract, version 1

This freezes the producer profile for the approved inspection milestone in [the shared collision design](shared-static-collision-design.md). The writer is tracked in [issue #72](https://github.com/djhenry/eqoxide_asset_server/issues/72). Decoding, segment queries, package pairing and source coverage require their own implementation and acceptance. Nothing in this profile enables movement or navigation.

## Inputs and coordinates

The normalized input is either `StaticCollisionScene::Available { meshes, instances }` or the explicit `StaticCollisionScene::AvailableEmpty`. Missing/unextracted/unsupported data cannot be passed as known-empty. The available variant requires at least one mesh and instance. Each mesh has a nonempty finite f32 position pool and nonempty indexed triangle primitives; indices are u32 and each primitive's index count is a positive multiple of three. All declared meshes and primitives are validated, including unused ones.

Positions and column-major instance matrices use server geometry axes. Counterclockwise index order defines the geometric normal. The shared asset mapping is `(x,y,z) -> (x,z,-y)` with matrix conjugation by that same proper rotation. No winding reversal or actor-origin height adjustment occurs at this boundary. Source adapters must resolve their own reflections and eligibility first.

Instances allow finite affine translation, proper rotation and positive uniform scale. The structural affine row is exactly `(0,0,0,1)`; normalized axis orthogonality and relative scale use the existing visual writer's tolerance of `1e-5`. Both source and serialized asset transforms must produce finite f32 world positions for each indexed triangle reference. Unreferenced vertices are validated locally but are not evaluated per instance. Cross products widened to f64 must have nonzero squared length for every indexed triangle in local space and both transformed frames; rounding that collapses a triangle is a refusal. Near-degenerate but nonzero triangles remain representable; this does not promise robust gameplay contacts.

Each instance has a unique `owner_id`, a stable opaque importer/authored identity independent of node order. It uses the existing capability-token grammar: 1–128 ASCII lowercase letters, digits, `.`, `_` or `-`, beginning with a lowercase letter or digit. The writer neither invents identities nor infers ownership from names. Distinct owner identities can share a mesh. Local node/mesh/primitive/triangle indices are references within a component, not stable enhancement identities.

## GLB and metadata

Nonempty output is GLB 2 with exactly one JSON and one BIN chunk. Geometry is embedded in buffer 0; buffer views have four-byte alignment and no stride. Each mesh has one shared FLOAT VEC3 POSITION accessor and one UNSIGNED_INT SCALAR index accessor per primitive. Primitives explicitly use TRIANGLES (mode 4). POSITION accessors have exact stored-coordinate min/max. No normals, UVs, materials, images, textures, skins, morphs, animations or external resources are emitted. Index order is preserved.

The document has one flat scene selecting every node exactly once. Nodes have only mesh, matrix and the application owner record. Root metadata is:

```json
{
  "eqoxideAsset": {
    "schemaVersion": 1,
    "role": "collision",
    "coordinateProfile": "eqoxide-static-y-up-v1",
    "unitScale": 1,
    "bakeRevision": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    "requirements": {
      "reader_version": 2,
      "capabilities": ["static-collision-triangles-v1"]
    }
  },
  "eqoxideCollision": {
    "version": 1,
    "scope": "declared-static-triangle-candidates-v1",
    "availability": "available",
    "selectedNodes": [0],
    "queryPolicy": "inspection-segment-front-v1"
  }
}
```

Each node's extras contains exactly `{"eqoxideCollisionInstance":{"ownerId":"authored-floor"}}`. Collision application records have no optional fields or hidden behavioral keys. Bake revision is a precomputed canonical input/policy identity of 64 lowercase hexadecimal digits, not this GLB's hash. The header requirements describe geometry decoding only. Do not advertise `static-segment-query-v1` until a query implementation passes its separate acceptance.

The writer accepts `InspectionQueryPolicy::FrontFaces` or `BothFaces`, encoded as `inspection-segment-front-v1` or `inspection-segment-both-v1`. These identifiers reserve distinct segment inspection semantics over the encoded dataset; they do not certify native movement or LOS. A reader without a recognized, implemented operation must report unsupported. Query arithmetic, endpoint/edge rules, work limits and identity binding must be frozen and tested before the operation is shipped, as required by the design.

Known-empty output has `availability: "available-empty"`, `selectedNodes: []`, the same header and explicit query policy. It is a JSON-only GLB with `scene: 0` and `scenes: [{}]`. Omit meshes, nodes, accessors, bufferViews and buffers entirely; there is no BIN chunk or zero-length core buffer. It contains no dummy triangle. This certifies only an empty declared candidate dataset, not the absence of volumes or dynamic actors.

The [official scene schema](https://github.com/KhronosGroup/glTF/blob/main/specification/2.0/schema/scene.schema.json) permits an omitted node list but requires a nonempty array if present. The pinned `gltf-json` 1.4.1 importer rejects that legal omission ([tooling issue #73](https://github.com/djhenry/eqoxide_asset_server/issues/73)). Validate empty artifacts against core schema and explicit container/profile rules; do not emit an invalid empty core array to satisfy the importer. The dedicated collision decoder must support the legal representation.

Fixed numeric header values denote exact mathematical integers: equivalent decimal or exponent spellings are legal when their original token evaluates exactly to the expected value. A future decoder must not accept a rounded binary64 alias or compare serde JSON number representations. Resource indices/counts/offsets use unsigned integer tokens. Serialization emits ordinary integer tokens, except finite f32 geometry and matrices.

## Bounds and publication

`CollisionWriteLimits::default()` uses the limits below. Callers may lower them for bounded tools or tests; raising them is an explicit caller choice and does not change wire semantics. All counters use checked arithmetic and are charged before corresponding copies, scans or output allocation.

| Limit | Default | Accounting |
|---|---:|---|
| `max_glb_bytes` | 134217728 | Header, aligned JSON, BIN headers and aligned bytes |
| `max_json_bytes` | 8388608 | JSON bytes including padding; bounded serialization |
| `max_geometry_bytes` | 134217728 | `12 * positions + 4 * indices`, all meshes |
| `max_resources` | 100000 | Each collection; aggregate primitives, buffer views and accessors |
| `max_vertices` | 5000000 | Position entries across all meshes |
| `max_indices` | 15000000 | Index entries across all primitives |
| `max_expanded_triangles` | 5000000 | Indexed triangles summed over instances |
| `max_world_vertices` | 50000000 | Paired source/asset evaluation visits: three per instanced triangle |

World visits count index references rather than unique vertices. Unused vertices are still finite-validated and included in accessor min/max but never establish collision surface bounds. Collection/geometry counters do not replace expanded-instance limits. The padded JSON serializer allowance is the smaller of its own limit and the remaining total GLB budget after framing and aligned binary payload; enforce it while writing, before growing the encoded buffer. The limits bound logical payload and work, not allocator overhead or total process RSS. No spatial grid is constructed by this writer.

Validate fully before atomically replacing the destination through a temporary file in its directory. On Unix the published file is mode 0644; sync the staged file before replacement. Failed validation or serialization preserves the prior file and its permissions. This provides atomic file replacement, not a manifest transaction or crash-durable parent-directory synchronization. Package publication requires the separate pairing work.
