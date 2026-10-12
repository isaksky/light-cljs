# Raw ns* in compiled evaluation sessions

Owner: light-cljs-164. The original complete source at
`tests/fixtures/compiler-nsstar/syntax/nsstar.cljs` remains byte-identical:
SHA-256 `ebe59a510ec23b6b0d43bbf8927dc149714e1c248abee2c6ca06faef9180e957`.
Its original forms and boolean exported-callback expectation are unchanged.
The complete program's final `pr-str` has the separate actual string value
`"true"`, including through file/stdin CLI evaluation with explicit source roots.

The session uses the namespace compiler's raw directive/libspec validation.
It stages aliases, referrals and host imports, loads ordinary `.cljs`/`.cljc`
dependencies, validates public binding ownership, and then publishes the new
context. Failed/missing imports cannot leak a staged alias. Top-level `do` forms
containing raw source directives execute their operations in order; ordinary
forms retain their existing analysis path. Runtime `load-file` uses the same
staging and dependency validation in the active application engine.

`tests/eval_nsstar.rs` executes the complete unchanged source twice in a reused
session and twice in fresh sessions. It checks the actual string value,
namespace ownership, alias reader resolution and namespace isolation. Its
malformed/unquoted/missing/conflicting/nested cases retain source locations,
recover using the original program, and reject stale aliases. Supplemental
source-bound Node/QuickJS namespace execution and fresh/reused session execution
agree on renamed value/macro referrals, imported constructor identity, ordinary
`.cljs` macro ownership despite a divergent `.clj` companion, and ordered effects
`[:before :after]`. Repeated referrals preserve resolved identities. Separate
ordinary namespace and runtime source-loading controls execute successfully.

Current raw receipts are in `/tmp/light-cljs-164-session` and
`/tmp/light-cljs-164-report-final`; the committed projection is
`eval-nsstar-verification.json`. Both freshly checked pins (release 1.12.134 and
checkout a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e) compile the complete original
source under none/static-fns false/def-emits-var false. A fresh Node VM captures
the final expression's actual completion string `"true"` from the exact emitted
namespace file, then separately observes exported callback boolean true. Both
have empty ordered effects and stdout. The repaired report independently
preserves its complete-result and exported-callback contracts and full-ledger
zero-missing gate. Its two selected obligations do not complete light-cljs-125.
Historical receipts and superseded attempts remain historical evidence.

Verification commands:

```sh
cargo test --locked --test eval_nsstar --test eval_sessions -- --skip cancellation_and_limits_require_reset_but_host_compilation_recovers --skip source_output_limits_and_oom_are_explicit
cargo test --locked --test eval_cli -- --skip runtime_and_rendering_limits_fail_without_partial_results
cargo test --locked --test compiler_syntax_context --test namespaces --test compiler_namespace_completion --test compiler_namespace_shadowing --test compiler_macros --test compiler_bootstrap
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tests/reference/compiler-language -p test_report.py -v
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tests/reference/compiler-language -p test_full_ledger.py -v
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tests/reference/compiler-language -p test_namespace_results.py -v
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tests/reference/compiler-language -p test_syntax_context.py -v
PYTHONDONTWRITEBYTECODE=1 python3 tests/reference/compiler-language/extract_inventory.py --check
PYTHONDONTWRITEBYTECODE=1 python3 benchmarks/run_compiler.py --output /tmp/light-cljs-164-production-final --samples 9 --iterations 1
```

The current production timing/allocation run validates every result and retains
raw samples, environment, source and binary hashes, and its source archive. No
before/after timing claim is made. The first benchmark at
`/tmp/light-cljs-164-production` correctly rejected a concurrent supplemental
test edit as source drift; it is not a valid performance report. The initial
session attempts (including a test-import spelling error and an invalid duplicate
refer-global reuse probe) are retained without passing credit. The original
reference and fixture expectations were never changed to accommodate these
failures. The final verified session driver repeats the genuine value/macro
referrals while retaining the namespace compiler's duplicate-global diagnostic.

Externally blocked light-cljs-156 was not retried, investigated, delegated or
repaired. Cancellation/limits suites and the evaluator benchmark's limit-recovery
phase were not run. The unrelated `.DS_Store` modification is preserved. No push
or PR was made.
