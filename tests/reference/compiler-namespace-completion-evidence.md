# Namespace completion evidence

The complete pinned bodies for `test-cljs-1274`, `test-cljs-1346`,
`test-cljs-1508-rename`, `test-cljs-2037`, `test-cljs-3239`,
`test-analyze-refer-global`, `test-require-global`, the public namespace
acceptance assertion in `analyze-ops`, and `core_test.cljs::test-cljs-2184`
were read at revision `a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e`.
All original source bodies, assertions, spans, expected values and hashes remain
in the inventory. This extension adds **42 executable assertion mappings**:
30 positive sites in 18 namespace programs and 12 negative sites in four
explicit mixed-body diagnostic groups.

## Implemented behavior

`src/compiler.rs` now handles original top-level `require`, `require-macros`,
`use`, `use-macros`, `refer-clojure`, `refer-global`, `require-global`, and
`import` source operations, including operations in top-level `do`. Namespace
operations yield nil. Their quoted arguments, referred names, core exclusions,
renames, repeated identical aliases, and conflicting aliases are validated.
Both header and top-level imports register actual JavaScript constructor paths.
A bare import such as `goog` contributes no constructor binding.

Explicit `goog.*` dependencies use host JavaScript namespaces when no matching
`.cljs` or `.cljc` source exists under the caller's roots. Caller-provided source
still wins. The compiler binds aliases, referred values and dotted host access;
it does not synthesize Closure library implementations. Other missing source
namespaces still fail filesystem resolution. The warning API accepts reserved
`:as js` with the upstream `js-used-as-alias` warning; strict compilation retains
its existing rejection. Repeated identical runtime and macro header aliases
coalesce in both APIs; genuinely conflicting aliases still reject. Warning mode
reports the exact single-segment namespace warning before the reserved alias
warning when both occur.

`src/macro_host.rs` implements compiler `ns-publics`, `ns-interns` and `ns-imports`
from actual definitions/private metadata and import declarations registered during
this compilation. Var maps emit real `(var namespace/name)` values. Constructor
maps emit actual host references. Imported symbols also appear in the source
macro environment's `:imports` and `:requires` maps. Compilation resets this
state, including when a macro host is reused. These macros describe loaded source
namespaces; this evidence does not claim a complete reflected inventory of the
bounded virtual `cljs.core` implementation.

## Positive mappings

| Original body | Added sites | Source observation |
|---|---|---|
| `core_test.cljs::test-cljs-2184` | 001–005 | All five original predicates retained: public join, private replace-all exclusion, real Var equality, imported StringBuffer presence and constructor identity. |
| `test-require-global` | 001–002 | Exact React require-global declaration; both aliases and createElement refer share and invoke the supplied host object/function. |
| `test-analyze-refer-global` | 001 | Exact top-level Date refer; identity with js/Date and actual constructor invocation. |
| `test-cljs-1274` | 002 | Exact own-namespace qualified definition and original value 43. |
| `test-cljs-3239` | 001 | Exact bare import; public ns-imports returns the original empty map. |
| `test-cljs-1346` | 001–018 | Original top-level require/use/macro/import/core-refer declarations; actual dependency functions, source macro import maps and constructor mapping. |
| `test-cljs-2037` | 003 | Complete original repeated-require do accepted; both resolved function identities execute. |
| `analyze-ops` | Its one in-scope site | Original namespace declaration, imports and dependencies resolve; actual imported constructor identity and loaded join run. |

The `clojure.set` support files retain the exact pinned namespace,
`bubble-max-key`, `union`, and `intersection` forms. The `clojure.string` support
files retain the exact namespace, private `replace-all`, public `join` and
`upper-case` forms. Original EPL notices remain. No complete-library coverage is
claimed. The StringBuffer host adapter embeds the **complete unmodified**
Apache-2.0 `goog/string/stringbuffer.js` from reference dependency
`org.clojure/google-closure-library:0.0-20250515-f04e4c0e`; it initializes the
constructor only when the host does not already supply it. Both references keep
their existing Closure constructor. Join is executed as an additional check.

The macro-registration programs retain complete upstream `is` and `deftest`
macro declaration bodies. Those original assertions concern registration, so
framework expansion is not invoked. The fixture's macro namespace imports the
same original `*load-tests*` declaration through a distinct observer helper
namespace, avoiding replacement of the checked reference analyzer. The ordinary
Clojure `vary-meta` helper is included because the bounded native interpreter
lacks that helper. An observer reads actual `&env` maps and removes only its own
harness namespace plus implicit `cljs.core`, neither present in the original
ns* AST. All original expected maps and sets are retained; a nil AST dependency
map corresponds to an empty environment dependency map. The import assertion
retains its full `{Long goog.math.Long, Integer goog.math.Integer}` map and its
equality with the actual namespace requirements, without fabricating constructors.

