# Selected WLD terrain bake diagnostics

`wld_collision::normalize_selected_wld_terrain` prepares explicitly selected, already parsed modern WLD records for the shared collision writer. It does not select a zone's terrain automatically, parse asset bytes, publish packages or enable gameplay collision. See the [selected terrain contract](wld-selected-terrain-contract.md) for admission rules and ownership limits.

## Exclusions and errors

The normalizer validates every source face index first. It then excludes faces carrying source flag `0x10`. Among remaining candidates it excludes only faces with at least two exactly equal raw signed XYZ positions. This includes distinct indexes with coincident positions and repeated valid indexes. It preserves the original position pool, valid neighboring faces, retained face order and caller-supplied owner identity.

The returned report identifies policy `wld-selected-terrain-coincident-exclusion-v1`. For each owner and for the total it records:

- `source_faces`: all examined faces;
- `source_flag_excluded`: faces excluded by source flag;
- `raw_candidates`: faces remaining after that flag exclusion;
- `coincident_source_positions_excluded`: candidates excluded by exact raw-position coincidence;
- `retained_faces`: candidates retained for common admission.

The partitions conserve counts: `source_faces = source_flag_excluded + raw_candidates` and `raw_candidates = coincident_source_positions_excluded + retained_faces`. A source-flag exclusion takes precedence over coincidence, so no face is excluded twice. Per-owner raw-flag histograms count all source faces, including exclusions. Reports retain context, slot, exponent, center, mesh flags and raw reference kind/value without copying names or resolving materials.

A nonempty valid selection with no retained faces returns `AvailableEmpty` and retains owner diagnostics. Missing or unsupported selections, malformed indexes, inconsistent records, three-distinct-position raw collinearity, reconstruction collapse and exhausted budgets return errors with no partial scene/report. There is no epsilon filter or general degenerate-face skip. Geometry also passes the existing common local and serialized-world admission checks.

`ownership_verified`, `world_complete` and `full_native_query_preparation_applied` remain false. The policy identifies this narrow preparation rule, not full native query preparation. The caller's bake identity must include actual source identity, explicit selection, owner assignments and this policy version; the context token alone is not a verified digest.

## Logical accounting

