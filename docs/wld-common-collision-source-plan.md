# WLD source boundary for common collision inspection

Tracked in [issue #79](https://github.com/djhenry/eqoxide_asset_server/issues/79). This is a proposed implementation sequence for the approved geometry/query inspection milestone. It adds no runtime implementation, package activation or gameplay collision. The client continues to consume the source-independent [static collision contract](static-collision-contract.md).

## Start with the raw API that already exists

The inspected [libeq revision](https://github.com/djhenry/libeq/commit/86f44611bba397410293530e18e6fc8d2dccb575) exposes `WldDoc`, typed borrowed fragment iteration, modern `DmSpriteDef2` meshes and `Actor` placements through its public parser API. Modern WLD geometry does not need another byte-format parser. The first server adapter can consume these immutable records directly.

High-level visual helpers are unsuitable as the source of collision facts. They allocate decoded positions or flattened indices, discard face flags or field presence, and apply visual coordinate/placement conventions. In particular, `ObjectLocation::scale()` returns bounding radius and uniform scale as a pair; they are different quantities. Material and primitive helpers may resolve references and panic. Collision extraction must not depend on material validity, transparency, UVs, normals or vertex colors.

A future upstream contribution may expose an immutable encoded mesh view through the high-level `Mesh` wrapper and a borrowed source actor through `ObjectLocation`. Such accessors should retain raw slices, index order, complete flags, exponent, center, declared and actual counts, and optional fields without decoding, filtering, allocation or reference resolution. They should borrow from the document owner, permit checked indexing and preserve absent versus explicit zero. This is optional API ergonomics, not a prerequisite or a new physics interface. Source normalization and eligibility stay in the asset server.

Borrowing bounds work after parsing. Existing parsing owns vectors and strings and has legacy panic paths. Neither this plan nor an O(1) view promises bounded byte ingestion, panic-free parsing or a total process-memory limit. Byte ingestion needs its own reviewed unit.

The [selected terrain contract](wld-selected-terrain-contract.md) refines the positional selection, admitted flags, pinned-parser behavior and proposed budgets. The narrow coincident-position exclusion is owner-selected; the [implementation plan](wld-selected-terrain-implementation-plan.md) scopes the existing inspection milestone. [Native source feasibility](wld-terrain-feasibility.md) records the owner-selected coincident-face exclusion policy and a separate PFS ingestion defect.

## First adapter: selected modern terrain candidates

Accept an explicit bounded selection of modern `0x36` mesh references with caller-supplied stable owner identities. Validate references against their document context rather than relying on names or iteration order. Report the selected scope and unsupported records. Enumerating `Wld::meshes()` alone cannot establish full world coverage: it enumerates modern meshes and omits older geometry and other fragment kinds.

The first adapter is deliberately limited to selected static terrain records. Actor definitions, placement activation, additional definition meshes, hierarchy, animation, skin assignments, mesh operations, hulls and region membership require separate resolution. Refuse selected records with unresolved dynamic or other required semantics; do not silently flatten them or call an unextracted world empty. Unknown flags or optional source records need an explicit admission/diagnostic policy before implementation, with required interpretation distinguished from unrelated render-only data.

Verified against the native RoF2 client, the modern terrain candidate rule is `face.flags & 0x0010 == 0`. This establishes default terrain triangle membership independently of render materials. It establishes neither placed-actor activation nor movement, navigation, LOS or native query behavior. Retain complete face flags for diagnostics, including excluded triangles. The owner-selected preparation additionally excludes raw candidates with any exactly coincident pair of source position tuples, with explicit per-owner counts. Other invalid geometry still refuses; this is not full native query preparation.

Normalize once in the baker:

| Source fact | Proposed first adapter behavior |
|---|---|
| Encoded signed 16-bit XYZ position `q`, exponent `e` | Decode `q * 2^-e` in source XYZ; support only `0..=15` initially. Larger exponents are unresolved and refused, not masked or shifted. |
| Mesh center | Require the center-presence flag and finite center; add center in source XYZ. No safe absent-center default is established. |
| Server geometry axes | Map source XYZ to `(Y, X, Z)` before the common writer's existing asset-axis mapping. This is a source adapter reflection, not a client branch. |
| Modern WLD face order | Preserve `[i0, i1, i2]` through the reflection. Native WLD collision normals use the negative conventional cross product; preserving order produces that mapped orientation. EQG's source adapter reverses winding for its different source convention. |
| Triangle indexes | Validate every source reference before indexing, including excluded faces. Partition source-flag exclusions, raw candidates, coincident-source-position exclusions and retained faces. Other malformed required data must cause an explicit refusal. |
| Owner identity | Use a supplied stable opaque identity satisfying the common contract. Fragment or node indices are contextual references, not stable enhancement identities. |
| Older `0x2c` geometry | Detect and report unsupported coverage. Its face-word interpretation is unresolved; do not apply modern indexes or the modern eligibility rule. |

The source-to-server mapping is corroborated by established EQEmu mesh/map conventions. This investigation does not establish fresh packet-to-geometry or live-server alignment. The supported exponent range follows the native terrain decoder's initialized 16-entry quantum table; it does not declare larger encoded values universally invalid across all WLD uses.

The existing common writer remains the final admission boundary: finite local geometry, permitted affine transforms, and noncollapsed indexed triangles in the serialized f32 world frames. Do not bypass those checks or weaken them to admit a source fixture. Preserve reusable indexed meshes; do not expand each owner into a duplicate vertex pool.

## Placement work is a separate gate

The existing legacy visual placement helper must not become the authoritative common-collision transform. It ignores two rotation components and can substitute bounding radius for geometry scale; [issue #80](https://github.com/djhenry/eqoxide_asset_server/issues/80) tracks the separate fix and visual regression acceptance.

For the audited simple actor-instance source path, the ideal column-vector composition is:

```text
worldXYZ = translation + Rz(a) * Ry(-b) * Rx(c)
                         * uniformScale * (meshCenter + q * 2^-e)
```

The rotation operators take radians: `a = rotate_z * 2*pi/512`, `b = rotate_y * 2*pi/512`, `c = rotate_x * 2*pi/512`. The raw fields use 512 units per turn. The mesh center is added before actor scale and rotation. All three angles matter; bounding radius is a separate quantity. For example, encoded `q=(20,-28,44)`, exponent 2, center `(1,2,3)`, scale 2, translation `(3,-4,5)` and all three raw angles 128 produce ideal source-world position `(31,6,17)`. Native trigonometry uses interpolation, so analytic trigonometry is not a promise of bit-identical native coordinates. After source composition, conjugate the transform into server axes consistently with the mesh mapping.

In this particular source path, absent scale and present zero both reach an actual zero scale; no default one is established. Preserve their raw presence distinction and refuse both under the initial common contract's positive uniform scale profile. Absent location has no verified identity/zero-location default and must be refused. Do not generalize these observations to all actor APIs. Do not use a radius fallback, a `0.01` heuristic, or a guessed missing-field default.

Before supporting placements, resolve definition reference kind, every required mesh association, hierarchy, static ownership and activation. A successful first-mesh or name lookup cannot prove complete membership. Freeze a numerical transform policy and its fidelity limits, then test asymmetric multi-axis examples against an independent source interpretation. Additional unsupported source kinds stay visible in diagnostics.

## Bound work before doing it

The implementation should define explicit caller-lowerable limits with checked counters before scans, allocation and copies. Freeze exact defaults and error categories in the implementation PR.

- Count document fragments and selected references before collection or resolution. Bound repeated reference visits, string/name lookup and selection multiplicity; do not perform an unbounded name search for each owner.
- Charge raw vertices, all examined faces, retained indexes, included/excluded records and expanded owner/triangle work separately. Excluded faces still consume inspection work.
- Bound cumulative logical output, scratch buffers, maps, diagnostic records and copied identity strings. A final GLB limit cannot bound earlier intermediate allocations.
- Validate declared versus actual counts and optional-flag consistency, including manually constructed records. No unchecked shift, unchecked index subtraction, material lookup or silent zero/default conversion belongs in this path.
- Separate source preparation limits from the common writer's geometry, serialization and transformed-world limits. These are logical work/payload limits, not allocator overhead or RSS guarantees.

An available-empty result requires a successfully extracted explicit selection whose required records were examined and whose eligible candidate count is zero. Missing inputs, unsupported required records and unresolved required membership within the declared selection cannot become available-empty. Unresolved coverage outside that selection remains an explicit diagnostic and cannot be represented as an empty world. Report source scope, exclusions and unresolved coverage without claiming ownership verification or world completeness. Do not publish packages or advertise a segment-query capability from extraction alone.

## Reviewable implementation sequence

1. **Freeze the selected-source contract.** Define explicit selection/context, unsupported-record policy, owner binding, limits and refusal categories. Use existing borrowed parser records; decide whether a small upstream accessor contribution has real reusable value.
2. **Implement bounded modern terrain normalization.** Add a pure in-memory adapter to the existing common scene type. Keep legacy visual baking, ingestion, discovery, registry and gameplay unchanged in this unit.
3. **Accept through the actual writer and decoder.** Author and independent agent each exercise the source adapter, common writer and current client CPU decoder with synthetic source records. Mutate eligibility, axis mapping, winding, center, exponent guard, ownership, known-empty handling and work accounting to demonstrate meaningful refusals or changed observable geometry.
4. **Resolve placements and coverage separately.** Add verified selection/reference/hierarchy/activation policy and explicit fidelity choices before claiming actor or complete-world support. Representative private corpus inspection and live alignment are later gates; do not distribute native inputs.
5. **Activate paired packages only after source coverage acceptance.** Continue the shared manifest/component pairing and client capability gates. Segment-query arithmetic still awaits the separate [client proposal](https://github.com/djhenry/eqoxide/pull/1166); movement, volumes and dynamic actors remain later milestones.

The first acceptance matrix should include asymmetric signed coordinates, exponent boundaries `0`, `15`, `16`, `31`, `32`, `65535`, center presence and nonfinite data, mixed face flags and malformed indexes, count/presence inconsistencies, duplicate and missing owner bindings, exact budget boundaries and diagnostic amplification. Include unsupported older geometry, animation and actor dependencies, plus fully examined source-flag-excluded and coincident-position-excluded selections and missing/unextracted inputs. Preserve valid faces beside excluded faces and test count conservation without double counting. Test malformed material references without invoking visual helpers. Native eligibility and winding premises require independent source evidence; synthetic fixtures alone cannot establish them.

## Current evidence and limits

This proposal is based on a read-only libeq API inventory, native source-semantics investigation and existing common-contract code. A standalone Rust probe of the legacy signed-shift expression observed a negative factor at exponent 31, an overflow panic at 32 with debug assertions, and a factor of one at 32 in the optimized probe; [libeq issue #3](https://github.com/djhenry/libeq/issues/3) tracks that defect. That probe did not execute a libeq document or a native client. Neither a WLD common adapter, corpus conversion, full library regression suite nor live client test was run for this document. Separate tracked defects and implementation PRs must carry their own acceptance.
