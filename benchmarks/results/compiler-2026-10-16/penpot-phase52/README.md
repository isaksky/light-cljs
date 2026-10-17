# Validated QuickJS allocator and registration quoting

The native CLI uses mimalloc 0.1.52 (`libmimalloc-sys` 0.1.49, bundled v3).
The library does not select a global allocator, and the WebAssembly normal
dependency tree excludes mimalloc. Compile-time registration strings use the
existing compact JavaScript quoting function, preserving non-ASCII UTF-16,
control characters, quotes and backslashes. Registration order is unchanged.

The isolated candidate passed 1,401 tests, zero failures, 106 ignored under
normal test concurrency; strict Clippy, formatting and all three Node workflows
passed. Its first test invocation failed because the scratch checkout omitted
`examples/`; the diagnostic is preserved. After restoring the fixture directory,
the suite passed. No compiler change was needed for that staging failure.

`promote.bb` checks every source, test, vendor and benchmark file against the
tested candidate after applying the incremental patch. Production release was
then rebuilt, frozen, and benchmarked one job at a time in fresh processes.

| Build | Wall seconds | Peak RSS bytes |
|---|---:|---:|
| light render | 10.70 | 400,719,872 |
| light main none | 93.83 | 993,214,464 |
| light main advanced | 99.31 | 1,161,510,912 |
| shadow release main | 55.06 | 4,046,192,640 |
| shadow compile main | 30.17 | 2,232,352,768 |

The isolated candidate's earlier main measurement was 94.14 s / 1,007,058,944
bytes RSS. Complete production outputs match frozen phase42. The 996-file port
hash is unchanged. Explicit ESM syntax checking passes render and rejects both
main outputs with the known baseline duplicate `var`/`const` declaration.
There was no bundling or browser execution.

The existing Linux ARM64 GCC 16.1.1 built the exact bundled mimalloc v3 C source
with release allocator settings and passed alignment, zeroing, reallocation
preservation and cross-thread free checks. Rust is absent on that host; no new
tool was installed, and the Rust compiler/full suite was not tested on Linux.
`linux-run.json` records the owned remote run, command, exit and result location.
Readiness logs were saved and the task's SSH master closed after retrieval.

`quickjs-allocator-quotes.patch` applies to production phase47. `production/`
contains the full tracked implementation patch plus the separately untracked
`name_index.rs`, logs, timings, commands, identity check and explicit ESM checks.
Binary and complete JavaScript hashes are preserved; large binaries/outputs
remain in `/private/tmp/light-cljs-phase52/production`.