`WldCollisionUsage` exposes the ten counters in `WldCollisionLimits`. The [contract's limit table](wld-selected-terrain-contract.md#result-and-logical-limits) gives their caller-lowerable hard maxima. Payload accounting uses these fixed engineering units, independent of Rust layout:

| Payload | Logical bytes |
|---|---:|
| Fixed result, report and usage fields, including output collection handles | 256 |
| Prepared collection handle / borrowed prepared entry | 24 / 80 |
| Each duplicate-set handle / slot entry / borrowed owner entry | 24 / 8 / 16 |
| Reusable flag bitset, allocated once | 8192 |
| Per-owner report including text and histogram handles | 160 |
| Output histogram entry, flag plus count | 10 |
| Temporary histogram map handle / entry | 24 / 16 |
| Contributor mesh / primitive / instance | 48 / 24 / 96 |
| Output position / retained index | 12 / 4 |
| Copied context and owner text | UTF-8 byte length |

Charges are cumulative, including temporary collections later dropped. Context text is copied once; each owner is copied into its report and once more into its output instance if it contributes geometry. Resource entries count collection containers and entries: prepared storage, duplicate sets, bitset, owner reports, temporary maps, histogram output entries, and contributing meshes, primitives and instances. The fixed root fields are not a collection entry. These limits exclude allocator overhead and storage already owned by the parser; they are not an RSS guarantee.

Preflight checks selected records, indexes and raw face eligibility, reserves all logical output allowances, and collects bounded flag diagnostics before decoding any positions or copying identity text. Two passes over each selected record’s faces classify/count and tally flags; contributing records incur a third pass to emit retained indexes. Touched bitset entries are cleared during the second pass, avoiding a full flag-domain scan per owner. Every selected vertex is decoded once, including unused positions in empty-output owners. Source geometry is charged once as `6 * vertices + 8 * faces`. The common writer applies its own separate limits.

## Output boundary

Contributing records become indexed meshes with identity instances in the common collision scene. The WLD frontend maps source coordinates to server `(Y, X, Z)` and preserves source winding; the common writer then applies its asset-axis rotation. The normalizer itself does not declare query-unit frames; callers choose the existing common writer mode. The client uses the existing shared GLB role/package decoders with no WLD-specific behavior.

The `wld_collision_fixture` example writes authored mixed, empty and deliberately inconsistent packages for CPU inspection. These fixtures exercise the real parser, normalizer, writer, descriptor and manifest APIs; they are not a native-zone conversion command or proof of gameplay fidelity.

```sh
cargo run --locked --offline --example wld_collision_fixture -- OUTPUT_DIRECTORY [--declare-preserved-source-query-frames]
```

Without the flag, query-unit frames remain unspecified. The optional flag asserts preservation of the supplied source collision query frames and adds exactly `eqoxide-source-query-frames-preserved-v1` to the collision requirements; the descriptor and manifest propagate those requirements. Missing output, misplaced, unknown, duplicate or trailing options refuse before output is written. The disagreement package remains deliberately inconsistent and must be refused by the package decoder.

The authored `wld-selected-passed-record-query-frames-v1` profile assumes selected records use source world units after decoding each position as `raw * 2^-exponent + center`. This example explicitly uses exponent 2 and a nonzero center, then the documented axis conversion, with identity source instances; shared fixture defaults are unchanged. Mode and profile version participate in its deterministic bake identity alongside the full example source bytes, selection and normalization policy. The authored exponent-2 fixture deliberately differs from older artifacts generated with the shared helper default; that helper default remains unchanged. The naming correction adds no geometry change, but intentionally changes revisions and bytes even in unspecified mode. These generated artifacts carry no historical byte-compatibility promise. Both modes preserve geometry and diagnostic truth flags; known-empty output remains JSON-only.

The eqoxide assertion preserves the supplied source collision query frames and is conditional on those passed-record units. Unknown source frames must stay unspecified. It asserts no universal EQ/RoF2 units or game identity and does not verify importer provenance. The retired `--canonical-query-units` flag and `static-collision-query-unit-frames-v1` capability have no accepted alias. It does not verify source history, complete terrain membership, WLD actors, full native preparation or numerical query agreement. It adds no discovery, lighting, query operation, online publication or gameplay activation.

## Binding inspected Region associations

`wld_associated_terrain::normalize_associated_wld_terrain` composes a fresh inspection of the same borrowed document with the selected frontend. The caller supplies an opaque source context and a nonempty binding slice containing one `(region_slot, owner_id)` for every resolved modern Region association, in Region order. `region_slot` means a zero-based document slot of the Region, not its ordinal or its target mesh slot. Owner tokens pass unchanged to the selected frontend for validation.

The initial profile requires a positive stored Region count agreeing with the document, known accepted Region layouts and no observed inline/legacy geometry or unresolved required references. It explicitly refuses shared targets, missing/extra bindings and mismatched Region slots. It preserves source Region order regardless of tree reachability, and leaves unassociated meshes in diagnostic coverage. An input with no resolved modern associations returns an error; a valid nonempty selection with all faces excluded may still return `AvailableEmpty` with owner diagnostics.

An accepted `NoModernField` Region requires no owner binding and contributes no selected modern mesh. It remains in the returned `association_report.regions` and `scope.no_modern_field_count`; the result retains that diagnostic rather than silently discarding the Region. Such a Region still must have an accepted typed layout and no observed inline/legacy geometry. This status does not certify absence of other source semantics or full-world coverage.

The result contains `association_report`, `normalized` and separate `binding_usage`. `WldAssociatedTerrainLimits` holds `inspection`, `binding` and `normalization` limits. Binding limits validate first, followed by the supplied-length and fixed-root preflight and the initial one-unit binding work charge, before association inspection. A zero binding work cap therefore refuses before inspection. Each existing component validates its own limits before its phase; the inspector's separately bounded work is not charged to the binding work domain. For R Regions and S successfully matched bindings, exact binding usage is S bindings, `2 + 2*R + S` work units, `88 + 24*S` logical bytes and `2 + S` resource entries. Default/hard caps are 100000 bindings, 300002 work units, 2400088 bytes and 100002 resources. The only new collection is a precharged borrowed selection vector; owner/context copies remain charged by the normalizer. The `Bindings` preflight charges the supplied slice length before inspection and before the missing/extra comparison, bounding oversized caller input early. An oversized supplied slice exceeding that cap returns `BindingBudgetExceeded` even when it would also be `ExtraBindings`; with sufficient budget it reaches the ordinary missing/extra diagnostic. Successful usage remains S matched bindings because success requires supplied and required counts to agree. The selection loop uses checked binding access and verifies its consumed count before normalization; an internal report/traversal disagreement returns a distinct invariant error, rather than masquerading as a caller binding-count error. These logical charges are not ingestion or RSS bounds.

This handoff supplies no persistent ownership registry, source certification or complete-world claim. Existing ownership/world/native-preparation flags remain false. The ordinary common writer continues to emit unspecified query-unit frames; no production routing, publishing or activation follows from a successful handoff.
