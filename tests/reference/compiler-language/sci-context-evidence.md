# Supplemental SCI source contexts

This supplemental catalog preserves 120 complete SCI source bodies outside the
initial 1,174-source expression cohort. It is source accounting, not a passing
coverage declaration. The initial catalog, full ledger, original SCI rows and
evaluator dispositions are unchanged by this adapter.

Each supplemental row retains its original source bytes, independent original
row/source hashes, complete disposition-row hash and original result, ordered
effects and stdout expectation. Its executable source changes only structural
JVM catch-selector tokens (`Exception`, `Throwable`,
`clojure.lang.ExceptionInfo`) to ClojureScript `:default`. Every edit records
its original UTF-8 byte span and token. Strings, comments, quoted forms, reader
discard and syntax quote are not rewritten. The generated adapted source must
equal the previously reviewed evaluator source; another adaptation or fixture
profile cannot enter this cohort implicitly.

Named record methods resolve namespace Vars. The original SCI fixture supplies
`user/observe` as a namespace host callback, whereas the expression driver
normally supplies a lexical callback. For `sci-record-method-exception` the
visible prefix `(def observe observe)` supplies that original fixture binding
before executing the unchanged adapted body. It is recorded separately as
`fixture_prefix`; the original source and expectation remain intact. Both pins
and both local engines execute this setup and retain the `:finally` effect.
It is not a missing record-method implementation.

All cases use independent Node processes and fresh QuickJS heaps. Canonical
printing gets the initial cohort's visible streaming-print setup. Return,
ordered effects, stdout and empty stderr must all match. Node and QuickJS raw
observations remain independent; neither a prior evaluator disposition nor
source analysis can grant a current execution pass.

## Execution receipt at 5a099863eaed17d9d999715719cd861b67380ccb

```sh
python3 -B tests/reference/compiler-language/sci_execution.py \
  --write-context-catalog --context \
  --plan-context /tmp/light-cljs-125-sci-context-plan.jsonl
python3 -B tests/reference/compiler-language/test_sci_execution.py
python3 -B tests/reference/compiler-language/sci_execution.py \
  --context --local --references --timeout 240 \
  --output /tmp/light-cljs-125-sci-context-execution

# Execute the added namespace-global callback setup after its focused review.
python3 -B tests/reference/compiler-language/sci_execution.py \
  --context --ids sci-record-method-exception --local --references \
  --timeout 180 --output /tmp/light-cljs-125-sci-context-global-record
```

The complete first run matched 119 local cases and 117 cases in each reference.
The subsequent complete-body record-method setup matched in all four engines.
Together, **118 source contracts have matching independent Node, QuickJS,
Maven ClojureScript 1.12.134 and checkout
`a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e` observations**. Both reference
identities and the `none/false/false` optimization/static-fns/def-emits-var
settings were checked. The first receipt intentionally retains the failed
record-method observation; the second owns its changed executable source hash.
Every other executable source remains unchanged between these receipts.

Two original sources still differ from both canonical references:

- `sci-io-input-exception`: the project returns `[:caught "outer"]`; both pins
  throw a type error before producing a return observation.
- `sci-io-eof-catch`: the project returns `:eof`; both pins throw a type error.

These bodies use host input/stream contracts. They remain missing pending an
explicit source/host-oracle contract; they are not silently counted as passes or
declared implementation defects. Every original source, expectation and current
observation is retained. No broad SCI error or dialect reclassification is
performed by this supplemental adapter.

Eleven adapter unit tests passed, including structural token protection,
UTF-8 edit reconstruction, complete-body/original-expectation preservation,
stale-catalog failure, exhaustive missing-row accounting and failure outcomes
that cannot project into passing evidence. The existing initial cohort remains
validated separately. Full-language completion still requires ledger integration
and the broader gate.

## Remaining-context planning

`--plan-context PATH` accounts for all 1,301 retained rows outside the initial
catalog, with original source/disposition hashes and retained reasons. Every
planning row says `not-run`; planning is never execution evidence. Categories
identify complete namespace/file setup, library imports, tagged readers,
loader/host profiles, structured failure adapters, retained dialect/result
domains and unresolved canonical reconciliation. No body, expectation or
profile setup is discarded to turn a missing row into a pass.

`load_context()` and `cases(load_context())` expose the supplemental mappings
for the full ledger to integrate. The two mismatching host-input rows must
remain nonpassing after integration. Source hashes bind each raw execution
receipt to its actual adapted source and visible fixture setup.
