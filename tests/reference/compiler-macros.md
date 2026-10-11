# Compiler macro environment and resolution evidence

The compatibility anchors are ClojureScript checkout
`a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e` and Maven ClojureScript `1.12.134`.
The tests in `tests/compiler_macros.rs` are independently written executable
Rust/Node cases; no upstream implementation was copied. Normal tests require no
JVM, research checkout, or network.

| Contract | Pinned source anchor |
| --- | --- |
| Locals suppress macro expansion; explicit `:use-macros` precedes core fallback; core qualifiers map to CLJS core | `src/main/clojure/cljs/analyzer.cljc`, `get-expander-ns`, `get-expander*`, `get-expander` (4210–4260) |
| Macro arguments are `form`, full CLJS env, then raw args; dynamic caller `*ns*`; primitive dispatch first | `analyzer.cljc`, `macroexpand-1*` (4293–4354) |
| Fn parameter `:op :binding`, `:local :arg`, `:arg-id`, `:binding-form?`; fn body `:return` | `analyzer.cljc`, fn method analysis (2228–2277) |
| Named fn self binding has `:local :fn` and omits `:binding-form?` | `analyzer.cljc`, fn name creation (around 2430) |
| Let bindings have `:local :let`, `:op :binding`, `:binding-form?`; shadow links | `analyzer.cljc`, `analyze-let-bindings*` (2509–2545) |
| Let value-position body returns; multi-form do value-position tail returns | `analyzer.cljc`, `analyze-let-body*` (2559), `parse 'do` (2475+) |
| Runtime aliases merged before macro aliases; only `.cljc` inputs enable reader conditionals with `:cljs` | `analyzer.cljc`, `get-aliases`, `forms-seq*` (4685–4728) |
| Core fn macro signature/metadata handling | `src/main/clojure/cljs/core.cljc`, `fn` (267+) |

Executable probes against Maven 1.12.134 observed these outcomes during this
implementation, in addition to reading the pinned source:

- A macro inside an initializer sees `:expr`; inside a value-position let body or
  fn body it sees `:return`; a discarded let body sees `:statement`.
- `(js/console.log (do 1 (capture)))` gives its tail macro `:return`; a one-form
  `do` preserves `:expr`.
- Let records contain `{:name x :op :binding :local :let :binding-form? true}`.
  Argument records use `:local :arg`, `:arg-id 0`, and `:binding-form? true`.
  Named fn self records use `:local :fn` and omit `:binding-form?`.
- `(ns-name *ns*)` inside a macro reports the CLJS caller, not the macro's
  definition namespace.
- A runtime `def` does not shadow a same-named explicitly referred user macro.
  A lexical binding does; a qualified macro call still expands. A runtime `def`
  of a core macro spelling suppresses unqualified core fallback.
- Reader aliases for `::m/k` and `::r/k` resolve to macro/runtime targets;
  a shared alias selects the macro target. `.cljs` reader conditionals fail with
  “Conditional read not allowed”; the same `.cljc` form selects `:cljs`.

The implemented environment is a documented subset, not a serialized upstream
AST. In particular it omits upstream inferred types, defs, compiler state, `:info`,
and binding source locations; `:light-cljs/id` is a local extension. This compiler
returns its final expression instead of emitting only top-level statements.
Shared native reader syntax-quote rules remain an intentional boundary, and
runtime helper availability is checked by the existing JS analyzer. SCI's normal
macro APIs keep their separate flat environment semantics.


## Structured JavaScript bridge execution provenance

### Qualified local identities (light-cljs-131)

The original nested-call regression passes at starting commit `bc46624`: reader
work in `ff3b6b3` already changed `MacroHost::compiler_environment` to split
qualified keyword names into namespace/name parts. This task retains that local
construction repair and canonical keyword equality/hash. The historical bounded
environment contract remains owned by light-cljs-107; light-cljs-131 supplies
the qualified-key regression and deeper source/bridge evidence. These are local
contract fixtures, not upstream analyzer AST or full-language ledger passes.

Two additional hand-authored fixtures in `tests/compiler_macros.rs` execute:

- `javascript_form_bridge::qualified_environment_keys_keep_a_long_shadow_chain_and_binding_shape`
  observes IDs `[8 7 6 5]` through four bindings, the terminating nil shadow,
  let/argument/fn kinds, binding flags, argument index, names/forms and binding
  operations. Inspecting the actual key returns `["light-cljs" "id"]`. The
  returned nested call retains its original source span and executes in a fresh
  application evaluator.
- `imported_macro_environment_keeps_qualified_ids_across_a_long_shadow_chain`
  imports an ordinary `.cljs` macro and observes
  `[[3 2 1 0 nil 0 :return app env.tools 6 10 (e/capture) 1] 0]` in both Node and
  fresh QuickJS application heaps. The quoted macro counter is one; the imported
  application atom remains zero. Recompiling with the same host repeats those
  literal observations, proving that macro state is fresh on reuse. IDs,
  argument index, context, alias, caller namespace, call form and location come
  from the actual analyzer environment.

