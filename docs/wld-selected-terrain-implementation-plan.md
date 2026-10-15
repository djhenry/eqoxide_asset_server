# Selected WLD Terrain Normalization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Tracking:** [Implementation issue #84](https://github.com/djhenry/eqoxide_asset_server/issues/84).

**Goal:** Normalize explicitly selected modern WLD terrain into the existing common collision scene, excluding exact coincident-source-position faces with bounded diagnostics.

**Architecture:** A pure post-parse baker module borrows existing libeq records. It validates selected sources, prepares indexed geometry and returns a scene plus an inspection report. The existing writer and client decoders remain the source-independent admission boundary.

**Tech Stack:** Rust, locked libeq WLD parser, existing collision writer, authored binary fixtures and current CPU role/package decoders.

**Spec:** [Selected terrain contract](wld-selected-terrain-contract.md), with the owner's selected [coincident-face policy](wld-terrain-feasibility.md).

## Global Constraints

- Keep the existing libeq revision and the shared GLB/client standard.
- Select by zero-based document slot and supplied opaque owner identity; do not infer membership from names or render materials.
- Require the admitted static subset, canonical null animation reference and count consistency before output.
- Validate all indexes, including excluded faces. Source bit `0x10` exclusions precede exact raw-position coincidence exclusions; counts are disjoint.
- Preserve source vertex pools and retained face order. Decode supported exponents `0..=15`, add the finite center and map source axes once to server `(Y, X, Z)`.
- Other invalid geometry remains an atomic error. No small-positive-area threshold, vertex welding or general reconstructed-degeneracy skip is authorized.
- Return known-empty only after a nonempty valid selection has been fully examined. No error becomes empty success.
- Retain `ownership_verified: false`, `world_complete: false` and `full_native_query_preparation_applied: false`; identify the narrow preparation policy separately.
- The first bridge keeps query-unit frames unspecified. No production routing, byte ingestion, package activation, gameplay, query backend or native-parity claim.
- Use the contract's caller-lowerable hard limits. Publish the exact logical charge schedule and repeated-pass work bounds with boundary tests; logical charges exclude allocator overhead and prior parsing.
- Human review precedes merging.

## Review Focus

1. An invalid index on an otherwise excluded face must still refuse.
2. Distinct indexes with identical raw tuples must be excluded; three distinct collinear tuples or reconstruction-only collapse must refuse.
3. Unknown fragments must keep their positional selection meaning and never become empty output.
4. Output and diagnostics must conserve face counts, preserve valid neighboring faces and charge empty owners and repeated work.
5. Writer/client acceptance must expose geometry, owners and empty state through actual decoders, not merely JSON structure or a correctly computed hash.

### Task 1: Add the pure frontend and typed inspection contract

**Files:** Create `src/wld_collision.rs`; export it from `src/lib.rs`. Expose the existing triangle-admission helper in `src/static_collision.rs` only at crate visibility if needed for reuse.

- [ ] Define borrowed selection/input, caller-lowerable limits, typed refusal categories, source-reference diagnostics, per-owner face counts and bounded raw-flag histograms.
- [ ] Define a usage record and explicit logical charges for report fields, identity copies, borrowed selection scratch, duplicate tracking, histogram entries and output payload/containers. Establish allowances before each allocation/copy; document fixed bounded scratch and maximum passes.
- [ ] Write direct-record tests for all nine declared/actual count pairs and canonical animation-null discrimination. Run RED before implementation.
- [ ] Implement record validation without material lookup or dependency resolution.
- [ ] Implement checked positional selection, coverage accounting and duplicate-owner/slot refusal. Budget coverage visits and repeated selected lookups explicitly.
- [ ] Add exact-limit/one-below tests for every budget, including a valid nonempty selection producing empty geometry, with nonzero report costs. Verify overflow-safe refusal and invalid caller limits.

### Task 2: Normalize and diagnose source face preparation

**Files:** `src/wld_collision.rs` and its internal tests; `tests/wld_collision.rs` with shared authored fixture support if needed.

- [ ] Author binary WLD fixtures for actual `WldDoc::parse`, including a positional `Unknown` preceding a selected valid mesh. Do not construct private document internals.
- [ ] Write RED cases for distinct-index/coincident-position and repeated-index exclusions; source-flag precedence; mixed valid/excluded geometry; all-coincident known-empty; malformed excluded indexes; distinct-source collinearity; reconstruction collapse; and preserved unused positions.
- [ ] Implement index validation, complete raw-flag accounting, exact raw-tuple comparison, power-of-two reconstruction, source reflection and unchanged retained winding.
- [ ] Validate retained geometry through shared triangle admission; produce one indexed pool and identity instance per contributing owner. Keep reports for successfully examined empty owners.
- [ ] Check per-owner and total conservation: `source = flag_excluded + candidates`, `candidates = coincident_excluded + retained`.
- [ ] Exercise both WLD texture-coordinate representations, accepted mesh flags, exponent boundaries, source center, unsupported dynamics, bad materials and missing/duplicate bindings.

### Task 3: Accept through real collision output and client inspection

**Files:** `tests/wld_collision.rs`; add an authored fixture example only if needed to reproduce the writer-to-client pipeline.

- [ ] Pass normalized mixed and known-empty scenes through `write_static_collision`; check positions, retained indexes, owners, ordinary requirements and atomic writer refusal.
- [ ] Independently assemble a descriptor/manifest with actual common APIs and load it through freshly built current client CPU role/package probes.
- [ ] Check the source-to-server reflection and writer's later asset-axis rotation separately; never use the writer's axis helper as the WLD reflection.
- [ ] Include a correctly hashed package disagreement so integrity validation cannot hide a binding error.
- [ ] Mutation-check source eligibility, coincidence comparison, index validation order, axes, winding, center, exponent, owner identity, known-empty handling and budget charges. Require actual runtime assertion failures, exact restoration and fresh passing runs.
- [ ] Author and independent acceptance agent separately run the applicable suite and CPU pipeline. No live-game claim applies to this isolated producer module.

### Task 4: Publish the bounded unit for human review

- [ ] Update contract/implementation status and bake diagnostics documentation to match actual behavior and exact charge schedule.
- [ ] Independently review new prose references and title/body claims; run whitespace and privacy checks on tracked files and public text.
- [ ] Publish the implementation on its own branch/PR, clearly depending on the selected-source design, with exact verification results and remaining source-coverage limits.
- [ ] Leave merging and production rollout to human review.
