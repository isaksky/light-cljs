# Analyzer and compiler assertion-scope audit

This audit reads the complete remaining bodies in the pinned checkout
`a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e`, including the full `analyze-ops`
body and the supporting protocol/specify! programs used by `test-cljs-1925`.
It changes only objective internal API/code-shape classifications; implementation
limits and warning-versus-error differences are not exclusion rules.

## Narrow corrections

The following **27** assertion sites are now `unrelated / upstream-internals`.
Every original body, assertion, expectation, span, and source hash is retained.
None had an existing executable witness. Namespace behaviors asserted through
AST projections remain in scope when they describe names, aliases, metadata,
source acceptance, or public diagnostics.

| File / test | Sites | Exact contract outside source-language compatibility |
|---|---:|---|
| analyzer / test-cljs-975 | 1 | `desugar-ns-specs` returns exact intermediate require/macro forms and reload placement. |
| analyzer / test-rewrite-cljs-aliases | 3 | `rewrite-cljs-aliases` returns exact synthetic alias lists; no source namespace is analyzed. |
| analyzer / test-cljs-1763 | 2 | `parse-ns-excludes` returns an exact empty map schema and set representation. |
| analyzer / test-cljs-2292 | 2 | The same helper returns an exact rename map schema and set representation. |
| analyzer / test-parse-global-refer | 1 | `parse-global-refer-spec` returns the internal `:use`/`:rename` schema. |
| analyzer / test-parse-require-global | 2 | `parse-global-require-spec` helper ABI and internal `:require`/`:use`/`:rename` schema. |
| analyzer / test-canonicalize-specs | 5 | Direct helper unquoting and intermediate vector/list normalization. |
| analyzer / test-canonicalize-import-specs | 3 | Direct helper import unquoting and intermediate vector/list normalization. |
| analyzer / test-cljs-1992 | 1 | Cached `:fn-var` implementation bit after declare; no invocation or public Var metadata assertion. |
| compiler / test-js-negative-infinity | 1 | Exact emitted expression text `-Infinity`; no numeric runtime observation. |
| compiler / test-cljs-2352 | 3 | Exact emitted IEEE spellings from JVM Double values; equivalent JavaScript spellings would fail this assertion. |
| compiler / test-resolve-dotdot | 1 | Direct `resolve-var` return-map schema; no invocation of the source macro. |
| compiler / test-goog-ctor-import-gen | 2 | Exact `return` and `new` JavaScript substrings for Closure constructor imports; no constructor invocation/result observation. |

Existing excluded groups remain unchanged: analyzer node fields/children and
representation, inferred tag/type sets, constant table allocation and generated
identifiers, compiler namespace caches, Closure/module caches, and warning
configuration APIs; emitter formatting/munging, source-output metadata, and
optimization-specific dispatch/temporary/code shapes. `analyze-ops` retains its
one direct namespace acceptance assertion. Its other exact AST assertions remain
excluded under the pre-existing rule.

## Historical public-scope snapshot

The following table records the unmapped state at this earlier audit, before the
later diagnostic, warning, metadata/rename and namespace completion work. It is
not the current missing-site count. See
[namespace completion evidence](compiler-namespace-completion-evidence.md) and
the current strict report for the final dispositions. Namespace completion also
adds the two narrowly audited rename storage exclusions described there.

### Public scope requiring witnesses at this snapshot

After the correction, these two files contain **148** in-scope assertion sites:
**115** analyzer sites and **33** compiler sites. Eleven compiler sites already
have witnesses. The following **137** sites across **38** bodies remain unmapped.
Their presence here is not a claim that their behavior is unsupported; many
have independent regression tests, but original assertion-faithful evidence and
both references are still required. Warning counts/messages must not be replaced
by a successful runtime evaluation or an unrelated hard compiler rejection.

