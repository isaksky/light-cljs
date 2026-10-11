# Utility, regex and tagged-data literal oracle links

`compiler-surface-gate-links.py` proposes 51 ordinary obligation links without
changing the shared catalogs: 39 utility rows, ten regex rows, and the two public
`tagged-literal`/`tagged-literal?` fixed-arity rows. It preserves every existing
body/signature identity, local test hash, exact phase mapping, and rationale.
`attach_reference(mapping)` lets an existing family generator preserve precisely
these three source/literal identity fields.

The utility oracle is exactly existing `cljs-utilities-contract-0`, reproduced
by `compiler-utilities-completion.py` from the complete independent Rust witness.
It keeps the controlled random source and restoration; all parse inputs,
exceptions, UUID normalization and scalar boundaries; UTF-16 substrings; direct
and saved array functions with one/two arguments and rejected zero/three calls;
clone dispatch/identity; and all literal metadata checks. Same-file macro setup
remains in the separate exact phase test. The canonical oracle omits only that
local compiler-executed defmacro and the duplicate invocation.

The regex namespace adapter retains the entire original
`compiler-regex-reference/reference/main.cljs`, including its namespace/require,
complete 23 upstream observations, every captured group, zero-length sequence,
UTF-16/flags input, regex identity/state, error type and replacement effect. It
adds only a one-observation-argument export around the original zero-argument
`run`, and returns the independent original literal `[[23 true] true]`.

Tagged-data rows reuse the previously reviewed complete
`cljs-public-gate-core-factory-taggedliteral` fixture. Its actual constructor,
factory, saved/applied, fixed-two/fixed-one predicate calls and tag/form/nominal
observations remain intact. No hosted `clojure.core/read` or `clojure.edn/read`
row is linked to a different `cljs.reader` API. The reader `&` row remains outside
this cohort.

## Reproduction and evidence

```
python3 tests/reference/compiler-surface-gate-links.py --propose /tmp/light-cljs-125-surface-proposal
python3 tests/reference/compiler-surface-gate-links.py --run --artifacts /tmp/light-cljs-125-surface-final-oracles
```

The final two new oracles pass emitted Node and hosted QuickJS, Maven release
1.12.134, and clean checkout a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e. Reference
build receipts verify `:optimizations :none`, `:static-fns false`, and
`:def-emits-var false`. Namespace CLI evaluation calls the same complete source
and export directly; its exact command/stdout is retained under `hosted-export`.
The previously reviewed tagged-data oracle is reused, not duplicated.

All four exact named phase tests pass one test, one pass, zero ignored:

- `compiler_utilities_completion::public_utilities_execute_in_applications_and_macros`
- `compiler_regex::regex_api_arities_and_complete_upstream_bodies_execute_in_both_phases`
- `compiler_reader::spelling_metadata_gensyms_and_public_tagged_data_execute_in_both_phases`
- `compiler_printing_completion::output_sinks_capture_nesting_flush_and_restoration`

The printing phase pass is diagnostic evidence only; printing links are excluded.
Commands and full results are in `/tmp/light-cljs-125-surface-phases.json`.
The 51 proposed rows also pass the ordinary exact-source/literal schema loader
against isolated copied catalogs in `/tmp/light-cljs-125-surface-schema`.

Integration is explicit: `--write` installs exactly the proposal; `--check`
verifies installed bytes. `--fixtures-write` reproduces only this generator's
new namespace adapter files. This work did not invoke shared-catalog writes.

## Original printing failures retained

The two new printing namespace files retain the complete original literal Rust
witnesses and the namespace `cljs.user`, which the original record tags require.
Only the local macro declaration/duplicate invocation are omitted. They are
uninstalled diagnostic adapters, not passing cases or declaration links.

The complete printing-values witness passes locally. Release 1.12.134 fails its
original `"-0.0 ##NaN ##Inf ##-Inf"` expectation. The isolated probe in
`/tmp/light-cljs-125-negative-zero-probe` shows release prints `"0"` for literal,
stored and JavaScript-created negative zero, while checkout and both local
engines print `"-0.0"`. Both `Object.is` observations prove the values remain
negative zero. This is a release/check-out version difference, not a defect
shared by both pins. Local `src/js_runtime.js` explicitly prints `-0.0`.

The complete checkout printing-values witness fails later at the original
print-level-zero expectation for vectors, maps, sets, JavaScript arrays and
objects. That failure is separately isolated in
`/tmp/light-cljs-125-print-level-zero-probe`; no expectation is rewritten here.

The complete printing-output witness also passes locally but fails both pins at
`(true? *print-newline*)`. Disabling Node bootstrap with the identity/settings-
verified `nodejs_rt:false` option still fails the same original assertion:
`/tmp/light-cljs-125-printing-output-no-bootstrap-{release,checkout}.json`.
An exploratory outer binding of `*print-newline*` was not retained in any source
or proposed case. All 25 printing mappings remain unlinked; their positive
variadic calls are not silently treated as zero-argument calls.

The isolated depth expression is exactly:

```clojure
(binding [*print-level* 0]
  (pr-str [1] {:a 1} #{1} #js [1] #js {:a 1}))
```

Both checked pins return `"# # # # #js #"`; emitted Node and fresh hosted
QuickJS return `"# # # # #"`. Canonical `core.cljs` writes the JavaScript-object
`#js ` prefix before its delegated map printer applies the depth guard. Local
`src/js_runtime.js` includes that prefix inside `printMapEntries`, so the guard
also suppresses it. The JavaScript-array prefix is suppressed on both sides.
This is a confirmed semantic mismatch, independent of the release's negative-
zero version difference and the reference Node printing environment.

Run the checked, literal, per-pin reproducer:

```
python3 tests/reference/compiler-surface-printing-probes.py --artifacts /tmp/light-cljs-125-printing-probes-confirmed
```

This diagnostic intentionally retains local `mismatch` for depth zero. Its
negative-zero local expectation matches the checkout; release and checkout have
separate explicit literal expectations. The CLI creates a fresh `EvalSession`
(`src/eval_cli.rs`), rather than reusing contaminated output state. The full
original printing witnesses and their expected `"# # # # #"` value remain
unchanged. An implementation follow-up must move only the JavaScript-object
prefix before map depth suppression, preserve array/vector/map/set behavior and
all options/callbacks, and validate actual ordinary/macro/fresh-session behavior
against both pins before changing the original incorrect local expectation.
