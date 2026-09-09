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

Normal zone bakes also print the unsupported EQG count. This first slice does not
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
