# Raw ns* source directives

Owner: light-cljs-162. The byte-identical complete alias program from the prior
independent audit is retained in `tests/fixtures/compiler-nsstar/syntax/nsstar.cljs`.
Its SHA-256 is `ebe59a510ec23b6b0d43bbf8927dc149714e1c248abee2c6ca06faef9180e957`.
The initial ordinary require supplies the genuine clojure.string dependency;
only the raw quoted `ns*` directive installs the `text` alias used by both calls.
No original obligation, program form, literal expectation or phase boundary was
removed or changed.

Current source-bound observations and separate generic-report failure are in
`compiler-nsstar-verification.json`. The complete program returns the string
`"true"` when its final `pr-str` executes. The Node reference export returns
boolean true under both independently checked pins: Maven ClojureScript 1.12.134
and checkout a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e, with optimizations none,
static-fns false and def-emits-var false. Full build/run evidence is at
`/tmp/light-cljs-162-reference`.

`syntax_raw_nsstar_alias_executes_in_fresh_and_reused_macro_hosts` executes the
unchanged complete program in Node and QuickJS, and the same whole run body as a
real ordinary-source macro. Two rounds cover fresh/reused hosts in both phases.
The companion tests verify located malformed/unquoted/missing libspecs, referrals,
conflicting aliases, nested restrictions, reuse recovery, no stale alias leakage,
renamed referral identity, `.cljs` value/macro import ownership, provider refresh
and divergent `.clj` companion exclusion. Unresolved unused refers retain the
existing namespace-header location; malformed libspecs retain their own spans.

Verification:

```sh
cargo test --locked --test compiler_syntax_context --test namespaces
cargo test --locked --test compiler_namespace_completion --test compiler_namespace_shadowing --test compiler_macros --test compiler_bootstrap
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tests/reference/compiler-language -p test_syntax_context.py -v
PYTHONDONTWRITEBYTECODE=1 python3 tests/reference/compiler-language/extract_inventory.py --check
PYTHONDONTWRITEBYTECODE=1 python3 benchmarks/run_compiler.py --output /tmp/light-cljs-162-production-final --samples 9 --iterations 1
```

The relevant production benchmark uses release Rust timing and a separate
allocation build, resident namespace fresh/reused MacroHosts, and fresh CLI/Node
processes. Every sample validates complete results. Raw samples, exact source and
binary hashes, environment and manifest are retained in the stated directory;
this is a current receipt, without a before/after timing claim.

The exact driver and checked-pin projection can credit only `syntax::ns*`.
Original ledger rows and every other light-cljs-125 requirement remain intact.
A generic reporter attempt is explicitly failed: it calls a scalar namespace
program result as a callback and feeds namespace stdin to eval without source
roots. Dedicated light-cljs-163 owns this separate harness defect, ordered before
125; no full integrated report or strict zero-missing acceptance is claimed here.
Historical receipts never confer a new pass. Externally blocked light-cljs-156
was not retried, delegated, investigated or repaired; its cancellation/limits
suites were not run. The unrelated `.DS_Store` change is preserved.
