# Phase12 scoped evaluator service lookup

ActiveEvaluator retains the immutable JavaScript service object only within its
active context lifetime. Function lookup and the bridge check use that object
instead of parsing a new JavaScript lookup expression on every operation. There
are no Persistent roots, captured engine handles or changes to transported data.

All 1,378 tests pass, zero failures, 106 ignored; strict Clippy and formatting
pass. Focused evaluator tests include release/GC, failures, reentry, context
rejection and terminal resource limits. Runtime JavaScript is unchanged, so the
three direct Node workflows from phase11 still apply.

A cold render measured 26.06 seconds / 566,476,800 bytes RSS, versus phase11's
27.32 seconds / 570,523,648 bytes. No CPU-heavy work overlapped the benchmark.
The complete emitted file is byte-identical to phase11 and passes node --check.
The main builds were not measured at this intermediate step.

The binary, patch, untracked name-index source and generated JavaScript remain
under `/private/tmp/light-cljs-phase12/`; hashes and validation logs are here.
The optional runtime lookup micro-experiment in that temporary directory was
not applied to production code and is not part of these measurements.
