# Selected WLD terrain normalization contract (proposal)

This refines [issue #79](https://github.com/djhenry/eqoxide_asset_server/issues/79) and the [source boundary plan](wld-common-collision-source-plan.md). It specifies the first pure, post-parse frontend to the existing [common collision scene](static-collision-contract.md). It is a design proposal, not an implemented adapter or a native-query profile.

The client receives the same GLB standard used by EQG. WLD source flags, quantization, axes and contextual references remain baker responsibilities. Selected geometry does not establish actor activation, ownership verification or world completeness.

## Input and selection

Use the public parser records in the current locked [libeq revision](https://github.com/djhenry/libeq/commit/39fb132fe0d206b7e5878213649eeb5e33c67d78). No new library API, dependency update or local-fork override is required.

The proposed entry point is `normalize_selected_wld_terrain(input, limits)`. Input borrows one `WldDoc`, a caller-supplied opaque source-context token, and a nonempty slice of `(fragment_slot, owner_id)` selections. Slots are **zero-based document positions**, not one-based wire references or filtered mesh iteration positions. Owner identities and the context token satisfy the existing common token rules. Each slot and owner must occur exactly once. No names, material references or dependency searches resolve selection.

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
3. Validate all face indexes, including excluded faces, before indexing. Retain source face order. Include only faces with `flags & 0x0010 == 0`; count every face's complete raw flags independently of this decision.
4. Apply the existing common local/serialized-world triangle admission rules to retained faces. Refuse collapsed retained geometry rather than silently skipping it. Unused vertices remain in a contributing pool; indexed bounds need not include them.

Modern WLD winding differs from EQG's frontend: preserve `[i0,i1,i2]` through the source reflection. The common writer then performs its existing proper asset-axis rotation. There is no WLD dispatch in the client.

This is raw default-face candidate membership, **not exact native query membership**. Native small-face plane preparation and query mode/direction gates are not reproduced here. In particular, do not invent a threshold that silently deletes otherwise admitted small triangles. A later native-fidelity query profile needs its own preparation and arithmetic acceptance.

If a nonempty selection is fully examined and has zero included faces, return `AvailableEmpty` with no dummy geometry. This includes a structurally valid zero-face record and an all-excluded selection. Invalid indexes, failed parses, unsupported flags/dependencies, absent selection and work exhaustion are errors, never empty success.

## Result and logical limits

Return the common `StaticCollisionScene` plus a source report. The report binds each supplied owner to its source-context/slot, source exponent/center, raw mesh flags, raw fragment3/4 references, source and included/excluded counts, and a bounded full-face-flag histogram. It also carries the fixed coverage counts described above and explicit `ownership_verified: false`, `world_complete: false` and `native_query_preparation_applied: false`.

Do not return one diagnostic per face or copy source names. Every histogram insertion, owner copy and output/scratch collection entry is budgeted before allocation. The report is baker-side inspection data, not new GLB metadata.

| Limit | Proposed default and hard maximum | Charged work/payload |
|---|---:|---|
| Document fragment visits | 100000 | One bounded coverage pass plus selected lookups; charge visits separately, including repeated work. |
| Selected owners | 100000 | Selection records, before reference/identity collection. |
| Selected source vertices | 5000000 | Every selected position, including unused vertices and empty-output sources. |
| Selected source faces | 5000000 | Every examined face, including excluded faces. |
| Retained indexes | 15000000 | Checked three-index contribution of every included face. |
| Inspected source geometry bytes | 134217728 | `6 * selected vertices + 8 * selected faces`; excludes already parsed document storage and uninterpreted visual payloads. |
| Owned logical payload bytes | 134217728 | Position/index output, copied context/owner text, report numeric fields and histogram keys/counts; cumulative temporary payload is also charged. |
| Resource entries | 100000 | Selection scratch, owner/slot sets, source reports, output meshes/primitives/instances and diagnostic histogram entries, cumulatively. |

These are proposed engineering budgets, not measured native limits or RSS guarantees. Limits may be lowered, not raised beyond the hard maxima. Checked addition/multiplication and validation of limits precede source work. A count-only selected-record preflight must establish payload/resource allowances before decoding or copying. The implementation must freeze each logical report/scratch charge in its API documentation and exact-boundary tests; allocator overhead and prior parsing are excluded explicitly.

Use distinct error categories for invalid limits, exhausted budget, invalid selection/identity, unsupported required source, inconsistent source structure, invalid geometry and common-writer refusal. No partial scene or success report escapes an error. A separate writer call preserves its existing atomic-output behavior.

## Package boundary and acceptance

Normalization does not publish or activate a package. The first bridge uses the existing writer with query-unit frames unspecified; it must not automatically claim source certification. A later caller may explicitly declare canonical frames after source-frame provenance acceptance using [the declaration writer](https://github.com/djhenry/eqoxide_asset_server/pull/83). That declaration remains separate from ownership, world coverage and native query parity.

Use two fixture layers. Author bytes for `WldDoc::parse` to exercise positional selection and the full pipeline, including selected `Unknown` refusal. Separately pass direct `DmSpriteDef2` literals to a proposed internal record-validation helper for declared/actual count disagreement. The pinned document constructor and fields are private, and successfully parsed arrays match their declared counts; malformed byte fixtures becoming `Unknown` cannot substitute for that helper test. Neither layer requires a new library API.

Before implementation is accepted, author and independent reviewer must separately exercise the authored document fixtures through normalization, the real common writer and fresh client CPU role/package decoders. Include both container versions' supported texture-coordinate representations without making UVs a collision dependency. Verify asymmetric signed positions/center, exact source winding, stable positional selection past an unknown fragment, owner binding, flags `0x18003`/`0x14003`, unknown-bit refusal, exponent boundaries, count consistency, unused bad material references, all-excluded/valid-empty results and every budget boundary.

Inject eligibility, axis, winding, center, exponent, owner, empty-result and budget faults. Require runtime assertion failures, exact source restoration and fresh passing runs. Add a correctly hashed package disagreement case so integrity checks cannot hide a binding defect. Synthetic tests establish the selected dataset contract; representative private native corpus, server alignment, placements, complete ingestion and gameplay remain separate gates.
