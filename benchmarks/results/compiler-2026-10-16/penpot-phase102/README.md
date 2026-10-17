# Phase 102: V8 with the Penpot output fix

Phase99's isolated V8 backend with the exact raw-JavaScript assignment fix from
phase101. This changes common compiler code, tests and documentation; there are
no new V8 runtime, watchdog, stack or allocator changes.

## Validation

**1,421 tests pass, zero fail, 106 are ignored**, at normal concurrency with V8
selected for noninteractive compiler macros. Strict Clippy, formatting and all
three Node workflows pass. The release build forces all five relevant Rust,
build-script and vendored QuickJS inputs before using the shared target.

All three complete outputs match phase101 production byte for byte and pass
explicit `node --check --input-type=module` checks. This includes the formerly
invalid main outputs. No port source was changed. These checks do not establish
bundled or browser execution of the application.

## Cold builds

One CPU-heavy job at a time, with the same roots and macro budgets as production,
256 MiB V8 heap setting and the established thin-LTO V8 release profile:

| Output | Wall seconds | Peak RSS bytes |
| --- | ---: | ---: |
| render | 3.90 | 478,838,784 |
| main none | 24.21 | 970,833,920 |
| main advanced | 30.51 | 1,180,123,136 |

Compared with phase101's fresh Shadow results, main none is 33.1% faster and
uses 41.6% of the RSS; advanced takes 47.0% of the wall time and 27.2% of the RSS.
These measurements are not production performance: V8 remains unpromoted.
Backing-store limits, startup monitoring, owned scheduler-creation failures and
packaging still need work. The overall performance goal remains active.

## Evidence

`lexical-js-vars.patch` is the phase100-to-phase101 common-code change applied
to phase99. Its isolated project had no compiler documentation, so preparation
first copies that baseline document. `prepare.bb` records that setup and can
restore just its affected candidate files before applying the patch. Validation
logs, scripts, all three benchmark commands, timing, syntax logs and source/output
hashes are retained here. Complete outputs and the frozen binary remain under
`/private/tmp/light-cljs-phase102/`.
