# Preserve metadata test callbacks in actual macro computations

The complete existing `cljs-definitions-var-test` source succeeds in both pinned
CLJS references and fresh/reused current applications. During demanded actual
MacroHost computation, both fresh and reused hosts reject the original assertion
`(= :ok (test cljs-3411-function))`. Production remains unchanged; this evidence
identifies a phase discrepancy and does not infer the implementation root cause.

The original namespace fixture is
`tests/fixtures/compiler-namespaces/definitions/cljs/var_test.cljs`, unchanged
as owned `tests/fixtures/arity-after188/cljs-definitions-var-test.cljs`. Its SHA256
is `a6aea8026522804962ec49a6f5e2531c482eb9830c1c16b5962cf21f22b847b0`;
the complete original case contract SHA256 is
`ba0f8526d6a7da4acec44ba0de3e781e0529e2e88664ba705e66e0c3ac5fcd86`.
The body retains the :test metadata callback, its (2,3)/(4,4) calls and literal
sums 5/8, both function and Var test assertions, every definition/doc/metadata
form and the original `(run nil)` callback arity. No callback is replaced by
literal success. Independent expected packet remains returned true, no trace,
empty stdout, and the full namespace/case provenance remains in the receipt.

Both release 1.12.134 and checkout a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e
freshly compile and execute the same unchanged body with two checked identity
markers and canonical default build settings. Current HEAD
f6656a5499e0fcfea09330170f86e13e516c759e independently executes four successful
application engine observations, then preserves both precise located actual
macro-host diagnostics. QuickJS performs the demanded macro computation and
rejects. Node emission/evaluation cannot proceed after that same compiler-host
rejection; these intended Node macro contexts are explicitly not-run, never
called semantic passes. All eight intended phase/engine/lifecycle dispositions
are retained. The same hosts subsequently compute `(+ 40 2)` through a real
new macro and return 42 in all eight fresh/reused/application/macro recovery
engine observations. Six actual emitted/source input hash packets bind both
successful original applications and all four recovery contexts.

The historical negative diagnostic test is explicitly ignored so a future repair
does not fail ordinary regression runs. The exact named diagnostic/recovery
test was explicitly executed with `--ignored` and passed once in 2.68 seconds. Its current
source/compiler/executable/pin/phase artifact receipt is
`test-metadata-after188-receipt.json`, SHA256
`67e64a28c8ada7c402152c65a9a2abd3ecec03380cab2be65b37462eb74dd4d3`.
The raw guard receipt is `test-metadata-after188-raw-guard-receipt.json`, SHA256
`8b13dfb5ea5cf00a59dac35e843cb0f2755556d033fa03a68588d3d97bfbcbac`. The pre-ignored diagnostic and raw receipts are retained
byte-exact as `test-metadata-after188-receipt-before-ignored-diagnostic.json` and
`test-metadata-after188-raw-guard-receipt-before-ignored-diagnostic.json`.
Ten restored mutations of original source, actual application/recovery frames,
precise diagnostic, emitted inputs, canonical output/full source, macro callback
input and missing diagnostic artifact reject through the executable proof.
Pristine restored replay reconstructs the identical receipt. Raw current
artifacts, canonical inputs and diagnostics remain under
`/tmp/light-cljs-125-test-metadata-after188-ignored-final`; actual log is
`/tmp/light-cljs-125-test-metadata-after188-ignored-final.log`.

Public `test` arity1 is `api::src/main/cljs/cljs/core.cljs::test::arity-1`, body
SHA256 `cb90c7a0018fa171249cc89399b844b1ee3841ed859ab96d8ee071cdcdbc858c`,
signature fixed 1/v. Local metadata row is
`local::src/core_var_metadata.tsv::test`, SHA256
`06b6629193d00596290ed96790200f1ee802f1e358d9da83a642f32217e504e8`.
Neither receives new operation/phase credit. Defn's independent passing mapping
uses the unchanged complete deftype workflow instead of this failing metadata
body. The parent owns duplicate checking/focused Pellet creation; no queue
write or production implementation is part of this proof.

```sh
LIGHT_TEST_METADATA_AFTER188_ARTIFACTS=/tmp/light-cljs-125-test-metadata-after188-ignored-final/modules cargo test --locked --message-format=json --test compiler_test_metadata_after188 metadata_test_original_source_preserves_phase_failures_and_recovery -- --ignored --exact --nocapture
python3 -B tests/reference/compiler-language/test_metadata_after188.py --run-pins /tmp/light-cljs-125-test-metadata-after188-ignored-final/pins
python3 -B tests/reference/compiler-language/test_metadata_after188.py --actual /tmp/light-cljs-125-test-metadata-after188-ignored-final.log --modules /tmp/light-cljs-125-test-metadata-after188-ignored-final/modules --references /tmp/light-cljs-125-test-metadata-after188-ignored-final/pins --output tests/reference/compiler-language/test-metadata-after188-receipt.json
python3 -B tests/reference/compiler-language/test_metadata_after188_raw_guard.py --receipt tests/reference/compiler-language/test-metadata-after188-receipt.json --output tests/reference/compiler-language/test-metadata-after188-raw-guard-receipt.json
```

Use fresh empty canonical directories and capture the actual log. This is
ordinary synchronous source/macro/callback failure/reuse evidence. No Pellet 156
cancellation, native async, resource limits, native-crash investigation or repair
occurs; those blocked operations remain excluded. Strict completion remains
incomplete.
