# Multithreaded Build Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Run the `build --raw` conversion loops in parallel across CPU cores, with a `--jobs`/`-j` CLI flag to cap worker threads (default: all-but-one core).

**Architecture:** Add the `rayon` crate. `main` resolves a worker count, builds a scoped `rayon::ThreadPool`, and passes it into the two build functions, which run their per-item loops via `pool.install(|| par_iter())`. A concurrency-safety fix to `Cas::put` (unique temp filenames) makes parallel writes of byte-identical content safe.

**Tech Stack:** Rust 2021, rayon, clap (derive), blake3, existing `Cas`/`ManifestStore`.

## Global Constraints

- Edition: Rust 2021 (`Cargo.toml`).
- New dependency allowed: `rayon = "1"`. No other new dependencies.
- Build output must remain **byte-identical** to the serial build (content-addressed CAS + per-set manifests). Only log ordering may change.
- Default worker count when `--jobs` omitted: `available_parallelism().get().saturating_sub(1)`, floored at 1; fall back to 1 if `available_parallelism()` errors.
- `--jobs`/`-j` is typed `Option<usize>`, validated to reject `0` (clap `value_parser` range `1..`).
- Preserve existing per-item panic isolation (`std::panic::catch_unwind`) and `tracing::warn!` skip behavior.
- The `baked` zone list returned by `build_zones_from_raw` must be sorted before return so the printed summary is deterministic.

---

### Task 1: Make `Cas::put` concurrency-safe (unique temp files)

**Files:**
- Modify: `src/cas.rs:20-33` (the `put` method) and add a module-level atomic counter.
- Test: `src/cas.rs` (unit test in the existing `#[cfg(test)] mod tests`).

**Interfaces:**
- Consumes: nothing new.
- Produces: `Cas::put(&self, bytes: &[u8]) -> std::io::Result<String>` — unchanged signature; now safe to call concurrently from multiple threads, including with identical `bytes`.

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `src/cas.rs`:

```rust
#[test]
fn concurrent_put_identical_content_is_safe() {
    let dir = tempfile::tempdir().unwrap();
    let cas = Cas::new(dir.path());
    let expected = Cas::hash(b"shared-bytes");
    std::thread::scope(|s| {
        let handles: Vec<_> = (0..16)
            .map(|_| s.spawn(|| cas.put(b"shared-bytes").unwrap()))
            .collect();
        for h in handles {
            assert_eq!(h.join().unwrap(), expected);
        }
    });
    assert!(cas.has(&expected));
    assert_eq!(cas.get(&expected).unwrap(), b"shared-bytes");
}
```

- [ ] **Step 2: Run test to verify it compiles and the intent is exercised**

Run: `cargo test --lib cas::tests::concurrent_put_identical_content_is_safe`
Expected: PASS or intermittent FAIL on current code (shared `.tmp` race). Either way, proceed to the fix; the fix makes it reliably correct.

- [ ] **Step 3: Implement unique temp filenames**

At the top of `src/cas.rs`, after the existing `use` line, add:

```rust
use std::sync::atomic::{AtomicU64, Ordering};

static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);
```

Replace the body of `put` (lines ~20-33) with:

```rust
    pub fn put(&self, bytes: &[u8]) -> std::io::Result<String> {
        let hash = Self::hash(bytes);
        let path = self.path_for(&hash);
        if !path.exists() {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            // Unique temp name per write: two threads writing byte-identical
            // content resolve to the same final hash path, so a shared
            // `<hash>.tmp` would race. Atomic rename onto the final path is
            // harmless if another thread won (content is identical).
            let n = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
            let tmp = path.with_extension(format!("tmp.{}.{}", std::process::id(), n));
            std::fs::write(&tmp, bytes)?;
            std::fs::rename(&tmp, &path)?;
        }
        Ok(hash)
    }
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test --lib cas::tests`
Expected: PASS (all cas unit tests, including `concurrent_put_identical_content_is_safe`).

- [ ] **Step 5: Commit**

```bash
git add src/cas.rs
git commit -m "fix(cas): unique temp filenames so concurrent put is race-free"
```

---

### Task 2: Add rayon, a job-count resolver, and parallelize the common-model build

**Files:**
- Modify: `Cargo.toml` (add `rayon = "1"`).
- Modify: `src/build.rs` (add `use rayon::prelude::*;`, add `resolve_jobs`, change `build_from_raw` signature + parallelize its loop).
- Test: `src/build.rs` (unit test for `resolve_jobs` in the existing `#[cfg(test)] mod tests`).

