# Complete namespace result execution — 2026-10-11

Owner: light-cljs-163. Both complete syntax namespace programs retain every
original form, including `(pr-str (run nil))`. Their actual final result is the
string `"true"`; their checked upstream exported callback contract remains
boolean true. The report now dispatches only these two exact frozen source,
root, entry, export, expectation and named driver contracts as scalar programs.
It executes the full emitted program in fresh Node and QuickJS heaps and runs
the named actual MacroHost driver. The test adapter substitutes the lexical
runtime factory; it preserves every emitted source form and its order.

`namespace-result-verification.json` retains the exact full source text, original
expectation/source hashes, actual macro-source hashes, compiled JavaScript and
observation-script/raw-output hashes, both engine outcomes, exact macro drivers,
all compiler producer source hashes, checked reference identities/settings and
the historical native observations. The native observations remain explicitly
historical; current QuickJS namespace evaluation is separate. The upstream
program loader executes the complete original namespace before invoking its
named export under the original boolean outcome contract. No failed generic
stdin or callback execution is used as passing evidence.

The genuine CLI report exited zero at
`/tmp/light-cljs-163-report-verified/report.json`:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 tests/reference/compiler-language/report.py \
  --run --no-build --fixture syntax-context-namespace-header-context \
  --fixture syntax-context-namespace-nsstar-alias --reference both \
  --artifacts /tmp/light-cljs-163-report-verified \
  --output /tmp/light-cljs-163-report-verified.json
```

Both syntax obligations are covered; all 27 supplemental boundary checks pass.
Explicit fixture execution selection is recorded. Every other obligation stays
in the report and strict gate: 2 covered, 8,598 missing and 1,391 unrelated;
`strict_complete` is false. Default unfiltered execution and all light-cljs-125
zero-missing criteria remain intact. A requested fixture selects only its bound
phase drivers; absent reference/phase/source evidence still remains missing.
The new full-ledger guard also requires current exact scalar engine observations,
original expectation, source/producer hashes and the actual driver before credit.

The independent callback control at `/tmp/light-cljs-163-callback-final` retains
all original callback source forms. Its explicit console embedding produces
`setup\ncallback\n`, ordered effect `[2]` and boolean true in Node and QuickJS,
and both checked pins agree. Original namespace callback dispatch continues to
execute the complete program, then its callback and full outcome protocol.
Earlier invalid control attempts lacked the explicit print sink or its console
newline; those failed raw receipts remain at `/tmp/light-cljs-163-callback` and
the initial files under the final callback directory. They receive no credit.

Focused validation passes 77 Python accounting/dispatch/ledger/syntax guards and
all 10 Rust syntax tests. Inventory freshness, scoped Rust formatting and
whitespace checks pass. Repository-wide `cargo fmt --all -- --check` still reports
preexisting formatting differences; none were rewritten. The first superseded
report at `/tmp/light-cljs-163-report` captured an older producer and unnecessarily
ran the broad phase cohort. Its exact owned process tree was stopped and all
artifacts retained. The later superseded report remains at
`/tmp/light-cljs-163-report-current`; only the verified report above supplies this
receipt's current acceptance. No performance change is claimed for this harness
repair; production compiler/runtime source is unchanged.

A distinct fresh production eval defect is retained at
`/tmp/light-cljs-163-eval-current`: even with explicit roots, the original raw
`ns*` source is rejected by EvalSession after a current locked build. Duplicate
searches found no focused live owner; light-cljs-164 is immediately before 125.
This defect was not repaired or relabeled. Externally blocked light-cljs-156 was
not investigated, retried, delegated or repaired; its cancellation/limits suites
were not run. The unrelated `.DS_Store` modification is preserved. No push or PR.
