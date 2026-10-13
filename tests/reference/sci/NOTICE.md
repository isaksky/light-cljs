# SCI fixture attribution

This notice is kept for attribution of SCI-derived material (see the adjacent
`SCI-LICENSE.txt`). The `tests/interpreter_*.rs` files it names belonged to the
removed native interpreter and no longer exist; the adapted fixture rows they
describe remain in `tests/fixtures/sci-outcomes.tsv`, with per-row origins in
`tests/reference/fixture-origins.tsv`. References below to "README.md" mean the
mapping now kept in `fixture-origins.tsv`.

Some assertions in `tests/fixtures/sci-outcomes.tsv` are adapted from SCI
(`https://github.com/babashka/sci`), revision
`5fe89056384418be50d232b4278344cb3a1a526b`.
The scalar/source assertions in
`tests/interpreter_values.rs::retained_scalar_source_matches_sci_parse_plus_string_assertions`
are also adapted from `test/sci/parse_test.cljc`, `parse+string-test`, lines
23–31, at that revision; the Rust lifetime behavior is an independent addition.

The metadata and map assertions in `tests/interpreter_collections.rs` adapt
`core_test.cljc::meta-test` (1610–1641) and `array-based-map-test`
(1667–1671). The array-map assertion compares contents instead of iteration
order. The new corpus also preserves `eval-colls-once` (1225–1227) for future
evaluator coverage; the native storage tests do not execute that program.

The incremental reader assertions in `tests/interpreter_reader.rs` adapt
`test/sci/parse_test.cljc::parse-test` (9–21), `parse+string-test` (23–35),
and `test/sci/read_test.cljc::read-eof-value-test` (106–131) at the same pin.
They compare ordinary parsed forms, exact source substrings and EOF distinction;
Rust ownership, provenance, discard and invalid-input checks are independent.
The two adapted reader corpus rows and 25 independently authored reader probes
are identified separately in `fixture-origins.tsv`.

`tests/interpreter_eval.rs` executes the existing adapted do/if programs and
new primitive `let*` nested scope and zero-operand `if` assertions from
`test/sci/core_test.cljc::let-test` (176–177) and `core-test` (65–66) at the
same revision. Wrappers are removed and typed results or stable error categories
are compared. Other primitive probes and Rust lifetime/order instrumentation
are independent; the fixture manifest identifies each source.

`tests/interpreter_native.rs` executes extracted zero?-if and int? calls adapted
from `test/sci/core_test.cljc::core-test` (61–62) and `permission-test` (500–504)
at the same pin. The input binding becomes lexical let*; permission setup is
omitted, so the latter asserts registered predicate behavior only. The manifest
separates these adaptations from independent numeric and callback probes.

The function reference corpus adapts `test/sci/core_test.cljc::fn-test`
(205–244), `letfn-test` (1186–1197), and `self-ref-test` (1706–1709).
Simple fn/let macros become primitive fn*/let*, selected calls are combined,
and fixed function factories are called through both supported arities.
Native letfn comparisons use the explicit equivalent raw letfn* sources in
`native-letfn.tsv`, preserving calls without reproducing SCI's Var expansion.
The manifest separates these adaptations from the independent capture,
argument-order, wide-call and permissive raw-signature probes.

The recur/loop corpus adapts ten assertions from
`test/sci/core_test.cljc::recur-test` (706–758) and `loop-test` (817–850).
Primitive fn*/loop*/let* replace the simple macros and one defn, while preserving
backedges, target arity, tail checks, initialization and the twenty-slot swap.
The manifest records exact assertion lines. Other recur/capture/polling probes
and the Rust trampoline are independently implemented.

The namespace/Var corpus adapts `test/sci/core_test.cljc::def-test` (255–266),
`declare-test` (1096–1108), `defonce-test` (1218–1219), `resolve-test` (350–374),
`conditional-var-test` (1973–1978), and `test/sci/namespaces_test.cljc` namespace
partitioning (120–121), namespace-map shadowing (191–192), and namespace identity
(103–105) assertions. Wrappers are removed, simple macros become fn*/let*, and
Var/namespace metadata is projected with get. Exact per-row adaptations and
independent ordering/identity/introspection probes are separated in the manifest.
Additional assertions adapt `core_test.cljc::compatibility-test` (1212–1213)
for an unbound self initializer and `namespaces_test.cljc::find-ns-test`
(231–234) for nil lookup.

The exception corpus adapts `test/sci/core_test.cljc::throw-test` (976–980),
`try-catch-finally-throw-test` (1010–1017), and `ex-message-test` (1154–1156).
Message assertions project portable exception text, and the two nil-body
assertions are combined into a vector. Other catch/finally/provenance/lifetime
probes and the Rust exception implementation are independently authored.

