# Equality, hashing and metadata evidence

`compiler-equality.py` reproduces 22 complete upstream `deftest` workflows from
`hashing_test.cljs`, `hash_map_test.cljs`, `hash_set_test.cljs`,
`set_equiv_test.cljs`, `metadata_test.cljc`, `map_entry_test.cljs` and
`var_test.cljs` at checkout `a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e`.
The TSV records 418 ordered boolean observations and preserves every helper declaration, setup binding, loop and collection
size, including the 1,000-entry hash map, 100-element metadata collections and
all 55 map-entry interface assertions repeated over three entry sources.
`compiler-equality-provenance.json` pins original complete body and helper hashes.

The adapter records ordered booleans for `is` and each `are` row, converts
`testing` to `do`, implements `thrown?` as a typed `try/catch`, and selects the
`:cljs` reader-conditional branch. It does not replace higher-order calls,
transducers, constructors or collection iteration with precomputed values.
Helper-internal `assert` forms remain assertions. The original notices remain
in each pinned upstream source; adapted material is under EPL 1.0.

Exact adaptations with a public behavior basis:

- Closure `gobject/get` becomes ordinary ECMAScript `aget` for the same missing
  property, preserving the undefined hash observation without a Closure library.
- Seven static `EMPTY` storage fields become public empty constructors; all
  comparisons with public ordered/unordered hashing remain intact.
- The exact `NodeSeq` and `ArrayNodeSeq` class-identity probes are omitted. These
  are private trie-storage identities; every metadata assertion and the full
  large-map transducer setup surrounding them executes unchanged.
- Private `BlackNode`/`RedNode` construction becomes entries obtained from
  one- and two-entry public sorted maps. The complete map-entry helper executes
  for each and for the public `MapEntry` constructor. Tree colors/storage shapes
  are not public interface obligations.

The hash-set upstream body contains `(= {:foo 2} (hash-set :foo :foo))`.
It is preserved verbatim and both pinned references return false at this site.
The fixture expects `[true true false true]`, explicitly recording that upstream
assertion defect instead of correcting source or claiming it passes.

`cargo test --test compiler_equality` checks fixture provenance and runs each
complete workflow through QuickJS and Node, then through a compiler-emitted
QuickJS macro that returns its boolean vector and through both application
engines. No compiler acceptance alone establishes execution. Run
`cargo test --test compiler_equality equality_fixtures_match_pinned_release_and_checkout -- --ignored --nocapture`
for the pinned release 1.12.134 and checkout oracle comparison. Both oracle
runs passed on 2026-10-11. Ordered literal observations are stored in the TSV;
there is no private object serialization comparison.

The intended-language ledger gains 46 previously unmapped public assertion
witnesses and 14 full workflow fixtures. Existing historical witnesses are
retained as historical evidence, not promoted to proof of the larger workflows.
The direct suite above supplies the complete replacement execution evidence.
The intended ledger marks only the two exact storage-class probes unrelated,
checking their original source text before applying the exclusion. The historical
ledger remains unchanged. Neither excluded probe receives a semantic pass.

Thirty additional obligation/phase mappings bind exact complete named Rust tests
for the public names/arities, retained local APIs and assertion-free upstream
workflows. A local execution witness requires the current source hash and a
passed exact test that executes both application engines and the macro phase.
Local contract coverage never fabricates release or checkout observations: those
columns remain `not-run` unless the independently executed upstream fixture
provides them. This distinction matters for custom protocols and macro contracts
that have no corresponding unchanged upstream body.

Verification on 2026-10-11:

- `cargo test --test compiler_equality`: 2 passed; the separate pinned-oracle
  test is opt-in and was also run successfully (both references, 22 workflows).
- `python3 -B tests/reference/compiler-equality.py`: all 22 full-body/setup
  adaptations reproduce the checked fixture exactly from the pinned revision.
- `python3 -B tests/reference/compiler-language/extract_inventory.py --check`:
  inventory, source/API provenance and manifest hashes verified.
- `python3 -B -m unittest discover -s tests/reference/compiler-language -p 'test_*.py'`:
  72 ledger/accounting regression tests passed.
- `cargo test --test compiler_language --quiet`: 1 provenance/accounting test
  passed. This is not an upstream semantic pass count.

Metadata independence means value equality and hashing for collections, symbols,
records and Vars. Callable JavaScript functions use upstream `MetaFn` wrapper
identity: a new metadata wrapper remains callable but is a new identity, and
`MetaFn` defines neither `IEquiv` nor `IHash` in the pinned core source. The suite
does not claim structural function equality. Host objects, arrays and regular
expressions likewise use identity. Dates compare by time value; UUID values
retain the upstream constructor/normalizing-helper distinction. Numeric zero and
negative zero are equal keys and hash as zero; NaN remains unequal even to itself,
with its upstream pinned numeric hash recorded separately.


The ledger also links the existing complete `clojure.walk/test-preserves-meta`
workflow through `library.equality`, retaining its original library setup and
both assertions. The eight pinned `analyzer_tests.clj::test-namespace-metadata`
assertions inspect JVM analyzer `*cljs-ns*` bindings or AST `:name` metadata;
only that exact body hash is excluded as upstream analyzer representation. Runtime
namespace, Var and value metadata remain public obligations. Reader/eval tests
that inspect metadata through those APIs retain their reader/reflection owners.

An exploratory `report.py --scope intended-language --run --reference both
--feature hash-equality` run executed the broader source selection during
implementation. Its actual source-assertion observations are useful diagnostics,
but its process predated the final runtime and accounting edits, so it is not
presented as final-revision proof or as a whole-language completion result.
The final focused suites and separately executed pinned oracles listed above
provide this change's verification.
The linked walk metadata workflow additionally passed direct local
lexer/parser/Node execution and both pinned reference namespace executions on
2026-10-11, using `report.local_layers` and `report.reference_program`; the latter
retained `:optimizations :none`, `:static-fns false`, and `:def-emits-var false`.
