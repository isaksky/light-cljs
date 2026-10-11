# Quoted macroexpand-1 source and phase evidence — light-cljs-145

The four original SCI binding probes now return `let*`, `fn*`, `def` and `true`
in bare compiler-emitted Node and QuickJS applications and as intermediate
computations of demanded source macros. Each macro wrapper increments its own
phase atom; the independent application atom remains zero. The normal named test
is `quoted_expansion_runs_without_services_in_application_and_macro_phases` in
`tests/compiler_sci_service_probes.rs`. Its complete source hash is
`77a535e899df0ac29e2e3a99b1e604e802fcbaf5c2fd8ee5246b1b49f647f839`.

The original `sci-binding-macroexpand-{let,fn,defn,letfn}` source bytes, SCI
expectations and historical dispositions remain unchanged. Their existing
`sci-execution-catalog.jsonl` mappings retain every source/row/expectation hash.
Four phase mappings bind those exact local SCI obligations to the named test;
a fifth binds the canonical `core.cljc:3323` public `macroexpand-1` arity
`[quoted]`. The public ordinary mapping also names the separate literal oracle
`literal-macroexpand-1-quoted-binding-probes`, a vector containing the same four
unchanged expressions with literal symbol/boolean expectations. No upstream
assertion site was added, substituted or discarded.

Fresh execution records:

- `/tmp/light-cljs-145-sci/evidence.json`: all four exact source rows passed Node,
  QuickJS, release `1.12.134` and checkout
  `a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e`.
- `/tmp/light-cljs-145-public-oracle/evidence.json`: the literal public-arity
  oracle passed both pins. Both reference workflows independently verified
  `optimizations=none`, `static-fns=false` and `def-emits-var=false`.
- `/tmp/light-cljs-145-phase/evidence.json`: the runner executed exactly one
  normal named Cargo test, with one pass and zero ignored tests, supplying all
  five phase bindings. Its invocation and raw Cargo output remain in the same
  directory.
- `/tmp/light-cljs-145-phase/focused-ledger.json`: the existing ledger classifier
  reconciled these actual captured observations as five covered rows, each with
  a passed macro phase column. This is a focused receipt; the full strict gate
  was not executed and the earlier unfiltered counts remain historical.

The public `macroexpand` arity has no new mapping from these four one-step
probes. Dynamic-form hosted services and the retained ignored service diagnostic
keep their separate evidence boundaries.

Reproduce the exact four-source execution and normal phase test:

```sh
python3 -B tests/reference/compiler-language/sci_execution.py \
  --ids sci-binding-macroexpand-let,sci-binding-macroexpand-fn,sci-binding-macroexpand-defn,sci-binding-macroexpand-letfn \
  --local --references --timeout 240 --output /tmp/light-cljs-145-sci-repeat
cargo test --locked --test compiler_sci_service_probes \
  quoted_expansion_runs_without_services_in_application_and_macro_phases \
  -- --exact --nocapture
python3 -B tests/reference/compiler-language/extract_inventory.py --check
python3 -B -m unittest discover -s tests/reference/compiler-language -p 'test_*.py'
```

The ordinary and phase catalogs validate the exact declaration, signature,
complete test source and supporting original corpus hashes. Inventory
regeneration changed only the analyzer source cross-check hash; the 97 files,
1,328 bodies, 4,728 original assertions and 1,327 public arity rows remain intact.

Scoped implementation verification passed 153 unique tests across
`compiler_macros` (104), `eval_sessions` (20),
`compiler_destructuring_completion` (9), `compiler_reader` (17) and the three
normal service tests. The ignored service diagnostic supplies no coverage.
Logs are `/tmp/light-cljs-145-tests.log`, `/tmp/light-cljs-145-final-tests.log`
and `/tmp/light-cljs-145-precommit-tests.log`. The Python accounting suite passed
all 82 tests, inventory freshness passed, and catalog validation checked all
ordinary/phase mappings against their retained targets and complete test sources.

`cargo bench --bench compiler -- --samples 9 --iterations 8` completed 47
application/macro phase pairs and 423 checked samples; its output is
`/tmp/light-cljs-145-perf.csv`. This is normal benchmark validation and supplies
no baseline comparison or performance-improvement claim.
