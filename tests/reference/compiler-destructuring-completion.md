# Checkout destructuring completion

The compiler host implements the map destructuring extensions in the pinned
ClojureScript checkout `a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e`. This is an
explicit version difference from Maven ClojureScript **1.12.134**. Neither
release failures nor compiler-only passes establish shared-version coverage.

The source contract is `src/main/clojure/cljs/core.cljc` (`destmap*`,
`destvec*`, and `destructure`) and the complete seven bodies in
`src/test/cljs/cljs/destructuring_test.cljs`, retained in the compiler-language
body inventory. This work does not change the native interpreter's ordinary
binding macros.

## Implementation

`src/macro_host.rs` recognizes extended patterns and lowers them to compiler
primitives before ordinary native expansion. Plain binding forms retain the
existing native expander. Supported extensions include required
`:keys!`/`:syms!`/`:strs!`, unbound keys after `&`, `:select`, `:defaults`,
`:excess`, `:missing`, and the related `:all` operation. Namespace-qualified
and auto-resolved directives keep their names and localize binding names.

Defaults are evaluated once, before the map initializer, and duplicate local/key
defaults, required defaults, orphan defaults, missing `:or`, repeated `&`, and
binding symbols after `&` are rejected. Required lookup distinguishes absent keys
from present nil and false. Nested selections, missing reports, and excess maps
propagate recursively; excess retains nil values and excludes defaults.
Nested pattern augmentation preserves original entry order. Generated forms use
the original macro provenance and preserve reader payloads such as `#js`.

The same expansion supports let, fn/defn overloads, rest parameters, pre/post
conditions, function and loop recurrence, letfn, conditional binding macros,
comprehensions, and source methods on reify/defrecord. The runtime `req!` and
`some-vals` helpers were added by the coordinating worker; their direct behavior
is also exercised by these tests.

The coordinating worker requested compiler `lazy-cat` support during this audit.
The macro wraps each collection expression in its own lazy-seq before concat.
Focused tests check zero arguments and delayed expression effects.

## Complete-body reference observations

The new `tests/compiler_destructuring_completion.rs` includes all seven complete
original test bodies verbatim and adapts only the deftest/testing/is/are harness.
Independent full-body fixtures replace those harness forms with do/assert and
typed try/catch checks, preserving every original predicate and expected value.
An `is` message is never promoted into an asserted predicate.

Both pinned identities were verified through `tests/reference/execution.clj`.
Successful runtime comparisons checked `:optimizations :none`,
`:static-fns false`, and `:def-emits-var false`. The checkout compiled all seven
bodies together, and the compiled program returned true in Node. The separate
seven fixture run also returned true for every checkout fixture.

| Complete body | Assertion sites | Checkout and light-cljs | Maven 1.12.134 |
|---|---:|---|---|
| keys-bang | 18 | All assertions pass | Throws `error` |
| syms-bang | 17 | All assertions pass | Throws `error` |
| strs-bang | 11 | All assertions pass | Throws `error` |
| select-directive | 15 | All assertions pass | Analysis error |
| select-or-defaults | 21 | All assertions pass | Analysis error |
| excess | 22 | All assertions pass | Throws `error` |
| missing-directive | 21 | All assertions pass | Throws `reference-error` |
| **Total** | **125** | **All assertions pass** | **Explicit version differences** |

Runtime categories are the execution harness's normalized error categories, not
claims that the release implements the extension. Release executions for the
first five runtime-difference families stop at the first failed original
assertion; later sites in those bodies are not claimed to execute on release.

The select/default families were each checked independently with the exact
fixture source through the reference analysis driver. Both yielded
`verified-compile-difference`, `category: analysis-error`, with the diagnostic
fragment `cannot be cast`: `clojure.lang.PersistentList` cannot be cast to
`clojure.lang.Named` in the release destructurer. The analysis oracle verified
release identity and effective settings. Infrastructure or generic build errors
are not accepted as this result.

The 89 sites in the five runtime-difference families use `version_difference`.
The 36 sites in the two analysis-difference families use `compile_difference`.
All 125 must remain classified as **version-difference**, never **covered**.
The coordinating worker integrates the fixture and witness records into the
shared accounting files; the dedicated Rust tests alone do not update inventory
status or complete the overall compiler-language work.

## Reproduction and scope

Fast regression suite:

```sh
cargo test --test compiler_destructuring_completion --test compiler_macros \
  --test compiler_sequences --test provenance
```

The destructuring suite contains nine tests: complete pinned bodies, required
keys and unbound ampersand keys, recursive directives, all binding contexts,
malformed patterns, standalone runtime helpers, evaluation order and reader
provenance, lazy-cat behavior, and selector validation.

Reference run artifacts from this audit are in
`/tmp/light-cljs-destructuring-audit`: `checkout-fixtures/results.json`,
`release-fixtures/results.json`, and `verified-compile-differences.json`, with
file-backed build/stdout/stderr logs. The seven fixtures and 125 source-hash
mappings supplied to the coordinating worker are
`/tmp/light-cljs-99-destructure-{fixtures.tsv,witnesses.jsonl,origins.tsv}`.
These temporary paths are local audit artifacts, not a substitute for the
committed fixture contracts and pinned reference runner.

The nine active compiler_tests bang assertions expect undeclared-variable
warnings; this compiler's stricter unbound-name diagnostics reject such programs.
Those warning assertions are not mapped as passing here. The three compiler
`:defaults` error forms are covered by focused rejection probes, but are also
not assigned imported assertion coverage by this file. No claim is made for
unrelated or still-unmapped upstream bodies.

## Selector completion

The native macro host also implements the complete checkout `selector-impl`
contract from `src/main/clojure/cljs/core.cljc`: its argument must be a literal
map containing at least one of `:select`, `:excess`, `:missing`, or `:all`.
It creates fresh result locals and a one-argument function whose map binding
uses the same compiler destructuring pipeline. One requested directive returns
its value directly; several return the non-nil/non-empty values under their
directive keys, using `some-vals`.

The Rust suite now retains eight complete bodies: the seven above plus
`core_test.cljs::selector-test`, totaling **139 active assertion sites**.
The selector body's four reader-discarded compile-error assertions remain
reader-discarded and are not mapped. Separate focused checks verify malformed
macro arguments without converting those inactive sites into coverage.

The exact standalone full-body fixture was checked against both references:
checkout returned true after all 14 original assertions; Maven **1.12.134**
compiled with undeclared-var warnings and then threw `type-error`. Thus these
14 mappings use `version_difference`, not `compile_difference`, and never count
as shared-version passes. The exact same source passed local lexer/parser and
compiled JavaScript checks; native interpretation is explicitly not applicable.

The additional fixture and source-hash mappings are
`/tmp/light-cljs-99-seventh-extra-{fixtures.tsv,witnesses.jsonl,origins.tsv}`.
Reference logs and observed envelopes are under
`/tmp/light-cljs-destructuring-audit/{checkout-selector,release-selector,local-selector}`.
