# Manifest reader compatibility

This change requires a coordinated upgrade of the asset server and its consumers. It does not change GLB geometry or advertise the future unified geometry contract. Reader version `1` with capability `legacy-assets-v1` describes the existing asset reader.

The manifest now requires `schema_version: 1`, `requirements` and `revision`. The existing `digest` remains the file-content identity. `revision` also binds reader requirements, file sizes and chunk sequences. Manifest storage and HTTP ETags use revision; CAS chunk names remain unchanged.

Consumers send `X-Eqoxide-Asset-Readers: 1` and `X-Eqoxide-Asset-Capabilities: legacy-assets-v1`. Supported reader versions are an explicit set: advertising version 2 alone does not advertise version 1. Missing or incompatible advertisements receive HTTP 409 with `asset_reader_incompatible`; malformed advertisements receive 400. Authentication precedes this gate, and no-auth mode still enforces compatibility. Conditional requests are evaluated only after loading and validating the same immutable manifest revision and checking reader support.

For the coordinated cutover:

1. Preserve a backup of the current data store and binaries for rollback.
2. Upgrade the server and all consuming clients together. Older clients are intentionally rejected; there is no second active publication format.
3. Run the existing `migrate --data STORE` command. It now upgrades both numeric-keyed and old content-digest-keyed manifests, verifies referenced chunk and assembled file hashes and sizes, and atomically changes each set's latest pointer after writing its new envelope. Old manifest files and chunks remain available for rollback. Migration is per set, so keep the service stopped until all sets succeed.
4. Alternatively, rebake with the updated publisher after migrating the previous latest records needed by its shrink check. Every bake call explicitly declares its reader requirements.
5. Verify compatible 200/304 and incompatible 409 responses before resuming service. New clients treat old cached manifest records as needing a fresh manifest while retaining reusable chunks.

Unknown future schema versions and corrupt records fail migration rather than being relabeled as legacy. The compatibility metadata is a contract, not evidence of visual, collision, navigation, or gameplay readiness.

The package declares its own empty Cargo workspace so this independent server can also be built from an isolated checkout nested below another repository's workspace.