The collection-core corpus and `tests/interpreter_collection_core.rs` adapt
`test/sci/core_test.cljc::core-test` literals (107–117) and
`call-quoted-symbol-test` (1607–1608). The supplied input becomes a primitive
lexical binding for the nested literals; the symbol invocation is unchanged.
The remaining collection, projection, numeric-index and callback probes are
independently authored. The existing `eval-colls-once` row still requires
unimplemented `sort-by`; no claim is made that it executes natively.

The finite traversal corpus and `tests/interpreter_sequences.rs` adapt
`test/sci/core_test.cljc::core-test` map assertion (95–96), `fn-test` apply
assertion (214), and `try-catch-finally-throw-test` catch assertion (998).
The map probe uses eager mapv with the same input, the function uses primitive
fn* and vec to compare rest contents, and the catch probe is unchanged. The
manifest distinguishes the other independently authored sequence/reduction/
combinator probes. Rust implementations are independently authored.

Copyright © 2019-2022 Michiel Borkent

SCI is distributed under the Eclipse Public License 1.0. A verbatim copy is
included in [SCI-LICENSE.txt](SCI-LICENSE.txt). Exact source paths, named test
anchors, line ranges and adaptation explanations are recorded in
`tests/reference/fixture-origins.tsv`. These fixtures extract source programs
from assertions, remove SCI test/evaluation wrappers, and compare typed values,
effects and stable error categories. Independently authored probes are marked
separately; the Rust transport, Python launcher and Clojure driver are local
reference infrastructure, not a port of SCI's implementation.


`tests/interpreter_limits.rs` adapts interruption assertions from
`test/sci/interrupt_fn_test.cljc` at the same revision: uncatchable interruption,
finally execution/non-masking, interrupt-marker forge resistance, ordinary
recursive interruption and successful under-budget evaluation. Host setup becomes
Rust resource limits/tokens, simple macros become loop*/fn*, and reduction uses
a finite vector. Detailed test anchors and independent native additions are
listed in README.md. These adaptations retain the SCI attribution above and
the adjacent SCI-LICENSE.txt.


The binding/function corpus adapts the pinned SCI assertions documented in
`README.md` and `fixture-origins.tsv`: ordinary destructuring, function/Var
metadata, lexical macro environments, pre/post conditions and keyword arguments.
The singleton-map and trailing-map assertions in
`test/sci/clojure_destructure_test.cljc` originate in Clojure's
`test/clojure/test_clojure/data_structures.clj` at
`98d735fab02f337cee654cb0629bddc09883a75a`. Their upstream notice is preserved:

Copyright (c) Rich Hickey. All rights reserved.
The use and distribution terms for this software are covered by the
Eclipse Public License 1.0 (http://opensource.org/licenses/eclipse-1.0.php)
which can be found in the file epl-v10.html at the root of this distribution.
By using this software in any fashion, you are agreeing to be bound by
 the terms of this license.
You must not remove this notice, or any other, from this software.
Author: Frantisek Sodomka.

The adjacent SCI-LICENSE.txt supplies the Eclipse Public License 1.0 text.
The Rust macro implementation is independently authored and follows the selected
behavior of SCI's `sci.impl.destructure`, `sci.impl.fns` and `sci.impl.namespaces`.

The comprehension/iteration corpus adapts `test/sci/core_test.cljc::for-test`
(867–877), `doseq-test` (889–897), `dotimes-test` (1176–1180), and `while-test`
(1404–1405) at the same SCI pin. Results are projected with vec, println/atom
callbacks become observe, and while state becomes an isolated context Var.
The manifest records each adaptation. The native Rust expansion is independently
authored using the behavior of `sci.impl.for-macro`, `sci.impl.doseq-macro` and
`sci.impl.namespaces`; the SCI attribution and adjacent license apply to the
adapted fixture programs.

Contextual reader assertions additionally adapt `core_test.cljc` reader-conditionals,
readers-test and data-readers-test, and `read_test.cljc` tag-fallback-test at the
same pinned SCI revision. Exact anchors, host-option adaptations, and independent
probes appear in `fixture-origins.tsv` and the reference README.

Namespace loading assertions adapt `namespaces_test.cljc` nested-libspecs-test,
cyclic-load-test, self-require-test, as-alias-test and refer-clojure-exclude, plus
`core_test.cljc` load-fn-test, reload-test and reload-all-test at the same pin.
The shared in-memory source modules preserve dependency shapes; observe replaces
printing and selected namespaces are renamed. Exact mappings are recorded in
fixture-origins.tsv. The native Rust provider/loader is independently authored.
