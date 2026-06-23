# eqoxide_asset_server

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

This is an **addon**: it does not modify the EQEmu source tree.
