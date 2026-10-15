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

Contributing records become indexed meshes with identity instances in the common collision scene. The WLD frontend maps source coordinates to server `(Y, X, Z)` and preserves source winding; the common writer then applies its asset-axis rotation. Query-unit frames remain unspecified in this first bridge. The client uses the existing shared GLB role/package decoders with no WLD-specific behavior.

The `wld_collision_fixture` example writes authored mixed, empty and deliberately inconsistent packages for CPU inspection. These fixtures exercise the real parser, normalizer, writer, descriptor and manifest APIs; they are not a native-zone conversion command or proof of gameplay fidelity.
