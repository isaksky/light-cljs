# ClojureScript reference namespace fixtures

These are the unchanged ClojureScript dialect versions of the project-owned
programs under `tests/fixtures/namespaces`, retained before `light-cljs-103` migrated compiler namespace imports.
They use `.clj` macro companions and legacy macro-import syntax because the
pinned Maven ClojureScript 1.12.134 and checkout
`a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e` require that dialect.

Local compilation uses the corresponding `tests/fixtures/namespaces` programs,
whose macros live in `.cljs`/`.cljc` and use ordinary `:require`. Macro bodies,
application expressions, literal observations and expected output are preserved;
`shared` additionally merges same-name macro/runtime sources and selects `:cljs`
for compile-time helpers. The reference programs retain their original reader
selection, macro companion precedence, macro sugar and separate alias maps.
Those syntax/loading rules are explicit dialect differences, not conformance
claims. Runtime behavior is compared against the preserved reference programs.

No upstream oracle fixture under `tests/fixtures/compiler-namespaces` or
`tests/fixtures/compiler-boundary-support` is rewritten by this migration.
Their provenance hashes and original syntax remain reference evidence.

The full original namespace tree is retained because upstream whole-root builds
scan every source namespace under their supplied program root. Generic reference
program builds use this tree; local compiler execution uses `../namespaces`.
