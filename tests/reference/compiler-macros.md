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
