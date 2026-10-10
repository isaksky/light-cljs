# Runtime assertion dependency and profile audit

This audit retains the complete pinned source at ClojureScript revision
`a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e`. It corrects dependency and
build-profile classifications; no original body, assertion predicate,
expectation, ID, span, or source hash is removed or rewritten.

## Dependency corrections

The extractor now captures a local initializer's current lexical dependencies
before replacing that local name. For example, the inner `s1` in
`[s1 (conj s1 1)]` still depends on the outer sorted-set initializer. Previously
it became a self-reference and lost that dependency. Bindings in `doseq` and
`for` retain every original input, and nested vector/map destructuring projects
literal components separately. A normal sibling binding therefore does not
inherit another binding's sorted or transient producer. Unknown producers are
followed conservatively. This remains structural scope guidance, not an
executable dependency resolver; full bodies must still be preserved by witnesses.

The following originally in-scope sites now carry their existing precise
implementation exclusion. All source IDs use their unchanged source ordinal.

| Complete body | Assertion ordinals | Category | Reason |
|---|---|---|---|
| `collections_test.cljs::test-sorted-map` | 007–018 (12) | sorted-collections | Inner `m1`/`m2` updates retain their outer sorted-map constructors. |
| `collections_test.cljs::test-sorted-sets` | 007–032 (26) | sorted-collections | Nested `s1`–`s4` updates retain sorted-set constructors and comparators. |
| `collections_test.cljs::test-784` | 001–005 (5) | sorted-collections | Complete doseq inputs include sorted-map and a sorted-map constructor function. |
| `collections_test.cljs::test-cljs-1497` | 011–013 (3) | sorted-collections | `[k' v']` is obtained by `find` on the sorted-map local. |
| `collections_test.cljs::test-phm-fixed-hash` | 016–018 (3) | transient-collections | The final persistent map was produced by the original transient/assoc!/persistent! chain. |

The first two bodies' other sites were already excluded. The normal hash-map
parts of mixed bodies remain in scope. **Eight** existing mappings initially
needed removal (test-784 001–005 and test-cljs-1497 011–013). During concurrent
fixture integration, test-phm-fixed-hash 016–018 also gained mappings; those
**three** were reported for removal. No witness is allowed to count an excluded
source site as coverage.

## Default and explicit lite profiles

Pinned `src/main/cljs/cljs/core.cljs:58` defines `LITE_MODE` as false. The checked
reference builds use that default. `seqs_test.cljs::test-empy-and-seq` assertion
005 is only in the true/lite branch of an `if-not`; it is retained as
`inactive-upstream` with reason `non-default-lite-mode-branch`. Its default
branch remains active. Literal core-flag checks unwrap metadata, recognize the
qualified name, and avoid treating a lexical `LITE_MODE` local as the build flag.

The source docstrings explicitly identify `obj-map` (core.cljs:12813),
`hash-map-lite` (13043), and `set-lite` (13164) as lite-mode implementation APIs
not intended for direct use. They use ObjMap, HashMapLite, and SetLite, outside
the named default-profile persistent collection helper scope. Direct dependent
assertions are active but classified `unrelated / lite-profile`; they are not
marked inactive merely because the APIs are specialized.

| Complete body | Newly excluded ordinals | Sites |
|---|---|---:|
| `core_test.cljs::test-kv-reduce` | 001 | 1 |
| `collections_test.cljs::test-obj-map` | 001–012 | 12 |
| `collections_test.cljs::test-hash-map-lite` | 001–012 | 12 |
| `collections_test.cljs::test-set-lite` | 004 | 1 |

The reduce-kv site iterates an original ObjMap input. The other reduce-kv site
remains in scope. Crucially, test-set-lite 001–003 only use ordinary set literals
and remain in scope. The obj-map/hash-map-lite transient sites retain their
pre-existing transient exclusion. No existing witness was attached to these
26 newly excluded lite sites. Quoted implementation names alone do not create
implementation dependencies.

## Accounting and validation

Relative to the preceding analyzer/compiler audit, **76** in-scope sites were
reclassified: 46 sorted, 3 transient, 26 lite-profile, and 1 inactive lite branch.
The regenerated inventory still has **3,237 sites** and **696 complete bodies**;
**3,222** sites are active, **2,537** are in scope, and **700** are unrelated.
`bodies.jsonl` remains byte-identical (SHA-256
`1f22e904625c3b8c0b7fbb51954ce8c96991aaffe8c95c943c4163c8e6794372`).

Seventeen extractor unit tests pass, including lexical rebinding, mixed
iteration inputs, nested vector/rest and map projection, aliases, absent vector
slots, default/qualified/shadowed lite flags, quoted names, and ordinary sibling
assertions. Regeneration `--check` passes. These are accounting tests and make
no additional runtime coverage claim. The exact reclassified records supplied
to the coordinating worker are `/tmp/runtime-scope-reclassified.jsonl`.
