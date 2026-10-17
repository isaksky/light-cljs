# Unread core macro `&form` — QuickJS trial, not retained

Phase52 with phase65's conservative unread-form shortcut, including its final
focused test sources. Both crate roots and `build.rs` were touched before the
build to avoid stale shared-target code or generated runtime artifacts.

Main none takes **92.23 s / 1,012,367,360 bytes peak RSS**, versus production's
93.83 s / 993,214,464 bytes. The 1.7% timing difference is too small to justify
the added transport complexity; memory also rises slightly. The change is not
retained or promoted. No full suite or separate focused QuickJS tests were run
for this rejected candidate. Phase65 preserves ten passing V8 bootstrap tests.

Complete output is byte-identical to phase42, including its known main ESM
syntax defect. Exact commands, the patch relative to phase52, timings and hashes
are archived; the executable and complete output remain under
`/private/tmp/light-cljs-phase66`.
