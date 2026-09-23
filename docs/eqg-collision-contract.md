# EQG collision candidate contract, version 1

This is an opt-in staging artifact for asset-server issue #55. It is not a production zone asset and must not be published into an ordinary zone set. Existing clients are not required to understand its metadata and could silently apply legacy behavior.

The artifact is one GLB with a dedicated, identity-transform, flat collision node named `__collision__`. Its positions are baked world-space coordinates under the signed permutation `(x,y,z) -> (x,z,-y)` from source EQG coordinates. Triangle order is the source collision order `[0,2,1]`. Normals and render materials do not decide collision eligibility.

Root `extras.eqCollision` contains:

```json
{
  "version": 1,
  "coordinates": "eqg_gltf_y_up",
  "scope": "default_static_triangle_candidates",
  "nodes": [0]
}
```

The scope is intentionally narrow. Source triangles with `(flags & 1) == 0` participate irrespective of material validity or visibility. The flag mask `2` does not exclude a triangle from this default query. Other raw flags are not assigned new meanings. This contract does not establish actor-level runtime filtering, alternate query modes, collision hull selection, dynamic doors, skeletal geometry, regions, water, ladders, navigation, or alignment with server coordinates.

Terrain is emitted once at identity. Each supported static placement contributes once, with source rotation order, uniform positive scale, and translation applied before coordinate conversion. Version-2 record zero supplies terrain lighting rather than another actor; null placements and terrain references do not duplicate terrain. Source underground placements are retained. Degeneracy is tested after placement and coordinate conversion on the emitted f32 world positions, using a cross product computed in f64 and an exact zero comparison. Collapsed triangles are omitted and counted, including triangles collapsed by f32 rounding after a large translation. Transformed coordinates must remain finite. Reports count triangles after placement expansion: considered = excluded by mask 1 + emitted + degenerate. Invalid references, indices, transforms, unsupported skeletal inputs, or an empty result fail explicitly. Output replacement is atomic so a failed export preserves the previous artifact.

## Consumer requirements before integration

A consumer must recognize this version, scope, and coordinate convention explicitly; validate the referenced node and its triangle indices; and reject unknown or malformed metadata without falling back to rendered geometry. Dedicated collision geometry must be kept separate from visual meshes. The collision builder must use the explicit geometry alone, never append rendered object meshes or infer ladder volumes from their names. An explicitly empty collision source must never select legacy fallback.

Unmarked legacy assets retain their existing sentinel-plus-object behavior. A combined render/collision artifact and publication require a capability gate, so older clients cannot mistake unsupported metadata for ordinary geometry. One future artifact should bind visuals and collision atomically; this first collision-only export establishes and tests the geometry contract before that integration.

## Acceptance

Synthetic checks distinguish visible-passable from invisible-solid geometry, retain flag-2 triangles, verify reversed winding, and apply terrain/placements exactly once. Transform fixtures must include rotation plus translation and scale, not only identity. Failed and empty exports must preserve last-good output. Native-corpus diagnostics compare candidate counts and bounds, without distributing source assets. Independent review must mutate collision selection or placement handling and observe test failure.

Successful export means candidate geometry exists under the stated policy. It does not make the zone gameplay-ready.

## Implementation sequence

1. Establish failing synthetic selection, winding, placement, and atomic-output tests.
2. Implement the candidate builder, explicit CLI, and optional GLB root metadata.
3. Validate synthetic regressions and representative source exports independently.
4. Review and publish the opt-in exporter before implementing client consumption and publication gates.

## Running the staging exporter

```sh
eqoxide-assets export-eqg-collision --archive /path/to/crescent.eqg \
  --descriptor /path/to/crescent.zon --out /path/to/crescent-collision.glb
```

Use `--member` instead of `--descriptor` for a descriptor inside the archive. Textures are not required. Standard output is a JSON report; it contains placement-expanded counts grouped by raw flags and identifies skipped placements. Do not feed this file to an ordinary zone publisher or treat it as a visual-preview replacement.

Representative source validation produced:

| Zone | Candidates before degeneracy | Emitted triangles | Degenerate triangles | Object placements |
| --- | ---: | ---: | ---: | ---: |
| Crescent | 300,775 | 300,306 | 469 | 2,341 |
| Guild Hall | 40,431 | 40,431 | 0 | 86 |
| Anguish | 459,923 | 459,859 | 64 | 695 |

Crescent's pre-degeneracy total was independently calculated from source triangle flag groups and raw descriptor placements. All three output files were checked for metadata, flat identity-node structure, finite positions, valid indices, and report/accessor count agreement. These checks establish the staging contract, not playable collision behavior.