**Interfaces:**
- Consumes: `Cas::put` (Task 1, now thread-safe).
- Produces:
  - `pub fn resolve_jobs(requested: Option<usize>) -> usize` — returns `requested` when `Some` (already validated `>= 1`), else `available_parallelism().get().saturating_sub(1)` floored at 1 (fallback 1 on error).
  - `pub fn build_from_raw(cas: &Cas, store: &ManifestStore, raw_dir: &Path, work_dir: &Path, pool: &rayon::ThreadPool) -> anyhow::Result<Vec<Manifest>>` — new trailing `pool` parameter.

- [ ] **Step 1: Add the rayon dependency**

In `Cargo.toml`, under `[dependencies]`, add (after the `glam` line):

```toml
rayon = "1"
```

- [ ] **Step 2: Write the failing test for `resolve_jobs`**

Add to the `tests` module in `src/build.rs` (it currently only imports `is_zone_archive`; add the import):

```rust
    use super::resolve_jobs;

    #[test]
    fn resolve_jobs_honors_explicit_request() {
        assert_eq!(resolve_jobs(Some(3)), 3);
        assert_eq!(resolve_jobs(Some(1)), 1);
    }

    #[test]
    fn resolve_jobs_default_is_at_least_one() {
        assert!(resolve_jobs(None) >= 1);
    }
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test --lib build::tests::resolve_jobs_honors_explicit_request`
Expected: FAIL to compile — `resolve_jobs` not found.

- [ ] **Step 4: Implement `resolve_jobs` and parallelize `build_from_raw`**

At the top of `src/build.rs`, add after the existing `use` lines:

```rust
use rayon::prelude::*;
```

Add this function near the top of the file (e.g. just above `build_from_raw`):

```rust
/// Resolve the worker-thread count for a build. An explicit `--jobs N` (already
/// validated `>= 1` by clap) is used as-is; otherwise default to all-but-one core,
/// floored at 1, falling back to 1 if the core count can't be determined.
pub fn resolve_jobs(requested: Option<usize>) -> usize {
    match requested {
        Some(n) => n,
        None => std::thread::available_parallelism()
            .map(|n| n.get().saturating_sub(1))
            .unwrap_or(1)
            .max(1),
    }
}
```

Replace `build_from_raw` (lines ~62-88) with the parallel version. Note: the `COMMON_MODELS` conversion loop runs in parallel inside `pool.install`; the closing `ingest_dir` stays serial.

```rust
pub fn build_from_raw(
    cas: &Cas,
    store: &ManifestStore,
    raw_dir: &Path,
    work_dir: &Path,
    pool: &rayon::ThreadPool,
) -> anyhow::Result<Vec<Manifest>> {
    let common_out = work_dir.join("common");
    std::fs::create_dir_all(&common_out)?;
    pool.install(|| {
        COMMON_MODELS.par_iter().for_each(|(archive, model_code, out_name)| {
            let src = raw_dir.join(archive);
            if !src.exists() {
                tracing::warn!("skip missing archive {archive} (for {out_name})");
                return;
            }
            // Per-model conversion can panic on malformed archives; isolate each so one
            // bad model doesn't abort the whole common build.
            let out = common_out.join(out_name);
            let result = std::panic::catch_unwind(|| s3d_to_glb_model(&src, &out, true, *model_code));
            match result {
                Ok(Ok(())) => {}
                Ok(Err(e)) => tracing::warn!("skip model {out_name} from {archive}: {}", short_err(&e)),
                Err(_) => tracing::warn!("skip model {out_name} from {archive}: conversion panicked"),
            }
        });
    });
    let common = ingest_dir(cas, store, "common", &common_out)?;
    Ok(vec![common])
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --lib build::tests`
Expected: PASS. Then `cargo build` to confirm `src/build.rs` compiles (the `main.rs` call site is still 4-arg and will fail to compile — that is fixed in Task 4; build the lib only here).

