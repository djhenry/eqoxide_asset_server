# Source lighting diagnostics

`eqg::lighting::diagnose` inspects an already assembled `BinaryZoneScene` and
explicit caller-provided `ExternalResource` inputs. It returns borrowed packed
data and target/resource diagnostic rows. It performs no filesystem access,
archive extraction, provider discovery, color decoding, rendering or bake work.

The report borrows the source archive path, descriptor identity/location and zone
version alongside its rows. The caller owns the scene and every resource byte
buffer. Resource names and
provenance labels are supplied by the caller; a provenance label does not prove
native provider selection. Every supplied resource appears in the result,
including unused resources. Matching uses this API's explicit ASCII-case-fold
policy; duplicate case-equivalent input names fail, even when unused. This is
not a claim about native provider precedence or case behavior. A missing candidate
means only **missing in these explicit inputs**.

## Target identity and candidate selection

There must be exactly one distinct terrain mesh. Version 1 emits its terrain
row, followed by one row per placement. A terrain candidate comes from the actual
resolved terrain member spelling: remove the final four bytes and append `.LIT`.
`NamingBasis::ResolvedTerrainMemberDiagnostic` labels this as a diagnostic
candidate. The native loader request spelling is not retained separately by the
assembler and can differ from the resolved member spelling; this candidate does
not certify literal native request spelling or lookup behavior. This API accepts
only a nonempty stem with a `.ter` extension, under ASCII case folding.

Version 1 placement candidates append `.LIT` to the raw placement name, without
removing a model extension. They never use a runtime actor name or the model's
resource name. Empty, non-UTF-8, path-bearing or control-bearing names are refused
explicitly. Raw placement bytes remain borrowed and unchanged, including names
that cannot safely produce a candidate. These conservative refusals are this
API's policy, not claims of native graceful rejection.

Version 2 emits terrain from placement record zero's inline words, followed by
one row for every remaining placement's own inline words. Record zero's model
reference need not point to terrain: its raw placement identity and resolved mesh
identity remain separate from the terrain target. Inline selection suppresses
external candidates even when it is empty or mismatched. A missing record zero is
explicitly unsupported, with no external fallback. Supplied external resources
still remain visible in the resource diagnostics.

Each row retains placement ordinal, raw name, nullable original model-table slot,
nullable resolved placement mesh, target mesh, exact source member name, and
current source-pool count. Null references and repeated terrain references are
unassigned/unsupported object targets, not successful native association. A
nullable reference report does not promise native graceful fallback. Placements
sharing a deduplicated mesh remain separate owners; version 2 arrays retain their
own record ordinal and borrowed bytes.

## Raw shape results and limits

Classic external resources use the versionless `EQGP` header and counted raw
little-endian words. The raw libeq parser rejects malformed, truncated and
unsupported inputs, including headerless arrays. The original input is still
borrowed in the report, and parser errors remain available. Successful parsed
views expose complete packed words/bytes and the opaque suffix without copying
or interpreting them. Inline arrays expose their complete borrowed packed bytes.
There is no redundant sample vector.

Word results distinguish `Empty`, `DiffersFromCurrentPool` and
`MatchesCurrentPoolOnly`. Counts refer to the complete assembled mesh vertex
pool in source order, independent of triangle corners or material grouping. An
empty array is reported as empty even when the current pool is also empty.

**Count equality is not a native lighting acceptance certificate.** LOD consumers
can require concatenated additional source pools. Hierarchical consumers have a
different binding path. Provider resolution, LOD enumeration, hierarchy semantics,
channel meaning, gamma, coverage and applying lighting are unimplemented here.
Every row has a `ConsumerScope`: ordinary pools have unresolved native context,
nonzero bone counts report a hierarchical consumer as unsupported, and unassigned
owners remain explicit. No ready/applied/complete association flag is exposed. Existing visual bridge
extension refusal and GLB/runtime behavior remain outside this API.

`DiagnosticLimits` defaults are:

| Limit | Default |
| --- | ---: |
| Target rows | 100,000 |
| Supplied resources | 10,000 |
| Aggregate logical records | 16,000,000 |
| Aggregate name/scan units | 16 MiB |
| Aggregate input bytes | 128 MiB |
| Aggregate classic declared and inline words | 16,777,216 |

Preflight checks all limits and every source reference subsequently accessed
before allocating report arrays, building resource keys, generating candidate
names or parsing resources. Checked arithmetic rejects overflow. Logical records
include all supplied resources, source meshes, model offsets/slots, placements,
regions, lights, material/property records, vertices, triangles and counted words.
Input bytes include every supplied resource (including suffixes/unused inputs),
source string tables, all placement extension bytes and source descriptor/mesh
suffixes. Geometry record cardinality is bounded without copying the geometry.

Name accounting covers source labels/tables, resource names/provenance, generated
suffixes, and conservative units for repeated string scans and generated/lowercase
copies. Each referenced NUL search is capped by the remaining name budget before
it searches. Source indices, model-table null consistency, string termination,
zone/mesh versions and inline four-byte alignment are validated. Version 1 with
inline bytes is rejected. Geometry and opaque material data are not revalidated
or assigned new semantics.

Every supplied resource is parsed once, independently of target reuse. The word
budget charges classic declared counts and all inline words, including unused
resources/records; it does not iterate or copy those words. Full borrowed views
can be visited by the caller later; that later work is outside these limits.
Limits bound this function's incremental logical work, not total process RAM or
allocator internals. The existing assembler reads/deserializes resources before
this API and is not retrospectively bounded by these limits.

## Calling the API

```rust
use eqoxide_asset_server::eqg::lighting::{diagnose, DiagnosticLimits, ExternalResource};
# use eqoxide_asset_server::eqg::BinaryZoneScene;
# fn inspect(scene: &BinaryZoneScene, bytes: &[u8]) -> anyhow::Result<()> {
let inputs = [ExternalResource {
    name: "instance.LIT",
    provenance: "caller-selected buffer",
    bytes,
}];
let report = diagnose(scene, &inputs, &DiagnosticLimits::default())?;
// Inspect report.targets and report.resources without mutating source data.
# Ok(())
# }
```