| Complete body | Unmapped sites | Public contract to preserve |
|---|---:|---|
| `analyzer_tests.clj::spec-validation` | 19 | Malformed namespace/libspec diagnostics (19 original errors). |
| `analyzer_tests.clj::test-defn-error` | 1 | Invalid defn parameter declaration diagnostic. |
| `analyzer_tests.clj::test-namespace-metadata` | 8 | Namespace metadata merge and override behavior exposed to macros. |
| `analyzer_tests.clj::test-constants` | 2 | Reject constant redefinition and set! of a constant. |
| `analyzer_tests.clj::test-cljs-1508-rename` | 11 | Runtime/core/macro rename resolution, exclusions, valid use and invalid un-referred rename. |
| `analyzer_tests.clj::test-cljs-1274` | 2 | Foreign namespace def rejection and same namespace qualified def acceptance. |
| `analyzer_tests.clj::test-cljs-1702` | 1 | Private Var access warnings, including Var-quote bypass cases. |
| `analyzer_tests.clj::test-cljs-1785-js-shadowed-by-local` | 1 | Warning when js/foo is shadowed by a lexical foo. |
| `analyzer_tests.clj::test-cljs-2005` | 1 | Duplicate fixed arity warning. |
| `analyzer_tests.clj::test-cljs-2863` | 2 | Fixed/variadic arity ordering and duplicate variadic warnings. |
| `analyzer_tests.clj::test-cljs-1346` | 26 | Top-level require/use/import/refer-clojure bindings, quoted argument checks, and nested ns/require rejection. |
| `analyzer_tests.clj::test-analyze-refer-global` | 1 | Source refer-global imports Date from js. |
| `analyzer_tests.clj::analyze-ops` | 1 | One direct namespace acceptance assertion; all other AST-shape sites remain excluded. |
| `analyzer_tests.clj::quote-args-error-test` | 3 | Three wrong-arity quote diagnostics. |
| `analyzer_tests.clj::var-args-error-test` | 3 | Two wrong-arity var diagnostics and non-symbol argument rejection. |
| `analyzer_tests.clj::test-cljs-1871` | 1 | Declared versus defined arglist mismatch warning. |
| `analyzer_tests.clj::test-cljs-2139` | 1 | Undeclared Var warning from source function body. |
| `analyzer_tests.clj::test-cljs-2148` | 5 | Checked array/object access warnings and valid array access without warning. |
| `analyzer_tests.clj::test-cljs-2037` | 3 | Conflicting alias errors and repeat-identical require acceptance. |
| `analyzer_tests.clj::test-cljs-2182` | 1 | Resolve requires a quoted symbol. |
| `analyzer_tests.clj::test-cljs-2475` | 1 | Recur argument count mismatch. |
| `analyzer_tests.clj::test-cljs-2476` | 1 | Invalid recur in try/catch/finally: one assertion site iterates four source forms. |
| `analyzer_tests.clj::test-cljs-2819` | 1 | Earmuffed Var without dynamic metadata warning. |
| `analyzer_tests.clj::test-cljs-3031` | 3 | Loop type-flow numeric warnings and a no-warning control. |
| `analyzer_tests.clj::test-cljs-3190` | 2 | Record factory argument-count warning. |
| `analyzer_tests.clj::test-cljs-3210` | 5 | Four arithmetic type warnings and their combined count. |
| `analyzer_tests.clj::test-cljs-3239` | 1 | A bare goog import leaves no imported bindings; conservatively retained namespace behavior. |
| `analyzer_tests.clj::test-cljs-3320` | 1 | Reserved js namespace alias warning. |
| `analyzer_tests.clj::test-cljs-3371` | 3 | Record constructor valid and invalid arity warnings. |
| `analyzer_tests.clj::test-cljs-3473` | 2 | Exception-result type flow into numeric use: no-warning and warning cases. |
| `analyzer_tests.clj::test-require-global` | 2 | Source require-global alias and referred global bindings. |
| `compiler_tests.clj::or-doesnt-create-bindings` | 2 | An :or key alone creates no local: undeclared Var warning category and message. |
| `compiler_tests.clj::test-cljs-1925` | 2 | No warnings for mutually defined protocol extensions and specify! Object methods; preserves both support programs. |
| `compiler_tests.clj::test-amp-as-local-clj-2954` | 6 | Three binding forms reject & as a local; six original exception-type/message assertions. |
| `compiler_tests.clj::keys-bang` | 4 | Four unbound names after & diagnostics. |
| `compiler_tests.clj::syms-bang` | 3 | Three active unbound names after & diagnostics (one extra reader-discarded assertion remains inactive). |
| `compiler_tests.clj::strs-bang` | 2 | Two unbound names after & diagnostics. |
| `compiler_tests.clj::select-or-defaults` | 3 | Three invalid defaults forms must fail compilation. |

The compile-time `&` tests place assertions inside catch handlers: retain the
original control flow and exception contracts instead of inventing a stronger
assertion. The recur/try test has one assertion site but iterates four distinct
forms. The `:or` and bang diagnostics require that an unbound name remain
unbound; a generic malformed-pattern rejection is not equivalent evidence.

## Checks

```sh
python3 -B tests/reference/compiler-language/test_inventory.py
python3 -B tests/reference/compiler-language/test_report.py
python3 -B tests/reference/compiler-language/extract_inventory.py --check
```

Observed: **12 inventory tests**, **19 accounting/report tests**, and exact pinned
regeneration all pass. Global inventory counts remain **3,237** assertions,
**3,223** active assertions, and **696** full bodies. In-scope sites change from
**2,640** to **2,613**; unrelated sites change from **597** to **624**. The full-body
digest is unchanged:
`1f22e904625c3b8c0b7fbb51954ce8c96991aaffe8c95c943c4163c8e6794372`.

New regression tests distinguish direct internal helpers and exact emitter
spellings from retained public namespace projections and binding diagnostics.
No witness assertion hash was invalidated, and no existing witness points to a
newly excluded site. These are audited scope corrections, not passing coverage.