Run: `cargo build --lib`
Expected: lib compiles cleanly.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock src/build.rs
git commit -m "feat(build): rayon + parallel common-model conversion, job-count resolver"
```

---

### Task 3: Parallelize the zone build

**Files:**
- Modify: `src/build.rs` (`build_zones_from_raw`, lines ~138-177).
- Test: covered by existing `tests/build_zones.rs` (ignored, needs real EQ files) and a compile check; no new unit test (the per-zone work needs real archives).

**Interfaces:**
- Consumes: `Cas::put` (thread-safe), `resolve_jobs`/`pool` from Task 2.
- Produces: `pub fn build_zones_from_raw(cas: &Cas, store: &ManifestStore, raw_dir: &Path, work_dir: &Path, pool: &rayon::ThreadPool) -> anyhow::Result<Vec<String>>` — new trailing `pool` parameter; returned `Vec<String>` is sorted.

- [ ] **Step 1: Rewrite `build_zones_from_raw` to collect paths then process in parallel**

Replace `build_zones_from_raw` (lines ~138-177) with:

```rust
pub fn build_zones_from_raw(
    cas: &Cas,
    store: &ManifestStore,
    raw_dir: &Path,
    work_dir: &Path,
    pool: &rayon::ThreadPool,
) -> anyhow::Result<Vec<String>> {
    // libeq panics (not Errs) on some malformed WLDs; we catch_unwind each zone and
    // log a clean WARN, so silence the default hook's verbose backtrace dump. Set once
    // before the parallel region (the hook is process-global).
    std::panic::set_hook(Box::new(|_| {}));

    // Collect zone archive paths first, then fan out the per-zone conversion.
    let mut zone_paths = Vec::new();
    for entry in std::fs::read_dir(raw_dir)? {
        let path = entry?.path();
        let fname = match path.file_name().and_then(|s| s.to_str()) { Some(f) => f.to_string(), None => continue };
        if !is_zone_archive(&fname) { continue; }
        zone_paths.push(path);
    }

    let baked: anyhow::Result<Vec<Option<String>>> = pool.install(|| {
        zone_paths
            .par_iter()
            .map(|path| -> anyhow::Result<Option<String>> {
                let short = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
                let obj = raw_dir.join(format!("{short}_obj.s3d"));
                let zdir = work_dir.join("zone").join(&short);
                std::fs::create_dir_all(&zdir)?;
                let glb = zdir.join(format!("{short}.glb"));
                let result = std::panic::catch_unwind(|| {
                    bake_zone(path, obj.exists().then_some(obj.as_path()), &glb)
                });
                match result {
                    Ok(Ok(())) => {
                        ingest_dir(cas, store, &format!("zone/{short}"), &zdir)?;
                        if let Err(e) = build_zonedoors_from_raw(cas, store, raw_dir, &short) {
                            tracing::warn!("zonedoors {short}: {}", short_err(&e));
                        }
                        Ok(Some(short))
                    }
                    Ok(Err(e)) => {
                        tracing::warn!("skip zone {short}: {}", short_err(&e));
                        Ok(None)
                    }
                    Err(payload) => {
                        let msg = payload
                            .downcast_ref::<&str>()
                            .copied()
                            .or_else(|| payload.downcast_ref::<String>().map(|s| s.as_str()))
                            .unwrap_or("<non-string panic>");
                        tracing::warn!("skip zone {short}: bake_zone panicked: {}", short_err(&msg));
                        Ok(None)
                    }
                }
            })
            .collect()
    });

    let mut baked: Vec<String> = baked?.into_iter().flatten().collect();
    baked.sort();
    Ok(baked)
}
```

- [ ] **Step 2: Verify the lib compiles**

Run: `cargo build --lib`
Expected: lib compiles cleanly (`main.rs` still 4-arg; fixed in Task 4).

- [ ] **Step 3: Run the zone test (skips gracefully without EQ files)**

Run: `cargo test --test build_zones -- --ignored`
Expected: the test runs; if `~/eq_assets/EQ_Files/qcat.s3d` is absent it prints `skip` and passes. Note: `build_zones.rs` calls `build_zones_from_raw` with 4 args and will fail to compile until Step 4.

- [ ] **Step 4: Update the ignored zone test call site to pass a pool**

In `tests/build_zones.rs`, replace the `build_zones_from_raw` call (line ~12) with a version that builds a single-thread pool:

```rust
    let pool = rayon::ThreadPoolBuilder::new().num_threads(1).build().unwrap();
    let zones = eqoxide_asset_server::build::build_zones_from_raw(&cas, &store, &raw, &work, &pool).unwrap();
```

- [ ] **Step 5: Run the zone test again to confirm it compiles and skips/passes**

Run: `cargo test --test build_zones -- --ignored`
Expected: compiles; prints `skip` (no EQ files) or PASS (with EQ files).

- [ ] **Step 6: Commit**

```bash
git add src/build.rs tests/build_zones.rs
git commit -m "feat(build): parallelize zone bake, sort baked list for stable output"
```

---

### Task 4: Wire the `--jobs`/`-j` CLI flag

**Files:**
- Modify: `src/main.rs` (the `Build` variant fields, lines ~24-32, and the `Cmd::Build` match arm, lines ~78-100).
- Test: `tests/build_cli.rs` (add a clap-parse test).

**Interfaces:**
- Consumes: `resolve_jobs`, `build_from_raw`, `build_zones_from_raw` (Tasks 2-3), all expecting a trailing `&rayon::ThreadPool`.
- Produces: a working `eqoxide-assets build --jobs N`/`-j N` flag; `--jobs 0` rejected.

- [ ] **Step 1: Write the failing CLI parse test**

`main.rs` defines `Cli`/`Cmd` privately, so the test parses via the compiled binary. Add to `tests/build_cli.rs`:

```rust
#[test]
fn jobs_flag_rejects_zero() {
    let exe = env!("CARGO_BIN_EXE_eqoxide-assets");
    let out = std::process::Command::new(exe)
        .args(["build", "--out", "/tmp/unused-eqoxide", "--jobs", "0", "--zones-only"])
        .output()
        .unwrap();
    assert!(!out.status.success(), "expected --jobs 0 to be rejected");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("jobs") || stderr.contains("0"), "stderr was: {stderr}");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test build_cli jobs_flag_rejects_zero`
Expected: FAIL — `--jobs` is an unknown argument (clap exits non-zero but for the wrong reason; the test asserts on `jobs`/`0` in stderr, which won't match the "unexpected argument" message). Proceed to implement.

- [ ] **Step 3: Add the `--jobs` field to the `Build` variant**

In `src/main.rs`, inside the `Build { ... }` variant (after the `zones_only` field, line ~31), add:

```rust
        /// Number of worker threads for conversion (default: all-but-one core).
        #[arg(long, short = 'j', value_parser = clap::value_parser!(u32).range(1..))]
        jobs: Option<u32>,
