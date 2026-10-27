# Binary EQG collision bridge into the common static scene

`eqg::static_collision::build_static_collision` borrows an assembled `BinaryZoneScene`, caller-supplied `CollisionOwnerBindings` and independent limits. It returns a positions-only `StaticCollisionScene` and extraction diagnostics for the [common collision writer](static-collision-contract.md). This implements the bounded source adapter in [issue #77](https://github.com/djhenry/eqoxide_asset_server/issues/77) and follows the [implementation plan](eqg-common-collision-adapter-plan.md).

The adapter operates in memory. It does not discover archives, publish manifests, implement segment queries or certify native movement, server alignment or complete world coverage. Archive ingestion has its own costs and validation; passing these limits does not bound prior source allocation or total process memory. The existing source assembler also validates finite visual attributes, so this pure API's acceptance of irrelevant visual data does not expand the assembler's acceptance.

## Geometry and supplied ownership

Raw source positions map to server geometry axes as `(a,b,c) -> (b,a,c)`. This reflection reverses each eligible triangle from `[i0,i1,i2]` to `[i0,i2,i1]`. Placement uses `T * Rz(rotation[0]) * Ry(rotation[1]) * Rx(rotation[2]) * S` with radians and positive uniform scale, conjugated by the same X/Y swap once. Terrain contributes once at identity. Repeated included placements share one model-local indexed mesh. Underground placements are retained. The common writer subsequently performs the server-to-asset mapping; old staging collision GLBs use a different coordinate convention and cannot be relabeled as common components.

Default candidate membership is exactly `(flags & 1) == 0`. Flag 2 and unclassified bits remain candidates and diagnostic data; visual material membership, sentinel values, normals, UVs and colors do not select collision. Every source position and triangle index is structurally checked, including unused meshes and excluded faces. Unused visual metadata is size-accounted but its material/property/attribute semantics are not interpreted.

`terrain_owner_id` and every included actual placement's `owner_id` are explicit stable opaque caller values using the common token grammar. IDs must be unique, including owners with zero candidates. The adapter never invents IDs from filenames, names or ordinals. Placement indexes and source mesh indexes are association handles into the borrowed source, not stable enhancement or native entity identities.

Each actual nonterrain placement needs exactly one `Include { owner_id }` or `Exclude { reason }` declaration. Exclusion reasons use the same bounded token grammar. Bindings for absent records or automatically excluded records refuse. Automatic exclusions are descriptor-v2 placement zero (terrain lighting metadata), unassigned records and repeated terrain references. Descriptor-v1's first actual placement is included normally. Actual targets must resolve even if the caller excludes them; a v2 lighting record may reference a legal null model slot because it is classified before geometry resolution.

The report records source-to-output association, supplied owner decisions, included zero-candidate owners, automatic exclusions and their extension lengths, all-source flag counts and included-owner-expanded counts, work counters and unconsumed region/light counts. `zero_candidates` describes checked included owners; caller-excluded owners have no candidate conclusion. `ownership_verified` and `world_complete` remain false. Supplied exclusions are caller evidence, not independent proof that dynamic actors or alternate collision hulls have been handled.

## Refusals and known-empty data

Unsupported descriptor/mesh versions, skeletal geometry, malformed slot/name tables, source string references, nonfinite positions/placement fields, invalid indices, descriptor/mesh suffixes and unresolved included placement extensions refuse. Descriptor-v1 extensions and incomplete extension words refuse even on excluded records. Bounded extension lengths on v2 lighting or caller-excluded records are reported without interpreting their meaning.

Required source string references are descriptor model, placement, region and light names. Visual material/property name, shader and value offsets, and mesh visual string-table contents, are size-accounted but uninterpreted; invalid visual offsets alone do not invalidate collision extraction.

Included transforms are validated even when they have no eligible triangles. Selected local triangles and the f32-rounded server/asset world triangles must satisfy the common writer's widened nonzero-cross predicate. A face collapsing in one instance refuses the conversion; it is not removed globally or silently skipped. Tiny nonzero triangles remain valid. Source geometry is borrowed and unchanged on success or failure.

`AvailableEmpty` is returned only after all structural and selected ownership obligations succeed and the declared candidate selection contains no triangles. It can describe all faces excluded by the verified default flag rule. Missing identity declarations or unresolved required behavior are errors, not known-empty. Zero-candidate geometry is omitted with explicit owner diagnostics. Empty static candidates say nothing about dynamic actors, volumes or an empty world.

## Limits and diagnostic work

`CollisionBridgeLimits` defaults to 128 MiB each of logical input and cumulative adapter-created output/scratch, five million source vertices, fifteen million source indices, 100,000 resources, five million expanded eligible triangles and fifty million paired world vertex visits. Three indexed references per expanded triangle count as three paired server/asset evaluations. Checked arithmetic rejects overflow.

Preflight charges all borrowed source collections, strings, extensions and material/property payloads, including unused entries, before payload validation or output construction. Conservative output charges cover geometry, report/string copies and temporary mapping, selection, candidate-handle, histogram and multiplicity storage. These are logical accounting bounds rather than allocator-layout or RSS measurements; counters are not reclaimed when scratch is dropped. Each new histogram/global flag record is charged before insertion. Per-mesh histograms scan source faces once; included multiplicities produce expanded diagnostics by checked multiplication and group merging. World checks visit only eligible triangle handles. Many zero-candidate owners therefore cannot amplify excluded-face diagnostic scans.

The common writer and reader independently enforce their own artifact limits. Passing the bridge does not substitute for either boundary or imply every client can load its output.

## Synthetic artifact inspection

```sh
cargo run --locked --offline --example eqg_collision_fixture -- OUTPUT_DIRECTORY [--declare-preserved-source-query-frames]
```

The example uses authored raw records and stable identities, calls the actual adapter and common writer, and produces `collision.glb` plus legal JSON-only `empty.glb`. Nonempty data includes asymmetric indexed terrain and shared object instances, including an underground owner. Its front/both policy values are reserved metadata. With no flag, query-unit frames remain unspecified and the geometry requirements contain only `static-collision-triangles-v1`. The optional flag asserts preservation of the supplied source collision query frames and adds exactly `eqoxide-source-query-frames-preserved-v1`. Missing output, misplaced, unknown, duplicate or trailing options refuse before output is written.

The authored `eqg-passed-record-query-frames-v1` profile assumes terrain records already use source world units and model records use source local units. Model-local positions stay unchanged apart from the documented axis conversion; positive placement scale stays in each referring instance matrix. The mode, profile version and full example source bytes participate in the deterministic fixture bake identity. The EQG example now derives revisions from that identity rather than the historical fixed revision; shared helper defaults and geometry are unchanged. This naming correction changes generated revisions and bytes even in unspecified mode. Historical artifact bytes are not a compatibility promise. Both modes preserve the same geometry, ownership, candidate scope and face-policy metadata; known-empty output remains JSON-only.

This is an eqoxide bake assertion over the supplied source collision query frames, conditional on the passed-record profile. Unknown source frames must remain unspecified. It asserts no universal EQ/RoF2 units or game identity and does not verify importer provenance. The retired `--canonical-query-units` flag and `static-collision-query-unit-frames-v1` capability have no accepted alias. It does not establish their source history, complete source membership, native preparation or numerical query agreement. It adds no source discovery, lighting, query operation, online publication or gameplay activation.

The client `eqoxide-assets` example `static_collision_probe` can decode these artifacts and inspect owners, policy and indexed world bounds. That is CPU source-to-format acceptance. It does not perform a segment query, verify a paired package or establish gameplay readiness. Real-source corpus coverage, stable importer registries, native actor exclusions and representative server landmarks remain separate acceptance gates.