The unchanged exact regression still observes
`[(macros/identity) 2 1 0 :return tools 1 8]` and checks the nested call's span.
Verification:

```sh
cargo test --locked --test compiler_macros javascript_form_bridge::form_and_bounded_environment_use_actual_nested_call_and_shadow_ids -- --exact --nocapture
cargo test --locked --test compiler_macros --test compiler_reader --test namespaces --test compiler_namespace_completion --test provenance --quiet
git diff --check
```

All 156 relevant tests passed: 104 macro, 12 reader, 28 namespace, 10 namespace
completion and two provenance cases. The exact original regression and diff
whitespace check also passed. No new production change or performance claim is
needed for this acceptance coverage.

`tests/compiler_macros.rs::javascript_form_bridge` contains original,
hand-authored fixtures for light-cljs's compiler-owned bridge, rather than
imported upstream tests. They execute Rust-compiled macro functions in the pinned
QuickJS evaluator and analyze returned forms directly. Observations cover:

- Identity and literal categories, ordered map/set structural keys, metadata,
  regex/instant/UUID/explicit `#js` payloads and lone-surrogate UTF-16.
- Returned-code execution with effects in order and exact Number special values.
- Actual nested call `&form`, bounded environment shadow IDs/args/context/aliases,
  reader definition-site syntax quote and session gensym freshness.
- Nested compiler services with GC during conversion, mutable literal detection,
  constructed metadata and session-owned handles.
- Bounded lazy realization and rejection of unsupported/cyclic results, located
  definition/helper/call diagnostics, and fresh-session recovery. Getter,
  proxy-trap and `toString` probes verify that rejection does not inspect arbitrary
  host objects by executing their code.

These bridge fixtures do not claim source `defmacro` declarations/default
bootstrap have switched to JavaScript, nor implement demand-driven source helpers.
Those integration seams retain their separate migration scopes. Executed checks:

- `cargo test --locked --test compiler_macros --test provenance --test evaluator`:
  56 macro tests (including 11 bridge tests), 2 provenance tests and 8 evaluator
  tests passed. The existing ignored startup measurement was not executed.
- `cargo test --locked --test evaluator --test compiler_reader`: 8 evaluator and
  7 reader prerequisite tests passed; the same startup measurement was ignored.
- `cargo test --locked --lib compiler_owned_origin_tests`: 1 provenance/lifetime
  unit test passed.
- `node tests/support/js_runtime.test.js`: all canonical runtime assertions passed.
- `git diff --check`: passed. No full suite or all-features build was run for this
  focused bridge delivery.

### Ordinary source imports (light-cljs-103)

The upstream observations above describe the pinned reference dialect. The
filesystem compiler intentionally uses ordinary `:require` for both macros and
runtime bindings in `.cljs`/`.cljc`; it does not load `.clj` companions. Aliases,
refers and renames share namespace identity, both phases select `:cljs`/ordered
`:default`, and each clean compilation creates fresh macro/helper roots.
Legacy macro clauses/options produce located migration diagnostics. Native
embedding tests retain their explicit adapter until evaluator consumer migration.
Original reference namespace programs are preserved under
`tests/fixtures/reference-namespaces`; local fixtures use this import dialect.
Migrated macro producers use `(array)` in generated code instead of returning
quoted host arrays, whose explicit compiler-literal bridge coverage is tracked
separately. Compiler assertion tests use emitted `assert` forms and configured
host options instead of the native interpreter's `*assert*` Var/macroexpand scope.

Executed checks for the ordinary import migration:

- `cargo check --quiet`: passed.
- `cargo test --test compiler_macros declaration_bundle_demand --quiet`: six
  declaration-bundle prerequisite cases passed.
- `cargo test --test compiler_macros --test namespaces --test cli --quiet`:
  100 macro, 28 namespace and eight CLI cases passed, including direct API/CLI
  execution of the `tools.cljc` example yielding `[42 42 42]` with native
  evaluation disabled for the API checks.
- `cargo test --locked --test compiler_macros --test namespaces --test cli --test compiler_namespace_completion --quiet`:
  the 100 macro and eight CLI cases passed; one namespace metadata expectation
  needed the renamed core macro entry. After correcting that expectation,
  `cargo test --locked --test namespaces --test compiler_namespace_completion --quiet`
  passed all 28 namespace and ten namespace completion cases.
- `cargo test --test execution namespace --quiet`: nine cases passed, 11 pinned
  reference cases stayed ignored. `cargo test --test execution integrated_language_program --quiet`:
  the local combined-language program passed; its reference comparison stayed
  ignored.