The `analyze-ops` source assertion explicitly disables dependency analysis
upstream. Its unused `clojure.repl` dependency is consequently a namespace-only
support file; the fixture makes no claim about that library's behavior.

## External globals and checked execution

Both pinned compilers accept the original require-global source in analysis.
Their default Node loader then tries to load an external foreign dependency
without a file URL. This is a reference launcher issue, not an expected source
rejection. The require-global case therefore explicitly declares
`nodejs_rt: false`. No other program inherits that setting.

The checked driver verifies the effective `:nodejs-rt false` during fixture
analysis and emits `REFERENCE NODEJS_RT false`. It still verifies the pinned
reference identity and unchanged `:optimizations :none`, `:static-fns false`, and
`:def-emits-var false`. The runner loads the generated Closure Node bootstrap
and dependency table, then requires the compiled `semantic.program` namespace.
It executes generated code in Node without rewriting it. A prerequisite source
namespace supplies React before the compiled entry checks the external global.
No React library behavior is substituted or claimed.

## Negative mappings

Four diagnostic cases retain complete original bodies and hashes, while declaring
an explicit ordered `assertion_subset` and a split rationale. The strict report
still requires every other in-scope assertion to have independent evidence.

| Original body | Negative sites | Preserved rejection |
|---|---|---|
| `test-cljs-1274` | 001 | Defining a foreign namespace-qualified name; the positive own-namespace control is separately executed. |
| `test-cljs-1508-rename` | 011 | Renaming an unreferred symbol. |
| `test-cljs-1346` | 019–026 | Both unquoted require arguments, all three nested ns forms, all three nested require forms. |
| `test-cljs-2037` | 001–002 | Conflicting aliases for runtime and macro requires. |

The namespace CLI requires an explicit first namespace declaration. Where the
original test supplies a `cljs.user` analyzer environment, the adapter prepends
only `(ns cljs.user )` before the unchanged original operation. The checked
exception observer retains all complete source forms and permits at most one
failing top-level form; single-form probes remain unchanged. Every probe requires
parser acceptance, the exact local rejection reason and phase, and both checked
reference `ExceptionInfo` classes, original cause messages and warning sets.
No unsupported-form or missing-dependency rejection counts as a pass.

## Two exact internal storage assertions

Rename sites 001 and 002 assert only nil for `(-> parsed-ns :uses (get 'foo))`
and `(-> parsed-ns :uses (get 'intersection))`. They prescribe the upstream
storage choice after rename, although `foo` is present in `:renames`. Those two
exact source projections are now `unrelated / upstream-internals`. Adjacent
renamed binding presence, canonical identity, core exclusions and diagnostics
remain in scope and have separate executable witnesses. The regression test
also keeps unrelated `:uses` projections and actual unresolved-name observations
in scope. This does not classify the whole body as internal.

The inventory still contains 3,237 sites, 3,222 active sites and 696 complete
bodies. In-scope sites change from 2,537 to 2,535; unrelated sites change from
700 to 702. No existing witness is removed by this change. The full-body digest
remains `1f22e904625c3b8c0b7fbb51954ce8c96991aaffe8c95c943c4163c8e6794372`.

## Verification and records

All **18 positive programs** pass local JavaScript and both references:
ClojureScript **1.12.134** and the pinned checkout. All **12 negative probes**
return `verified-rejection` locally and against both references. No version
difference is claimed. Lexer/parser validation passes for every fixture, and
native interpretation is explicitly not applicable to compiler-only programs.

Focused Rust verification passes **80 tests**: seven namespace completion tests,
28 existing namespace tests and 45 existing macro tests. The 18 inventory tests
and exact pinned regeneration also pass. Accounting/report unit checks pass.

Fixtures are under `tests/fixtures/compiler-namespaces/namespace-completion/`.
Integration sidecars are
`/tmp/light-cljs-99-thirteenth-extra-{namespace-cases.jsonl,diagnostic-cases.jsonl,witnesses.jsonl,origins.tsv}`.
Checked positive observations and compiler/Node output are under
`/tmp/light-cljs-namespace-completion/{local,release,checkout}/`.
Checked negative contract verification is under
`/tmp/light-cljs-namespace-negative-verified/{local,release,checkout}/`.
Earlier failed adapter probes are retained in their original output logs rather
than recast as successful source behavior. The coordinating worker owns shared
registry integration and the final strict report.

### Supplemental alias verification

The completion audit additionally executes duplicate same-library runtime and
macro imports in a single header. Aliased and referred values retain identity,
and runtime functions plus both macro spellings execute. Both pinned compilers
match these complete observations. Single-segment namespace and reserved `js`
alias warnings are independently checked in their exact order. These independent
probes do not increase the 42 imported assertion mappings above.

The updated focused run passes 91 tests: nine namespace completion, nine warning,
45 macro and 28 namespace tests. The historical same-target rejection expectations
were replaced by these positive checks; different-target conflict checks remain.
