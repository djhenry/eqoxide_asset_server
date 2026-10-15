# Native WLD source feasibility and an open exclusion decision

This is bounded source inspection for [issue #79](https://github.com/djhenry/eqoxide_asset_server/issues/79), supplementing the [selected terrain proposal](wld-selected-terrain-contract.md). It does not implement that normalizer or establish required terrain membership, ownership, GLB conversion, native query parity or gameplay readiness.

## Observed inputs and method

Two independently implemented private probes inspected the primary named WLD member in each of six installed S3D archives, using the locked libeq revision `39fb132fe0d206b7e5878213649eeb5e33c67d78`. Inputs are a small explicit sample, not a corpus-wide survey or a claim about all installations. Exact input hashes and native numeric witnesses remain private; no native payload is distributed.

Each input ran in a separate sequential process with a 768 MiB address-space limit, 30-second CPU limit and 45-second wall timeout. Archive/member byte allowances were 16 MiB; declared and actual member lengths had to agree. Checked header/string/fragment extents and a 50000-fragment allowance preceded document parsing. Both probes used exact named-member reads rather than inflating every archive member or using visual mesh helpers. Every input completed successfully.

| Primary WLD | Modern records | Source faces | Raw bit-0x10-clear candidates | Coincident-source-position candidates | Affected records |
|---|---:|---:|---:|---:|---:|
| befallen | 3609 | 23314 | 23290 | 4 | 4 |
| blackburrow | 2282 | 19962 | 18263 | 0 | 0 |
| gfaydark | 1597 | 74981 | 72694 | 22 | 22 |
| greatdivide | 3581 | 93952 | 90365 | 12 | 12 |
| paludal | 1446 | 56937 | 54621 | 2 | 2 |
| poknowledge | 1802 | 105084 | 104972 | 2 | 2 |
| **Total** | **14317** | **374230** | **364205** | **42** | **42** |

All modern records used mesh flags `0x18003` and exponent 5. Neither probe found bad face indexes, declared/actual count disagreement, unknown fragment placeholders, absent/nonfinite centers, outside-mask flags, or animation/skin/mesh-operation markers in these inputs. These observations support the proposed field screening; they are not complete normalizer acceptance.

All 42 collapsed candidates have three distinct indexes but only two distinct raw signed position tuples. The reconstructed coordinates also have only two distinct positions. They are not new collapses caused by center addition or f32 rounding. Independent synthetic controls distinguished repeated indexes, different indexes sharing positions, and three distinct collinear positions. The six-input result contains only the second class.

## Consequence for the current proposal

The proposed normalizer is atomic: a retained collapsed face refuses the **entire selected dataset**, with no partial scene. If every modern record in each primary WLD were selected, five of these six illustrative selections would therefore refuse. Actual required terrain selection and native activation were not resolved, so this is not a claim that five production zones necessarily fail.

Omitting entire affected meshes would be a different, currently unauthorized policy. Those 42 records contain 4009 raw candidate faces: the 42 coincident faces plus **3967 other candidate faces**. The remaining-record screen totals 360196 candidates; that number is not a normalizer output or an approved incomplete bake.

The audited native face preparation invalidates an exact-zero plane. This provides source evidence for excluding these coincident-source-position faces individually; no native binary/query was executed. Broader small-positive-face preparation, numeric rounding, traversal and query modes remain separate gates.

**Recommended revision for owner consideration:** after validating every source index and record, exclude only candidate faces whose raw source position tuples coincide, with explicit per-owner exclusion counts. Preserve the original position pool and other faces. Keep other malformed or unresolved geometry as errors, including three-distinct-position collapses until their rule is accepted. This leaves the common writer's admission rules and the one GLB/client standard unchanged.

If selected, the implementation must add an exclusion reason and tests, distinguish raw face candidates from prepared output counts, and describe its preparation status explicitly. It must not claim full native preparation from this narrow rule. Required supported selections with no remaining candidates may be known-empty only after all required records are successfully examined. The existing atomic-refusal policy remains the written contract until this revision is approved.

## Separate PFS reader defect

The audit also reproduced [libeq issue #4](https://github.com/djhenry/libeq/issues/4): a synthetic block and file entry declaring 8192 bytes successfully yielded 1048576 bytes. The first block inflates inside `get_reader`, before an outer read limit can constrain its allocation. The exact public reproducer was independently compiled and executed; valid-size and corrupt-zlib controls were also exercised. No game assets or out-of-memory crash were needed for that reproduction.

The process limits above were necessary safeguards for this inspection. A caller's `Read::take` alone is not a complete ingestion bound. This library concern does not turn the post-parse terrain proposal into a byte-ingestion implementation.

## Evidence ceiling

Author and independent inspection agree on all six per-input counts and aggregate classifications. The independent audit also checked the relevant native preparation source and assembly. This is raw-source feasibility and bounded synthetic API reproduction, not a source-normalizer regression suite, real producer-to-GLB conversion, native runtime agreement, server alignment or live client testing. Canonical-frame declaration, complete collision membership and numerical query acceptance remain distinct milestones.
