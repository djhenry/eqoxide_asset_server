# Proposed WLD terrain association inspection contract

This is the proposed first runtime boundary for [issue #86](https://github.com/djhenry/eqoxide_asset_server/issues/86), refining the [membership investigation](wld-terrain-membership-inspection.md). It is a specification for human review, not an implemented API or an implementation plan. It neither selects geometry nor emits GLB. The existing source-independent collision format and the selected frontend's diagnosed coincident-position exclusion remain unchanged.

The purpose is to answer **which modern mesh associations are exposed by this document's Regions, and what could not be inspected?** It must not answer whether a zone has available collision. Inspection produces no scene, enhancement owner identities, canonical query frames, package capability, source certification, native query agreement or complete-world claim. Materials, vertex/face data, actors, visibility contents and legacy geometry are not resolved.

## Input and parser boundary

Accept one immutable borrowed `WldDoc`, limits, and an optional caller context consisting of exactly 32 opaque bytes. Context is an unverified caller label, not a content hash or a binding certificate; copy neither source names nor arbitrary text. Derive header evidence from that same document. Do not accept an unrelated caller-supplied Region count.

At the pinned [libeq revision](https://github.com/djhenry/libeq/commit/39fb132fe0d206b7e5878213649eeb5e33c67d78), `header_bytes()` allocates a fixed 28-byte serialization of seven stored little-endian words; Region count is bytes `12..16`. Reserve its logical charge before calling it. This upstream helper uses infallible allocation; inspector error handling cannot guarantee recovery from its allocation failure or a global out-of-memory abort. This binds observations to stored header state in this in-memory document, not to original input bytes, full body consumption or successful ingestion certification. Magic and version are recorded without validation: no header-admission or native-loader-acceptance claim follows. Existing parsing, strings and fragment storage are outside this inspector's limits. Parsing can allocate or panic before inspection; this contract does not repair that boundary. No upstream dependency or header-accessor change is proposed here.

Use `fragment_count()`, `iter()` and checked `at(slot)` access. Slots are zero-based document positions. Count original kind `0x22`, including `Unknown` of that original kind, to construct one-based Region ordinals. Select the last original kind `0x21` as the tree candidate, even if it is `Unknown`; never fall back to an earlier typed tree. Typed-only iteration would erase failed records and is prohibited for these operations.

## Deterministic report schema

The proposed identifier is `wld-terrain-association-observations-v1`. This is baker inspection data, not additional GLB metadata. All enums below are fixed tags; absent numeric values have an explicit presence tag. Error and observation labels are static values, not copied source strings.

| Collection or field | Required observations and order |
|---|---|
| Header | Seven stored words, actual document/Region counts, context presence/value; `stored_region_count_gate = zero / nonzero`; `region_count_agreement = equal / different`. These describe stored values only, not loader activation, header admission or collision availability. |
| Regions | Exactly one record per original `0x22`, in document order: document slot, one-based Region ordinal, typed/unknown body, optional complete raw flags, layout status, count/pool summary when typed, association status and optional raw reference representation/target classification. Unknown Regions retain ordinals and have unavailable flags, not flags zero. |
| Flag histogram | All known raw flag values, including unsupported values, with occurrence counts. Order is first occurrence among typed Regions, not numeric order. Report a separate unavailable-flags count for unknown bodies. Histogram totals plus unavailable count equal Region count. |
| Modern records | One record per original `0x36`, in document order: slot, typed/unknown body, observed resolved association multiplicity. A typed target's multiplicity may exceed one. An unknown `0x36` is never a resolved modern target. |
| Tree | Total original-tree count, selected candidate slot, `missing / unknown_body / typed` status, declared and actual node counts when typed, and their agreement. Earlier tree bodies are not traversed. |
| Nodes | One record per actual node of the selected typed tree, in node-vector order: one-based node ordinal and the three raw reference representations/classifications. Region references additionally retain the target Region's original document slot/body status when in range. No node geometry or plane values are copied. |
| Scope and usage | Separate header, association-layout, unresolved-association and legacy/inline summaries; `graph_analysis = not_performed`; exact charged usage. No single `complete` or `available_empty` flag. |

Summaries count observation classes without creating additional per-error diagnostic arrays. A node or Region can carry several independent fixed status fields; errors do not amplify copied text. Repeated associations remain separate Region records, and their target multiplicities remain visible. These contextual ordinals/slots are not stable owner identities. In particular, the selected frontend's unique-slot requirement is not authorization to deduplicate associations or feed this report to normalization.

### Region layout and coverage

The first engineering profile accepts exact full flag values **`0x81`, `0x101`, `0x181`**. This is not a universal native-valid bit mask. It is the smallest exact flag set covering the six recorded sample histograms in the membership investigation; the 26 recorded `0x101` Regions must not be described as covered by an `0x81`/`0x181`-only profile. `0x101` uses the parser's word visibility representation; both other accepted values use byte visibility representation. Contents are not interpreted here.

Classify a typed Region as `unsupported_layout` for any other flags, including any `0x40` bit, **before reading or trusting its parsed modern-reference value**. The pinned parser omits that preceding field and tolerates trailing body data. Upstream [libeq PR #8](https://github.com/djhenry/libeq/pull/8) has merged, but this inspector proposal does not update the asset server's pin or relax this gate.

For accepted flags, check only the accessible optional-field consistency: sphere present, reverb volume/offset absent, modern reference present iff `0x100` is set. Inconsistency becomes an explicit observation and prevents resolution of that Region's association. A no-`0x100` Region with no modern-reference field is `no_modern_field`, not a failed required modern association.

For every typed Region, retain the eight declared counts and the seven accessible collection lengths (`region_vertices`, `proximal_regions`, `render_vertices`, `walls`, `obstacles`, `vis_nodes`, `visible_lists`) using O(1) length reads. Do not traverse them. Do not require universal declared-count/vector equality: the pinned parser conditionally consumes render vertices, and cutting obstacles have no separately exposed collection. Do not serialize a Region to access private user data. Full body consumption, user data and visibility semantics remain unverified.

Independently mark `legacy_or_inline_present` when any of the region-vertex/render-vertex/wall/obstacle/cutting-obstacle declared counts or corresponding exposed geometry pools is nonzero. Otherwise mark `no_legacy_or_inline_observed` for that typed record; an unknown body is `unavailable`. Proximal and visibility counts alone are not evidence of geometry. This coverage status neither suppresses an otherwise inspectable modern association nor certifies absence of unsupported semantics. No-reference Regions with inline/legacy data therefore remain explicit outside coverage, never empty collision.

### Modern reference resolution

Inspect exactly one association status for every Region, including unknown bodies and unsupported layouts. Only an accepted, optional-field-consistent Region with `0x100` may resolve its reference. Pattern-match `FragmentRef`; do not call the high-level resolver or copy/search string data.

| Representation | Observation |
|---|---|
| Expected no field | `no_modern_field`. |
| Required field absent or unexpected field present | `inconsistent_optionality`; do not resolve. |
| `Name(0)` | `null_required_reference`. |
| Negative `Name`, including `i32::MIN` | `named_unresolved`, preserving the signed value without negation or absolute value. |
| Positive `Index` representable as signed i32 | After validating positivity, subtract one, then perform one checked `doc.at` lookup. Report `missing_target`, `resolved_modern`, `unknown_modern_target`, `older_geometry_target` only for exact typed `DmSpriteDef`, or `wrong_kind_target` retaining original kind and exact typed variant/unknown body. Original `0x2c` alone is insufficient: the parser can dispatch it as other typed variants or `Unknown`, none of which establishes older EQ geometry. |
| `Index(0)`, `Index` above i32 maximum, or nonnegative nonzero `Name` | `invalid_reference_representation`, preserving variant and original numeric value; public enums permit in-memory mutation beyond genuine parsed representations. |
| Unknown Region or unsupported layout | `unavailable_body` or `unsupported_layout`, without inventing an absent/zero reference. |

Only an exact typed `DmSpriteDef2` increments resolved association counts and its slot's multiplicity. Missing, named, null, unknown, unsupported and inconsistent cases remain unresolved required coverage. Resolve aliases independently; no unique-target cache may erase contributions. Source slots use document indexing, not Region/node ordinals.

Report counts of typed modern records with **zero observed resolved associations**, and the number of distinct typed targets with at least one such association. Zero observed edges does not establish genuinely unassociated source geometry: unsupported layouts, unknown Regions or unresolved references may hide edges. Define `unresolved_region_association_count` as every Region association status except `resolved_modern` and `no_modern_field`, so `R = L + no_modern_field_count + unresolved_region_association_count`. Here `L` counts occurrences, not distinct targets. Even when unresolved count is zero, this profile does not certify native completeness or approve automatic all-modern selection.

### Linear tree diagnostics, separate from membership

Inspect all actual nodes of the selected typed tree once, even when declared node count disagrees. Child references use one-based **node** ordinals; node Region references use one-based **Region** ordinals, including unknown Region placeholders. Only canonical `Name(0)` is absent. As in the modern-reference table, `Index(0)`, an `Index` above signed i32 maximum, and positive nonzero `Name` are invalid representations. Negative `Name` is an invalid ordinal-domain value, preserving its signed value; it is not an unresolved document-name lookup. Check representable positive child ordinals against actual node-vector length `N`, and positive Region ordinals against actual original-kind Region count `R`, including unknown placeholders. Declared tree/header counts do not replace those bounds, and agreement or disagreement with them does not establish validity. Never pass these wrappers to document-reference resolution. An in-range Region ordinal pointing to an unknown body retains `unknown_body` target status, not a shifted Region.

No traversal, root interpretation, reachability, cycle detection, recursive walk, plane evaluation or query-readiness assessment occurs. `graph_analysis = not_performed` is mandatory, including for a tree whose ordinals are all in range. Cyclic and disconnected graphs terminate after the same linear field checks and leave Region associations unchanged. Missing, unknown or count-inconsistent trees are observations, not grounds to erase links or return an available-empty world. No result may label such a tree `valid` merely because ranges pass.

This deliberately keeps graph validation outside the first milestone. A bounded iterative graph validator could report reachability/back-edges, but would add root semantics, scratch state, edge work and acceptance without changing the all-Region association rule. It requires a separate reviewed contract if subsequently needed. Linear ordinal diagnostics are the smaller first proposal, not a claim that cycles are harmless to a later consumer.

## Atomic limits and exact logical accounting

All numbers here are **proposed engineering limits**, not native limits, measured ingestion limits, allocator layout, serialized report size or RSS guarantees. Use checked u64 arithmetic. Caller limits may lower but never exceed these defaults/hard maxima. Zero is a valid lowered cap, not an invalid-limits error: a zero-use domain may fit it, while any scheduled positive charge refuses normally. Even empty documents require the fixed positive header/report/container/work charges to fit. Validate the entire limits object before any source work. Budget exhaustion, arithmetic overflow and failure of inspector-managed fallible reservations return a tagged error with counter/stage information; no partial report escapes. This does not guarantee recovery from the upstream header helper's infallible allocation, global out-of-memory aborts or arbitrary upstream panics. Structural anomalies described above return observations, never silent defaults. Unexpected failure of the pinned header API to return 28 bytes is an inspector-input/API invariant error, not an inactive header. A missing accepted modern reference, a resolved document slot outside target scratch, or an in-range Region ordinal outside the completed Region report slice also returns an explicit inspector/API invariant error. These defend cross-function assumptions; they are distinct from ordinary source observations such as `inconsistent_optionality`, `missing_target` and `out_of_range`. The immutable public construction currently keeps those private assumptions consistent; exercising inconsistent private helper inputs does not establish a reachable malformed-document failure.

| Counter | Default and hard maximum |
|---|---:|
| Document slots `D` | 100000 |
| Region records `R` | 100000 |
| Modern records `M` | 100000 |
| Selected-tree actual nodes `N` | 100000 |
| Distinct known flag values | 256 |
| Positive modern lookup attempts `P` | 100000 |
| Resolved association occurrences `L` | 100000 |
| Document visits | 300000 |
| Reference classifications/visits | 400000 |
| Histogram equality comparisons `C` | 1000000 |
| Total work units | 2000000 |
| Cumulative logical payload bytes | 67108864 |
| Cumulative resource entries | 600000 |

Each hard maximum is an independent ceiling, not a promise that every combination below the individual maxima succeeds. In particular, a distribution with many known flag values may exhaust histogram comparisons or total work while its Region count remains below the Region-record ceiling. Refusal at that earlier scheduled budget is intentional. The first-occurrence linear histogram and its comparison charges remain part of this v1 profile; changing the lookup strategy or its schedule requires a separate reviewed contract change.

The implementation must follow this charge schedule, rather than substitute a final-output-size check:

1. Charge one fixed report (512 logical bytes, one resource), the header container (24 bytes, one resource) and its 28-byte payload before calling `header_bytes`. Read its seven words, charging seven work units. Check O(1) document length against `D` before iteration.
2. One document pass counts original kinds, preserves last-tree slot and borrowed candidate record, and obtains `D`, `R`, `M`, `N` without copying bodies. Retain that borrow for subsequent tree inspection; no additional uncharged `doc.at` retrieves it. Charge a document visit/work unit before each slot inspection. Enforce each count limit before increment. Only the last typed tree's vector length contributes `N`; earlier tree node vectors are untouched. Let `Q = min(R, 256)` be reserved histogram capacity, not observed cardinality.
3. Before allocating report/scratch arrays, charge their entire logical capacities using the table below, with checked arithmetic. Reserve each bound once; no repeated growth/copy strategy, sorting, hash table or name scan is part of this profile. Count capacity entries even when unused. Release never refunds a cumulative charge. The fixed scalar report and counters remain covered by their initial charge.
4. Initialize `D` target multiplicities to zero, charging `D` work units. A second document pass charges a visit/work unit per source slot and emits Region/modern records in order. Charge one Region work unit and one reference classification/work unit per Region, including unknown/unsupported cases. The Region report array itself supplies the ordinal-to-slot/body table; allocate no duplicate table. Every typed Region scans histogram entries in first-occurrence order, charging one comparison/work unit before each equality check and stopping at the first equality. On equality increment that entry; after a complete unequal scan append a new entry. `C` counts all checks, including those preceding and including an equality for a repeated flag value. A new value beyond the configured cardinality limit is a budget error before insertion.
5. Charge one lookup attempt, document visit and work unit immediately before every positive checked `doc.at`, including missing/wrong-kind targets. Before each resolved association multiplicity increment, charge one `L` work unit and check both global and per-target checked counts. No target geometry is read. Finalize each modern record from the slot-indexed scratch counter in one charged work unit, `M` total.
6. After the Region report array exists, classify each actual selected-tree node in one node work unit plus three reference classifications/work units. Each field is examined once; checked indexed Region-report lookup is included in that field's unit. No tree graph traversal or extra document lookup occurs.

| Owned capacity | Logical bytes | Resource entries |
|---|---:|---:|
| Five array containers: Region reports, modern reports, node reports, histogram, target multiplicities | `5 * 24` | `5` |
| Region reports | `256 * R` | `R` |
| Modern reports | `32 * M` | `M` |
| Node reports | `96 * N` | `N` |
| Histogram capacity | `16 * Q` | `Q` |
| Target multiplicity scratch | `8 * D` | `D` |

These are fixed contract charges per capacity element, irrespective of optional-field presence or Rust representation. They are not `size_of::<T>()` measurements and must not be replaced by allocator-dependent sizes or compile-time layout assertions. Rust layout changes do not automatically change these logical units; changes to owned resources or to the accounting profile require explicit contract review. Borrowed fragment contents, fixed scalar/stack arithmetic and allocator overhead are excluded explicitly. Header storage is a separate sixth container, already charged in step 1; no copied identity strings or secondary diagnostic collections exist. Implementations using additional owned scratch, copies or containers must charge them under an amended contract before claiming this profile.

On a successful run, exact totals are:

```text
document_visits = 2*D + P
reference_visits = R + 3*N
payload_bytes = 684 + 256*R + 32*M + 96*N + 16*Q + 8*D
resource_entries = 7 + R + M + N + Q + D
work_units = 7 + 3*D + 2*R + 4*N + C + P + L + M
```

`684 = 512 + 6*24 + 28`. Limits are checked before their operation; counts must not wrap, allocate first or publish a successful prefix. In addition to `D/R/M/N/P/L/C`, the usage report retains observed histogram cardinality, document/reference visits, payload/resources and total work. Histogram capacity `Q` and observed cardinality are distinct. Histogram work is O(`R*Q`) with an explicit comparison cap; other work/storage is linear in the listed bounded counts. No hidden per-association full-document lookup is allowed.

## Authored oracle and future acceptance

This small asymmetric model is an independently hand-derivable **proposal example**, not a runtime execution result:

| Zero-based document slot | Record |
|---:|---|
| 0 | Typed modern mesh A. |
| 1 | Region ordinal 1, flags `0x181`, positive document reference 1 to A. |
| 2 | Unknown originally tagged `0x22`, retaining Region ordinal 2. |
| 3 | Typed modern mesh B, no observed resolved link. |
| 4 | Last typed tree, three declared/actual nodes. |
| 5 | Region ordinal 3, flags `0x101`, positive document reference 1 to A again. |
| 6 | Typed modern mesh C, no observed resolved link. |
| 7 | Region ordinal 4, flags `0x81`, no modern-reference field. |

Header Region count is 4. Accepted typed Regions have consistent optionality and zero inline/legacy geometry counts/pools. Node 1 has front child 2/back 0/Region 1; node 2 has both children 0/Region 2; node 3 has front child 3/back 0/Region 3. The node-1 component does not include node 3, and node 3 has a self-edge, but the inspector does not analyze these graph properties or assign native root semantics. It emits all three nodes and both A associations regardless. Region ordinal 2 targets the unknown body at slot 2 without compaction.

Expected `D=8, R=4, M=3, N=3, Q=4, P=2, L=2`. Histogram first-occurrence order is `0x181, 0x101, 0x81`, each count 1, with unavailable flags 1. Linear insertion makes `C=0+1+2=3`. A multiplicity is 2; B/C multiplicities are 0, explicitly qualified by unresolved Region-body coverage. Distinct resolved modern targets are 1. The stored Region-count gate is nonzero/counts agree; graph analysis remains not performed. Exact successful usage is **18 document visits, 13 reference visits, 2220 logical bytes, 29 resource entries and 61 work units**. Supplying exactly these caps succeeds; lowering each nonzero counter below its usage refuses atomically at the relevant check. Lowering histogram cardinality to 2 refuses before inserting the third flag. Unused reserved histogram capacity still contributes its fourth entry to payload/resource totals.

Before any implementation can be accepted, author and independent reviewer must separately derive expected reports/accounting and execute asset-free byte fixtures through the actual pinned parser and proposed inspector. They must not share a fixture-construction helper or infer expected values from implementation output. The model above must be supplemented with a differently ordered asymmetric model and exact boundary/refusal cases for every counter. Parser construction limits remain separately controlled; direct in-memory mutation tests may exercise representational/count inconsistencies inaccessible through successful parsed arrays.

Required discriminating cases include all-modern enumeration versus Region associations; repeated shared targets; an associated Region outside a chosen tree component; unknown Region ordinals; unknown last tree versus an earlier typed tree; `0x101` word visibility with a nonempty entry above 255 to distinguish it from the byte profile while leaving contents uninterpreted; `0x40`/`0x100` with different reference words; unknown/unsupported flags; null, named/minimum signed, missing, exact typed older geometry, alternate typed/unknown `0x2c`, wrong-kind and unknown-modern targets; optional-field disagreement; zero/nonzero stored Region-count gates and inconsistent headers; missing tree; declared/actual node mismatch; invalid child/Region ordinals including negative ordinal-domain values; disconnected and cyclic nodes terminating with `graph_analysis=not_performed`; and legacy/inline coverage independent of modern links.

Mutation checks must demonstrate changed observations or refusal for all-modern enumeration, reachability filtering, ordinal compaction, last-tree fallback, silent alias deduplication, unsupported-layout trust and budget bypass. Ordinary in-range cyclic data must not become a false tree-valid claim. Diagnostic amplification must remain bounded by the fixed record capacities. Exact restoration and fresh passing verification are required after faults. These are future acceptance requirements, not tests claimed to have run for this document. The existing six-sample receipts establish historical source associations only; no new native asset scan, normalizer/common-writer run, native execution or live client test accompanies this proposal.
