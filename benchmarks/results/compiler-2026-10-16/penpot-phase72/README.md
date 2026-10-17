# Phase72: production unit hygiene

QuickJS production now uses the raw-JavaScript hygiene pass for the generated
shared-runtime parameter. It no longer searches all accumulated string literals
for a safe parameter name before emitting each deferred unit. This ports only
the hygiene change from phase70, with no V8 dependency or backend change.

The full suite passes **1,400 tests, 0 failures, 106 ignored**, using normal
test concurrency. Strict Clippy, formatting and three Node workflows pass.
One obsolete internal scan test is removed; the existing raw-JavaScript test
now also checks the new parameter and its prefixed name. Production src, tests,
vendor, benches, manifests and build script match the validated candidate.

Sequential cold production rerun:

| Build | Wall seconds | Peak RSS bytes |
| --- | ---: | ---: |
| render | 10.60 | 396836864 |
| main none | 92.72 | 1008009216 |
| main advanced | 98.37 | 1153204224 |
| shadow release | 56.55 | 3797729280 |
| shadow dev | 30.19 | 2377187328 |

The isolated pre-promotion main run is 92.88 s / 984612864 bytes. The production
main gain over phase52 is only 1.2%, with slightly higher measured RSS. Its
structural removal of a repeated full-table scan is useful, but the overall
speed goal remains unmet: main is still 3.07 times slower than shadow dev.

All complete compiler outputs match phase42; the port hash is unchanged.
Explicit ESM syntax checks pass render and fail both main outputs at the known
duplicate declaration. No bundling or browser execution is claimed.

`unit-hygiene.patch` is relative to phase52. `production/implementation.patch`
plus the adjacent untracked `name_index.rs` preserve the complete working
implementation. Full logs, commands, output/source hashes, syntax diagnostics
and the port hash are archived here. Binaries and large outputs remain in
`/private/tmp/light-cljs-phase72/`. Nothing was committed.