- `cargo test --test provenance --quiet`: the unchanged `numeric-number-keys`
  hand-authored row has upstream fields and fails validation; repair is tracked
  by light-cljs-127. A focused validator checked all five changed provenance rows
  and verified all 50 preserved oracle files byte-for-byte against the starting
  checkout. The explicit `#js` macro-output descriptor issue is tracked by
  light-cljs-128.
- Parsing all 54 active example/namespace source files, matching the collection
  example to its executable fixture copies, and `git diff --check`: passed.

No full suite, all-features build, external pinned compiler comparison or
platform-specific build was run for this scoped migration. The shared evaluator
poll-budget failure/recovery case covers bounded compiled macro execution;
public load cancellation APIs remain with evaluator consumer migration.

### Compiler gate fixture migration (light-cljs-114)

The historical `host(source, refers)` fixtures now create ordinary `macros.cljs`
and `cljs/user.cljs` source namespaces and use the same namespace compiler as
application sources. Caller-owned syntax immutability and the raw form/environment ABI also have a
compiled source macro test. Every producer, helper, source effect and generated form in
`tests/compiler_macros.rs` therefore executes through compiler-owned JavaScript.
The helper preserves original caller-relative error spans by subtracting only its
synthetic `ns` header; definition/helper paths remain in the diagnostic. The
bridge and newer same-file/helper-demand/declaration tests keep their direct
compiler APIs. The complete target is checked with
`LIGHT_CLJS_DISABLE_NATIVE_EVAL=1`.

The old observations were adapted where they depended on the native embedding
API or its dialect:

- Source macros receive ordinary imports. A public helper value is available at
  runtime. Local runtime definitions shadow ordinarily referred macros, while
  qualified macro calls still expand. Switching a source declaration between
  macro and value kinds is a located source diagnostic.
  Lexical shadowing, explicit qualified expansion and core fallback still have
  direct expected-output checks.
- Returned UTF-16 keyword/symbol names and distinct keyword name parts are now
  built with compiled `keyword`/`symbol` calls, replacing native callbacks while
  retaining lone-surrogate preservation and executable-symbol rejection. Returned
  individual keywords preserve distinct namespace/name parts. Small literal map
  producers compare keys by canonical spelling equality and retain the first
  representative with the last value. Explicit hash maps also check hashes;
  equal spellings with different namespace/name hashes can remain separate.
  Executed release/checkout evidence and both application engines are recorded
  in [the keyword map reconciliation](compiler-equality.md#keyword-map-reconciliation-light-cljs-130).
- Macro execution is bounded by the JavaScript evaluator's poll budget, then
  checked for recovery with the same host. Expansion depth/count/generated-node
  limits remain separately asserted.
- A depth-exhausted producer uses a distinctive throwing body to prove that it
  does not run; restoring the depth budget exposes the throw. Quoted generated
  data uses a source macro assertion to prove its nested producer has no effect.
- Caller source line is read from `&env`; generated collection diagnostics point
  at the unresolved child symbol. The namespace compiler analyzes its final
  expression in `:return` context.
- Caller namespace/context observations and expansion-failure recovery remain
  source tests. Native dynamic `*ns*`, `ns-name` and namespace-object identity
  remain embedding API contracts in the adapter target. Macro declaration
  metadata is read from the descriptive
  `:light-cljs/macros` environment map, rather than retrieving a callable native
  macro Var.

The compiler gate permits canonical JavaScript interop in macro code (including
existing `js/Math.imul`, thrown `js/Error` values and runtime helper demand cases).
Unsupported arbitrary host results remain bridge errors; JavaScript source
spelling alone is not a prohibition.

Three API-specific native adapter tests are retained in the separate
`tests/compiler_native_adapter.rs` target until light-cljs-115 removes that seam:
`native_host_macros_use_the_same_form_env_and_arity_abi`,
`macro_statistics_attribute_native_and_interpreted_expansion_work`, and
`macro_call_namespace_binding_is_restored_and_do_context_matches_cljs`. This target
is explicitly outside the native-disabled compiler gate, and is still part of
the ordinary Rust suite. No compiler source fixture calls `context_mut`,
`register_macro`, or a native evaluator entrypoint.

Verification for this fixture migration:

- `LIGHT_CLJS_DISABLE_NATIVE_EVAL=1 cargo test --locked --test compiler_macros --quiet`:
  all 99 compiler macro cases passed, including the compiled ABI/syntax counterpart.
- Native-disabled `generated_map_keys_keep_reader_name_parts_before_runtime_spelling_equality`:
  passed after strengthening the returned-keyword name-part assertions.
- `env -u LIGHT_CLJS_DISABLE_NATIVE_EVAL cargo test --locked --test compiler_native_adapter --quiet`:
  all three explicit native adapter cases passed.
- `git diff --check` over the three changed fixture/provenance files: passed.
