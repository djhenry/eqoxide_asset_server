# Selected WLD terrain normalization contract

This refines [issue #79](https://github.com/djhenry/eqoxide_asset_server/issues/79) and the [source boundary plan](wld-common-collision-source-plan.md). It specifies the first pure, post-parse frontend to the existing [common collision scene](static-collision-contract.md). The isolated implementation in `src/wld_collision.rs` prepares explicitly selected records; it is not a production importer or a native-query profile.

**Selected exclusion policy (2026-10-15):** the owner chose to omit raw coincident-position collision candidates with explicit bake diagnostics, following [native source feasibility](wld-terrain-feasibility.md). This is a narrow source-preparation rule; other invalid required geometry still refuses the entire selection.

The [implementation plan](wld-selected-terrain-implementation-plan.md) scopes the pure frontend and its acceptance. The client receives the same GLB standard used by EQG. WLD source flags, quantization, axes and contextual references remain baker responsibilities. Selected geometry does not establish actor activation, ownership verification or world completeness.

## Input and selection

Use the public parser records in the current locked [libeq revision](https://github.com/djhenry/libeq/commit/39fb132fe0d206b7e5878213649eeb5e33c67d78). No new library API, dependency update or local-fork override is required.

The entry point is `normalize_selected_wld_terrain(input, limits)`. Input borrows one `WldDoc`, a caller-supplied opaque source-context token, and a nonempty slice of `(fragment_slot, owner_id)` selections. Slots are **zero-based document positions**, not one-based wire references or filtered mesh iteration positions. Owner identities and the context token satisfy the existing common token rules. Each slot and owner must occur exactly once. No names, material references or dependency searches resolve selection.

The document borrow binds selection to its actual context. The supplied context token is not a verified content digest or a source registry. The caller must include source content identity, explicit selection, owner assignments and normalization-policy version in the bake identity. Parsing and certification of that identity remain outside this API.

Check `fragment_count()` before traversal; select using checked `at(slot)` and an exact `FragmentType::DmSpriteDef2` match. The pinned parser can retain failed fragment bodies as `Unknown` at their original positions. A selected `Unknown`, including one labeled type `0x36`, is unsupported required data and must refuse. It must never become an empty mesh. Reject an empty selection rather than invent known-empty source membership.

## Admitted terrain subset

Require source center-presence bit `0x1`. Initially admit only flags within `0x0001c003`; outside-mask bits refuse. This is a conservative extraction profile, not a universal native valid-mask rule. It admits the documented representative values `0x18003` and `0x14003` without classifying either as verified static ownership.

A bounded native terrain audit supports the following separation:

| Source field | Selected terrain policy |
|---|---|
| Flag `0x2`, authored maximum distance | Does not scale positions; do not use it as geometry scale. |
| Flag `0x4000`, authored min/max bounds | Do not clip geometry using them; derive indexed bounds through the common writer. |
| Flag `0x8000` | Retain uninterpreted; not used by the audited decoder. No global meaning is assigned. |
| Flag `0x10000`, fragment4 sentinel choice | Does not change this terrain extraction. It affects a separate model-query preparation path, which is outside this contract. |
| Raw fragment3 and fragment4 references | Retain kind and value in diagnostics without resolving them. They are not consulted by the audited terrain position/face routine. |
| Material palette, material groups, shading normals, UVs and colors | Do not resolve or inspect their values to choose collision faces. Invalid material references cannot erase selected geometric candidates. |
| Animation reference, skin assignments, mesh operations | Require a canonical null animation reference and zero declared/actual skin and mesh-operation entries. Unresolved dynamic geometry refuses. |

A canonical null reference is the parser's `Name(StringReference(0))`; a manually constructed `Index(0)` is not an alternative null encoding. Count/payload checks compare all nine declared array counts with their corresponding actual lengths without walking visual attributes. The `scale` field is an exponent, not a tenth count. These checks do not certify material graphs, animation semantics or the original byte parser.

Unselected records are not silently promoted into this subset. A bounded document pass reports counts of unselected modern meshes, older `0x2c` records, unknown records, actors and other fragments. It does not traverse their dependencies or scan their payloads. Unsupported coverage outside the explicit selection is diagnostic; unsupported required selection is an error.

## Geometry and membership

For each selected record:

1. Require exponent `0..=15` and a finite source center. Decode signed positions as `q * 2^-exponent + center` in source XYZ, rounding each reconstructed coordinate to f32. Use explicit power-of-two arithmetic, never the legacy signed shift. Reject any nonfinite reconstructed value.
2. Map once into server geometry axes `(Y, X, Z)`. Retain a single indexed position pool per contributing source mesh and an identity instance matrix. Terrain positions therefore retain their world query units. Do not normalize mesh extents, weld vertices or apply a visual placement helper.
3. Validate all face indexes, including excluded faces, before indexing. Retain source face order and count every face's complete raw flags. Faces with `flags & 0x0010 != 0` are excluded by the source flag; all others are raw collision candidates.
4. For raw candidates only, compare the three referenced raw signed XYZ tuples. If any pair is exactly equal, omit that face with exclusion reason `coincident_source_positions`. This includes repeated in-range indexes and distinct indexes sharing a position. Use no epsilon, reconstructed-position comparison, welding or small-area threshold. Validate record structure and every source index before applying this exception.
5. Refuse three-distinct-position raw collinearity using an exact integer cross product, even if later f32 reconstruction would bend the points. Apply the existing common local/serialized-world triangle admission rules to all remaining candidates. Other collapsed geometry still refuses the entire selection, including three distinct collinear raw positions and positions that collapse only after reconstruction or serialization. Preserve the original position pool and the order of retained faces; indexed bounds need not include unused positions.

Modern WLD winding differs from EQG's frontend: preserve `[i0,i1,i2]` through the source reflection. The common writer then performs its existing proper asset-axis rotation. There is no WLD dispatch in the client.

This is default-face candidate membership with a narrow diagnosed source-position exclusion, **not exact native query membership**. Native small-face plane preparation and query mode/direction gates are not reproduced here. In particular, do not invent a threshold that silently deletes otherwise admitted small triangles. A later native-fidelity query profile needs its own preparation and arithmetic acceptance.

If a nonempty selection is fully examined and has zero included faces, return `AvailableEmpty` with no dummy geometry. This includes a structurally valid zero-face record, an all-source-flag-excluded selection and an all-coincident-position-excluded selection. All selected records and their indexes must still pass validation. Invalid indexes, failed parses, unsupported flags/dependencies, absent selection and work exhaustion are errors, never empty success.

## Result and logical limits

Return the common `StaticCollisionScene` plus a source report. The report binds each supplied owner to its source-context/slot, source exponent/center, raw mesh flags, raw fragment3/4 references, source-face, source-flag-excluded, raw-candidate, coincident-source-position-excluded and retained-face counts, and a bounded full-face-flag histogram. These counts partition faces without double counting: `source_faces = source_flag_excluded + raw_candidates` and `raw_candidates = coincident_source_positions_excluded + retained_faces`. A flag-excluded face is not also charged to the coincident-position exclusion reason. It also carries the fixed coverage counts described above and explicit `ownership_verified: false`, `world_complete: false` and `full_native_query_preparation_applied: false`. Give the narrow preparation rule an explicit report identifier, `wld-selected-terrain-coincident-exclusion-v1`; it does not certify full native preparation and is part of the caller's bake identity. The full-preparation flag stays false even though this one verified source exclusion has been applied.

Do not return one diagnostic per face or copy source names. Every histogram insertion, owner copy and output/scratch collection entry is budgeted before allocation. The report is baker-side inspection data, not new GLB metadata.

| Limit | Default and hard maximum | Charged work/payload |
|---|---:|---|
| Document fragment visits | 100000 | One bounded coverage pass plus selected lookups; charge visits separately, including repeated work. |
| Selected owners | 100000 | Selection records, before reference/identity collection. |
| Selected source vertices | 5000000 | Every selected position, including unused vertices and empty-output sources. |
| Selected source faces | 5000000 | Every examined face, including excluded faces. |
| Face visits | 15000000 | Two preflight visits per selected face plus one emission visit per face in a contributing source. |
| Decoded vertices | 5000000 | One reconstruction per selected position, including empty-output sources. |
| Retained indexes | 15000000 | Checked three-index contribution of every included face. |
| Inspected source geometry bytes | 134217728 | `6 * selected vertices + 8 * selected faces`; excludes already parsed document storage and uninterpreted visual payloads. |
| Owned logical payload bytes | 134217728 | Position/index output, copied context/owner text, report numeric fields and histogram keys/counts; cumulative temporary payload is also charged. |
| Resource entries | 100000 | Selection scratch, owner/slot sets, source reports, output meshes/primitives/instances and diagnostic histogram entries, cumulatively. |

These are engineering budgets, not measured native limits or RSS guarantees. Limits may be lowered, not raised beyond the hard maxima. Checked addition/multiplication and validation of limits precede source work. A count-only selected-record preflight must establish payload/resource allowances before decoding or copying. The API documentation and [bake diagnostics guide](wld-selected-terrain-bake.md) define each logical report/scratch charge. Exact-boundary tests cover every counter; allocator overhead and prior parsing are excluded explicitly.

Use distinct error categories for invalid limits, exhausted budget, invalid selection/identity, unsupported required source, inconsistent source structure, invalid geometry and common-writer refusal. No partial scene or success report escapes an error. A separate writer call preserves its existing atomic-output behavior.

## Package boundary and acceptance

Normalization does not publish or activate a package. The first bridge uses the existing writer with query-unit frames unspecified; it must not automatically claim source certification. A caller may explicitly assert preservation of supplied decoded source collision query frames using [the declaration writer](static-collision-contract.md); unknown source frames remain unspecified. This eqoxide bake assertion neither verifies importer provenance nor establishes universal EQ/RoF2 units or game identity. It remains separate from ownership, world coverage, implemented queries and native query parity.

Use two fixture layers. Author bytes for `WldDoc::parse` to exercise positional selection and the full pipeline, including selected `Unknown` refusal. Separately pass direct `DmSpriteDef2` literals to the internal record-validation helper for declared/actual count disagreement. The pinned document constructor and fields are private, and successfully parsed arrays match their declared counts; malformed byte fixtures becoming `Unknown` cannot substitute for that helper test. Neither layer requires a new library API.

Before implementation is accepted, author and independent reviewer must separately exercise the authored document fixtures through normalization, the real common writer and fresh client CPU role/package decoders. Include both container versions' supported texture-coordinate representations without making UVs a collision dependency. Verify asymmetric signed positions/center, exact source winding, stable positional selection past an unknown fragment, owner binding, flags `0x18003`/`0x14003`, unknown-bit refusal, exponent boundaries, count consistency, unused bad material references, source-flag-excluded/coincident-position-excluded/valid-empty results and every budget boundary. Distinguish repeated-index and distinct-index/coincident-position fixtures; verify exact per-owner exclusion counts, count conservation, retained face order and preservation of other valid geometry. Require errors for invalid indexes even on excluded faces, three-distinct-position collinearity and reconstruction-induced collapse.

Inject eligibility, axis, winding, center, exponent, owner, empty-result and budget faults. Require runtime assertion failures, exact source restoration and fresh passing runs. Add a correctly hashed package disagreement case so integrity checks cannot hide a binding defect. Synthetic tests establish the selected dataset contract; representative private native corpus, server alignment, placements, complete ingestion and gameplay remain separate gates.
