# Positive namespace assertion witnesses

This audit reads the complete `test-namespace-metadata`,
`test-cljs-1508-rename`, `test-require-global`, and
`test-analyze-refer-global` bodies in pinned ClojureScript revision
`a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e`. The original bodies, assertion
text, expected values, source spans, and hashes remain in the inventory.
The added files are executable adaptations of public namespace contracts;
the initial seven metadata/rename fixtures required no production changes.
Subsequent generic namespace and reflection support is documented in
[namespace completion evidence](compiler-namespace-completion-evidence.md).

## Namespace metadata

Four fixture roots retain the original `weeble.ns` namespace name and the exact
four declaration variants:

| Fixture suffix | Namespace declaration | Expected declared metadata | Original sites |
|---|---|---|---|
| metadata-attributes | `(ns weeble.ns {:foo bar})` | `{:foo 'bar}` | 001, 005 |
| metadata-symbol | `(ns ^{:foo bar} weeble.ns)` | `{:foo 'bar}` | 002, 006 |
| metadata-merged | `(ns ^{:foo bar} weeble.ns {:baz quux})` | `{:foo 'bar :baz 'quux}` | 003, 007 |
| metadata-overridden | `(ns ^{:foo bar} weeble.ns {:foo baz})` | `{:foo 'baz}` | 004, 008 |

Each root adds only the observer macro import and runtime checks. The native
source macro quotes `(meta (:name (:ns &env)))`, exposing namespace-symbol
metadata to JavaScript. This observes the same declared metadata that the
original assertions obtain from `*cljs-ns*` and the analyzed namespace `:name`.
Both original equality checks remain, with their original expected maps.

The first reference run identified a test-adapter difference: a real source
file's namespace symbol gains reader provenance fields, whereas the original
quoted analyzer inputs have no such fields. The adapter therefore removes only
`:file`, `:line`, `:column`, `:end-line`, and `:end-column`. It does not select only
the expected keys: every other metadata entry remains in the exact map equality
check. Merge order, override values, and symbolic values remain unchanged.
The initial failing observations and corrected passing observations are both
retained under the local audit directory.

## Actual runtime and macro renaming

Three fixture roots retain the original `foo.core` declarations and exercise
actual references between source namespaces:

| Fixture suffix | Original rename sites | Executed public contract |
|---|---|---|
| rename-require | 003, 004, 006 | Referred `foo` is callable and identical to both aliased `set/intersection` and fully qualified `clojure.set/intersection`. |
| rename-use | 005 | The exact `:use/:only/:rename` declaration compiles; its renamed function has the canonical identity and runs. |
| rename-core | 007, 008, 009, 010 | `always` runs core `when` semantics including skipped branch effects; `core-map` is identical to core `map`; both original names can be defined and invoked as own Vars. |

The require/use roots include the verbatim pinned `clojure.set` namespace,
`bubble-max-key`, and `intersection` forms, with the original EPL notice. This
preserves the original dependency and Var names. No substitute function or
whole-library coverage claim is introduced. Runtime calls additionally verify
that the actual loaded function intersects two sets correctly.

The core rename fixture requires a warning-free build on all three compilers.
This checks that the original names were excluded, rather than merely allowing
replacement warnings. The checked program runner now enforces `no_warnings`
for namespace programs as well as expression programs.

Rename sites 001 and 002 assert the upstream storage distinction between
`:uses` and `:renames`: `foo` resolves, but is absent from `:uses`. Those exact
storage claims have no separate runtime observation. The extractor now classifies
only those two source projections as upstream internals, with a regression that
keeps the adjacent public rename contracts in scope.
Site 011 is the original invalid un-referred rename diagnostic and is now verified
by the namespace completion negative-diagnostic fixture. The complete original body remains
retained; unmapped sites are not silently counted through neighboring tests.

## Verification and completed global support

All **seven** programs pass local lexer/parser and compiled JavaScript layers.
Native interpretation is explicitly not applicable. All seven also return true
against both checked references: Maven ClojureScript **1.12.134** and the pinned
checkout. The checked runner verifies each reference identity and effective
`:optimizations :none`, `:static-fns false`, and `:def-emits-var false` settings.
These programs provide **16** integrated mappings: eight metadata and eight positive
rename sites. None requires a version-difference exception.

The original `(refer-global :only '[Date])` form and
`:require-global [React :as react :refer [createElement]]` namespace clause are
now implemented and executed locally and against both pinned references.
The three original global assertions have independent runtime witnesses using
the actual Date constructor and an explicitly supplied host React object.
The require-global reference program declares and checks `:nodejs-rt false`
for external globals, then executes the generated namespace code through the
Closure Node bootstrap. The original reference settings remain checked.
See [namespace completion evidence](compiler-namespace-completion-evidence.md)
for these programs, all additional namespace mappings, exact negative probes,
and their full verification records.

Fixtures are under
`tests/fixtures/compiler-namespaces/namespace-positive/`.
Integrated sidecars are
`/tmp/light-cljs-99-tenth-extra-{namespace-cases.jsonl,witnesses.jsonl,origins.tsv}`.
Checked observations and file-backed compiler/runtime logs are under
`/tmp/light-cljs-namespace-positive/`: corrected metadata/core-rename results in
`{local,release,checkout}-provenance`, require/use results in
`{local,release,checkout}`, and historical pre-implementation global analysis probes in
`global-probes`. Current passing global program evidence is in
`/tmp/light-cljs-namespace-completion/`. Shared registries are integrated by the coordinating worker.
