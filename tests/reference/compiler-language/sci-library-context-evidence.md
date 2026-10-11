# Bounded original SCI library/import contracts

This isolated supplemental cohort contains 16 complete original SCI bodies:
four each for `clojure.string`, `clojure.set`, `clojure.walk` and `clojure.edn`.
Each library includes an original `require`/alias/`:refer` declaration plus
complete result or callback/effect contracts. The 197-candidate planning file
retains all original portable returned library sources; the other 181 say
`not-run`. Planning grants no execution or full-gate credit.

The catalog preserves original source bytes, corpus and disposition-row hashes,
profile, original expectation, declarations and complete runtime body. Leading
original require forms remain compiler-top-level; concatenating declaration and
runtime slices reconstructs the whole adapted source. This 16-source selection
has no catch-selector edits. The portable fixture explicitly preinstalls all
four libraries in both compilers. Local namespace `user` and independent
reference namespaces `sci.library.case0` through `case15` are visible fixture
settings; no observed returned identity is renamed to force agreement.

The local Rust driver compiles ordinary namespace applications, executes each
in a fresh Node process and QuickJS heap, and uses a test-only shallow inspector
built from public language predicates. Its inspector preserves actual symbols,
keywords, finite collections and sequences. The existing recursive SCI adapter
captures original return values, ordered callback effects, stdout and empty
stderr. Its host trace is an ordinary JavaScript array; the fixture's shallow
bridge does not treat such host arrays as branded language sequences.
Reference observations use the existing checked compiler helper and recursive
outcome protocol, with fresh Node processes for every case. Neither runtime
execution nor shallow predicate use claims additional public macro eligibility.

## Reviewed execution

```sh
python3 -B tests/reference/compiler-language/sci_library_context.py \
  --write-catalog --plan /tmp/light-cljs-125-sci-library-plan.jsonl \
  --local --references --timeout 240 \
  --output /tmp/light-cljs-125-sci-library-context-16
python3 -B tests/reference/compiler-language/sci_library_context.py \
  --local --references --timeout 240 \
  --output /tmp/light-cljs-125-sci-library-context-16-final
python3 -B tests/reference/compiler-language/test_sci_library_context.py
python3 -B tests/reference/compiler-language/sci_library_context_receipts.py \
  --evidence /tmp/light-cljs-125-sci-library-context-16-final/evidence.json
```

The final run matches all 16 original expectations in both local engines. Maven
ClojureScript 1.12.134 and checkout
`a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e` each match 15 originals and differ on
one. Both reference identities and `:optimizations :none`, `:static-fns false`,
`:def-emits-var false` were checked, as were complete input sources and raw
stdout/stderr case frames. Current driver, inspector, compiler-owned library,
oracle, helper and recursive observation-adapter hashes bind the active receipts.
The active source catalog and all literal contracts are checked by the receipt
validator; incomplete engines, stale hashes and mismatching observations cannot
be promoted to passes.

The **15 four-way agreements** cover string join/lazy effects, literal
replacement and character callbacks; set alias/refer imports, mapped joins,
indexing and select callbacks; walk alias/refer imports, prewalk/postwalk effect
order and nested transformations; and EDN alias/refer imports, reader precedence
and nested tag readers.

`sci-data-edn-tag-discard` remains **original-contract-unresolved**. Its original
source is `(clojure.edn/read-string {:readers {'tag observe}} "#_#tag 1 2")`.
SCI and both local engines return `2` with empty callbacks/stdout. Both reference
pins throw an error with empty callbacks/stdout. The checked-in receipt retains
all three contracts and the original expectation, with
`original_expectation_status: unresolved`. This is a source-specific EDN discard
contract to reconcile; this cohort does not identify a missing production
feature or infer a broad EDN dialect retirement.

The first complete 16-source run retained local harness failures caused by an
incomplete test bridge for raw JavaScript trace arrays. Its local driver hash
and raw harness output remain in `prior_evidence` for every receipt. After
correcting that fixture, the final local run passes all 16; both sets of pinned
reference outcomes agree literally between runs. Historical harness failures
are never reported as passing evidence. Temporary build/log locations are
optional replay artifacts; checked-in receipts retain all active and earlier
raw observations independently of those directories.

Six focused Python tests cover complete original/expectation reconstruction,
leading declarations, bounded planning without execution credit, unresolved EDN
observations, current driver/library/inspector/oracle identities, and mutations
that attempt to turn missing engines or canonical mismatches into passes.
The Clojure/CLJS helper files were structurally parsed with `clj-surgeon`.

## Integration proposal

These 16 rows are isolated supplemental evidence, with no main catalog, ledger
or disposition mutation. A future full-report integration can specialize their
exact matching generic identities after checking original-source/row/body and
expectation bindings, then execute the same library fixture afresh. It may
credit only the 15 original agreements when both local engines and both checked
pins match; the EDN discard row must remain missing until explicit
reconciliation. The other 181 planned sources still require complete original
fixture execution. No broader gate pass is claimed.
