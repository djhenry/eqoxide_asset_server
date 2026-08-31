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

Read-only WLD/PFS inspectors, for answering "what is actually in this archive?"
when a bake looks wrong. They are not shipped: the `Containerfile` builds only
`--bin eqoxide-assets`.

    cargo run --bin wlddump    -- <archive.s3d>                          # fragment inventory, skeletons, raw fragment histogram
    cargo run --bin wlddump    -- extract <archive.s3d> <file> <out>     # pull one file out of a PFS archive
    cargo run --bin trackdump  -- <archive.s3d> [NAME_FILTER]            # Track (0x13) animation fragment names, per WLD
    cargo run --bin skinbones  -- <archive.s3d> <wld> <mesh> <skeleton>  # vertex-range -> bone, face-range -> material
    cargo run --bin skelmeshes -- <archive.s3d> <wld> <skeleton>         # attached-mesh list + per-mesh bbox/scale/skin groups

This is an **addon**: it does not modify the EQEmu source tree.
