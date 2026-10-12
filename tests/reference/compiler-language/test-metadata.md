# Executable metadata callbacks in demanded definitions

The complete unchanged `cljs-3411-function` declaration and `cljs-3411` body
from the pinned upstream `cljs/var_test.cljs` now execute in ordinary applications
and demanded macro computations. Both original `(test cljs-3411-function)` and
`(test #'cljs-3411-function)` assertions retain literal `:ok`, with both callback
assertions retaining their exact argument lists and sums. The original adapter
and `(run nil)` invocation are unchanged. Source SHA256 remains
`a6aea8026522804962ec49a6f5e2531c482eb9830c1c16b5962cf21f22b847b0`;
whole-case SHA256 remains
`ba0f8526d6a7da4acec44ba0de3e781e0529e2e88664ba705e66e0c3ac5fcd86`.

Top-level deferred registration formerly retained only a function initializer.
Its executable `:test` metadata was attached by application emission outside the
registered unit, so demanded roots never received the callback. The definition
snapshot now carries that original callback form. Analysis lowers initializer,
property assignment and return through ordinary let/set!/do IR in one unit,
used by both emissions. The outer deferred definition avoids evaluating the
metadata expression a second time. Nested definitions retain their existing
lowering. No source callback, assertion, literal or invocation is substituted.

Fresh release ClojureScript 1.12.134 and clean checkout
`a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e` independently return the exact original
true packet with optimizations:none, static-fns:false and def-emits-var:false.
Fresh/reused application and actual demanded macro contexts return true in
QuickJS and Node: eight original observations. The identical hosts then execute
new `(+ 40 2)` macros and return 42 in all eight recovery observations. The proof
freezes its own actually executed binary at test entry, binding the copy to the
Cargo executable and a Node SHA256 observation before further Cargo relinking.
Every complete source, emitted module, pin input/command/output, producer and
binary copy is retained under `/tmp/light-cljs-189-proof`; actual Cargo log is
`/tmp/light-cljs-189-proof.log`. `test-metadata-receipt.json` binds those artifacts.

Controls execute real function/Var callbacks and ordinary calls, overridden Var
metadata, absent/false/non-callable tests, callback expression evaluation exactly
once, independent application/macro mutable state in reused compilations,
throwing callbacks followed by same-host recovery, and nested definitions.
`test-metadata-raw-guard-receipt.json` records ten restored source, actual frame,
macro input/emission, pin output/source, binary-copy and missing-artifact
mutations. Pristine restoration reconstructs the exact acceptance receipt.

The prior ignored diagnostic, original failure receipts and all original
expectations remain byte-exact. `test-metadata-before189-receipt.json` records the
independently executed pre-repair disagreement at starting HEAD `59f4484`, using
the original producer snapshot captured in both pin receipts; its historical
executable is the distinct ignored diagnostic target. Early delivery receipts
are retained as `*-before-executable-snapshot.json`. Later Cargo relinking made
their live executable paths stale; they were not rebound to newer binaries.
The final receipt is from genuine fresh execution with an immutable binary copy.

A distinct direct `(fn? (:test (meta #'add)))` projection remains false in demanded
macro computation while ordinary applications and both pins return true. Its
exact full source, commands, actual QuickJS/Node observations and both pin
receipts are preserved in `test-metadata-var-reflection-gap-receipt.json`, with
raw artifacts `/tmp/light-cljs-189-var-meta-gap`. Duplicate-checked owner
light-cljs-190, **Reflect executable :test callbacks in deferred Var metadata**,
is immediately before125. Its repair must preserve non-forcing metadata lookup.
This delivery repairs the complete original callback invocation contract;
that separate metadata projection remains explicit and receives zero credit.

Inventory regeneration changes only the analyzer source hash in the full
manifest; original inventories, bodies, support sources and obligations are
unchanged. No new125 operation/source acceptance link or full-language completion
claim is added.156 remains externally blocked and untouched. Cancellation and
singular/plural resource-limit tests remain excluded. Unrelated .DS_Store is
preserved; no push or PR is involved.

```sh
LIGHT_TEST_METADATA_ARTIFACTS=/tmp/light-cljs-189-proof/modules cargo test --locked --message-format=json --test compiler_test_metadata metadata_test_original_source_passes_both_phases_and_recovers -- --exact --nocapture
python3 -B tests/reference/compiler-language/test_metadata.py --run-pins /tmp/light-cljs-189-proof/pins
python3 -B tests/reference/compiler-language/test_metadata.py --actual /tmp/light-cljs-189-proof.log --modules /tmp/light-cljs-189-proof/modules --references /tmp/light-cljs-189-proof/pins --output tests/reference/compiler-language/test-metadata-receipt.json
python3 -B tests/reference/compiler-language/test_metadata_raw_guard.py --receipt tests/reference/compiler-language/test-metadata-receipt.json --output tests/reference/compiler-language/test-metadata-raw-guard-receipt.json
```

Use fresh empty pin/module directories and capture the real Cargo output.
