# Multithreaded build — design

**Date:** 2026-06-25
**Status:** Approved

## Goal

Make the `build --raw` conversion process use the full CPU instead of running one
conversion at a time. Add a CLI option to cap the number of worker threads so a user
can throttle the process. Default to all-but-one available core.

## Background

The CPU-heavy work in a raw build lives in two serial loops in `src/build.rs`:

- `build_from_raw` — iterates the `COMMON_MODELS` table (~60 entries), calling
  `s3d_to_glb_model` for each and writing an independent `.glb` into the work dir.
  A single `ingest_dir` runs at the end.
- `build_zones_from_raw` — iterates zone archives discovered in `raw_dir`, calling
  `bake_zone` per zone, then `ingest_dir` + `build_zonedoors_from_raw` per zone.

Both call into shared `&Cas` and `&ManifestStore`. Both types hold only a `PathBuf`
with no interior mutability, so shared references are `Sync` and safe across threads.
Each set writes to a distinct manifest path (`common`, `zone/<short>`,
`zonedoors/<short>`), so there is no manifest-file contention between parallel tasks.

## Threading approach

Use the **rayon** crate (new dependency).

- Add `rayon = "1"` to `Cargo.toml`.
- Build a **scoped** `rayon::ThreadPool` via
  `rayon::ThreadPoolBuilder::new().num_threads(jobs).build()` in `main`, rather than
  mutating rayon's global pool. Run the parallel loops inside `pool.install(|| ...)`
  with `par_iter()`. This keeps the thread count explicit and testable and avoids a
  process-global side effect.
- The two build functions take the worker count (or a `&ThreadPool`) as a parameter.

## Thread-count CLI option

- New flag on the `Build` subcommand: `--jobs <N>` with short `-j <N>`,
  typed `Option<usize>`, validated with a clap `value_parser` range of `1..`
  (so `--jobs 0` is rejected).
- Resolution when omitted: `std::thread::available_parallelism()` minus one, floored
  at 1 — i.e. `available_parallelism().map(|n| n.get().saturating_sub(1)).max(1)`,
  falling back to 1 if `available_parallelism` errors.
- `--jobs 1` ⇒ a one-thread pool, equivalent to the old serial behavior.

## Parallelization details

### `build_from_raw`
- Convert the `COMMON_MODELS` loop body to run under `par_iter()` inside the pool.
  Each iteration converts to a distinct output `.glb`, so iterations are independent.
- The per-model `catch_unwind` + `tracing::warn!` isolation is preserved per task.
- The closing `ingest_dir("common", ...)` stays serial (runs once after the parallel
  region; it is I/O + chunking, not the bottleneck).

### `build_zones_from_raw`
- Collect zone archive paths into a `Vec` first (the current code reads the dir
  inline in the loop), then `par_iter()` the per-zone work: `bake_zone` →
  `ingest_dir("zone/<short>")` → `build_zonedoors_from_raw`.
- Per-zone `catch_unwind` + WARN handling preserved.
- Collect the successful `baked` short-names with a thread-safe pattern
  (`par_iter().filter_map(...).collect()` into a `Vec`, or a `Mutex<Vec<String>>`),
  then **sort** the result before returning so the printed summary is deterministic.
- `std::panic::set_hook(...)` is called once before the parallel region (it is a
  process-global; setting it once is correct).

## Required concurrency-safety fix: `Cas::put`

`Cas::put` currently writes to `path.with_extension("tmp")` (a single shared temp
path per content hash) then renames. Under parallelism, two threads writing
byte-identical content (e.g. a texture shared across zones) resolve to the same hash
and therefore the same `.tmp` path, and would clobber each other mid-write.

Fix: make the temp filename unique per write — e.g.
`cas/<hash>.<pid>-<counter>.tmp` using a process-global atomic counter (and/or thread
id) — then atomically rename onto the final `cas/<hash>`. The existing
skip-if-`path.exists()` idempotence is preserved; an atomic rename onto an existing
final path is harmless. This keeps content-addressed dedup intact while removing the
temp-file race.

## Behavior / compatibility

- Output is **byte-identical** to the serial build (content-addressed CAS + per-set
  manifests). Only log line ordering and intra-run scheduling change.
- The `baked` zone list is sorted before printing, so the build summary stays stable
  across runs.
- No change to the `Serve`, `Convert`, or `Analyze` subcommands.

## Out of scope (YAGNI)

- Parallelizing chunking inside `ManifestStore::build_and_write`.
- Threading the single-archive `Convert` subcommand.

## Testing

- All existing tests must still pass: `tests/build_cli.rs`, `tests/build_zones.rs`,
  `tests/bake_zone.rs`, `tests/convert.rs`, and the `cas`/`manifest` unit tests.
- Add a `Cas` test that spawns multiple threads calling `put` with identical content
  concurrently and asserts no error and a correct stored result (guards the tmp-file
  race fix).
- Add/extend a CLI test asserting `--jobs`/`-j` parses and that `--jobs 0` is
  rejected.
