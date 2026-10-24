# Tiled terrain: native initial-load preparation candidate

This proposal follows [issue #100](https://github.com/djhenry/eqoxide_asset_server/issues/100) within [M4](eqg-zone-support-project-plan.md#m4--eqtzpdat-terrain-and-materials). It specifies the next proof target, not an implemented or certified terrain bake. Source-specific preparation stays in the asset server; visual and collision outputs continue to use the common GLB contracts.

## What is established

Raw format decoding is separate from scene preparation. The proposed library readers for [EQTZP metadata](https://github.com/djhenry/libeq/pull/18) and [packed DAT v20](https://github.com/djhenry/libeq/pull/20) preserve source bytes and record boundaries. They do not establish a prepared surface, tile membership, resource precedence or gameplay collision. Neither contribution is treated here as merged or as a producer dependency update.

The following preparation facts were checked against the native RoF2 client:

| Boundary | Established behavior | What does not follow |
|---|---|---|
| Segment-collision provider | Supplies working heights and current quad flags | Untouched DAT heights and stored diagonal bits are not a sufficient preparation specification |
| Load traversal | Creates concrete tiles, then performs an ascending-x/ascending-y seam pass followed by an update pass in that order | Sorting an arbitrary selected subset is not proof of matching loader membership |
| Seam operation | Copies an east neighbor's first column to the current last column, then a north neighbor's first row to the current last row of working heights | Averaging all borders or inventing a corner owner is not established |
| Hierarchical refinement | Flags or child requests can force splitting. Otherwise, for finite comparison operands, the error test requests a split only when deviation exceeds tolerance; equality alone does not request one | Zero tolerance does not guarantee a fully split topology |
| Index representation | Full-grid indexing shares the same refinement passes as compact indexing | Full-grid indexing is not a maximum-detail override |
| Initial controls | The tessellator constructor initializes global tolerance to `1.0`; tile override state initially selects that global value | The final outer load/quality-setting order has not been verified |

Preparation can interpolate unused working midpoint heights and regenerate diagonal bits from emitted triangles. Different raw and working buffers do not by themselves prove different final surfaces: interpolated points might lie on the same emitted surface. Exact equivalence remains unproved.

Confirmed preparation paths include the initial load and a raw-height reset/edit path. Camera-, frame- or gameplay-triggered changes have not been established. Mutable buffers and an unrestricted tolerance setter are not sufficient evidence that the client needs runtime terrain preparation. A faithful fixed load snapshot may satisfy the intended bake architecture.

## Candidate to prove

The ordinary native packed-load path derives its DAT request from the parsed project `NAME` plus `.dat`, rather than substituting the descriptor stem. Namespace selection, loose/archive precedence and malformed-name admission remain separate producer policies; a resource-name observation does not establish those policies.

Target the actual initial-load preparation first, without forcing detail or changing authored flags:

1. Admit the complete concrete tile set using the loader's membership and coordinate rules, with bounded counts and explicit source/resource identities. Start with packed DAT v20 and `q = 32` as the limited geometry proof target. The broader positive dimensions accepted by the raw parser do not imply geometry-profile support.
2. Initialize working heights from raw heights and preserve initial quad flags. Retain the complete neighbor context; do not treat a missing required input as a proven outside boundary.
3. Reproduce the ordered seam pass, then the ordered preparation pass with self/east/north/northeast contexts, cached split state and dependency propagation.
4. Establish the actual outer control initialization. Constructor tolerance `1.0` is a candidate input until that ordering is verified; it is not a claimed universal final setting.
5. Reproduce the hierarchy, emission, working-height stores and diagonal regeneration with a stated numerical contract. Capture the appropriate render stream and prepared collision stream, comparing their surfaces rather than assuming equality or difference.
6. Convert verified results into the common coordinate basis once. Preserve canonical query units and the existing visual/collision component roles and pairing. Separate geometry is permitted if the proof requires it; sharing geometry requires evidence.

This candidate neither selects a deliberately forced full-detail policy nor introduces a runtime client capability. Source-specific hierarchy and seam rules belong in the bake. Future enhanced legacy content may use any resulting source-independent capability under the same contract.

## Proof required before a geometry bake

- Verify exact hierarchy neighbor links, emitted corner ordering, holes and dependency propagation, including missing outer neighbors and repeated preparation state.
- Verify loader tile membership, source-order versus preparation-order rules, duplicate coordinates and incomplete input refusal. Establish the geometry dimension domain rather than inferring it from the raw parser.
- Verify floating-point evaluation and stored-value boundaries. Widened arithmetic, ordinary `f32` arithmetic and native extended intermediates must not be assumed interchangeable at refinement thresholds.
- Verify outer load/quality initialization and mode-specific height-buffer selection. Do not call a constructor default the active profile without tracing its caller.
- Freeze independent expected prepared heights, indices and flags for asymmetric noncoplanar fixtures, exact-threshold ties, holes, forced-detail source bits, mismatched shared borders and corner cases. Include resource/work limits and explicit unsupported inputs.
- Compare render and collision surfaces and preserve any verified difference in their respective common components. Test winding after basis conversion; decoding success is not a query or movement certificate.

A next implementation plan should name the complete preparation profile, its input membership, numerical rules and independent oracle. It should not substitute a uniform diagonal, shortest-edge heuristic, raw stored diagonal or altered tolerance when proof is missing. Native behavior for unverified inputs remains unavailable, rather than becoming a successful-looking guessed surface.

## Scope and consequences

No new GLB schema, manifest requirement, producer route, dependency pin, material approximation, collision operation or gameplay activation is proposed here. Existing [visual](static-visual-contract.md) and [collision](static-collision-contract.md) contracts remain the output boundary. Reader/capability requirements must be reviewed if a later implementation actually adds one.

The additional cost is bake-side hierarchy, neighbor-state and numerical verification. Runtime terrain work is only a contingency if a required surface-changing runtime dependency is later demonstrated. A deliberately forced-detail policy or a source-height inspection approximation would be a separate fidelity choice to explain before adoption. The material/lighting scope decision remains independent.

This proposal enables neither production publication nor a native-query certificate. Raw readers and a plausible visual preview do not close M4; representative source, geometry, delivery and gameplay acceptance remain separate gates.
