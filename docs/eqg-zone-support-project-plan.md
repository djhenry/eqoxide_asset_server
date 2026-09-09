# EQG Zone Support Project Plan

**Date:** 2026-09-09

**Tracking issue:** [#51 — EQG-only zones are never baked](https://github.com/djhenry/eqoxide_asset_server/issues/51)

**Status:** In progress. The first slice implements EQG inventory and unsupported-format reporting; zone conversion and native source selection remain unimplemented.

**Goal:** Bake and serve EQG zones with correct terrain, placements, collision, and region data, beginning with Crescent Reach, while reporting unsupported or incomplete resources explicitly.

**Architecture:** Develop reusable format readers in libeq and separate asset-server importers for binary EQGZ zones and text EQTZP terrain projects. Both importers feed a shared scene representation and reuse the existing PFS reader, texture decoding, GLB writer, CAS, and manifests. Coordinate the collision and region contracts with the eqoxide client.

**Tech stack:** Rust, libeq_pfs, existing libeq_wld support, glam, image, glTF/GLB, and the existing asset-server build pipeline.

**Design basis:** The repository and RoF2 asset review associated with issue #51. This document records the project design, work breakdown, research gates, and acceptance criteria; exact parser implementations follow after their format questions are resolved.

## 1. Scope and observed coverage

Issue #51 correctly identifies S3D-only discovery as a cause of missing zones. Its count uses loose `.zon` files and does not include all archive-contained zone descriptors.

A read-only scan of the inspected RoF2 installation successfully read 1,073 EQG archives and associated loose and internal descriptors case-insensitively:

| Descriptor family | Distinct EQG archives | Without matching S3D archive |
| --- | ---: | ---: |
| Binary EQGZ v1 | 34 | 33 |
| Binary EQGZ v2 | 165 | 161 |
| Text EQTZP | 53 | 52 |
| **Total** | **252** | **246** |

These are manifest-bearing asset archives, not independently verified playable server zones. Counts are specific to the inspected installation and must be regenerated against deployment inputs. The review did not remeasure the live store's served-zone count.

There were 167 loose `.zon` files and 89 internal `.zon` files in 88 archives. Aliases and overlap mean these counts cannot simply be added. An additional archive, `pokport_gukta.eqg`, contains terrain without a discovered zone descriptor and is excluded from the 252.

Across 200 terrain-bearing archives, 206 `.ter` members were observed: 172 EQGT v3, 33 v2, and one v1. A version-3-only terrain implementation is insufficient for the full corpus.

### Representative fixtures

| Resource | Observed structure | What it exercises |
| --- | --- | --- |
| `crescent` | Loose EQGZ v2 descriptor; `ter_crescent.ter` v3; 2,342 descriptor placement records | First binary-zone milestone; substantial instancing |
| `anguish` | Internal EQGZ v1 descriptor | Archive-contained discovery and v1 placement layout |
| `guildhall` | Loose EQGZ v2 descriptor and TER v3 | Second binary fixture |
| `arcstone` | Internal `farstone.zon`, EQTZP project named `farstone`, `farstone.dat` | Project-name alias resolution and tiled terrain |
| `feerrott2` | Internal `feerrott.zon` and `feerrott.dat` | Another tiled-terrain alias |
| `oldcommons` | Multiple internal descriptors; external descriptor selects `commonlands` | Resource precedence and invalid alternate dependencies |
| `Foundation.zon` / `foundation.eqg` | Mixed filename case | Case-insensitive loose resource lookup |

### Included

- Descriptor discovery, dependency resolution, and coverage reporting.
- Binary EQGZ v1/v2 scenes, EQGT terrain, and static EQGM models.
- EQTZP projects, versioned terrain DAT data, terrain materials, and referenced object groups.
- Correct transforms, static collision, water/region integration, and door-model delivery.
- Reproducible bakes and explicit publication rules for incomplete results.

### Separate follow-up scope

General EQG character skinning, skeletal animation, equipment coverage, and complete reproduction of every visual effect are separate projects. Preserve relevant material data and disclose visual approximations, but do not make those projects prerequisites for usable zone geometry.

## 2. Existing code and reuse boundaries

| Existing code | Current behavior | Required change |
| --- | --- | --- |
| `src/build.rs::is_zone_archive` | Accepts S3D filenames using exclusions | Introduce format-aware resource discovery |
| `src/build.rs::build_zones_from_raw` | Calls the WLD zone baker | Dispatch discovered zone sources by format and collect structured outcomes |
| `src/build.rs::build_gamedata_from_raw` | Reuses discovery for WLD BSP water generation | Dispatch region generation independently of filename extension |
| `src/build.rs::build_zonedoors_from_raw` | Reads a sibling `_obj.s3d` | Resolve EQG door models through the zone dependency catalog |
| `src/zone.rs` | WLD terrain, object instances, materials, dedicated terrain collision | Preserve the WLD frontend and extract reusable scene/export responsibilities |
| `src/convert/mod.rs::parse_eqg_model` | Parses EQGM/EQGT static geometry | Extract a checked, version-specific parser that retains semantic fields |
| `src/convert/mod.rs::eqg_to_glb_model` | Chooses one visual mesh and produces a static model | Keep boat behavior covered; do not use its mesh-selection heuristic for zones |
| `src/convert/mod.rs::write_glb_instanced` | Writes reusable meshes and placement nodes | Reuse and extend only where the agreed asset contract requires it |
| `src/bsp_regions.rs` | Converts WLD BSP regions into water maps | Keep format-specific decoding separate from region serialization |

The existing EQG parser discards triangle flags and vertex colors, retains only diffuse texture properties, substitutes material zero for invalid material references, and treats only version 1 as the smaller vertex layout. These assumptions must be checked against each supported terrain/model version before reuse. The current converter also emits opaque materials and selects one mesh by shortest filename.

### Proposed module boundaries

These are proposed paths, not existing APIs:

Reusable binary/text format recognition and parsing belong in a new `libeq_eqg`
crate in [libeq](https://github.com/cjab/libeq), contributed from the maintained
[fork](https://github.com/djhenry/libeq). PFS already supports EQG containers and
needs no new format-specific responsibility. Keep filesystem discovery, provider
precedence, coordinate conversion, rendering approximations, and publication in
the asset server. Server modules listed below adapt parsed records rather than
duplicating the library's byte readers.

The first library contribution is a header-only `identify` API for EQGZ, EQGT,
EQGM, EQTZP, and EQOBG, exposed through an optional `eqg` facade feature. It
preserves raw binary versions and distinguishes unknown signatures from matching
truncated headers. Recognition is explicitly not full validation or parser support.
Follow-up contributions add binary ZON records, mesh records, and text/DAT readers
as separately reviewable changes. Base contributions on upstream main, excluding
unrelated changes from the fork's WLD compatibility branch.

Initial upstream contribution: [libeq PR #56](https://github.com/cjab/libeq/pull/56).
The inventory slice pins the fork revision containing that API; it does not
change the existing PFS/WLD dependency revisions.

- `src/zone_source.rs`: case-insensitive resource catalog, source selection, dependency resolution, and discovery results.
- `src/eqg/mod.rs`: importer dispatch over libeq parsed records.
- `src/eqg/mesh.rs`: adapt EQGM/EQGT geometry and material records.
- `src/eqg/zon.rs`: assemble binary EQGZ v1/v2 scenes from parsed records.
- `src/eqg/terrain.rs`: assemble EQTZP projects and decoded terrain DAT tiles.
- `src/eqg/material.rs`: source material properties and explicit export approximations.
- `src/eqg/object_group.rs`: referenced object-group resources and composed transforms.
- `src/eqg/regions.rs`: EQG water/region decoding.
- `src/zone_scene.rs`: format-neutral meshes, instances, materials, collision, regions, and diagnostics.

Add each module with the milestone that needs it. Avoid a broad rewrite of the WLD converter or speculative APIs before the corresponding importer is understood.

## 3. Shared contracts and constraints

### Discovery and dependency resolution

Discover both loose and archive-contained descriptors. Identify contents by magic/version, not extension alone. Follow model tables and project names instead of assuming `<archive-stem>.ter` or `<archive-stem>.dat`.

Preserve original names for diagnostics while using normalized lookup keys. Detect case collisions and ambiguous descriptors explicitly. Establish resource precedence against native behavior, including zones that have both S3D and EQG assets; do not silently replace an existing S3D bake based on directory iteration order.

Dependency records must identify the requesting resource, requested name, selected provider, and result. Missing required terrain/models must prevent a complete publication. Optional omissions and visual approximations must be visible in the bake report.

### Scene representation

Retain model-local geometry, material identifiers and properties, full instance matrices, render/collision roles, region information, and source dependencies. Keep collision geometry distinct from visual geometry. Preserve source properties even when the initial renderer uses a simpler representation.

Perform coordinate conversion at a defined boundary. Validate positions, winding, normals, rotation order/units, nonuniform scale, and mirrored transforms against independent fixtures. Do not assume the boat converter's axis conversion is the zone consumer's contract.

Emit flat GLB placement nodes with fully composed matrices initially. The current client traverses child nodes but reads their local matrices without accumulating parent transforms; hierarchical groups would otherwise be misplaced. Bake dedicated collision geometry into the coordinate space expected by its consumer.

### Checked parsing

Use explicit supported-version dispatch, checked arithmetic and reads, bounded counts, validated indices and string offsets, and finite transform values. Unknown versions or unsupported required records return contextual errors instead of plausible partial success.

Binary ZON v1 placements use 36-byte records. V2 uses a 40-byte prefix followed by a counted array of four-byte entries. Validate and consume the variable-length data; establish its semantics before claiming complete support. Do not apply the v1 stride to v2.

### Client compatibility

The following companion changes belong in `djhenry/eqoxide` and must be linked to this project when implementation issues are created:

1. **Collision:** `crates/eqoxide-nav/src/collision.rs` always adds expanded rendered object triangles even when a dedicated terrain collision mesh exists. Define a versioned complete-static-collision contract that replaces both terrain and static-object render-derived collision for new assets, while preserving old-bake behavior. Retain dynamic door collision separately.
2. **Regions:** `crates/eqoxide-core/src/region_map.rs` supports the existing BSP water-map v1/v2 formats. Convert EQG regions into that representation only when semantics are preserved. If not, add an explicitly versioned representation and consumer; do not reinterpret existing version numbers.
3. **Asset loading:** `crates/eqoxide-assets/src/lib.rs` must recognize any new collision/material/region metadata. Distinguish known-empty collision or known-dry regions from absent/unsupported data.

Version selection and capability negotiation must be agreed before publishing assets requiring new client semantics. Existing clients must not silently misinterpret new assets.

### Publication and observability

Report discovered sources, selected sources, unsupported formats, dependency failures, parser failures, approximations, and published zones separately. Keep resource counts distinct from unique zone counts.

Bake into clean staging directories and publish only validated outputs so stale files cannot make a failed bake appear complete. Preserve the last good manifest on failure. Align these rules with [#48](https://github.com/djhenry/eqoxide_asset_server/issues/48) and the shrink-control work in [#49](https://github.com/djhenry/eqoxide_asset_server/issues/49).

## 4. Milestones

Every milestone is a separate reviewable unit. During execution, create or link implementation issues, use isolated worktrees, run focused regression tests, and obtain independent review before merge. User-visible milestones also require an independent live acceptance test. This document does not claim any milestone has passed.

### M1 — Resource discovery and coverage reporting

**Files:** Add `src/zone_source.rs`; modify `src/build.rs`, `src/main.rs`, and `src/lib.rs`; add `tests/zone_inventory.rs` and extend `tests/build_zones.rs`.

**Dependencies:** None. Reuses issue #51 as the originating feature gap.

**First slice:** `inventory-zones` recognizes headers through `libeq_eqg`, reports
archive-contained and loose descriptors, and exposes support gaps in ordinary
bakes. It deliberately retains all descriptors without selecting native
precedence. The remaining M1 work is dependency/provider resolution and shared
terrain/door/region dispatch; the complete milestone is not yet accepted.

- [ ] Add synthetic archives covering loose/internal descriptors, case differences, aliased project names, duplicate descriptors, model-only EQGs, terrain without a descriptor, and S3D/EQG twins.
- [ ] Resolve and document native descriptor/provider precedence; ambiguous inputs must remain explicit errors until their selection rule is established.
- [ ] Build one shared source catalog for terrain, doors, and regions. Preserve established WLD conversion behavior.
- [ ] Add a structured build report and readable CLI summary with distinct unsupported and failed counts.
- [ ] Regenerate the corpus inventory from a supplied asset directory; record the method and input identity without distributing native assets.
- [ ] Verify discovery/reporting can ship while EQG importers remain unsupported, without publishing empty zones or passing EQG into WLD water parsing.

**Acceptance:** Every candidate receives a classified outcome; binary and text descriptors are recognized; case and aliases resolve deterministically; unsupported EQG zones are visible in the summary. The known inventory is reproducible for the same inputs.

### M2 — Binary ZON/TER/MOD scene import, Crescent first

**Files:** Add `src/eqg/mod.rs`, `src/eqg/mesh.rs`, `src/eqg/zon.rs`, and `src/zone_scene.rs`; modify `src/convert/mod.rs`, `src/zone.rs`, `src/build.rs`, and `src/lib.rs`; add `tests/eqg_mesh.rs` and `tests/eqg_zone.rs`.

**Dependencies:** M1. Coordinate shared collision/region fields with M3 before fixing the exported contract.

- [ ] Extract the existing static mesh parser and retain boat regression coverage.
- [ ] Contribute checked EQGZ and EQGM/EQGT byte readers to `libeq_eqg`; consume those readers through server adapters, with pinned dependency revisions and synthetic library tests.
- [ ] Establish vertex layouts for the observed EQGT v1/v2/v3 and supported EQGM versions; reject other layouts explicitly.
- [ ] Add synthetic truncation, count-overflow, invalid-index, invalid-string, and material-reference tests before implementing checked parsing.
- [ ] Parse ZON v1/v2 model tables, placements, lights, and region records, retaining unsupported semantic data in diagnostics rather than silently dropping it.
- [ ] Test multiple v2 placement records with nonempty trailing arrays so an incorrect fixed stride fails deterministically.
- [ ] Resolve models by descriptor references; retain material properties, flags, and vertex colors in the scene.
- [ ] Establish rotations, scale, winding, and coordinate conversion using independent fixtures; compose placement transforms before export.
- [ ] Bake Crescent, Guild Hall, and a v1 fixture in staging. Account for every placement record as emitted, intentionally nonvisual, or rejected with a reason.
- [ ] Verify geometry, textures, bounds, instancing, and deterministic output. Rendering-only previews must not be published as gameplay-complete before M3.

**Acceptance:** Crescent and a binary v1 fixture produce validated scenes without lost record alignment or unexplained missing dependencies. Shared models remain instanced. The 2,342 Crescent placement records are accounted for; this does not require 2,342 rendered nodes if some records have verified nonvisual roles.

### M3 — Gameplay contracts: collision, regions, and doors

**Files:** Add `src/eqg/regions.rs`; modify `src/zone_scene.rs`, `src/zone.rs`, `src/build.rs`, and region serialization as needed; extend `tests/bake_zone.rs` and `tests/http_api.rs`; add `tests/eqg_regions.rs`. Companion client work touches the three modules identified in Section 3 and the relevant door-loading integration.

**Dependencies:** M1; binary acceptance uses M2. Contract design can proceed alongside M2.

- [ ] Establish face flags and collision-hull semantics before assigning render/passable/solid roles.
- [ ] Define versioned complete-static-collision metadata and preserve legacy terrain-plus-object fallback only for legacy assets.
- [ ] Test visible-passable faces, invisible-solid boundaries, collision-only hulls, and absence of duplicate object collision.
- [ ] Define region export after establishing water-volume and zone-line semantics; test known-dry, unavailable, malformed, and supported region data as different outcomes.
- [ ] Extend door-model discovery to EQG dependencies; verify door naming against runtime requests and avoid baking dynamic doors as permanent static obstructions.
- [ ] Validate consumer compatibility before publication; ensure unsupported clients receive an explicit incompatibility outcome.
- [ ] Exercise asset-server delivery and live client floors, walls, doors, water where present, and zone transitions for representative binary zones.

**Acceptance:** A delivered binary zone is usable for movement and navigation. Collision uses the intended surfaces, doors remain dynamic, and unknown region data does not become a false dry result. Failed bakes preserve the last good manifest.

### M4 — EQTZP/DAT terrain and materials

**Files:** Add `src/eqg/terrain.rs`, `src/eqg/material.rs`, and `src/eqg/object_group.rs`; extend `src/eqg/regions.rs`, `src/zone_scene.rs`, and build dispatch; add `tests/eqg_terrain.rs` and `tests/eqg_object_groups.rs`.

**Dependencies:** M1 resource resolution, M2 shared scene/export infrastructure, and M3 publication contracts.

- [ ] Parse project metadata including project name, bounds, units per vertex, quads per tile, coverage/layer sizes, and version.
- [ ] Contribute reusable project/DAT parsing to `libeq_eqg`; keep project resource resolution and terrain rendering policy in this repository.
- [ ] Resolve DAT resources by project references. Distinguish terrain data from auxiliary files such as water and exclusion data.
- [ ] Establish supported terrain versions and decode each with explicit layout gates. Determine topology, seam handling, holes, and collision bits before meshing.
- [ ] Preserve terrain layer, coverage, blending, repeat, detail, and normal-map properties. Implement deterministic diffuse layer baking as the initial approximation, with the approximation recorded in the report.
- [ ] Load object groups only through verified inclusion rules. Detect dependency cycles, compose transforms, and test that resources are not appended twice.
- [ ] Resolve the inclusion rule for TOG resources associated with binary zones and enable those dependencies through the same resolver when verified.
- [ ] Convert water data using established semantics and the M3 region contract.
- [ ] Bake Arcstone and Feerrott2; test Old Commons descriptor precedence and mixed-case project-name references.
- [ ] Run live acceptance for seams, terrain holes, object collision, water boundaries, and zone transitions.

**Acceptance:** Representative text-project zones publish usable terrain without cracks, filled holes, duplicated objects, or false region results. Required unsupported versions and dependencies block publication with actionable diagnostics.

### M5 — Corpus validation and rollout

**Files:** Add a reproducible inventory/validation command or script under `scripts/`; extend `tests/build_zones.rs`, `tests/build_cli.rs`, and `tests/http_api.rs`; document operation and supported formats in `README.md`.

**Dependencies:** M1–M4. Binary-family rollout may happen earlier after M3 if unsupported text projects remain explicitly reported.

- [ ] Run inventory and staged conversion across the supplied corpus; report source/version coverage and every remaining failure.
- [ ] Validate GLB references, finite coordinates, geometry bounds, placement accounting, required textures, collision semantics, region availability, and repeat-bake determinism.
- [ ] Check repeated instances do not multiply stored geometry unnecessarily and record bake time, output size, and peak-memory observations for large zones.
- [ ] Exercise failure publication paths, stale-staging protection, last-good preservation, and client compatibility.
- [ ] Run representative WLD and boat regressions to verify existing formats remain functional.
- [ ] Publish the operator coverage report and supported-format matrix; separate unsupported required behavior from documented visual approximations.
- [ ] Close #51 only when its intended zone coverage and delivery behavior are verified, with remaining work explicitly tracked rather than silently skipped.

**Acceptance:** Every discovered source has a reproducible outcome. Enabled zones meet geometry and gameplay acceptance; unsupported inputs are visible. A full bake cannot imply complete coverage merely because some zones succeeded.

## 5. Research gates

| Question | Blocks | Required evidence |
| --- | --- | --- |
| Loose/internal descriptor, archive-provider, and S3D/EQG precedence | M1 source selection | Native behavior and conflicting-resource fixtures, including Old Commons |
| TER/MOD vertex layouts for each supported version | M2 parsing | Version-specific fixtures and boundary-consumption checks |
| ZON rotation units/order, coordinate axes, and v2 trailing-array semantics | M2 complete scene support | Independent transformed-point checks and full record accounting |
| Triangle flags, terrain holes, and collision-hull selection | M3/M4 gameplay | Fixtures separating visible-passable and invisible-solid geometry |
| TOG inclusion and group composition | M2 completeness where referenced; M4 groups | Dependency and duplicate-placement checks |
| Water sheets versus volumes and region/zone-line meanings | M3/M4 regions | Inside/outside/boundary queries with independent expected results |
| Material layer ordering, coverage sampling, and repeat factors | M4 appearance | Representative texture-composition comparisons |

Unresolved semantics are implementation gates, not permission to guess a bit mapping or report a partial bake as complete. Native-format findings may be recorded generically as verified against the native RoF2 client; keep private investigation material outside public project files.

## 6. Validation and execution policy

- Use synthetic fixtures for ordinary CI. Native corpus tests take an explicitly supplied asset directory and do not distribute game assets.
- A requested corpus-validation run must fail when required fixtures are unavailable; it must not silently skip and report success.
- Add focused regression tests that discriminate the incorrect behavior, including fixed-stride v2 parsing, bad provider selection, filled terrain holes, and duplicate object collision.
- Use independent expected coordinates and region/collision queries rather than round-tripping the same parser assumptions.
- Run the relevant test targets for each milestone, then integration and live acceptance when it changes observable zone behavior. Capture evidence before marking checkboxes complete.
- Keep project-wide Rust test coverage green. Run the repository's documentation/public-detail check for tracked documentation and inspect new untracked documents explicitly before commit.
- Do not assign calendar estimates until the research gates and corpus failure distribution are measured.

## 7. Completion checklist

- [ ] Discovery covers loose and internal descriptors with deterministic selection.
- [ ] Binary EQGZ v1/v2 and observed EQGT versions have explicit tested support.
- [ ] Crescent loads through the asset server and passes gameplay acceptance.
- [ ] EQTZP/DAT zones have tested topology, materials, dependencies, and regions.
- [ ] Collision and region contracts are understood by the consuming client.
- [ ] Door geometry and runtime behavior are validated.
- [ ] Unsupported, failed, approximate, and complete outcomes remain distinguishable.
- [ ] Publication cannot expose stale or incomplete output as a successful replacement.
- [ ] Full-corpus coverage is reported with reproducible inputs and methods.
- [ ] Existing WLD zones and EQG boats retain their verified behavior.
