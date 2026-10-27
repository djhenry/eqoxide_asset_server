# WLD terrain input membership and the next inspection boundary

Tracked by [issue #86](https://github.com/djhenry/eqoxide_asset_server/issues/86), following the [explicit selected-record frontend](https://github.com/djhenry/eqoxide_asset_server/pull/85). This document proposes a bounded association inspector. It adds no automatic selection, runtime implementation, package activation or gameplay collision. The existing client GLB standard is unchanged.

## Grounded source-input rule

In the audited native RoF2 loading path, a positive document header region count enables terrain loading. The loader examines every Region (`0x22`) in document order and processes its modern mesh association (`0x100`), when present, before decoding the selected WorldTree (`0x21`). The selected tree is the last such source record in document order. Authored BSP reachability does not select these source inputs. Merely finding a modern mesh (`0x36`) elsewhere in the document is insufficient.

This is source-input membership, not final query membership or order. The concrete terrain consumer subsequently groups/reorders faces and remaps vertices before common query preparation. Native execution, numerical query agreement, ownership, server alignment and complete-world coverage are not established by this audit. The selected frontend's `full_native_query_preparation_applied: false` remains appropriate.

Three reference domains must remain distinct:

| Source field | Domain |
|---|---|
| Region modern mesh reference | Signed document reference: positive values are one-based document entries; negative values request source name resolution. |
| WorldTree front/back child | One-based node ordinal; zero means no child. |
| WorldTree node region value | One-based Region ordinal; zero means no region result at that node. |

Original document slots and Region ordinals must survive unsupported or failed records. A parsed-type iterator that omits an `Unknown` originally tagged `0x22` cannot construct a correct Region ordinal table. A typed `FragmentRef<Region>` wrapper does not turn a node's ordinal into a document reference.

Native negative-name resolution is also distinct from ordinary string-content lookup: it chooses the first matching resolved string-table position, subject to the loader's name initialization rules. The existing high-level name lookup is not a substitute. A first defensive profile should report this unresolved reference class rather than guess.


## Bounded private sample

Offline inspection of the same six primary WLD members used in the [earlier feasibility audit](wld-terrain-feasibility.md) found the following source associations. These are installed-sample observations, not corpus-wide guarantees.

| Primary WLD | Region records | Positive modern associations | Distinct linked modern meshes | Linked coincident-position candidates |
|---|---:|---:|---:|---:|
| befallen | 4046 | 3609 | 3609 | 4 |
| blackburrow | 2301 | 2282 | 2282 | 0 |
| gfaydark | 2905 | 1597 | 1597 | 22 |
| greatdivide | 9105 | 3581 | 3581 | 12 |
| paludal | 1446 | 1446 | 1446 | 2 |
| poknowledge | 1802 | 1802 | 1802 | 2 |
| **Total** | **21605** | **14317** | **14317** | **42** |

Every modern mesh in these six primary members has one positive Region association; no modern target is shared or unlinked. The other 7288 regions have no modern reference and zero inline geometry counts. Declared Region counts match the observed source Region counts. These samples contain no Region `0x40` layout, named/null/invalid modern reference, unparseable Region, or legacy wall/obstacle record. Their absence here does not justify dropping those cases from future diagnostics.

The same previously recorded six-sample inspection has the following aggregate full-flag frequencies. This is historical sample evidence, not a new asset scan or a universal native-valid flag mask.

| Complete Region flags | Region count |
|---|---:|
| `0x81` | 7288 |
| `0x101` | 26 |
| `0x181` | 14291 |
| **Total** | **21605** |

All 42 previously observed raw coincident-position candidates occur in Region-associated meshes. This resolves their source-input membership for these samples; it does not establish their final native query behavior. Face statistics count each distinct mesh once and are not weighted by hypothetical shared associations.

Each sample ran serially in a separate process with 768 MiB address-space, 30-second CPU and 45-second wall limits. Archive/member limits were 16 MiB, with checked WLD extents and at most 50000 fragments before parsing. The author probe additionally capped document/reference work at 200000 visits each, nodes at 100000, face visits at 5000000, histogram cardinality at 256, exceptions at 8 and emitted JSON at 64 KiB; redirected output files had 128 KiB limits. These are research controls, not a production ingestion or logical-payload contract. The locked PFS reader can inflate eagerly before an outer read limit, so process limits remain necessary. Exact inputs, witnesses and provenance remain private. No normalizer, GLB writer, native executable or live client ran for this inspection.

## Parser prerequisite

The locked [libeq revision](https://github.com/djhenry/libeq/commit/39fb132fe0d206b7e5878213649eeb5e33c67d78) exposes the modern association fields needed for inspection, but lacks a preceding Region reference selected by flag `0x40`. That field follows user data and precedes the `0x100` reference. When both flags occur, the latter is the modern association. The expected target kind of a `0x40`-only reference is not established here.

A successful typed parse is not proof that this unsupported layout was understood: the document parser discards a successfully parsed body's remaining bytes. A byte-authored `0x140` control with references 111 and 222 returned 111 as the modern reference and left the actual 222 word unconsumed; the document still returned a typed Region. [Libeq issue #7](https://github.com/djhenry/libeq/issues/7) records the asset-free reproducer. Therefore gate `0x40` before trusting a parsed modern association. Parser support and byte-authored distinguishing-reference tests belong in libeq; source admission and ownership remain baker responsibilities.

## Recommended first inspector

The [proposed association inspection contract](wld-terrain-association-contract.md) makes this boundary concrete: exact first-profile flags, per-record observations, atomic logical budgets and authored acceptance oracles. It proposes linear tree-ordinal diagnostics with graph analysis explicitly not performed; cyclic/disconnected fixtures must terminate without filtering Region associations or claiming tree validity. This remains a specification for human review, with no runtime implementation or automatic selection.

Inspect one explicitly supplied document and its header evidence. Enumerate all source Region entries in document order, preserving unsupported placeholders. Record declared/actual counts, full Region flag frequencies, reference kinds, target kinds and modern records with zero observed resolved Region associations, qualified by unresolved coverage. Keep tree structure and ordinal diagnostics separate from terrain input membership.

Define an explicit supported Region layout/flag profile; a typed parse alone is insufficient admission. Initially resolve only positive modern references with checked subtraction, bounds and exact target-kind checks. When a Region declares a modern association, report null or named references, missing targets, and targets that are unknown, older or of the wrong kind as unsupported required associations. A supported Region without `0x100` is reported separately; it is not a missing required modern reference. A legacy-only region profile remains explicit unsupported coverage, rather than evidence of complete empty terrain. Header inactivity or inconsistent/missing required records must not become an available-empty world.

Preserve each region-to-mesh association in the report. Native imports a shared target repeatedly when multiple regions reference it; silent deduplication changes those source contributions. The selected frontend currently requires unique source slots, so feeding aliases into it needs a separately reviewed association/ownership contract. Contextual ordinals and slots do not create stable enhancement owner identities.

The inspector should return bounded observations and explicit incomplete/unsupported coverage, rather than a collision scene or publication-ready selection. A later caller may supply verified bindings to the selected frontend after the source profile is accepted. No automatic source certification or source query-frame preservation declaration follows from association inspection.

## Limits and acceptance

Work must be bounded before traversal, allocation and copies: document visits, Region/node/reference visits, association multiplicity, target tracking, flag-map cardinality, diagnostic entries and cumulative logical payload. Unsupported records still consume work. Named lookup stays outside the first profile; a later implementation must explicitly budget it. Byte ingestion and the selected normalizer/common writer keep their own separate limits.

Acceptance should use authored bytes through the actual parser, including an unreferenced modern mesh; an associated region outside BSP reachability; interleaved document/Region/node ordinals; an unsupported Region placeholder; repeated target associations; the `0x40`/`0x100` layouts with different references; null/named/out-of-range/wrong-kind targets; inactive or inconsistent headers; multiple trees with a failed last tree; and invalid child ordinals/cycles. Tree failures must remain explicit without silently redefining source membership.

Author and independent inspection must agree on bounded private sample counts and source associations. Synthetic fault checks should distinguish all-modern enumeration, BSP-reachability filtering, ordinal compaction, silent deduplication and trusting unsupported layouts. Any runtime implementation needs its own reviewed contract, exact logical charges and boundary tests. Human review precedes merging.