```

- [ ] **Step 4: Use the flag in the `Cmd::Build` arm**

In the `Cmd::Build { ... }` match arm, update the destructure to include `jobs`, then build a pool and thread it through. Replace lines ~78-100 with:

```rust
        Cmd::Build { set, from, raw, out, zones_only, jobs } => {
            let cas = Cas::new(&out);
            let store = ManifestStore::new(&out);
            if let Some(raw_dir) = raw {
                let n = eqoxide_asset_server::build::resolve_jobs(jobs.map(|j| j as usize));
                let pool = rayon::ThreadPoolBuilder::new().num_threads(n).build()?;
                println!("building with {n} worker thread(s)");
                let work = out.join("work");
                if !zones_only {
                    let ms = eqoxide_asset_server::build::build_from_raw(&cas, &store, &raw_dir, &work, &pool)?;
                    println!("built {} set(s) from raw archives", ms.len());
                }
                let zones = eqoxide_asset_server::build::build_zones_from_raw(&cas, &store, &raw_dir, &work, &pool)?;
                println!("baked {} zone(s): {}", zones.len(), zones.join(", "));
                let gd = eqoxide_asset_server::build::build_gamedata_from_raw(&cas, &store, &raw_dir)?;
                println!("built 'gamedata' set version {} ({} files)", gd.version, gd.files.len());
                let ge = eqoxide_asset_server::build::build_gameequip_from_raw(&cas, &store, &raw_dir)?;
                println!("built 'gameequip' set version {} ({} files)", ge.version, ge.files.len());
            } else {
                let set = set.expect("--set required without --raw");
                let from = from.expect("--from required without --raw");
                let m = ingest_dir(&cas, &store, &set, &from)?;
                println!("built set '{}' version {} ({} files)", m.set, m.version, m.files.len());
            }
            Ok(())
        }
```

Note: `--jobs` is accepted on `build` even without `--raw`; it is simply unused in the non-raw `ingest_dir` path (single set, no conversion loop). This keeps the flag definition simple and harmless.

- [ ] **Step 5: Run the CLI test to verify it passes**

Run: `cargo test --test build_cli`
Expected: PASS (`jobs_flag_rejects_zero` and the existing `ingest_dir_chunks_all_files_with_relative_paths`).

- [ ] **Step 6: Run the full suite**

Run: `cargo test`
Expected: PASS (ignored tests remain ignored). Then `cargo build` to confirm the whole binary compiles.

- [ ] **Step 7: Commit**

```bash
git add src/main.rs tests/build_cli.rs
git commit -m "feat(build): add --jobs/-j flag to throttle build worker threads"
```

---

## Self-Review

**Spec coverage:**
- Multithreaded conversion → Task 2 (common models) + Task 3 (zones). ✓
- `--jobs`/`-j` CLI throttle on Build → Task 4. ✓
- Default all-but-one core → `resolve_jobs` (Task 2), used in Task 4. ✓
- Reject `--jobs 0` → clap `range(1..)` (Task 4), tested. ✓
- rayon scoped ThreadPool → Task 4 builds it, Tasks 2-3 consume `&pool`. ✓
- `Cas::put` temp-file race fix → Task 1. ✓
- Deterministic summary (sorted `baked`) → Task 3. ✓
- `set_hook` once before parallel region → Task 3. ✓
- Byte-identical output → preserved (content-addressed; only ordering changes). ✓
- Out of scope (chunking parallelism, Convert threading) → not included. ✓

**Placeholder scan:** No TBD/TODO/"handle edge cases"; every code step shows full code. ✓

**Type consistency:** `resolve_jobs(Option<usize>) -> usize`; CLI field is `Option<u32>` converted via `jobs.map(|j| j as usize)`; both build functions take a trailing `pool: &rayon::ThreadPool`; all three call sites (main.rs, build_zones.rs test) updated to match. ✓
