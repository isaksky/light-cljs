# Exact inference optimizer body scope

The proposal applies only to
`body::src/test/cljs/cljs/inference_test.cljs::test-cljs-2825`, SHA256
`1f5d427a5971afda26215c00fa46166ecccbcec700f3e725ad610799eb9fa8bb`.
The complete original body, all 65 forms, original module and genuine JVM
companion macros remain byte-for-byte unchanged. No public API or source
assertion is returned by `inference_optimizer_scope.validate()`.

The adopted compiler-language-completion group186 revision2 says:
“Exact upstream analyzer AST/cache shape, private hash-trie node/storage classes,
generated JS spelling and optimizer shape are not compiler API goals.” The
context SHA256 is
`f3e25d6a72914bb406675ce959d086d5112288d036622c224d1fd38b1c2f151a`.
Applying this boundary to this single body follows its exact test mechanism:
`truth_-not-called?` generates 65 genuine `cljs.test/is` checks around
`truth_-called?`; that helper temporarily replaces `cljs.core/truth_`, evaluates
each original form as an `if` test, and checks whether the private emitter
helper was called. It checks emitted optimizer shape, rather than the result of
any public predicate. Both actual core sources define `truth_` with the docstring
`Internal - do not use!`; the same declaration has SHA256
`c0d998211ec06329661fdd0cf395211227a10202c142fbd86bec5a36d0cc2e75`.

The exact body scope is `upstream-internals`, with zero semantic, local
application or actual-macro credit. Public predicates, their declaration/arity
obligations, the existing `cljs-2866-test` and `cljs-3068-test` source assertions,
and all unrelated inference contexts remain intended. The 65 macro-generated
checks do not become invented ledger assertions or language execution passes.
No public form is replaced by a precomputed result, and no JVM helper is supplied
as a local compiler macro.

Fresh genuine reference execution uses the complete original
`inference_test.cljs` and `inference_util.clj` in isolated source roots. Both files
are identical between release tag `r1.12.134` (commit
`c4295f303100bbf5afac449242d30bca1126f1a1`) and the required checkout
`a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e`. The real release compiler is loaded
from its Maven JAR; the checkout compiler is checked against its clean commit.
The common oracle checks actual `optimizations=none`, `static-fns=false` and
`def-emits-var=false` while analyzing the original namespace. Genuine
`cljs.test/run-tests` executes all three original module tests, with the genuine
companion macros and `cljs.pprint` dependency intact. Each pin reports:

```
Ran 3 tests containing 67 assertions.
0 failures, 0 errors.
```

This proves the original optimizer contract in each upstream profile. It is
not a local compiler/application/phase pass. There was no attempt to provide
upstream optimizer internals in production.

Reproduce the receipt and six independent fail-closed guards:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 -B tests/reference/compiler-language/inference_optimizer_scope.py --output /tmp/inference-optimizer-scope-new
PYTHONDONTWRITEBYTECODE=1 python3 -B -m unittest discover -s tests/reference/compiler-language -p test_inference_optimizer_scope.py -v
```

The fresh producer-bound receipt is
`/tmp/light-cljs-125-inference-optimizer-scope-v2/receipt.json`. A durable exact
copy is `inference-optimizer-scope-receipt.json`. The earlier v1 reference
execution remains retained; v2 freezes the current producer across execution.
The guards reject a changed pin, working source, removed/reordered/replaced
original forms, a discarded body, altered neighboring assertions and fake
helper checks; they also preserve the intended public predicate rows.

Proposed parent integration: use only the returned exact body ID/hash/span as an
explicit `upstream-internals` scope overlay on the independent assertion-free
body obligation. Retain the whole source/body inventory and all public rows;
call the validator before applying the overlay. Do not register semantic source
or phase witnesses from this scope receipt. The helper, guards and document
make no edits to the shared report, ledger, inventories, witness tables or
production sources. Full-language strict acceptance remains incomplete.


## Integrated exact disposition

The generator now validates and applies this sole original body disposition,
with a frozen helper hash in full-manifest.json and a second validation at load.
The whole original body remains in full-bodies.jsonl. Its three pass-credit
fields remain false, all public truth_/predicate and neighboring source rows
remain intended, and no source/phase witness is registered from this receipt.
The exact durable receipt SHA256 is
`d36851e045e8d12b569bd94a95fa2d55faa95fc5e1690460605668e677762079`.
An independent read-only review found no actionable issue with the integrated
scope, exact source binding or preserved public neighbors.
