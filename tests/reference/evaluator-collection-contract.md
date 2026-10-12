# Evaluator collection fixture: canonical replacement

On 2026-10-12, both required ClojureScript pins independently compiled the
complete, unchanged evaluator source:

```clojure
[{:a 1} #{2 3} (apply (fn [a b] (+ a b)) [4 5])]
```

Maven **1.12.134** and clean checkout
**a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e** both print
`[{:a 1} #{3 2} 9]`. The observed result is a vector of a one-entry map
from keyword `:a` to number `1`, a two-member set traversed as `[3,2]`, and
number `9`. Effective options were `:optimizations :none`, `:static-fns false`,
and `:def-emits-var false`; compiler and runtime identities were checked by
`execution.clj` before and after each build.

The original evaluator expectation was `[{:a 1} #{2 3} 9]`. The identical
QuickJS failure was reproduced independently in an archive of
`fbfa9364e51d5414db3b2e54d46aef8af9736919` during Pellet 187 and again at
starting HEAD `1184bc6a9ccb1c43297885432446c4ce64056d66` before this repair.
The [historical and fresh receipt](evaluator-collection-receipt.json) preserves
both failure outputs and the two original-source reference observations.
The existing [lint receipt](compiler-clippy-strict-evidence.json) remains intact.

Pellet 188 changes only this stale evaluator expectation and adds a full typed
observation of the same source in QuickJS and Node. The
[observer](../support/evaluator_collection_observation.js) preserves map entries
and set traversal without sorting, projection, or unordered comparison. Its
[expected packet](../fixtures/evaluator-collection-observation.json) was copied
from the release observation only after confirming byte equality with the
checkout observation. Both original collection expressions and the full apply
expression remain present. No production implementation changes were needed.

## Reproduction

Create fresh absolute directories `$OUT/release` and `$OUT/checkout` outside the
repository, and write the exact source above to `$OUT/input.tsv` as one row with
ID `evaluator-collections`, a tab, and the source. From the repository root:

```sh
clojure -Srepro -Sdeps '{:deps {org.clojure/clojurescript {:mvn/version "1.12.134"}}}' \
  -M tests/reference/execution.clj release 1.12.134 - "$OUT/release" cases "$OUT/input.tsv"
clojure -Srepro -Sdeps "{:deps {org.clojure/clojurescript {:local/root \"$PWD/research/clojurescript\"}}}" \
  -M tests/reference/execution.clj checkout a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e \
  "$PWD/research/clojurescript" "$OUT/checkout" cases "$OUT/input.tsv"
```

Each build directory's `driver.js` contains the following; substitute the absolute
observer path and execute `node driver.js` **with that build directory as cwd**:

```js
require('./reference.js');
const observe = require('node:vm').runInThisContext(
  require('node:fs').readFileSync('/ABSOLUTE/REPO/tests/support/evaluator_collection_observation.js', 'utf8'));
if (lightCases.length !== 1) throw new Error('Expected one original source');
console.log(observe(lightInspectValue, cljs.core.pr_str,
  lightCases[0](() => { throw new Error('Unexpected effect'); })));
```

The raw observed packet must equal the committed expected packet exactly.
Run the two evaluator tests by their exact names in `tests/evaluator.rs`; run
`cargo test --locked --test compiler_reader --test compiler_runtime_set_order --test compiler_printing_completion`
for relevant ordinary collection/reader/printing regressions.

Verification passed both focused evaluator tests, 19 reader tests, two runtime
set tests, and ten printing tests (33 total).
`cargo clippy --locked --all-targets -- -D warnings` completed with zero diagnostics.

This focused fixture reconciliation grants no new language-completion credit.
Pellet 125's strict original source, expectation, and evidence requirements are
unchanged. Pellet 156 was not retried or investigated; cancellation and resource
limit execution suites were excluded. Ordinary printing length/level checks ran.
