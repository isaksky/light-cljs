# Executable core

The compiler supports small programs with lexical closures, mutable globals,
minimal namespaces, and narrow JavaScript interop. The two-namespace example
prints exactly `4\n`. General macros, collections, library/runtime integration,
and optimization remain outside this subset.

## Pipeline and ownership

`parse(source)` constructs the existing syntax arena. `analyze(source, &syntax)`
constructs a separate `Program`; `emit_js(&program)` recursively writes one
JavaScript expression into a `String`. `compile(source)` composes these stages
and distinguishes parser errors from analysis errors. `--compile` accepts files,
stdin, or `--text`; `--analyze` prints the resolved representation. Existing token
and parser CLI modes retain their behavior.

`Program` owns `Vec<Expr>`, lexical/global binding records, JavaScript paths, and
UTF-16 string values. `ExprId`, `LocalId`, `GlobalId`, `JsGlobalId`, and `StringId`
are integer indices into those arrays. Expression and binding records retain
original byte spans and a source index (zero for standalone analysis; the
namespace's dependency-order index for a combined program). Analysis requires the exact source
used to parse the syntax; the resulting program no longer borrows that source.
IDs are local to a single compilation unit, and the emitter expects an
analyzer-produced program.

The environment is a stack of scopes containing binding IDs. Lookup walks scopes
and their bindings from newest to oldest. Binding records survive scope exit.
Each `let*` initializer is analyzed before its binding is installed; previous
bindings are visible, including earlier occurrences of the same spelling.
`fn*` installs its optional self-name and parameters before analyzing its body.
A parameter may shadow the self-name. Every emitted local name derives only from
its ID (`l0`, `l1`, …), so shadowing, Unicode names, and JavaScript reserved words
do not change binding identity.

## Supported syntax and values

| Construct | Behavior |
| --- | --- |
| `nil`, booleans | JavaScript `null`, `true`, `false`. |
| Integer literals | Signed decimal, hexadecimal, octal, and radix spellings, limited to ±(2^53−1). Larger integers report a range error. Integer `-0` is ordinary zero. |
| Floating-point literals | Finite IEEE-754 doubles; negative zero is preserved. Overflow reports a range error; underflow follows double rounding. |
| Strings | Exact decoded UTF-16, including lone surrogates. JavaScript escaping preserves quotes, backslashes, controls, and Unicode. |
| `(if test yes no?)` | Evaluate the test once and only the selected branch. The omitted branch is `nil`. |
| `(do forms...)` | Evaluate in order, returning the last value, or `nil` when empty. |
| `(let* [name init ...] body...)` | Sequential simple-symbol bindings and an implicit `do` body. Repeated names create distinct bindings. |
| `(fn* name? [params...] body...)` | A lexical closure with one fixed parameter list; optional self-name supports recursion. |
| `(fn* name? ([params...] body...))` | Equivalent single-method syntax. Multiple methods and variadic arguments are unsupported. |
| `(callee args...)` | Evaluate callee first, then arguments left to right, then call. |
| Multiple source forms | An implicit `do`; the file's result is its last value, or `nil` when empty. Top-level lexical scopes do not leak into following forms. |

Bindings must be simple symbols: no namespace slash, dot, destructuring, or `&`.
Duplicate function parameters are rejected. Calls use plain JavaScript function
invocation; this stage does not add arity checks or support collection/protocol
invocation. A literal `nil` callee is rejected during analysis. Other non-callable
values fail when invoked in JavaScript. There is no tail-call elimination.

Bare built-in library functions, including arithmetic, are unresolved; `+` has
no special arithmetic treatment. Characters, keywords, regexes, symbolic numbers, big integers, decimals, ratios,
empty lists, collection literals, reader prefixes, metadata, tags, reader
conditionals, and reader anonymous functions report unsupported constructs.
Parser comment/discard handling is retained. Unresolved symbols are errors even
in unselected branches. No implicit JavaScript global lookup occurs.

## Context, truthiness, and the macro boundary

Expressions carry `Value`, `Discard`, or `Return` context during analysis. The
emitter propagates those contexts recursively: conditionals can be ternary
expressions or statements; bodies discard earlier values and return their last
value when needed. `let*` and `do` in value position use small immediately invoked
arrow functions. This keeps evaluation order and avoids hoisting expressions
out of branches or call arguments.

The generated unit has a local truth helper, testing `value != null && value !==
false`. Thus `0`, negative zero, and empty strings are true. The nullish check
also handles `undefined` when a compiled function is invoked by a host harness.
Evaluating output produces the program's value; explicit host calls such as
`js/console.log` can print.

List analysis dispatches primitive special forms before touching binding vectors
or other operands. The macro hook then runs inside the analyzer, where the current
lexical environment and expression context are available, before ordinary call
analysis. Macro execution is unsupported. A small diagnostic catalog recognizes
common core macro spellings such as `let` and `fn`; it is not a complete macro
resolver. Lexical bindings and defined globals shadow those macro names.
Unknown names remain unresolved.

## Definitions and globals

`(def name initializer)` registers a namespace-qualified global before analyzing
its initializer. The initializer executes once whenever that expression runs;
the expression yields its assigned value. Redefinitions reuse `GlobalId` and
ordinary mutable JavaScript storage (`g0`, `g1`, …). A closure that reads a
global observes later redefinitions; a closure capturing a local retains that
lexical binding. Global functions can recurse and be passed as values. There
are no Var boxes. Locals take precedence over unqualified globals; qualified
`namespace/name` references identify globals even under lexical shadowing.
Standalone `compile`/`analyze` use namespace `cljs.user`.

`(def name)` declares a global without resetting existing storage. An initially
uninitialized global reads as JavaScript `undefined`. As a discarded expression
it has no effect; in tail position its result is `undefined`, including after
an earlier initialized definition. Uninitialized definitions in required value
positions, such as `(let* [x (def a)] x)`, are rejected: the pinned reference
emits invalid JavaScript for this shape. Definitions accept simple symbols and
an optional initializer, without metadata/docstrings.

Resolution is source ordered: a definition is visible to its own initializer
and later analyzed forms. Undeclared/forward references are errors, including
inside functions and unselected branches. This is a deliberate stricter subset
of the standard compiler's warning-and-emit behavior. An explicit preceding
`(def name)` permits a later assignment to that identity. No namespace/global
state survives across separate calls to `compile` or `compile_namespace`.

## Namespace loading

```sh
cargo run --quiet -- --compile --entry demo.main \
  --source-root examples/namespaces > /tmp/light-demo.js
node /tmp/light-demo.js
# 4
```

The library entry point is
`compile_namespace(entry: &str, source_roots: &[PathBuf])`. At least one explicit
root is required; CLI `--source-root DIR` may be repeated after `--entry NS`.
Every root must exist as a directory. Namespace dots become directories and
hyphens become underscores; only `.cljs` files are searched. Canonical duplicate
roots/files are deduplicated. Two distinct matching files are an ambiguity
error, rather than assigning precedence to one source root.

Each file must begin with `(ns NAME ...)` and declare exactly its requested
namespace. Names and aliases contain dot-separated ASCII components matching
`[A-Za-z_][A-Za-z0-9_-]*`; `js` is reserved. Supported require clauses are:

```clojure
(ns demo.main (:require demo.side [demo.math] [demo.util :as u]))
```

Aliases and full names of directly required namespaces can qualify globals.
The current namespace can qualify its own globals. Transitive-only and unknown
qualifiers are errors. Duplicate aliases (even to the same target), conflicting
aliases, or an alias redirecting a required/current full namespace name are
rejected. Namespace docstrings/metadata, `:refer`, `:rename`, macro imports,
JavaScript packages, and all other clauses/options are unsupported. The loader
validates names and clauses before resolving their dependencies.

A sequential depth-first walk uses Unseen/Loading/Loaded states. Dependencies
initialize before consumers, in require order; each namespace loads once,
including shared dependencies. Source forms execute in order within each
file. Loading an already-Loading namespace reports the cycle path. Missing
sources, mismatched declarations, analysis errors, and parse errors include the
originating file/location when available. No output is emitted on failure.
The result is one combined JavaScript expression whose value is the entry
namespace's final form. There is no caching, package manager, incremental
invalidation, or parallel scheduler.

Existing standalone file/stdin/text modes are preserved and now accept `def`
and interop. A standalone `(ns ...)` remains unsupported: select an entry with
explicit roots to request filesystem loading.

## JavaScript interop

| Form | Behavior |
| --- | --- |
| `js/Math`, `js/Math.sqrt` | Resolve a host global or property path. |
| `(js/Math.sqrt 256)` | Call a JavaScript path, retaining its final receiver. |
| `(.-length "hello")` | Evaluate the receiver once, then read a property. |
| `(.slice "hello" 1)` | Evaluate receiver, look up method, evaluate arguments left to right, and invoke with the receiver as `this`. |

Host roots are properties of `globalThis`; host lexical variables outside that
object are excluded. Missing roots throw `ReferenceError`; missing properties
produce `undefined`, while traversing a nullish intermediate fails with normal
JavaScript behavior. Paths and members accept ASCII JavaScript identifiers
`[A-Za-z_$][A-Za-z0-9_$]*`, emitted through validated bracket properties rather
than raw source interpolation. This subset excludes property munging of
hyphenated names and general Unicode host identifiers. As in the reference,
`js/x` resolves to a same-spelled lexical `x` when present; a ClojureScript
global `x` does not shadow `js/x`.

`MethodCall` has an explicit receiver and remains distinct from `Call`.
Ordinary non-host calls use `.call(null, ...)` with the callee evaluated once;
extracting a method as an ordinary function does not preserve its original
receiver. A host path's getter/receiver is evaluated before arguments and only
once. The tests include a getter-backed method depending on `this`, nested
paths, side effects, and missing properties. Constructors, property writes,
raw `js*`, explicit `.` syntax, and package imports remain unsupported.

## Verification

`tests/fixtures/execution.tsv` contains the shared tiny programs and their expected
JSON values/effect traces. The execution harness wraps each program in
`(fn* [observe] ...)` and supplies a host function that records its argument and
returns it. A reset host object supplies getters and receiver-sensitive methods
for interop probes; these are harness helpers, not implemented source forms. The cases cover shadowing and closures, self-recursion,
all three emission contexts, callee/argument/initializer order, branch effects,
truthiness, numeric spellings, and UTF-16 values.

`tests/fixtures/execution-outcomes.tsv` adds runtime failures, stdout, and host
boundary values without changing the original fixture format. Both compilers
use the same per-case `{id, outcome, trace, stdout}` protocol: an outcome either
returns an encoded value or throws a stable error category. The harness retains
effects and stdout before a throw and runs the next case with fresh host state.
Nil/null, `undefined`, negative zero, NaN, and both infinities stay distinct,
including inside trace arrays. Unsupported source literals remain unsupported;
host callbacks supply special values. Independent JavaScript probes check the
serializer and runner against literal expectations, so a shared harness error
cannot count as compiler agreement. See the [outcome protocol and evidence](../tests/reference/outcome-evidence.md).
Separate tests cover complete files, CLI diagnostics, invalid binding forms,
and resolved IDs.

```sh
cargo test
cargo test --release
cargo clippy --all-targets -- -D warnings

# Required pinned Maven comparison (also the default selection).
LIGHT_CLJS_REFERENCE=release cargo test --test execution -- --ignored --nocapture

# Supplemental comparison: compare both upstreams, then light-cljs against each.
LIGHT_CLJS_REFERENCE=both \
LIGHT_CLJS_REFERENCE_CHECKOUT="$PWD/research/clojurescript" \
LIGHT_CLJS_REFERENCE_REVISION=a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e \
cargo test --test execution -- --ignored --nocapture

# Reader regression coverage stays independent of executable-language coverage.
cargo test --release --test parser_corpus -- --ignored --nocapture
```

Normal execution tests require Node.js (`NODE` can select its executable).
The explicit differential tests invoke `tests/reference/execution.clj` using
Clojure CLI, Java, and Maven ClojureScript **1.12.134**. They compile temporary
Node programs and compare returned values or failures, effects, and stdout with
the Rust compiler. Options are explicitly pinned to `:optimizations :none` and
`:static-fns false`; the actual fixture analysis environment has
`:def-emits-var false` for ordinary compilation. The harness asserts the active
version/options instead of relying on defaults. Reference-only runtime/macros
are harness machinery, not a claim that light-cljs implements them.

The `conformance` CI job provisions Node, Java, Clojure CLI, and the pinned Maven
release, then runs all ignored execution comparisons. It is an unconditional
required job in the workflow (repository branch-protection settings are outside
this source change). Missing tools, reference build failure, timeout, absent
output, and mismatches fail the test; there is no passing skip. Fast Rust/Node
tests remain separate and require no Java. Successful temporary reference
builds are removed; failed builds are retained with diagnostics. See
[milestone evidence](../tests/reference/milestone-evidence.md) for exact local
commands, tools, effective settings, actual results, and intentional limitations.
No hosted CI run is claimed by these local results. The original 74 hand-authored
cases already passed before this milestone and passed again with the explicit
settings before capabilities were added. New probes are also hand-authored;
source inspection/adaptations are identified in the evidence and loader tests.
The research checkout is consulted for implementation semantics, not silently
substituted for the pinned release oracle. JavaScript text is never the
conformance criterion; repository corpora remain reader regression tests.

`LIGHT_CLJS_REFERENCE` selects `release` (the default), `checkout`, or `both`.
Checkout selections require an explicit path and full commit SHA; the harness
requires a clean repository at that exact revision, uses its `deps.edn` through
`:local/root`, and verifies compiler/analyzer/runtime resources actually came
from that checkout. `checkout` alone is useful when investigating a failure;
it does not satisfy the required release job. `both` reports a release-versus-
checkout discrepancy before comparing with light-cljs, retaining both builds
and their results. No reference is chosen because it agrees with light-cljs.
All three selections run the same cases and namespace fixtures. Missing paths,
revisions, dependencies, or failed builds are failures, with no fallback or skip.

Each build prints its exact shell invocation, reference identity, Java/Clojure
runtime and Node versions, and verified compiler settings. On failure, the
printed temporary directory retains generated source, `invocation.sh`,
`build.stdout`, `build.stderr`, and (when execution was reached) `driver.js`,
`node-version.txt`, and `results.txt`. Node subprocesses also use file-backed
input/stdout/stderr and a 30-second timeout. Crashes and hangs retain the active
case ID/source in stderr progress records and keep their script and partial
output in a separately reported process directory. Every expected case must
produce exactly one outcome with its correct ID and position. Successful comparisons remove their
temporary builds. See [checkout evidence](../tests/reference/checkout-evidence.md)
for upstream harness inspection, standalone reproduction, and actual runs.

The [coverage and provenance index](../tests/reference/coverage.md) maps inspected
upstream assertions to stable local fixture IDs, applicable gaps, and deliberate
subset exclusions. Its provenance sidecar preserves hand-authored origins and
records pinned sources/reductions for adaptations. `cargo test --test provenance`
validates the metadata without the research checkout; the index also documents
the procedure for adding and verifying an upstream regression.

The [lexical audit](../tests/reference/lexical-evidence.md) maps the detailed
binding assertions, verifies restored `LocalId` targets and namespace-aware
globals, and records the 143-case release/checkout comparison. It also retains
explicit reference limitations for returned-let spelling collisions and host
initializers; those differences are not counted as passing conformance cases.
