# Analyzer rejection and subset boundary audit

Audited locally on **2026-10-08** against
[clojure/clojurescript](https://github.com/clojure/clojurescript) at clean commit
**a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e**. All upstream paths and anchors below
refer to that commit. Maven **1.12.134** remains the required baseline; the
checkout reports **0.0.135109660**. This is a bounded analyzer audit, not complete
ClojureScript validation or a macro implementation.

## Results and changes

The shared [boundary matrix](../fixtures/analysis-boundaries.tsv) has **119**
cases. It records a stable ID, reference context, source before the diagnostic,
rejected source span, remaining source, local category, useful diagnostic text,
and **separate release and checkout outcomes**. Nothing infers that a locally
rejected form is invalid standard ClojureScript. The checks exposed these fixes:

- Reject a second `:require` clause at that clause's span, before loading dependencies.
- Reject literal nil property/method receivers at the nil token. Dynamic nil
  receivers retain their runtime failure behavior.
- Accept `/` as an unqualified binding/global name. `namespace//` and `alias//`
  resolve its namespace identity; `/` is not itself a namespace qualifier.
- Report malformed second function methods as invalid arity shapes, before
  applying the intentional multiple-arity restriction. Bodies are not analyzed.
- Recognize unsupported `ns*`, `case*` and operator `&` as special-form boundaries
  before ordinary invocation or child analysis.

The eight new execution cases are independent probes: `binding-slash-let`,
`binding-slash-parameter`, `binding-slash-self`, `binding-slash-global`,
`binding-ampersand-within-name`, `special-dispatch-before-local`,
`macro-shadow-catalog-locals`, and `macro-shadow-global`. They assert actual
values/effects, rather than generated names. All **238** runtime programs match
both baselines. Four namespace comparisons pass; `boundary.main` adds global
slash reads through aliases/full qualifiers, lexical alias shadowing and
redefinition. Existing namespace/global/interop programs remain positive tests.

## Classification protocol and limitations

`tests/reference/analysis_boundaries.clj`, invoked by `execution.clj analysis`,
loads the reference core, then clones that compiler state for each case. It
captures warning types separately, analyzes with `:def-emits-var false` and
`:static-fns false`, emits, and asks Node to syntax-check the emitted code.
It never executes these boundary programs. Each stage has a separate category:
`reader-error`, `analysis-error`, `emit-error`, `javascript-error`, `warning`, or
`accepted`. An accepted result establishes analysis/emission/syntax acceptance,
not runtime validity. For example `#(1)` will dynamically call a number.

Ordinary expression rows are analyzed in `:expr` and syntax-checked as returned
expressions, so discarded-function elision cannot hide invalid parameter names.
Type declarations and `ns*` use `statement` context. Namespace rows use their
normal top-level context with `*analyze-deps* false`: these test header validation,
not library availability or import execution. The `cljc` row enables standard
`.cljc` reader conditionals; ordinary `.cljs` rejects that syntax before analysis.
Local analysis rejects the raw conditional node in either context. Missing
source files, unknown aliases and nonexistent qualified globals have independent
loader/API/CLI checks with real temporary namespace files.

Missing tools, timeout, missing/reordered/extra results, version/settings drift,
and unexpected classifications fail the ignored Rust comparison. The same
comparison runs in the required conformance job. A `both` run checks equal
warning/category results except the explicitly recorded version difference;
error-message wording and generated names are not conformance contracts.
Temporary failures retain build logs, invocation and `analysis.tsv`; the source
remains in the versioned matrix.

**Confirmed version difference:** `let-binding-amp` retains the primitive
`(let* [& 42] &)` regression from `compiler_tests.clj`,
`test-amp-as-local-clj-2954` (423–426). Release 1.12.134 accepts and emits it;
the checkout rejects it with “Can't use & as a local binding”. Local rejection
matches the newer regression and the existing declared restriction. It is not
reported as an error from the release. The fixture retains upstream copyright
and EPL 1.0 attribution; the provenance sidecar records its exact reduction.
All other new boundary rows are independently written source hypotheses.

Other material distinctions:

- Unresolved/self/forward/leaked names normally produce **undeclared-var
  warnings** upstream. An unknown qualifier can also warn **undeclared-ns**.
  Local unresolved errors intentionally prevent accidental JavaScript globals.
  Unqualified `console.log` is accepted by the standard compiler but excluded
  here; explicit `js/console.log` remains supported.
- Duplicate parameters and `(fn* ())` are accepted upstream. Their local
  rejection is a stricter primitive signature policy. Some malformed primitive
  keyword binding names also pass upstream analysis/emission; they are not
  accepted local binding names. `(fn* ([] 1) ())` receives an upstream
  `overload-arity` warning; the local error identifies the missing parameter vector.
- Dotted fn parameters and qualified function self-names pass upstream analysis
  but emit syntactically invalid JavaScript. `(fn* [&] 1)` reaches an emitter
  error. These are reference limitations, distinct from invalid-analyzer cases
  and valid unsupported features.
- A same-namespace qualified def and def docstring are valid upstream but outside
  the local simple-name/optional-initializer def shape. Cross-namespace defs are
  upstream errors. Dotted global defs emit host-path assignment upstream;
  local globals deliberately remain separate from interop. A global named `&`
  is also outside the established local name restriction.
- Same-target duplicate aliases are accepted upstream; `:as js` warns.
  Local rejection is intentional. Single-segment `boundary` produces a harmless
  namespace warning in accepted header cases; it must not become an error.
  `ns-rename` specifically renames an un-referred name and is an upstream error;
  valid renaming itself is outside this loader's scope.

## Source assertions, local identities and dispositions

All implementation paths in this table begin with `src/main/clojure/cljs/`;
all test paths begin with `src/test/clojure/cljs/`. Prefixes refer to stable IDs
in the matrix; executable IDs refer to `tests/fixtures/execution.tsv`.

| Pinned source and stable anchor | Audited rule and local disposition |
| --- | --- |
| `analyzer.cljc`, `parse 'if` (1857–1869); `analyzer_tests.clj`, `analyze-ops` if; `analyzer/spec_tests.clj`, `test-if` | `if-empty`, `if-one`, `if-extra` fail before analyzing an unresolved test. Supported 2/3-operand if has existing context/truth/branch runtime coverage; upstream AST layout is not a local contract. |
| `analyzer.cljc`, `analyze-let` (2568–2571), `analyze-let-bindings*` (2508–2552); `analyzer_tests.clj`, `analyze-ops` let/local (671–687) | `let-missing`, `let-non-vector`, `let-odd`, all `let-binding-*`: require an even vector and simple local symbols. The amp version difference is isolated above. Empty/sequential/repeated-name lets already execute; destructuring macros stay excluded. |
| `compiler_tests.clj`, `test-amp-as-local-clj-2954` (411–433) | Adapt only the primitive let* assertion as `let-binding-amp`; macro let and loop* assertions are unsupported boundaries, not claimed as implemented. `binding-ampersand-within-name` ensures the exact `&` rule does not reject `a&b`. |
| `analyzer.cljc`, `analyze-fn-method-param`, `analyze-fn-method`, `fn-name-var`, `parse 'fn*` (2226–2415); `analyzer_tests.clj`, `analyze-ops` fn (800–859); `analyzer/spec_tests.clj`, `test-fn`, `test-fn-method` | All `fn-*` rows cover missing names/vectors, malformed method shapes, invalid binding kinds, duplicate parameters and variadic/multiple-arity exclusions. Primitive permissiveness and emitter failures are classified explicitly. Supported empty/direct/single-method/named functions retain existing executable coverage. Macro-expanded AST/schema assertions are not imported. |
| `analyzer_tests.clj`, `test-cljs-2005`, `test-cljs-2863` (425–455); `analyzer.cljc`, invocation arity warning (4046–4057) | Duplicate/incompatible overloads warn upstream. They do not prove malformed fixed-call invocations are compiler errors. Existing `arity-*` runtime fixtures test missing/extra arguments; valid multi-method and variadic declarations remain unsupported here. |
| `analyzer.cljc`, lexical lookup/resolve-var, let binding initializer order, function environments; `analyzer_tests.clj`, `analyze-ops` variables/local-shadow | `self-init`, `forward-local`, `self-closure`, `forward-global`, `leaked-*`, `unknown-global`, `unknown-alias`, `unqualified-host`, `macro-env` reject missing lexical/global identities. They retain upstream warnings/acceptance separately. Existing global self-reference, declarations, closures and qualified-name positives establish the intended supported boundary. No `&env` is simulated. |
| `analyzer.cljc`, `specials` (1508–1509), `macroexpand-1*` (4293–4354), `analyze-seq` (4372–4398) | `nil-call` fails before its unresolved argument. `special-*`, `macro-*`, `macro-shadow-exit` and all unselected-branch variants test dispatch before child analysis. `special-dispatch-before-local` and the two shadow runtime cases prove the special/macro distinction; the local macro catalog remains diagnostic-only. |
| `analyzer.cljc`, `parse 'case*`, `parse 'throw`, `parse 'try`, `parse 'ns*`, `parse-type`; `analyzer/spec_tests.clj`, `test-case`, `test-js`, `test-new`, `test-loop`, `test-letfn`, `test-defrecord`, `test-deftype`, `test-ns*` | `special-casestar`, `special-throw`, `special-try`, `special-nsstar`, type/record and other special rows use reference-accepted shapes in their legal contexts. Tail recur is inside fn*. Operator `&` itself is in upstream's dispatch set but has no parser method, so that row is an upstream error, not a valid feature claim. Runtime/library/type expansion remains excluded. |
| `analyzer_tests.clj`, `analyze-ops` constants/collections/quote/js-object; `analyzer/spec_tests.clj`, `test-const`, `test-js-array`, `test-js-object`, `test-map`, `test-set`; `analyzer.cljc`, reader `forms-seq*` (4702–4728) | All `reader-*` rows cover collection/list/keyword/character/regex/quote/syntax-quote/deref/metadata/tag/conditional/namespaced-map/reader-fn exclusions. Numbers/UTF-16 have their separate literal audit. Reader discards remain supported trivia. Reader expansion, runtime collection operations, tag resolution and macro execution are excluded. |
| `analyzer.cljc`, `parse 'def` (2041–2077); `analyzer_tests.clj`, `test-cljs-1274` (349–354); `analyzer/spec_tests.clj`, `test-def` | All `def-*` rows separate argument/name errors, docstring and current-ns-qualified exclusions, dotted host assignment and `&` restrictions. Existing initialized/uninitialized/redefined/self-referencing globals and new `/` runtime/namespace positives remain supported. Const/metadata/Var behavior stays excluded. |
| `analyzer_tests.clj`, `spec-validation` (68–180); `analyzer.cljc`, `parse-require-spec`, `parse 'ns`, one-clause guard (3501–3503) | All `ns-*` rows cover name/libspec/alias/option/clause shapes; repeated require is now rejected. Unknown clauses and invalid aliases are upstream errors; refer/import/package/macro/refer-clojure forms and permissive alias behavior are deliberate boundaries. Existing loader tests cover mismatched/first declarations, roots, cycles, alias collisions and dependency initialization. |
| `analyzer_tests.clj`, `test-cljs-1346` top-level ns/ns* assertions (548–574); `test-cljs-3320` (1538–1543) | `special-nested-ns` rejects ns inside a function, including unselected wrappers. `special-nsstar` is independently rejected as unsupported. `ns-as-js` preserves the reference warning distinction. No top-level require/REPL loader is implemented. |
| `analyzer.cljc`, `classify-dot-form`, `build-dot-form`, `analyze-dot` (3688–3827); `analyzer/spec_tests.clj`, `test-host-call`, `test-host-field`, `test-js-var`; `analyzer_tests.clj`, `analyze-ops` local shadow | `property-*`, `method-*`, `host-*`, `member-*` validate receiver count/literal nil and the narrower ASCII member/path spellings before operands. Explicit dot syntax, munged names and Unicode host paths stay excluded. Existing js-path/property/method runtime cases preserve getters, order, receivers and lexical `js/x` shadowing. |

## Exact local diagnostics

`tests/boundaries.rs` uses the same matrix for analyzer/API and executable CLI
checks. Each non-namespace row is tested in a do and in an **unselected branch**,
after both a newline and multibyte text on the diagnostic line. It asserts the
exact original byte range, line **2**, byte column, structured error category,
and diagnostic wording. Both `--analyze` and `--compile` must exit nonzero with
empty stdout. Successful forms before a failure cannot leak partial JavaScript
or partial AST output.

Namespace cases use real temporary dependency sources, checking canonical file,
exact span, line/column, useful loader category and CLI nonzero/empty output.
Additional missing-dependency, unknown-alias and missing-qualified-global cases
exercise loading/resolution, including Unicode before the offending qualifier.
Existing parser/lexer regression coverage is unchanged. The provenance validator
ensures every matrix/runtime ID has an origin and every adaptation has a pinned
source, stable anchor, reduction and asserted behavior.

## Retained reference limitation

[slash-statement-limit.cljs](slash-statement-limit.cljs) is the original
hand-authored namespace probe, outside the passing corpus. Release 1.12.134
emits `var ._1357 = (9);` (the suffix varies) for its top-level slash local;
Node rejects the dot-prefixed identifier. Local compilation returns stdout
`7\n7\n8\n9\n9\n`. The supported `/` identity is retained; the passing namespace
fixture omits that local binding and separately tests qualified/unqualified
globals. Passing function-context slash cases still test local/parameter/self
bindings. This finding is not a claim of a full upstream namespace or munging fix.

To reproduce without modifying the passing source fixture:

```sh
root=$(mktemp -d)
out=$(mktemp -d)
mkdir -p "$root/boundary"
cp tests/reference/slash-statement-limit.cljs "$root/boundary/main.cljs"
cp tests/fixtures/namespaces/boundary/names.cljs "$root/boundary/names.cljs"
clojure -Srepro \
  -Sdeps '{:deps {org.clojure/clojurescript {:mvn/version "1.12.134"}}}' \
  -M tests/reference/execution.clj release 1.12.134 - "$out" program "$root" boundary.main
(cd "$out" && node reference.js) # SyntaxError; generated files remain in $out.
cargo run --quiet -- --compile --entry boundary.main --source-root "$root" | node
```

## Reproduction and local verification

```sh
cargo test --locked
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
LIGHT_CLJS_REFERENCE=both \
LIGHT_CLJS_REFERENCE_CHECKOUT="$PWD/research/clojurescript" \
LIGHT_CLJS_REFERENCE_REVISION=a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e \
cargo test --locked --test execution -- --ignored --nocapture
```

The boundary comparison can be selected with `analyzer_boundary_classifications`;
the runtime comparison with `programs_match_standard`; the new namespace check
with `slash_bindings_match`. Standalone classification reproduction:

```sh
out=$(mktemp -d)
clojure -Srepro \
  -Sdeps '{:deps {org.clojure/clojurescript {:mvn/version "1.12.134"}}}' \
  -M tests/reference/execution.clj release 1.12.134 - "$out" \
  analysis tests/fixtures/analysis-boundaries.tsv
cat "$out/analysis.tsv"
```

Actual runtime settings were `{:optimizations :none, :static-fns false,
:def-emits-var false}`. Tools: Node **v24.18.0**, Rust **1.99.0**, Clojure CLI
**1.12.4.1618**, effective Clojure **1.12.4**, OpenJDK **25.0.2**. These are local
runs, not hosted CI results. CI already invokes all ignored execution comparisons,
so the new classification and namespace checks run there without an optional
skip. The large reader corpus was not repeated because no reader code changed.

Local verification passed **95 fast tests** (eight reference/corpus tests
explicitly ignored), Clippy and formatting. All six reference checks passed
across the full runtime run and focused reruns after the boundary/namespace
fixture corrections: 238 outcomes/traces/stdout, four namespace outputs and
119 boundary classifications against each baseline. Release classifications
are 51 accepted, 20 warning, 45 analysis-error, one emit-error and two
javascript-error; checkout moves only the amp binding from accepted to
analysis-error. These counts are probe results, not a claim that all accepted
programs execute successfully.
