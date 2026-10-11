# Shared compiler namespace evidence — 2026-10-09

Contract: Maven ClojureScript **1.12.134** and checkout
**a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e** (reported runtime
`0.0.135109660`), Clojure 1.12.4, Java 25.0.2 and Node v24.18.0.
The existing reference driver checks identity before/after compilation and
observes `:optimizations :none`, `:static-fns false`, and
`:def-emits-var false` while analyzing fixture namespaces, including CLJC files.

## Namespace migration note — light-cljs-103

The observations below describe the original ClojureScript dialect. Its complete
`shared` program is preserved under `tests/fixtures/reference-namespaces/shared`.
The active local fixture under `tests/fixtures/namespaces/shared` now merges
same-name macro/runtime sources, imports macros through ordinary `:require`, and
selects `:cljs` for both phases. `.clj` companion precedence, self-import inference,
macro sugar and separate macro aliases remain historical reference evidence;
legacy macro imports receive local migration diagnostics. Literal results,
metadata observations and application initializer order remain acceptance checks.

## Source and assertion mapping

The full relevant bodies were read from the pinned checkout. The sources below
are under `src/test/`; implementation references are under `src/main/`.

| Pinned source and anchor | Local evidence and adaptation |
| --- | --- |
| `cljs/cljs/core_test.cljs`, `test-cljs-1274`, assertions 001–002, and its preceding qualified `def` | `cljs-language-qualified-global` retains `cljs.core-test`, `(def cljs.core-test/foo-1274 42)`, and both complete equality expressions. Only the test wrappers become an exported function returning the two truth values. Both sites are mapped with their original hashes in `compiler-language/witnesses.jsonl`; the strict live matrix passes both. |
| `clojure/cljs/analyzer_tests.clj`, `spec-validation`, `test-cljs-975`, `test-cljs-1508-rename`, `test-cljs-2037`, `test-namespace-metadata`, `test-rewrite-cljs-aliases` | Namespace validation, renaming and metadata informed the independent integration/unit probes. Existing adapted malformed-libspec coverage is retained. These probes do not claim execution of the upstream analyzer's AST assertions or JVM APIs. |
| `cljs/cljs/ns_test.cljs`, `test-ns`, `test-cljs-1508`, `test-cljs-2292`; `macro_test.cljs`, `test-macros`; `clojure_alias_test.cljs`, complete body | Runtime/core exclusions, require/use renames, explicit macros and `clojure.*` fallback informed the independent `shared.main` program and focused namespace tests. Library implementations used by those upstream tests are not substituted into assertion coverage. |
| `cljs/cljs/syntax_quote_test.cljs`, complete `test-syntax-quote` body | Independent tests preserve qualified reader symbols and keywords through runtime aliases, referred and renamed Vars, and macro imports. |
| `cljs/cljs/metadata_test.cljc`, namespace header and shared interface helpers | The CLJC header's conditional runtime/macro imports informed structural branch selection. The collection metadata assertions remain separately inventoried and are not counted by namespace loading. |
| `clojure/cljs/analyzer_api_tests.clj`, complete file | This revision contains compiler API/warning tests, rather than a portable namespace execution suite; no JVM API emulation or coverage claim is made. |

`analyzer.cljc`'s `locate-src`, `macro-autoload-ns?`, `desugar-ns-specs`,
`check-use-macros-inferring-missing`, `check-rename-macros-inferring-missing`,
and `parse 'ns`, plus `util.cljc/ns->source`, establish the implemented loading
and resolution rules. The motivating Penpot `app.common.data.macros.cljc`
namespace header was also inspected; compiling its full dependency graph and
JVM analyzer-dependent macros is outside this member.

## Observed behavior

The original independent `shared` program, now retained under
`tests/fixtures/reference-namespaces/shared`, ran through the API and both pinned
compilers before namespace migration. It verifies selected `:cljs` branches, splicing,
`:default`, ignored JVM-only branches/dependencies, `.cljs` over `.cljc`, `.clj`
over `.cljc` macro companions, same-name self-import inference, explicit macro
sugar, runtime/macro refers and renames, `:use`/`:use-macros`, excluded/renamed
core names, qualified globals, `clojure.*` alias fallback, syntax quotes,
keyword qualification and namespace metadata/docstrings. Its complete stdout
and initializer order are compared, with direct expected-value checks in Rust.
The namespace docstring is `&env :ns :doc`; it is not added to name metadata.

Focused negative tests cover nonexistent and conflicting imports, aliases,
malformed rename/options, unselected discarded tags, invalid splicing and
macro-generated unsupported forms with original file/call-site diagnostics.
A companion file without a runtime self-import does not enable inference.
Existing native source-provider and reusable macro-host behavior stays separate.

Extension precedence matches upstream. Distinct canonical files at the same
winning extension remain an error under light-cljs's existing explicit-root API;
this intentionally retains its deterministic ambiguity contract instead of
upstream classpath first-match behavior. Runtime `.cljs` and the standalone
string API continue to reject reader conditionals; namespace `.cljc` inputs
select branches before dependency loading and analysis.

The primitive shared namespace `(ns main) #?(:clj jvm-only :cljs 3)` emits exactly
the same **103 CLI-output bytes** as `(ns main) 3` from `.cljs`, with no collection
runtime prelude or additional JavaScript initialization. Branch selection and
import inference add compiler work only, not a browser interpreter/runtime.

## Verification

```sh
cargo test --locked -j 4 --quiet
node --test tests/support/js_runtime.test.js
cargo clippy --locked -j 4 --lib -- -D warnings
python3 -B tests/reference/compiler-language/extract_inventory.py --check

LIGHT_CLJS_REFERENCE=both \
LIGHT_CLJS_REFERENCE_CHECKOUT=research/clojurescript \
LIGHT_CLJS_REFERENCE_REVISION=a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e \
cargo test --locked --test execution \
  shared_namespaces_and_macro_inference_match_standard_clojurescript \
  -- --ignored --nocapture

python3 -B tests/reference/compiler-language/report.py --run --strict \
  --id 'core_test.cljs::test-cljs-1274::assertion-001' \
  --id 'core_test.cljs::test-cljs-1274::assertion-002' --reference both
```

The full Rust suite passed **607 tests**, with **14 explicitly ignored** optional
reference/research tests. The focused shared program passes in generated JS and
both references. The language report has **2 covered, 0 missing** for the exact
selected assertion IDs, matching typed results, effects and stdout. Native
execution is explicitly inapplicable to these compiler namespace witnesses.
The Node runtime test, Clippy, provenance/accounting checks, and checked source
inventory also pass. These counts do not claim completion of all namespace,
reader, macro, library or JVM analyzer sections in the retained inventory.
