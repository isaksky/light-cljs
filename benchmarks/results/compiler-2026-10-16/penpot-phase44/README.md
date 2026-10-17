# Isolated embedded V8 compiler experiment

This is an experiment, **not a production backend change**. At the end of
phase44, production source, Cargo manifest and lockfile remained at phase42.
Phase45 subsequently promotes an independent namespace-count snapshot fix.
`prototype.patch` applies to the phase42
dirty worktree (including its essential `src/name_index.rs`); `git apply --check`
was verified without applying it. The prototype lives under
`/private/tmp/light-cljs-phase44/project`. It pins `v8 = 152.2.0` and, from v8
onward, enables pointer compression. It has no Node runtime dependency.

`LIGHT_V8_MACROS=1` selects V8 only for noninteractive compiler macro sessions.
The public evaluator, interactive sessions and custom-reader path still use
QuickJS. The complete suite with that selection passes **1,401 tests, zero
failures, 106 ignored** (`tests-v12.log`). This is not full replacement-engine
conformance. The earlier v8 integration also passed the full suite. Strict
Clippy has not been run on this prototype; a test-only unused trait method
still warns.

The bridge uses structured ABI 5 directly, with native UTF-16 transport,
GC-owned opaque provenance, shared `&form`/argument identity, canonical
snapshots, bounded environment caches and callback reentry. Provenance payloads
contain Rust `Rc`s, so cppgc marking/sweeping are atomic on the owner thread.
Isolate entry is scoped to operations: sessions can be destroyed in a different
order from creation. A new regression preserves the owned compiler diagnostic
when JavaScript catches a callback error and then spins until interruption.

## What the memory investigation established

The initial uncompressed whole-main trial completed in **49.95 seconds and
2,843,213,824 bytes peak RSS**. The compressed v8 diagnostic completed in
**50.22 seconds and 2,399,617,024 bytes**. Its terminal full GC left **382,992,912
bytes of live V8 heap**. These are improvements in time over production's
117.05 seconds, but not a win over shadow dev's 30.33 seconds / 2.22 GB.

A render heap snapshot showed **80,212,500 bytes in strings** out of about
90 MB live V8 heap. The largest strings were emitted macro functions carrying
roughly 800 KB of namespace-registration text each. V8 retained their script
source after the initializer closure was released. Disabling V8's compilation
cache barely changed the live heap (89.89 MB to 89.25 MB), so the cache was not
the main cause. The raw snapshot remains in the temporary directory; its hash
and a reproducible summary script are archived here.

Version 10 separates namespace registration into an immediately evaluated unit
before evaluating the body. The generated function no longer carries the large
registration source. Render's terminal live V8 heap falls to **10,766,844 bytes**;
main's falls to **27,355,768 bytes**. Main now completes with a 512 MiB engine
limit in **50.41 seconds / 1,717,747,712 bytes RSS**, reproducing production's
complete output byte for byte. The corresponding pre-split 512 MiB trial failed
during Instaparse expansion and is not a completed benchmark.

Splitting registration removed retention but still generated **422,893,442
bytes of source** across main's 15,414 evaluated units. Version 12 also uses the
existing readable string quoting for compile-time registration, avoiding six
source characters for every ordinary ASCII character. Application emission
keeps its existing encoding. An intermediate attempt changed application
encoding too and failed two textual output assertions; the final version keeps
those assertions unchanged and passes them.

Clean v12 runs with a 256 MiB engine heap limit (fresh processes, no terminal
GC/snapshot or stage timers) reproduce the entire phase42 outputs exactly:

| Build | Wall seconds | Peak RSS bytes |
| --- | ---: | ---: |
| render, repeated without archive work | 7.91 | 647823360 |
| main none | 47.32 | 1410547712 |
| main advanced, pseudo-names, elide-asserts | 53.84 | 1530937344 |

Main none is 2.47 times faster than production phase42 but has 14% higher
peak RSS. It remains 1.56 times slower than phase42's fresh shadow dev
comparison, using 64% of its peak RSS. Advanced is near shadow release's
55.72 seconds, using 33% of its peak RSS; scope still differs. These are single
observations. Shadow was not rerun during phase44, and the requested goal of
being dramatically faster and smaller remains unmet.

The separate scoped profile reproduces main output exactly in 47.17 seconds.
It measures 7.72 seconds in macro calls, 9.64 seconds in input/environment/
argument/result transport, 10.17 seconds in deferred analysis and 3.64 seconds
in unit emission after subtracting nested measured stages. About 16 seconds
remain outside those boundaries. Compared with phase35, macro calls fall from
54.67 seconds; transport falls from 26.60 seconds. These measurements include
their native and engine work and are not pure JavaScript execution times.
Compact quoting reduces emitted unit source from 422.89 MB to **107.33 MB**.
The final live V8 heap in that diagnostic is 25.28 MB.

A second profile locates that residual time: top-level namespace analysis
takes **14.61 seconds exclusive**, in addition to **10.19 seconds** deferred
analysis. Initial loading is 0.079 seconds and analysis finalization 0.016.
The CLI records 46.22 seconds through analysis and host destruction, 0.138
seconds for final application preparation and 0.418 seconds for output.
Whole-process time is 47.07 seconds. Its complete output also matches phase42.
The namespace timer includes its loop's source preparation and lazy loading;
it is not a claim that all 14.61 seconds are pure Rust AST traversal. Native
profiling is the next step. The generated source and profile binaries are
fingerprinted; `prepare-profile.bb`, `prepare-profile2.bb` and `timers.rs`
reproduce the instrumentation from the final prototype.

Terminal GC and heap snapshots are diagnostics, not clean timing runs.
V8 heap usage, estimated Rust provenance bytes, process RSS and macOS footprint
are different measures. A configured engine heap limit is not an RSS limit.
The render-v12-256m trial overlapped the lightweight archive/hash command; use
the separately repeated render trial for the clean comparison.

## Remaining requirements before promotion

The prototype does not yet honor the configured stack limit or use the bounded
ArrayBuffer allocator proven separately in phase43. It rejects limits below
16 MiB to avoid fatal startup OOM. The near-heap callback terminates execution
with 16 MiB emergency headroom; that is not strict hard-limit equivalence.
The watchdog still wakes every 100 microseconds while idle, and its poll-budget
behavior needs explicit review. Callback failure handling, platform packaging,
small-budget behavior and registration ordering under exceptional reentry need
further tests before replacing QuickJS. The executable is about 69 MB versus
11 MB for production.

## Output validation correction

Earlier `node --check FILE.js` commands returned zero for main, including the
frozen production output. **Explicit ESM checking rejects it**:
`node --check --input-type=module < FILE.js` reports a local declared both
`const` and `var` in rumext's native destructuring for `icon*`. V8's output is
identical, so this is a baseline defect, not an engine-induced change. Both
main-none and main-advanced fail explicit ESM checking; render passes. Earlier
syntax-pass conclusions were too strong. Penpot has still not been bundled or
run in a browser. Performance and byte-equivalence observations remain valid.

All compiler builds, full test runs and benchmarks were sequential. The port's
996-file fingerprint is unchanged. `run.bb` records options beside each job;
`benchmark-summary.txt` includes failed trials explicitly as zero output bytes.
Large JavaScript outputs, snapshots and frozen binaries remain in the temporary
directory, with hashes here. The original source preparation scripts describe
intermediate steps; `prototype.patch` is the complete final source delta.
