# eqoxide_asset_server

[![Client: eqoxide](https://img.shields.io/badge/client-eqoxide-blue?logo=github)](https://github.com/djhenry/eqoxide)

Derived-asset delivery addon for EQEmu. Converts raw `.s3d` to glTF, chunks them
into a blake3 content-addressed store, and serves incremental updates to the
eq_client_lite client over HTTP, authenticated against EQEmu's MariaDB.

## Build the asset store
    cargo run --release -- build --raw ~/eq_assets/EQ_Files --out ./data
    head -c 32 /dev/urandom > ./data/secret   # token signing secret

## Run the server (standalone)
    EQEMU_DB_URL=mysql://peq:peqpass@127.0.0.1:3306/peq \
      cargo run --release -- serve --data ./data --addr 0.0.0.0:8088 --secret-file ./data/secret

> The server reads `EQEMU_DB_URL` from the environment. When running the container directly with `podman run`, pass `-e EQEMU_DB_URL=mysql://peq:peqpass@<host>:3306/peq` and ensure `/data/secret` exists.

## Run alongside EQEmu (podman)
    podman compose -f ~/git/EQEmu/compose.yaml -f compose.assets.yaml up --build

## Diagnostics

### EQG zone coverage

Inventory EQG zone descriptors without baking or publishing anything:

```sh
cargo run --bin eqoxide-assets -- inventory-zones --raw /path/to/client
cargo run --bin eqoxide-assets -- inventory-zones --raw /path/to/client --json > eqg-inventory.json
```

The report inspects loose and archive-contained descriptors, retains original
resource names, and counts manifest-bearing archives separately from model-only
archives and unresolved terrain candidates. Header identification comes from
`libeq_eqg`; it does not validate whole files or establish playable-zone counts.
Competing descriptors are retained with `selection: "not_attempted"`.

Unknown/truncated descriptors, unreadable archives, and case collisions produce
diagnostics and a nonzero inventory-command exit status. JSON output remains
available when individual resources fail. Orphan descriptors and terrain without
descriptors are reported as unresolved; neither is counted as a recognized zone.

Normal zone bakes also print the unsupported EQG count. Normal bakes do not yet
convert EQG zones or change WLD terrain/water dispatch. Full dependency resolution,
source precedence, collision, and regions are tracked in the
[EQG zone support project plan](docs/eqg-zone-support-project-plan.md).

### Archive inspectors

Read-only WLD/PFS inspectors, for answering "what is actually in this archive?"
when a bake looks wrong. They are not shipped: the `Containerfile` builds only
`--bin eqoxide-assets`.

    cargo run --bin wlddump    -- <archive.s3d>                          # fragment inventory, skeletons, raw fragment histogram
    cargo run --bin wlddump    -- extract <archive.s3d> <file> <out>     # pull one file out of a PFS archive
    cargo run --bin trackdump  -- <archive.s3d> [NAME_FILTER]            # Track (0x13) animation fragment names, per WLD
    cargo run --bin skinbones  -- <archive.s3d> <wld> <mesh> <skeleton>  # vertex-range -> bone, face-range -> material
    cargo run --bin skelmeshes -- <archive.s3d> <wld> <skeleton>         # attached-mesh list + per-mesh bbox/scale/skin groups

This is an **addon**: it does not modify the EQEmu source tree.

### EQG static model conversion

`convert --archive /path/to/model.eqg --out model.glb` uses the checked
`libeq_eqg` reader for EQGM/EQGT versions 1, 2, and 3. The converter exports
static geometry with primary UVs and diffuse textures, including the geometry
portion of skeletal boat models. It does not export animation, vertex colors,
secondary UVs, or terrain material effects. Unsupported triangle material
references, malformed records, non-finite rendered attributes, and invalid text
used for texture lookup produce errors rather than guessed output.

This command retains its single-model selection heuristic; it does not assemble
zone descriptors. Optional native regressions verify rowboat and ship geometry,
textures, bounds, and deterministic output, plus a version-2 model from Anguish,
without bundling game data:

```sh
LIBEQ_TEST_RAW_DIR=/path/to/client cargo test --test eqg_conversion -- --ignored
```

### Binary EQG zone source inspection

Resolve a selected binary descriptor's model table and print source-level JSON:

```sh
cargo run --bin eqoxide-assets -- inspect-eqg-zone --archive /path/to/crescent.eqg --descriptor /path/to/crescent.zon
cargo run --bin eqoxide-assets -- inspect-eqg-zone --archive /path/to/anguish.eqg --member anguish.zon
```

Choose exactly one descriptor provider. The command resolves names within the
specified archive, preserving exact archive spelling for reads and rejecting
missing or ambiguous dependencies. Repeated model references share one loaded
mesh. The report records model-table mappings, selected members, geometry counts,
placement-record counts, and opaque data sizes. The in-memory source scene
preserves materials, flags, colors, UVs, and extension words for subsequent
processing; the JSON report contains counts and dependency mappings.

Placement transforms, mesh positions/normals, and primary UVs must be finite.
Secondary UV bit patterns are retained and non-finite values are counted in the
report: some source models contain them even when primary UVs are valid.

This command does not choose native source precedence, resolve textures, apply
world transforms, interpret placement roles or collision/region/lighting data,
or publish assets. In particular, placement counts are not rendered-object
counts. Optional native acceptance covers Crescent, Guild Hall, and Anguish:

```sh
LIBEQ_TEST_RAW_DIR=/path/to/client cargo test --test native_eqg_zone -- --ignored
```

### Binary EQG render previews

Export a selected binary zone to a standalone GLB and print a JSON export report:

```sh
cargo run --bin eqoxide-assets -- export-eqg-preview --archive /path/to/crescent.eqg --descriptor /path/to/crescent.zon --out /tmp/crescent-preview.glb
cargo run --bin eqoxide-assets -- export-eqg-preview --archive /path/to/anguish.eqg --member anguish.zon --out /tmp/anguish-preview.glb
```

The exporter shares object geometry across placements, emits terrain once at
identity, and converts source coordinates to glTF's Y-up basis. It requires one
unambiguous terrain member and positive uniform object scales. Every placement
is emitted or reported as skipped with a reason. Only referenced diffuse textures
are resolved; missing, ambiguous, or undecodable textures fail the export. Texture
decoding includes uncompressed 32-bit RGB DDS with byte-channel masks and optional
alpha. The output directory must exist; failures preserve an existing output file.

These are opaque render previews. The report lists omitted triangles and material
approximations. Vertex colors, secondary UVs, terrain effects, animation, lighting,
collision, water regions, and source-provider precedence are not implemented by
this exporter. No CAS assets or zone manifests are published, and these previews
have not been validated for gameplay.

Optional acceptance exports and imports Crescent, Guild Hall, and Anguish without
bundling game data:

```sh
LIBEQ_TEST_RAW_DIR=/path/to/client cargo test --test native_eqg_preview -- --ignored
```

### EQG material and surface diagnostics

`inspect-eqg-zone` includes `material_details`, raw `triangle_surface_groups`,
and `surface_assessments` per mesh. Names and shader/property strings include
original bytes and optional UTF-8; property values retain their exact bits.
Surface assessments distinguish material references, the hidden sentinel, and
out-of-range references independently of default collision-query participation.
Flag `0x1` excludes a triangle from that default query; `0x2` records a separate
query-dependent filter. Upper bits and unclassified lower bits remain visible.

These assessments do not emit collision geometry or establish shader fidelity.
Object hulls, dynamic doors, winding, the client collision contract, and exact
material blend/alpha-test states still need validation before gameplay export.
