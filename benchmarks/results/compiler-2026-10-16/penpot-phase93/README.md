# Phase93: ready-root shortcut, rejected

An isolated phase87 variant returns an ordinary ready root after observing
overlays and an executing declaration group, avoiding the general forcing path.
The valid clean main-none run takes **91.43 seconds / 916,439,040 peak RSS
bytes**. Complete output matches phase42. This does not improve the phase87
isolated result (91.34 seconds / 893,534,208 bytes); it is rejected and no full
suite was run.

The first run accidentally reused phase92's instrumented engine from the shared
Cargo target. Its 90.72-second result is **excluded**, and its log is preserved
under `stale-engine/`. The clean rerun touches the vendored engine build script
and C source as well as the crate roots and root build script. The frozen valid
binary was checked not to contain the `QJS_SAMPLE` marker. Scripts, patch, logs
and hashes are retained. No source change is promoted.
