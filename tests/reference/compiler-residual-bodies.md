# Residual complete-body evidence (light-cljs-125)

`compiler-residual-bodies.py` retains 19 complete pinned residual test bodies
and their required setup forms, plus the existing complete metadata and
map-entry equality workflows. The 21 fixture rows preserve original assertions,
literal expectations, loops, exception classes and patterns. Generation checks
each original source span against checkout
`a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e`; it does not claim execution.
The adapter maps `cljs.test` assertion forms to executing assertions and
qualifies library aliases. Four `test-717` Closure reads of existing string
properties use ordinary ECMAScript `aget`, following the reviewed equality
adapter. This establishes those property-read results, not general Closure
support. The reused equality workflows retain their explicitly documented
private-storage adaptations in `compiler-equality.md`.

The local exact test executes every row as an ordinary namespace application
and as a complete QuickJS macro computation whose result runs in each
application engine. Both QuickJS and Node compare canonical `pr-str` results
with the original scalar or ordered boolean-vector oracle. Printing happens
inside the compiled namespace's runtime: a collection from that runtime is a
foreign object to the evaluator's separate runtime instance. Macro declarations
stay at namespace top level, and the observation wrapper follows expansion.
The full reader instant body keeps all 7,392 round trips; its visible local
macro/evaluator timeout is 120 seconds.

Recovered reference evidence was reviewed on 2026-10-11. Both references use
`:optimizations :none`, `:static-fns false`, and `:def-emits-var false` and verify
their identities before observation: release 1.12.134 and checkout
`a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e`.

- `/tmp/light-cljs-125-residual-reader-reference/evidence.json` records all 19
  numeric fixture IDs returning `true` with empty trace and stdout in both
  references. Eighteen emitted source files match the current fixtures exactly.
  Its case2 source still uses `goog.object/get`, so that receipt alone does not
  prove the current `aget` adaptation.
- `/tmp/light-cljs-125-residual-case2-reference/evidence.json` is a fresh exact
  current-source run of case2 against both references; both return the complete
  expected contract. Together these receipts cover all 19 current numeric
  complete bodies in both references.
- `/tmp/light-cljs-125-residual-reference/evidence.json` contains an earlier
  11-body selection with older numeric numbering. It remains historical
  evidence and is not identified with current fixture sources by ID alone.
- The two unchanged equality source/oracle rows are reproduced from
  `compiler-equality.tsv`. The established
  `/tmp/equality-reference-test.log` records all 22 complete equality workflows
  matching both references; `compiler-equality.md` documents that independent
  pinned oracle run and its ordered vector contracts.

Recovered receipt SHA-256 values, in the order above:

```
e2df2590c3fe45147078e77cca18571eba6ba2068447cdc39de548f543be2434
e2fa5e930c74e848e2f41c61209262fc9376aee3e0bed9001692606e001b418b
85a9e3873ce771551dd06d26e3f537b74fa05dfc26eaf1c39b873e99545e4db7
c687aba567c477cf4cbe9090dff67b1d81aa6652be1097911dc0e9797b7597af
```

The generator owns 107 new assertion mappings and 26 residual phase rows.
Shared catalog checks compare only its owned rows; regeneration preserves
unrelated generator rows and their order. The residual fixture SHA-256 is
`06ab759c34f2207b38b4fccfe937435f5606c6ffa19950c8e174069524437994`.

Reproduce the provenance and local execution checks with:

```
python3 -B tests/reference/compiler-residual-bodies.py --check
cargo test --test compiler_residual_bodies complete_residual_bodies_execute_in_both_phases -- --exact --nocapture
```

Both checks passed on 2026-10-11. The exact Rust test executed all 21 rows
through both engines and both phases in 165.36 seconds (1 passed, 0 failed).
The retained final logs are
`/tmp/light-cljs-125-residual-recovery-local.log` and
`/tmp/light-cljs-125-residual-recovery-generator-check.log`.

Reference observations establish ordinary complete-body contracts. They do not
fabricate upstream macro-phase observations or a whole-language completion
claim. The separate local exact phase test supplies application/macro evidence.
