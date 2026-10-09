# Native interpreter compatibility profile

The native interpreter is a separate execution path for a portable Clojure
subset. Its `interpreter` module currently provides **owned scalar and immutable collection values, metadata, structural equality
and hashing, source spans, executable constants, and explicit garbage collection**.
It also provides **incremental semantic reading, owned forms, and native
evaluation of literals, collections, quote, if, do, sequential `let*`, native calls,
checked arithmetic and predicates, lexical closures, named/multiple/variadic
`fn*`, mutually recursive `letfn*`, stack-safe `loop*`/function `recur`,
namespace/Var identities, definitions, dynamic bindings, scoped root redefinitions,
Var metadata mutation and namespace introspection, single-threaded atoms,
validators/watches and volatiles, portable exceptions,
collection core functions, callable data, finite traversal, reductions and eager
higher-order helpers, user/native macros and macroexpand APIs, ordinary destructuring,
let/loop/fn/defn/defn-/letfn, function pre/post conditions, short-circuit/conditional/threading
macros, assertions and constant-data case dispatch, memoized delays/lazy sequences,
core lazy producers/transformations, bounded/unbounded materialization, lazy `for`,
effectful `doseq`, `dotimes` and `while`, contextual reader conditionals,
auto-resolved keywords, namespaced maps, explicit host data readers and host-owned I/O**. Explicit Rust callbacks can be registered;
native `--eval` scripts and `--repl` run without Java, Node or generated code.
The exact supported API inventory and executable exclusions appear in the
[conformance coverage](../tests/reference/sci/README.md#integrated-native-conformance) section.
Every committed SCI row is executed by the integrated native suite; declared
differences have separate exact native assertions. The
existing ClojureScript compiler and its JavaScript semantics remain supported
under [their own contract](compiler.md).

This profile follows the `rust-sci` group context, revision **1**, and JVM SCI
[`5fe89056384418be50d232b4278344cb3a1a526b`](https://github.com/babashka/sci/tree/5fe89056384418be50d232b4278344cb3a1a526b).
SCI supplies a semantic reference, including operations it obtains from host
Clojure. Native execution will implement selected operations in Rust without a
JVM, SCI library, subprocess evaluator, JavaScript engine, or generated JS.
The production crate currently uses only the standard library and safe Rust.

## Supported surface and current status

This table summarizes delivered behavior. The complete language API inventory
links each name to executable evidence in the same
[reference corpus](../tests/reference/sci/README.md).

| Surface | Current implementation | Native compatibility scope |
| --- | --- | --- |
| Lexing and syntax parsing | Shared borrowing `Tokenizer`, `Parser`, `SyntaxTree`/`NodeId`, UTF-8 byte spans, pull-based `read_form`, diagnostics and parser limits | Reuse the existing front end; opt-in discarded subtree retention supports semantic validation without changing default parsing. |
| Scalar values and reading | Native constructors for nil, booleans, i64, doubles, bounded reduced ratios, UTF-16 characters/strings, and interned symbols/keywords; checked reader conversion and bounded arithmetic are implemented | Exact i64/ratio operations, mixed doubles, comparisons and checked conversions are tested. |
| Reader semantics | Native pull reader lowers ordinary literals/collections, quote/deref/var, syntax quote/unquote/splicing, scoped gensyms, metadata and anonymous functions, preserving source order/provenance | Feature-selected conditionals/splices, live namespace aliases, namespaced maps and registered host tag/default callbacks work. Regex values and built-in inst/uuid values remain unsupported; `#=` always fails. |
| Evaluation and functions | Native literals, quote, if, do, sequential let*, lexical references, collection expressions, anonymous/named fn*, multiple/variadic methods, letfn*, loop* and validated recur; owned code and indexed locals/captures | Native/interpreted calls, lexical capture, recursion and escaping mutually recursive functions work. Ordinary destructuring and let/loop/fn/defn/defn-/letfn expand to these primitives; conditional/threading macros and indexed case dispatch also work. |
| Collections and core operations | Persistent lists/Cons, vectors/map entries, maps and sets; constructors, lookup/nested update, merge, stack operations, seq traversal, reductions, eager higher-order helpers, predicates and immutable metadata | Keyword/symbol/map/set/vector invocation and checked higher-order collection functions work. See the exact surface below. |
| Printing and strings | Common UTF-16 printer, readable/length/level/meta controls, core naming/slicing and literal clojure.string operations | Bounded lazy/cyclic printing, lossless strings, explicit Unicode/regex boundaries; see exact surface below. |
| Macros | defmacro, macro-marked Vars, registered host macros, macroexpand-1/macroexpand with optional env; direct owned-form expansion during analysis | Primitive/local precedence, top-level generated do interleaving, lexical env views, source provenance, shared budgets and optional per-macro diagnostics. Context-aware syntax quote, binding/function macros and the control macros listed below work; other core macros are outside the declared surface. |
| Namespaces and Vars | Isolated user/clojure.core contexts, stable namespace/Var IDs, def/declare/defonce, in-ns, qualified/unqualified resolution, Var quote/deref and introspection | Explicit source providers, ns/require, aliases, refers, reload and cycle handling work; dynamic bindings, scoped redefinitions, root and metadata mutation work. |
| Lazy sequences | delay/force/deref/realized?, captured lazy-seq producers, memoized realization and traced lazy Cons; finite-demand traversal, reduce, equality and collection integration | Lazy for comprehensions, map/filter/keep, range/repeat/repeatedly/iterate/cycle, take/drop, concat, partitions and interleaving work with bounded consumers, doall/dorun and shared resource limits. Native sequences are unchunked. |
| State and exceptions | Single-threaded atoms and volatiles, dereference, reset/swap/CAS, old/new pairs, validators, watches and atom metadata; portable exceptions and try/catch/finally | Reentrant callbacks use the common evaluator and traced heap. No cross-thread synchronization, STM, Java class hierarchy or interop. |
| Embedding, I/O, scripts and REPL | Typed context initialization/option merging, native functions/macros/Vars, eval results with final namespace, canonical name policy, explicit context forks, owned UTF-16 streams and callbacks | Library defaults grant no ambient input/output/filesystem/network/process capability. Standalone scripts and a multiline REPL use explicit streams and source roots. |
| Resource and lifetime controls | Owned roots and checked IDs; code, closures/captures, namespaces/Vars and call frames stay live across collection; traced recursive cycles are reclaimed; host-configured fuel, cumulative allocation, output and depth bounds; shared cancellation and bounded cleanup | Budgets share one outer evaluation scope through native reentry. They are cooperative, not OS memory isolation or hard preemption of native code. |
| Performance | Release native process, phase/load/call/GC, allocation/live-memory and existing frontend benchmarks | [Reproducible raw samples and saved-binary comparisons](../benchmarks/interpreter.md), with opt-in macro-definition and repeated code-generation attribution; no SCI/JVM speedup claim. |

## Semantic reader and forms

`Context::reader(&Source)` returns a `FormReader` over the existing `Parser`.
`read_form()` returns `Result<Option<Form>, ReadError>`: `None` means EOF and
`Some(form)` can contain literal nil. Completed forms are available before the
next read. The reader stops after its first syntax or lowering error. Errors
retain owned primary/related source spans; lines and columns use the existing
1-based byte-column convention. Parser errors retain their original category.
Semantic errors distinguish unsupported syntax, duplicate map keys/set members,
metadata targets, invalid anonymous arguments, numeric overflow and expansion
limits. Parsing a complete syntax form precedes its semantic validation; a
syntax error later in that same form can therefore precede an earlier semantic
error. Explicit host data-reader callbacks can run during semantic reading;
ordinary list data is never evaluated by the reader itself. Since the shared
parser validates a complete containing syntax form first, a later syntax error
prevents every tag callback in that form. SCI can invoke an earlier callback
before reporting that later error; the focused corpus records this difference.

A `Form` owns an immutable arena of `FormNode`s indexed by `FormId`, plus a rooted
constant/provenance pool. `data()` returns the ordinary immutable reader value;
`children()` exposes ordered forms, including alternating map key/value forms.
This order is independent of runtime hash collection traversal. `metadata()`
returns a metadata form, preserving expressions for later analysis. `span()`
retains source text, and `Origin` attributes generated quote/deref/var operators,
anonymous parameters, function scaffolding and metadata nodes to their original
syntax. No node borrows the parser arena. Forms and extracted data survive
parser/source/context drops and context reset. A `FormId` is local to its arena.

`Context::lower_form` accepts a root from an existing `SyntaxTree` and the exact
original `Source`. It cannot validate discarded subtrees that default `parse`
already removed; use the pull reader when reading untrusted source.
`Context::form_from_data(value, origin)` accepts owned data directly, retaining
metadata and exact UTF-16 without printing or parsing. This is the embedding
and direct macro-expansion boundary. The caller supplies a source span for
provenance; an empty span in a named synthetic source works for host-created
forms. Host data's existing map/set order remains unspecified. Foreign-owner
values/sources are rejected. Supplied data nesting is bounded at 128.

Reader expansion constructs ordinary `(quote form)`, `(clojure.core/deref form)`
and `(var form)` lists. Quote nesting is preserved and its contents stay data;
reader validation still applies within quote. Empty lists remain lists, distinct
from nil. Var-quote operands are read as data; symbol requirements belong to
analysis. Metadata keywords become `{keyword true}`, symbols/strings become
`{:tag value}`, and maps are applied directly. Outer metadata overrides equal
inner keys. Symbols and collections accept metadata; nil, scalar literals,
keywords and strings reject it. Source locations are side-table provenance,
not injected `:line`/`:column` language metadata. This differs from SCI's
location-bearing lists and top-level symbols.

Literal duplicate map keys and set members are rejected using structural value
equality, ignoring metadata and equating equal lists/vectors or reduced integral
ratios with integers. The diagnostic points to the repeated key/member and
retains the first occurrence as a related span. Integers and doubles remain
separate categories; the documented NaN equality behavior also applies here.

Anonymous `#()` becomes `(fn* [parameters] (body...))`. `%` aliases `%1`, missing
positive indices get parameters, and `%&` adds `&` plus a rest parameter. Canonical
placeholder names match SCI (`%1#`, `%2#`, `%&#`); equal shorthand expansions are
equal reader data and duplicate set/map keys are rejected. Each function
introduces its own lexical bindings during future analysis. Following
the pinned SCI reader, index spellings are decimal signed 32-bit integers:
`%010` denotes index 10, `%0` and `%-1` produce unbound symbols without parameters,
and slash spellings such as `%1/1` and `%3/2` stay ordinary symbols. Nonpositive
placeholders will therefore require an unresolved-symbol error during future
analysis. BigInt and radix argument spellings fail explicitly. The default
maximum is 1,024 positive parameters; `reader_with_limits` can change it, and
exceeding it is an explicit read error. Nested shorthand is rejected by the
existing parser. Arguments within quote still participate in parameter binding.

The semantic reader enables `Parser::retaining_discards()`. Its discarded-root
side table retains syntax for validation while ordinary children/roots continue
to omit it; default parsing still removes discarded subtrees and reuses capacity.
Semantic storage removes the temporary discarded forms after checking them.
Discarded literals must still have valid metadata and unique keys/members, and
`#=` remains forbidden everywhere. A discarded placeholder does not register an
argument in its surrounding function, matching SCI. A wholly discarded anonymous
function still validates its own expansion. Existing tokenizer/parser acceptance
is preserved, including stricter validation of malformed placeholder spellings
inside discards and contextual `%&rest`/`%1:kw` splitting; this does not claim
full Edamame spelling compatibility. Parser syntax is retained across pull reads;
per-form syntax recycling is not implemented.

Integer spellings convert with checked wide intermediates and then require i64
storage. Ratios parse signed i128 components, reduce exactly, then require i64
components; inputs exceeding i128 fail even when arbitrary-precision reduction
could fit. Floating-point overflow is an explicit error; infinity requires the
symbolic spelling. BigInt/BigDecimal literals always return specific unsupported
errors. UTF-16 string/character decoding is reused from the syntax parser.

`tests/interpreter_reader.rs` covers native reading and direct data construction,
including selected outcomes from the pinned SCI corpus. It does not
execute the fixture programs or claim evaluator conformance. Adapted assertions
come from `parse_test.cljc::parse-test`, `parse+string-test`, and
`read_test.cljc::read-eof-value-test`; attribution and source anchors are recorded
in the reference notice/manifest. Other reader probes are independently authored.

### Contextual reader configuration and tags

`Context::set_reader_features` replaces the keyword-name feature set, initially
`#{:clj}`. An empty set selects only `:default`. The first matching feature or
`:default` branch wins in source order. Each top-level read snapshots features;
existing reader cursors observe later host updates. An unmatched conditional
produces no form: collections omit it, while quote, tag and discard operands
continue to the next actual form, including across top-level syntax roots.
Selection and splicing precede map arity and literal duplicate checks.

Splice markers are consumed at collection boundaries. Nested conditionals and
metadata preserve them; quote wraps its complete operand. Following pinned SCI,
list/vector/set values splice members, maps splice entries, and scalar
nil/numbers/strings remain single members. An identity tag callback preserves
its input's splice marker. Host-created replacement values do not acquire a
marker automatically; SCI's internal `:edamame/read-cond-splicing` metadata is
not exposed as language metadata by this reader.

Auto keywords `::key`, `::alias/key`, and `#::`/`#::alias` maps read the same live
namespace registry and alias table as evaluation. Earlier `in-ns`, `alias` and
`ns-unalias` forms therefore affect later reads. An existing full namespace name
does not substitute for a missing alias. Explicit `#:name` needs no registered
namespace. Namespace-map rewriting applies only to immediate symbol/keyword
keys: unqualified keys acquire the namespace, `_` removes qualification, and
other qualified keys remain unchanged. Values and nested ordinary map keys are
unchanged. Raw duplicate keys fail after selection; as in SCI, collisions
introduced by qualification keep the later source value.

`register_data_reader(tag, &'static NativeDefinition)` installs a one-data-argument
callback; `remove_data_reader` removes it. `set_default_data_reader(Some(...))`
installs a fallback receiving the tag symbol and data; `None` removes it.
Registrations are live for existing readers. Callbacks use the normal checked
native ABI, rooted values, owner checks, reentry depth, cancellation and the
surrounding resource scope. No registry borrow is held across host code.
Returned forms own their data and record `Origin::DataReader` and the tag's
source span. Forwarded failures retain body frames/exception identity and gain
the read phase and tag location. Arbitrary host callbacks remain cooperative;
these controls cannot preempt blocking host code.

Selected tag operands are read once and supplied without evaluating their list
contents. Explicit discards still read their operands and can invoke handlers.
Unselected branches suppress handlers and anonymous parameter registration but
still validate aliases, metadata, and supported literal duplicates. Between-pair
discards run normally until a branch has matched; later discarded forms are
suppressed. Unsupported numeric/regex literals in skipped branches are inert
validation data; this does not extend the runtime numeric tower or promise
arbitrary-precision numeric equality during skipped-branch validation.

Unknown tags fail with `UnknownTag`; no constructor, classpath, filesystem or
namespace loader is consulted. `#inst` and `#uuid` are unsupported without a
host implementation supplying concrete values. They are never synthesized as
strings. This is an embedding API, not dynamic language `*data-readers*` binding,
EDN `tagged-literal` values or reader-resolver support. Language `read` and
`read-string` use this same configured feature/tag policy.

| Preserved front-end boundary | Difference from pinned SCI |
| --- | --- |
| Every branch is syntax-validated before semantic selection | Malformed skipped collections, reserved feature keys, malformed placeholders and `#=` fail; the parser never skips syntax validation. |
| Top-level `#?@` is rejected | SCI accepts it as the selected value without collection splicing. |
| Conditional syntax in metadata position is rejected | SCI can select the metadata operand before validating its kind. Conditional target operands work. |
| Whole containing syntax form parses before tag callbacks | A later syntax error suppresses earlier callbacks that SCI may already have invoked. Completed earlier top-level effects remain intact. |

`tests/interpreter_contextual_reader.rs` compares 76 focused native outcomes and
ordered effects against pinned SCI, records six parser/effect differences, and
covers retained cursors, source spans, ownership, callback failures and budgets.
`lower_form` has only its supplied syntax root, so a completely omitted root or
an operand needing an unavailable next root returns `MissingReaderForm`; use
`FormReader` for incremental continuation.

## Primitive evaluation API

`interpreter::eval_string(&str)` evaluates in a fresh context and returns an
owned `RootedValue`. `Context::eval_string`, `eval_source(&Source)` and
`eval_form(&Form)` reuse an existing context. Empty input, `(do)`, empty `let*`
bodies and an unselected two-argument `if` return nil. Only nil and false are
falsey; numbers, strings and empty collections are true. `let*` initializers
see earlier bindings and the outer scope before their own binding is installed;
repeated binding names shadow in order. Ordinary `let` and destructuring expand through the implemented macro layer;
they are not simple aliases for the primitive.

Top-level forms execute before the next form is read. Top-level `do` recursively
analyzes and executes each child before analyzing the next, preserving the point
at which later definitions and macro registration can affect analysis. An
ordinary expression, including both `if` branches and every `let*` initializer
and body, is analyzed fully before execution. A syntax error in a containing
form still prevents that form from executing. Required analysis never skips an
unreachable branch. Unknown symbols and unsupported forms fail explicitly.

Vector, map and set members execute exactly once in source order, followed by
their metadata expressions. Evaluated duplicate map keys or set members fail
after all members execute and before metadata executes; constructor APIs remain
the distinct already-evaluated, deduplicating storage operations. Quote returns
reader data, including unevaluated metadata and symbols. The empty list is a
constant, including its unevaluated metadata. Symbol metadata is diagnostic/type
information rather than an extra evaluated expression.

`Context::analyze_form(&mut self, ...)` executes required macro expansion and produces an owned `AnalyzedForm`, and `eval_analyzed`
executes it without rereading or reanalysis. Explicit analysis treats even `do`
as one expression; use the top-level evaluation methods for incremental `do`.
Each analyzed unit has typed expression/constant IDs, dense local slots, and a
separate source side table. Successful production execution performs no lexical
name lookup or source-span access. Units retain their constants and sources
through the same collector used by forms. Results do not borrow input, forms or
analysis scratch. Cross-context and reset-owner use is rejected.

### Bounded analysis reuse

Repeated `analyze_form` calls on the **same retained Form** reuse an immutable
unit when analysis performed no macro calls or definitions. Units contain the
complete expressions, function bodies, constants, resolved Var IDs, permission
requirements and source/provenance tables. They never snapshot mutable Var
roots or execution results. `eval_analyzed` continues to check current
permissions and read live Var roots. Reading new source still performs every
reader callback, and every macro-bearing analysis executes its macros again.
Definitions are excluded because interning and initial metadata are analysis
effects, including dynamic `*file*` capture.

The cache key includes the Form arena's owner/generation identity and local
FormId, current namespace identity, shared namespace/Var arena mutation
revisions, and context configuration revision. Var roots/metadata, bindings and
restoration, aliases/refers/unmapping, namespace changes, readers/features,
option changes, provider changes and reload invalidate conservatively. Forks
start with empty caches; shared Var mutations also invalidate sibling caches.
Existing Forms remain immutable snapshots of reading; a changed feature or tag
handler affects subsequent reads. No source-string or macro-output equivalence
is assumed.

The FIFO cache retains at most 32 entries, 8,192 combined semantic/executable
nodes and a conservative total of 256 KiB of source text, with a 64 KiB limit
per source. These are structural retention bounds, not a heap-byte limit:
supplied data and referenced Vars may reach larger graphs. Weak input keys do
not keep Forms alive. The next analysis or explicit collection drops entries
whose input died or environment changed. `analysis_reuse_stats()` reports
hits/misses and current retained counts; `clear_analysis_reuse()` drops entries
and resets counters. Reset clears reuse; externally retained code survives
under its original owner.

Finite active fuel/allocation limits, reduced analysis depth, execution polling
or cleanup bypass this optional reuse to preserve per-node control and exact
exhaustion diagnostics. Cancellation remains checked at entry. Required
analysis is never truncated to fit a cache budget. Oversized results simply
remain uncached. Analysis scratch reuses constants, provenance and resolved-Var
buffers, clears all roots on success/failure, and drops buffers above 8,192
slots. Reentrant analysis owns separate scratch while its caller is active.

Loaded namespaces already retain their initialization state and Var/code index:
ordinary `require` neither invokes the provider nor reanalyzes/reinitializes
loaded code. Configuration changes do not implicitly replay initialization.
Explicit `:reload`/`:reload-all` reread sources and execute required expansion
and initialization; failed loads discard namespace registration and retry on
later require. This necessary module state is proportional to loaded namespaces,
not an evictable code cache that could replay effects. See the measured
[baseline comparison](../benchmarks/results/interpreter-2026-10-08/reuse/README.md).

Local slots, collection members and native arguments use rooted handles in reusable
context scratch, cleared on both success and failure. Collection is explicit;
registered callbacks may collect or reenter evaluation. The parser's existing
nesting bound and configurable execution-depth, fuel and allocation checks apply.
User/native macro expansion uses the same bounds. Default contexts expose no
external effects. Tests use both an instruction completion probe and explicitly
registered callbacks to verify ordering and exactly-once execution.

`EvalError` separates read, analysis, macroexpansion and execution phases and retains primary and
optional related spans. `if` analyzes its operands before checking argument
count, matching SCI's unresolved-name precedence. Quote requires exactly one
argument. This API deliberately validates raw `let*` strictly: a binding vector
with even pairs and unqualified symbol names is required; destructuring and
malformed internal forms are rejected instead of inheriting SCI's permissive
internal `let*` handling.

`tests/interpreter_eval.rs` executes 36 selected programs from the 448-row pinned
SCI corpus, including newly adapted `core_test.cljc::let-test` scope and `if`
arity assertions. It checks native phases/values, input and source lifetimes,
repeated analyzed execution, reset, and reclamation after execution errors.
The rest of the reference corpus is not a claim of implemented native features.

## Lexical functions and calls

`fn*` accepts an optional self name and either one parameter vector/body or
multiple method lists. Empty bodies return nil. Parameters shadow the self name;
nested functions capture the visible binding's value at creation, including
transitive captures. Repeated calls get separate frame slots. Reader `#()` forms
execute through the same analyzed `fn*` representation.

Calls select an exact fixed arity before a variadic method. Variadic rest is nil
when there are no extras, otherwise a counted eager sequence (`ValueView::Seq`),
with sequential equality/hash and `seq?` true, `list?` false. Rest construction
keeps the original values without copying their graphs. General sequence
operations are still a later core-library feature.

The raw `fn*` signature behavior deliberately follows pinned SCI, including
its permissive edges: duplicate fixed signatures select the last body; duplicate
parameters select the last argument; qualified parameter names are allowed.
`[&]` has fixed arity zero; trailing forms after the rest binding are ignored.
SCI rejects two adjacent variadic methods and a fixed method wider than the
immediately preceding variadic minimum, but resets that check after a fixed
method. Fixed-before-variadic and separated variadic signatures therefore work;
the first variadic method supplies fallback dispatch. The focused fixtures record
these order-sensitive results. Parameter declarations must be vectors; primitive
parameters must be symbols. The source `fn` macro adds ordinary destructuring and
pre/post conditions as described below. Function-form metadata still reports an
explicit unsupported-form error rather than dropping its expressions; parameter
vector pre/post metadata is supported by `fn`.

`letfn*` takes a vector of symbol/`fn*` pairs, then a body. All names are visible
to all initializers, and initializers must be literal `fn*` forms. Evaluation
allocates every closure identity before populating its capture edges. Escaped
mutually recursive functions retain their peers and outside captures after the
source and creating frame have gone away. Internal edges are unrooted arena IDs,
so recursive cycles do not create Rust `Rc` cycles and are reclaimable by GC.
SCI exposes `letfn` as a macro rather than raw `letfn*`; eight committed explicit
primitive adaptations compare equivalent bindings and calls. The source `letfn`
macro now lowers directly to this primitive, as described below.

`Context::invoke(&callee, &arguments)` uses the same dispatch as language calls
for interpreted and native functions. `InvokeError::Dispatch` describes owner,
non-callable, arity or native failures; `InvokeError::Execution` preserves a
function body's source error. Language dispatch errors acquire the caller's span.
The callee and all arguments execute once, left to right, before arity checking.
Callbacks can collect and invoke interpreted arguments without an active storage
borrow. A callback resetting its context or returning a foreign value fails the
existing owner checks.

Each function's immutable methods and capture sources are resolved once in its
owned code unit. Execution indexes local/capture slots and reuses the context's
scratch vectors, releasing frame roots on success and failure. Lexical captures
retain values, while namespace reads retain Var IDs and fetch their current roots.
There is no per-call analysis. An escaped closure keeps constants and source
provenance alive through its code-unit edge. Function equality/hash use identity,
including self-returning functions and functions stored as collection keys.

The current recursive evaluator bounds active interpreted/native/combinator calls at 32 and nested
expression frames at 128, reporting `CallDepthExceeded` or
`EvaluationDepthExceeded` instead of exhausting the ordinary Rust test-thread
stack. Recur uses a loop and does not add calls or expression frames on each
iteration. These conservative ordinary-call limits remain temporary until
configurable resource controls and more general iterative execution are implemented. The counters are shared across
evaluation/invocation reentry in the same context.
A host callback's own stack and arbitrary host recursion remain trusted embedding
behavior outside these language-execution limits.

`tests/interpreter_functions.rs` executes all 42 new pinned SCI cases and tests
300-argument calls, owned source/error lifetimes, independent factory calls,
no per-call code growth, collection during callbacks, escaped recursion, cycle
reclamation, owner/reset errors and explicit depth failures. The compiler's
existing unchecked JavaScript invocation rules remain unchanged.

## Loop and function recur

`loop*` initializes a vector of symbol/value pairs sequentially, like `let*`,
then establishes a lexical recur target for its body. Each `fn*` method has its
own target, including anonymous functions and `letfn*` initializers. Recur always
selects the innermost loop or current method; a nested function cannot jump to
an enclosing loop or function. A loop can return a value from any expression
position, including a call argument or another recur operand. Empty loop bodies
return nil. Ordinary `loop` and destructuring expand to this primitive.

Analysis checks every recur, even in unused branches/functions: there must be a
target, the recur must be in tail position, and the operand count must equal the
target's slot count. Only the final body expression and its selected conditional
branches inherit tail position. Tests, callees, call/recur operands, binding
initializers and collection/metadata expressions do not. Every new function or
loop establishes its own body tail context. Errors retain the recur form's
original span, including reader-generated functions and host-generated forms
with an explicit origin. Operands are analyzed before the target/arity checks,
matching SCI's unresolved-symbol precedence.

Operands execute once, left to right, against the old frame. A distinct internal
control result then moves all operand roots from reusable scratch into the target
slots and restarts the body. It does not allocate a call frame or argument vector
on each hop, reanalyze a body, or dispatch a different function method. Variadic
recur includes exactly one operand for the rest slot: that value is installed
unchanged, including nil, list, vector or scalar, rather than repacked as call
arguments. Closures copy captured values when created, so retaining one from an
earlier iteration preserves that iteration's bindings.

`Context::set_execution_poll` accepts an optional static `ExecutionPoll` callback.
It also receives `PollPoint::Iteration` in eager sequence consumers and combinators.
`PollPoint::Function` is called on the common native/interpreted invocation path;
`PollPoint::Recur` is called after all recur operands and before slot replacement,
including a zero-argument backedge. Returning an error stops execution with
`ExecutionInterrupted`, preserving a language call/recur span (or a dispatch
error for direct invocation). Reset clears the hook. The hook receives no runtime
borrow or context; it can consult host state for cancellation or a host-maintained
counter. Built-in resource accounting also covers analysis, expressions and
collection traversal; these additional points do not call the legacy execution
hook. Neither interface forcibly stops a running host callback.

`tests/interpreter_recur.rs` runs 100,000 loop iterations and 100,000 function
iterations with temporary vectors/closures. It samples host-stack addresses and
live heap/root/arena counts, collecting explicitly every 1,000 iterations;
those counts stabilize, while unit tests check bounded scratch capacity and
released call/expression depths. It also covers 100 target slots, zero-slot
cancellation, nested loops, captures collected during operand evaluation, source
lifetimes and failure cleanup. This is constant-stack **recur**, not general
tail-call optimization. Heap collection remains explicit; allocating programs
still need periodic collection through host callbacks.

At the pinned SCI revision, raw analysis permits recur in an `if` test, a
computed callee and an ordinary call argument. The three `sci-recur-if-test`,
`sci-recur-callee` and `sci-recur-argument` oracle probes wrap those function
constructions in `fn?` and return true. Native analysis deliberately rejects all
three to enforce the required tail-position contract. The other 37 focused
recur/loop cases match oracle values, effects and error phases exactly.

The analyzer's target context records a protected-region depth. Source `try`
rejects recur to an enclosing target from its body, catches or finally, while
fresh loop/function targets inside the protected region work. This behavior is
covered by native tests and the pinned exception fixtures.

## Namespaces, definitions and Vars

Every context lazily creates separate `user` and `clojure.core` namespace objects.
`NamespaceId` and `VarId` belong to independent, generation-checked arenas; they
never enter compiler `GlobalId` storage. Namespace indexes map interned symbols
to Vars. Successful analyzed reads index the stored Var ID and fetch its current
root without hashing a name. Lexical bindings take precedence; unqualified names
then consult current namespace mappings and implicit core mappings. Qualified
names consult the named namespace. Definitions and `in-ns` switches persist
across evaluations in the same context, while fresh/reset contexts are isolated.

`def` accepts an unqualified name, optional docstring with initializer, or a
qualified name in the current namespace. Analysis interns its Var before
analyzing initializer and metadata, enabling recursive functions and leaving an
unbound Var even after an initializer analysis failure or an untaken branch.
Execution evaluates the initializer, then metadata against the old root, and
commits both only after success. No-init `def` preserves an existing root and
replaces metadata. A lone string is an initializer. Explicit `:ns`, location,
name/file and docstring fields follow the selected SCI precedence; locations
use this frontend's byte columns. `eval_string` uses nil `:file`; named source
evaluation records the source name. Qualified same-namespace definitions discard
symbol metadata, matching the pinned SCI behavior.

`declare` creates unbound Vars, evaluates symbol metadata, sets `:declared`, and
returns its last Var (nil when empty). A subsequent `def` replaces declaration
metadata. `defonce` analyzes its initializer even when already bound, and skips
execution for any existing root, including nil/false. Matching SCI's expansion,
it evaluates metadata before testing rootedness and again after a first
initializer; a skipped definition updates metadata once and returns nil. These
forms retain direct interpreter implementations; they are not yet registered expanders for macroexpand.
Malformed definition/Var forms report analysis errors; SCI reports some malformed
`def` arities as execution/type errors, a deliberate diagnostic difference.

`var`/`#'` ignores lexical shadowing. `deref`, reader `@`, `var-get`, `var?` and
`bound?` expose Var identity/rootedness. Unbound reads return a truthy
`ValueView::Unbound`, distinct from nil, and fail with `NotCallable` if invoked.
Unbound values compare as a common sentinel, although inspection retains the
originating Var name. An unbound initializer leaves the target Var unbound.
Vars are callable through their current root; cyclic Var-to-Var call targets
fail without growing the Rust stack. Redefinition preserves identity, while
`ns-unmap` followed by definition makes a new Var; retained closures still refer
to the old Var. Re-executing an analyzed definition resolves the current mapping
after initializer and metadata effects and interns a new Var when unmapped.
Var/namespace equality and hashes use owner-scoped identity.

Implemented introspection is `resolve` (optional environment), `ns-resolve`
(optional environment), `ns-name`, `find-ns`, `create-ns`, `the-ns`, `all-ns`,
`ns-interns`, `ns-publics`, `ns-refers`, `ns-map`, and `meta`. `get` and `contains?`
allow projections from returned maps. `refer` accepts namespace and selection/
rename filters; explicit refers are rejected as definition targets until unmapped.
Like pinned SCI, require/refer include private interns, including `:refer :all`;
`ns-publics` filters private Vars, while `ns-refers` reports explicitly referred
private Vars. Privacy does not prohibit qualified reads or explicit resolution. `ns-refers` retains implicit
core entries even when an own definition shadows them in `ns-map`. Namespace map iteration
is unspecified. Only implemented core functions appear in native core views;
this is not a promise of the JVM's full core namespace or native Var metadata.
Native/embedding Vars synthesize basic `:name`/`:ns` metadata on language `meta`;
static native arity/effect/error metadata remains available through Rust APIs.

Embedding APIs expose current/find/create namespace, checked ID recovery,
`intern_var`, `resolve_var`, `var_root`, `var_is_bound`, `set_var_root`,
`refer_var`, `unmap_var`, and `set_current_namespace`. Context operations use
`NativeEffects::Context`; they supply no external capabilities. `register_native`
installs a native callable into the same Var tables as language definitions.
Namespace records are rooted by their context, and the collector traces their
Var mappings plus code/metadata/root edges. Replaced roots and unmapped Vars are
reclaimed once unreachable; retained Vars/code survive reset and context drop
as owned handles but cannot enter a new owner. The compiler remains unchanged.

`tests/interpreter_namespaces.rs` executes the focused namespace/Var SCI fixtures
and covers persistent/isolate/reset behavior, lexical shadowing, old analyzed
reads, metadata failure ordering, embedding owner validation, collection during
callbacks, and bounded retained storage under repeated redefinition. Dynamic
binding and mutation are described below; explicit namespace removal remains
separate work. Loading is described below.

## Native calls and bounded numeric core

Native definitions contain public name/namespace, arity, effect and error metadata,
plus a typed `fn(&mut Context, &[RootedValue]) -> NativeResult` callback.
`builtin_definitions()`, `namespace_definitions()`, `exception_definitions()` and
`collection_definitions()` expose the finite registries for host inspection and
future permission decisions. `register_native` installs an explicit definition;
`native_function` creates an unregistered callable; `invoke_native` applies the
same checked ABI directly. Definitions are static Rust function pointers: they
cannot capture owned runtime roots and introduce hidden collector cycles. A
callback's trusted Rust implementation supplies its effects and error declarations;
metadata alone is not a sandbox. Stateful capturing callbacks and permission
configuration are outside this slice. Callback panics remain host panics; callbacks
should return `NativeError::Host`. Cancellation cannot forcibly stop arbitrary host code.

Native names resolve to Vars during analysis, after lexical bindings. Unqualified
names consult current namespace mappings before implicit `clojure.core`; qualified
names resolve only their explicit namespace. Runtime calls use current Var roots,
never Rust symbol lookup, reflection, filesystem paths or OS commands. Native
values can be stored in collections and local bindings or returned by expressions.
The callee executes first, then every argument once from left to right, then ABI
arity/type checking and invocation. A failing argument prevents subsequent
arguments. `NativeError` preserves arity counts, zero-based argument indexes,
numeric failures, owner errors or a host message; `EvalError` supplies execution
phase, qualified native name and the whole call's source span. A non-callable
value produces `NotCallable` after its arguments execute.

Arguments, code, callee and locals remain rooted across callbacks. No runtime
storage borrow crosses host code. Foreign arguments/results and a callback that
resets the context are rejected. Reset clears host registrations. Re-registering
a name replaces its Var root, including for previously analyzed reads. A directly
retained native callable still invokes its original static definition.

Implemented numeric calls are `+ - * / inc dec quot rem mod`, `== < <= > >=`,
`zero? pos? neg? even? odd?`, `min max`, `byte short int long float double char`,
and ratio `numerator denominator`. General operations are `= not= identical?
identity not boolean` plus nil/some/boolean, numeric-category/sign, string/char,
symbol/keyword (including simple/qualified), collection-category, `fn?` and
`ifn?` predicates. The registry is the exact supported-name list. BigInt,
BigInteger and BigDecimal conversion calls return explicit unsupported-number
errors; their literals are rejected during reading.

Exact arithmetic uses checked i128 intermediates, reduces ratios, normalizes
signs and narrows only the result to i64 components. Integral results are i64,
including results for which JVM ratios produce an in-range BigInt. `rem` follows
the dividend's sign; `mod` follows the divisor's sign. Remainder calculation does
not require an intermediate quotient to fit i64. Division by zero fails for
exact numbers and doubles, including negative zero. An overflowing exact result
fails instead of promoting; `quot MIN -1` also fails instead of JVM long wraparound.

Mixed operations/comparisons coerce to double, so `==` can equate a large integer
with its rounded double. Exact comparisons use cross products without rounding.
Ratio-to-double conversion follows JVM DECIMAL64's 16 significant decimal digits
and ties-to-even rounding before binary conversion. Floating arithmetic uses
IEEE-754, permitting NaN and infinities; nonfinite `quot` results are errors.
Floating `rem` computes `a - quot(a,b)*b`, preserving JVM signed-zero behavior.
Structural `=` distinguishes integer/ratio/double categories; numeric `==`
compares across them. Numeric comparisons check successive pairs and short
circuit; single-argument comparisons return true. `min`/`max` preserve the selected
operand category, choose the later tie and propagate NaN before other type checks.

Conversions truncate fractions toward zero, then check range. NaN converts to
integer/character zero. Long conversion rejects the rounded positive endpoint
`9223372036854775808.0` instead of JVM's saturated cast; infinities cannot become
integers/characters. `float` performs checked f32 rounding and stores the result
as a double, since the bounded runtime has no separate Float category. Thus
`double?` of a native float conversion is true, unlike JVM Clojure. `identical?`
uses the documented representation identity (all equal immediate i64s, exact
float bits, interned name IDs and heap identities), rather than JVM boxing rules.
`ifn?` recognizes Clojure callable categories. Symbols, keywords, vectors, maps
and sets now invoke through the same ABI as functions and Vars.

`tests/interpreter_native.rs` executes 39 new pinned SCI fixtures, with two
adaptations from `core_test.cljc::core-test` and `permission-test`; the latter
checks the extracted `int?` call, without claiming permission enforcement.
Independent tests cover bounds, declared representation differences, reusable
metadata, callback collection/reentry, reset/foreign-owner checks, failure cleanup
and effects before arity failures. Compiler numeric storage and JS behavior are
unchanged.

## Portable JVM semantics

The oracle uses a new SCI context for each case, Clojure **1.12.4**, explicit
`:features #{:clj}`, the default `user` and `clojure.core` namespaces, and disabled
read-time evaluation. `observe` is a deliberately supplied test host callback;
stdout and stderr are captured independently. JVM SCI is used only by the
explicit reference command, never by ordinary Rust tests.

Portable assertions compare typed values, collection kinds, evaluation/effect
order, arity failures and stable read/analysis/execution phases. They do not
compare SCI ASTs, object identities, Java stack text, incidental error messages,
or hash collection traversal order. Fixtures cover the supported surface, whole programs, deterministic generated
variants, closure capture, errors and effects. The corpus is not a
claim of passing SCI's entire suite or supporting all its Vars.

The analyzer/runtime uses interned names, compact typed IDs, arena
records, indexed local/capture/Var references and separate source provenance.
Per-context scratch storage retains useful capacity. Namespace/code caches may
not reuse mutable roots, effects, permissions or expansions across incompatible
environments. See [implementation rules](dos-and-donts.md).

## Value and lifetime foundation

`interpreter::Context` owns an isolated, single-threaded runtime. Constructors
return `RootedValue`; `inspect()` returns an owned `ValueView` without exposing
an arena borrow. Integers remain `i64`, doubles retain every bit (including
signed zero and NaN payloads), and characters contain one `u16` code unit.
Strings and both `Name` components retain exact UTF-16 units, including lone
surrogates. Names are interned once per live owner; symbols and keywords share
name storage while retaining separate value categories. `Name` construction
does not perform source-token validation or namespace resolution.

`same_identity` compares heap/name identities or an immediate value's exact
representation. It is the native `identical?` contract, which deliberately differs from
JVM boxing-dependent identity. `value_eq` compares contents across owners. Lists and vectors with equal
elements compare equal, maps compare associations without order, and sets compare
members without order. Metadata never affects equality or `value_hash`. Empty
lists/vectors compare equal; empty maps, sets and nil are distinct. Integers,
non-integral ratios and doubles remain separate equality categories. `ratio`
reduces using checked wider storage, normalizes the sign, returns an integer for
integral ratios, and rejects zero denominators or out-of-range reduced components.
Double signed zeros are equal and have equal hashes. Separate equal strings have
equal contents and hashes but different identities.

`try_value_eq` and `try_value_hash` report deferred sequence projection failures.
Language equality and map/set key operations use checked access. The original
`value_eq`/`value_hash` helpers are for eager storage values: failed comparisons
return false and failed hashes use one common fallback value. Identity matches
remain reflexive; callers requiring language semantics must use the fallible
methods. A failed projection is never silently admitted as a language map/set key.

`value_hash()` is an internal 64-bit content hash, not JVM Clojure's exact `hash`
integer. It is owner-independent and consistent with equality, including nested
list/vector keys, signed zero and metadata. Full-hash collisions still compare
keys structurally. It is not a cryptographic or adversarial collision guarantee.
Interpreted closures use heap identity equality and hashing; captures never enter
their hashes. Native callables use static definition identity. Future mutable
values must also keep their mutable contents out of key hashes.

The native scalar NaN contract remains IEEE equality (unequal even to itself),
with canonical NaN hashing. A retained immutable collection is reflexively equal
to itself, including one containing NaN; separately constructed collections
compare their contents. NaN scalar keys do not match during lookup and may be
inserted repeatedly. This deliberately does not reproduce JVM boxed-NaN identity
shortcuts or the JVM's differing array-map/hash-map NaN lookup behavior.

### Immutable collection API and sharing

`Context::list`, `vector`, `map`, and `set` validate every input owner before
allocation. List/vector inputs keep their supplied sequence order. Map inputs
are ordered key/value pairs: later equal keys replace values while retaining the
first key representative (including its metadata). Sets retain the first equal
member. Constructors accept already evaluated values and never evaluate forms.
The semantic form arena keeps ordered child expressions separately. The native
analyzer uses those children and evaluates
collection members before metadata; collection iteration cannot reconstruct
that order. Map/set iteration is unspecified, including small maps, so JVM
array-map traversal order is outside the native contract.

`RootedValue::count`, `nth`, `elements`, `entries`, `get`, `contains`, and
`metadata` provide shallow access with fresh roots for extracted values. `inspect`
reports collection kind/count rather than recursively copying its contents.
`get` distinguishes `None` (missing) from `Some(nil)` and `Some(false)`. Vector
lookup uses integer indices; `assoc` accepts an existing index or count to append.
`conj` prepends lists, appends vectors, inserts set members, or associates a
two-element vector into a map. These remain typed storage primitives; the
language functions add nil-as-collection and the checked map-entry sequence rules.

Lists share tails through traced arena IDs and add one cons record per prepend.
Vectors use 32-way persistent trees: updates copy one path and at most 32 values
or child pointers at each level. Maps/sets use a persistent 16-way hash trie
with at most 16 branch levels plus a collision bucket. Updates share unaffected
branches; collisions copy only that bucket. Nested members are immediate values
or raw IDs, never recursively copied graphs. Internal tests assert unchanged
branch pointer identity and bounded copied paths. Integration tests cover vector
boundaries through 32,768 elements, real text-hash collisions, and old versions.
Equality, hashing and GC walk language nesting iteratively; local memo tables
reuse results for repeated collection values and are dropped after each
operation. Distinct lists still scan their logical elements, so hashing many
shared list suffixes can take quadratic time in the number of cons records.
Traversal work and materialized snapshots consume the active fuel/allocation
budget. Exhaustion is terminal, including during equality and hashing.

`with_meta` accepts lists, vectors, maps, sets and symbols, with a map or nil
metadata value. Other targets, including nil, keywords and strings, fail
explicitly. Replacement shares the unannotated base, so it cannot retain a chain
of superseded metadata. Assoc/conj/dissoc/disj and vector pop preserve metadata.
List pop returns its retained tail and that tail's own metadata; singleton pop
preserves metadata on the resulting empty list, matching the pinned JVM behavior.
Metadata is itself traced immutable data and can carry metadata.


`Source` owns its original UTF-8 text and file name. `SourceSpan` checks range
order, bounds and UTF-8 boundaries and keeps the source alive. Line and byte
columns use the existing parser `LineIndex`, constructed lazily. No runtime
record borrows a parser node, input string or transient analysis buffer.
`ExecutableUnit` owns a constant pool and source locations, tracing both; it
is deliberately not a data value or reader form. Semantic forms use this
constant/provenance pool in a separate owned arena. Analyzed units additionally
own their typed instructions and function methods. Callable values retain these
units without changing the compiler's `Program`.

```rust
use light_cljs::interpreter::{Context, ValueView};

let mut context = Context::new();
let value = context.string(vec![0x61, 0xd800]);
let clone = value.clone();
drop(value);
context.collect_garbage();
assert_eq!(clone.inspect(), ValueView::String(vec![0x61, 0xd800].into()));
context.reset();
assert_eq!(clone.inspect(), ValueView::String(vec![0x61, 0xd800].into()));
```

### Rooting and reclamation contract

Public value, source and executable-unit handles hold a strong `Rc` to the owner.
Heap/name/Var/namespace values, sources and units share immutable root records;
the owner stores only `Weak` records. The last handle drop removes reachability.
Immediate nil, boolean, numeric, character and static-native values live inline
and need no collector root allocation or weak entry. They still charge the same
semantic allocation units and retain owner checks. `MemoryStats::roots` counts
collector records, so inline values do not contribute. Heap and code
records contain immediate values or typed, unrooted IDs, never public handles
or owner references. Immutable string/name buffers and collection tree nodes may share `Rc`
allocations. Tree leaves hold only unrooted values/IDs, and the collector traces
every list head/tail, vector member, map key/value, set member and metadata edge.
Tree sharing never creates a strong reference back to the runtime owner.

`collect_garbage()` runs an iterative mark/sweep at an explicit host boundary.
It traces live host roots, collections/metadata, executable constants and sources, removes dead
intern-index entries, and reclaims unreachable arena records. Slots receive a
new generation before reuse; generation exhaustion retires a slot. Owner
identities never wrap. `HeapId`, `SymbolId`, `SourceId`, `UnitId`, `NamespaceId` and `VarId` are private-
field typed handles: raw IDs do not root records and lookups report `StaleId`
or `WrongOwner`, never an unrelated replacement. Unit construction validates all
input owners before allocation, including immediate constants.

Collection reuses arena and tracing capacity, but the trace worklist is emptied
and dead root records are removed. Intern indexes do not keep unused symbols
alive. Allocation does not trigger collection, so a long-lived embedder must
call it periodically; this is not an automatic memory budget.
Atoms trace their state, metadata, validators and watches; volatiles trace their state. Deferred nodes trace their producers, results and cached failures. Namespace
mappings trace Var records; Vars trace namespace/name, metadata and root values;
code units trace their resolved Var IDs. Current closure records trace their code units and captured values;
active call frames retain rooted values. Analysis also roots every resolved Var
until the executable unit is installed: a later macro callback can unmap a Var
and collect while earlier expressions, a definition, or a `set!` target still
need its original identity. Test-only graph nodes additionally exercise
heap-to-unit-to-heap cycles independently of language construction.
No runtime storage borrow may cross an arbitrary
host callback; convert host-provided inputs before borrowing runtime storage.

`reset()` clears this context's namespace, reader, loader and temporary roots,
collects the old owner, and then creates a fresh one. Existing
host handles keep their old owner usable, but cannot enter the new context.
An old detached owner retains its remaining records/capacity until its last
host handle drops; no automatic collection runs on individual handle drops.
Dropping the final context/host handle releases even uncollected language cycles
because internal graph edges have no strong owner/record references. Direct
context drop does not collect; retained handles may keep unreachable records in
that owner until its last handle drops. Reset before drop when those records
should be reclaimed first. Other fork branches or retained readers still keep
their own context state reachable. Retained values/sources remain inspectable;
closures remain callable through a surviving context in the same owner family,
and fail owner validation in a fresh context. This is safe Rust throughout and
adds no production dependencies.

| Boundary | Roots and reclamation |
| --- | --- |
| Host handles | Values, sources, executable/analyzed units, forms and diagnostics retain their dependencies. Clones share root records; raw typed IDs retain nothing. |
| Analysis and active calls | Pending constants, resolved Vars, source spans, callees, arguments, locals/captures and intermediate results survive callback collection/reentry. Scratch clears references on exit while keeping capacity. |
| Dynamic state and callbacks | Binding guards retain prior bindings; atom operations root validator/watch snapshots; deferred nodes retain producers, realized values and cached failures. Native function pointers cannot capture runtime roots. |
| Cached modules | Loaded-state records cache names/status, not all historical code or source. Reachable Vars/closures retain required units; reload and redefinition release superseded units once their last roots disappear. |
| Context forks | Every branch contributes roots to the same owner. Copied namespace maps and dynamic-Var remaps stay rooted; dropping a branch and collecting from another releases unreachable branch records. Existing shared Vars and escaped values can intentionally retain original namespace records. |

Collection traverses runtime graphs iteratively. Persistent map/set and vector
tree nodes write directly into the reused trace buffer, avoiding a fresh
iterator stack per collection. This small traversal recurses only through the
bounded tree structure: at most 16 hash branches, or 12 vector branches on a
64-bit target. Nested language values are queued, never recursively traced.
The empty trace and weak-root registry retain capacity but contain no dead
runtime edges after collection. Vacant arena slots are reused with new
generations; high-water capacity is not a promise that RSS shrinks.

`tests/interpreter_reclamation.rs` combines 128 cyclic closure redefinitions,
48 lazy-prefix/cached-failure cycles, 64 fork/drop rounds and 64 module reloads
with retained historical closures. It asserts stable heap/name/root/code/source
counts, source and value generation rejection, reusable slots, macro callback
collection and live collection-tree edges. Internal tests check emptied reusable
root/trace capacity and release of an owner containing cycles. These are native
ownership tests; JVM GC timing is not part of the SCI semantic oracle.

### Reclamation cost measurement

Run `cargo bench --bench interpreter_gc` for timing and
`cargo bench --bench interpreter_gc --features count-allocations` separately
for allocation counts. The benchmark performs 256 read/analyze/evaluate cycles
per sample, redefines an atom/letfn closure cycle, and verifies the result is
always `42`. Seven samples report medians after two warmup batches. It separates
evaluation time from collection time; the fresh-process measurement includes
process launch, context construction, evaluation and shutdown. Normal timing
builds do not install allocation instrumentation (their zero allocation columns
are placeholders, not measurements).

Measured 2026-10-08 on arm64 Apple M3 Ultra (Virtual), Rust 1.99.0 release builds.
The baseline used raw analyzer Var IDs and iterator-based collection; the final
runtime roots pending Vars and traces persistent-tree nodes directly. Sequential
baseline/final runs gave these medians:

| Collect every evaluations | Evaluation µs/op, baseline → final | Collection µs/collection, baseline → final | Final arena slots |
| --- | --- | --- | --- |
| 1 | 106.98 → 105.10 | 62.15 → 44.18 | 1,396 |
| 16 | 107.62 → 106.91 | 366.52 → 343.00 | 2,731 |
| 256 | 110.29 → 108.64 | 7,269.25 → 7,262.96 | 24,091 |

Collecting every evaluation reduced total evaluation-plus-collection time by
about 12% in this workload; infrequent collection is dominated by reclaiming
accumulated records and showed little timing change. These are local benchmark
observations, not a general speed guarantee. Final fresh context-to-result time
was 247 µs; fresh process-to-exit was 1.89 ms (including launch overhead).

Separate allocation runs measured **307 allocations, 309 reallocations and
12,408 requested bytes per warmed collection before, zero after**. The instrumented
benchmark asserts zero collector allocations after warmup. Keeping temporary
analyzer Var roots adds two allocations and removes one reallocation per complete
two-form evaluation here: 2,120/444 allocation/reallocation calls become
2,122/443, with 64 additional requested bytes. These counts measure allocator
requests, not retained bytes or RSS. Allocation tracking is benchmark-only; the
production library remains safe Rust with no added dependencies.

Every cadence and both implementations retain exactly 309 heap values, 312
names, one source, one executable unit, six namespaces, 306 Vars and six roots
after collection (including the warmed builtins and latest definition). Slots
remain at each cadence's high-water mark. The correctness baseline reproduces
the pending-Var macro test's stale-ID panic; the final implementation passes
all ownership regressions and the unchanged 2,028-row pinned SCI oracle.

The [foundation tests](../tests/interpreter_values.rs) compare selected pinned
SCI scalar *outputs* with native storage, preserve source/value lifetimes, and
exercise ID reuse/reset. This is storage coverage, not execution of the SCI
programs. Reference anchors and the limited adapted source assertion are
documented in the [reference corpus](../tests/reference/sci/README.md).

## Deliberate number and reader differences

| Area | Native contract | JVM SCI reference |
| --- | --- | --- |
| Integers | Checked signed 64-bit values. Overflow errors explicitly; no rounding, silent promotion or unchecked wraparound. | Ordinary checked addition also errors on overflow; literals and promoting operations can produce BigInt. |
| Ratios | Exact, reduced, positive denominator; numerator and denominator fit signed 64-bit storage. Checked wider intermediates must detect overflow. Integral ratios become integers. | Host ratios can contain arbitrary-precision components. Out-of-range components are outside native support. |
| Doubles | IEEE-754 binary64, including `##NaN`, `##Inf`, `##-Inf` and signed zero. Numeric categories remain distinct in the test transport. | Portable binary64 behavior; the transport canonicalizes NaN payloads only. |
| BigInt and BigDecimal | Unsupported literals, suffixes, values and promoting operations fail explicitly, even for `1N` or a decimal that could be represented approximately. | `sci-bigint`, `sci-explicit-bigint`, `sci-decimal`, `sci-promoting-add` intentionally record successful reference outcomes as `bounded-number` rows. They are never native equality assertions. |
| Reading and diagnostics | Preserve the existing UTF-8 source, byte spans, UTF-16 units, parser diagnostics and finite reader limits. Semantic reading sits above the current parser. | SCI uses Edamame and JVM reader locations. Exact source spans/text and acceptance beyond native limits are not portable assertions. |
| Reader features and host tags | Default `:clj` feature selection, explicit host tag callbacks and contextual aliases; no `#=` execution. Arbitrary host reader/tag callbacks and JVM object construction are not implied by syntax recognition. | SCI exposes additional configurable readers/host facilities; the selected oracle does not enable them. |

`portable` rows define shared target semantics. `bounded-number` rows document
intentional differences and require explicit native rejection tests when those
features are implemented. The format reserves `reader-difference` for a
verified, individually documented divergence; there are currently no such
rows. Do not relabel a failure to make an unimplemented portable feature pass.
Strings and characters carry UTF-16 units, including lone surrogates; converting
them through lossy Unicode replacement is prohibited.

## Excluded facilities

This profile does not promise all SCI Vars/tests, Java classes or reflection,
arbitrary JavaScript interop, Java exception hierarchies, threads/futures/STM,
async, JIT, native protocols/proxy/reify, user-defined types, transducers,
sorted/transient collections, the full numeric tower, regex dialect parity, or
cross-process code caches. Limited language exception names such as
`Exception`, `Throwable`, and `clojure.lang.ExceptionInfo` may be provided without
Java class access. Regex/host object transport is deliberately unsupported and
fails the reference driver instead of being stringified.

An isolated context and explicit capabilities are an embedding boundary, not OS
process isolation. This document does not add host permissions or change the
existing compiler's f64 values, JS invocation rules, CLI modes or globals.


## Portable exceptions and diagnostics

`throw` accepts one exception value; invalid values produce a catchable language
error. `try` analyzes its complete body, every catch and finally before executing
anything. Body expressions run in order. Catch clauses use the first matching
category and bind the original exception identity; an error from that handler
passes to an enclosing try. Finally runs after body/handler success or failure,
discards its successful result, and replaces a pending language result/error
when it throws. Completed effects survive failure. As in pinned SCI's raw try
analyzer, ordinary forms after catch clauses still join the body; finally is
recognized only as the last clause. A nested `(catch)` call inside another
expression remains an ordinary function call.

The portable catch categories are `Exception`, `Throwable`, `AssertionError`, and
`clojure.lang.ExceptionInfo` (also `ExceptionInfo`), with `java.lang.Exception`
and `java.lang.Throwable` aliases. ExceptionInfo matches Exception and Throwable;
Exception matches Throwable; a plain Throwable does not match Exception.
AssertionError matches Throwable and its own category, but not Exception.
Arithmetic, type, arity and invalid-call failures are ordinary Exception values
when caught, not ExceptionInfo. No classes, reflection, arbitrary constructors,
`new`, instance methods or other Java exception types are exposed. Bare
Throwable/ExceptionInfo and Throwable construction are explicit native extensions
to the current JVM SCI reference options, which expose Exception and qualified
ExceptionInfo but do not import those two bare names.

`ex-info` accepts message/data and an optional cause. A message is a UTF-16
string or nil; data is a map or nil (normalized to an empty map, as in the pinned
Clojure 1.12.4 runtime); cause is an exception or nil. `ex-data`, `ex-message`
and `ex-cause` return nil for nonexceptions. The narrow `Exception.` and
`Throwable.` functions (and their `java.lang.` aliases) accept no arguments,
a string/nil message, or a message plus cause. They do not implement JVM
constructor overload resolution. Exception values use identity equality/hash,
keep raw data/cause edges in the tracing heap and remain usable through rooted
host handles after reset or context drop. `Context::exception` is the checked
embedding constructor; unlike the language ex-info wrapper it requires an actual
map for ExceptionInfo data. `exception_definitions()` exposes the installed
native function metadata.

`ErrorClass` distinguishes Language, Host, Control and Diagnostic failures.
Only Language participates in catch dispatch. Read/analysis/macroexpansion errors,
callback `NativeError::Host`, owner violations, depth exhaustion and cooperative
interruption cannot be caught. Finally is attempted during host/control unwinding,
with cancellation/main fuel and allocation temporarily replaced by separate
cleanup allowances (1,024 units each by default). Every finalizer in the same
unwind shares these allowances; they never refill between nested or outer
finalizers. Exhaustion produces terminal `CleanupLimitExceeded`, and the original
terminal failure wins over any cleanup failure. Later finalizers are attempted
but cannot execute after the shared cleanup allowance is exhausted. Depth and
output limits still apply. Arbitrary host callbacks remain uninterruptible. These embedding/control semantics are deliberate native contracts,
not JVM exception hierarchy conformance. Classification is exhaustive so adding
a new resource failure requires an explicit catchability decision.

Errors retain phase, original file/UTF-8 byte span, semantic origin, related span,
optional thrown exception, and ordered source call frames from innermost to
outermost. Call failures include argument/callee failures. Frames and diagnostic
strings are allocated on failure only; line/column indexing and formatting remain
lazy. Recursive execution carries boxed errors to keep those payloads off its
successful Rust frames; the existing 120-level nested-collection test continues
to pass. Exception data is stored separately and cannot replace diagnostic
file/line/column/phase. Data lowered through `form_from_data` retains its supplied
source span and `SuppliedData` origin through analysis and failure stacks.
Generated macro forms carry a `MacroExpansion` origin naming the originating
macro; unchanged carried subforms retain their original spans and origins. Native stacks
are source diagnostics, not JVM stack-map compatibility or a language callstack
introspection API.

Temporary arguments and partial let/loop bindings are released before catches
and finalizers execute. Catch bindings can escape through closures, with their
exceptions and causes traced normally. Tests collect during handlers, repeat
caught failures in a loop, preserve data/closures after reset, verify source
retention after context drop, and evaluate again after each failure. The focused
exception suite executes all 46 `sci-exception-*` rows plus the existing explicit
throw fixture, comparing exact values, phases, categories and completed effects.
The complete 289-row oracle was regenerated against the clean pinned SCI checkout
on 2026-10-08. Throwable aliases, terminal control handling, GC/lifetimes and source
stack formatting have separate Rust contract tests.


## Collection core and callable data

`collection_definitions()` exposes 33 implemented names: `list`, `list*`,
`vector`, `vec`, `hash-map`, `hash-set`, `set`, `conj`, `disj`, `assoc`,
`dissoc`, `get`, `get-in`, `assoc-in`, `update`, `update-in`, `contains?`,
`find`, `nth`, `count`, `empty`, `empty?`, `keys`, `vals`, `select-keys`,
`merge`, `merge-with`, `peek`, `pop`, `seqable?`, `indexed?`, `reversible?`
and `map-entry?`. Existing list/vector/map/set/sequential/seq/collection,
associative/counted and function predicates remain available. No transient,
sorted or user-defined collection type is implied.

Nil behavior, missing defaults, false/nil members, duplicate constructor keys,
vector append-at-count, nested empty paths and list-versus-vector stack order
follow the pinned reference. `get` is permissive on noncollections; `contains?`
and `find` enforce their supported categories. `nth` accepts numeric indices
with checked JVM i32 conversion; integer vector lookup/assoc/call and numeric
string lookup use JVM's unchecked narrowing. Strings preserve individual UTF-16
units. `vec`/`set` discard input metadata. `select-keys` preserves source metadata
and retained map keys. Other immutable updates retain metadata as documented
above; `empty` and `conj` on non-list sequences drop it.

Map entries are distinct vector values, produced by `find` and map sequence
conversion. They compare/hash like vectors and are callable, but `map-entry?`
distinguishes them, `empty` returns nil, metadata is rejected, and vector
updates/`vec` return ordinary vectors. Map `conj` accepts a direct two-element
vector, nil, or a sequence of actual entries; a sequence of ordinary pair
vectors fails. `list*` shares list/sequence tails and preserves the pinned
List-versus-Cons distinction, including nil and empty nonnil tails.

`keys` and `vals` retain a traced source and defer entry projection until an
element is requested. Construction, count, empty checks and out-of-range default
lookup do not read an invalid element. `nth` can skip invalid earlier members;
equality and `get-in` stop when their result is known. `merge-with` requests
entries incrementally, invoking each conflicting-key callback before reading
the next entry. Finite sequence views retain already evaluated members. Deferred producer values use the realization machinery described below.

Native maps always use the persistent hash trie. Hash collection iteration is
unspecified, and callbacks follow each map's native traversal, with input maps
processed left to right. As with JVM hash maps, using a failed projection as a
key forces hashing and reports the failure. JVM small array maps can postpone
that failure because they use linear comparison without hashing; that
representation-dependent timing is outside this hash-trie contract. Native
map sequence snapshots have constant-time count, without promising JVM
array-map/hash-map sequence implementation classes.

Keywords, symbols and maps accept one key/collection argument and an optional
default. Sets and vectors accept exactly one argument. Vector invocation is
strict about integer keys and bounds. Literal keyword calls with invalid arity
fail analysis after their arguments are analyzed, as in pinned SCI; dynamically
obtained keywords fail at execution through the common ABI. Other noncallable
values fail after their arguments execute.

`update`/`update-in` call their supplied function once with the prior value
(or nil) and additional arguments. `merge-with` calls only on a present key,
including a present nil/false value. Immutable inputs remain unchanged and
nested updates walk deep paths iteratively. Bulk construction and updates use
the existing persistent trees and raw shared members, never recursive copies.
Callbacks may collect or reenter evaluation without an outstanding store borrow.
`NativeError::Invocation` forwards `InvokeError` through native callbacks,
preserving original exception identity, source frames and terminal host/control
classification. `ForwardedError` compares by retained failure identity, not by
formatting diagnostic text. A callback that resets the context is rejected even
when it returns an error.

`tests/interpreter_collection_core.rs` executes all 159 `sci-coll-*` programs,
including two exact upstream adaptations from `core-test` and
`call-quoted-symbol-test`; the remaining probes are independently authored.
The existing `eval-colls-once` oracle row still requires `sort-by`, which
remains unsupported; this suite adds a separate evaluate-once collection probe
without claiming that upstream assertion passes. Every exported collection name
has positive/nil coverage plus checked arity and applicable type/bounds tests.
Independent Rust tests cover GC/reset, sharing, deep paths, metadata, direct ABI
calls and terminal callback failures. All 448 committed oracle rows regenerated
against the clean SCI pin on 2026-10-08.


## Finite traversal, reductions and higher-order helpers

The core registry additionally exposes `seq`, `first`, `rest`, `next`, `cons`,
`reduce`, `reduce-kv`, `reduced`, `reduced?`, `unreduced`, `into`, `mapv`,
`filterv`, `some`, `every?`, `not-any?`, `not-every?`, `apply`, `comp`,
`partial`, `complement`, `constantly` and `juxt`. `seqable?` includes nil,
strings, lists/Cons, vectors/map entries, maps, sets and existing sequence views.
`into` supports zero, one and two arguments; transducer arities remain excluded.
No eager implementations are registered as `map`, `filter`, `range`, `take`,
`drop`, `repeat` or `iterate`. `lazy-seq` is a real deferred producer macro.

`seq` returns nil for empty inputs. `first` returns nil for an empty input;
`rest` returns an empty list, while `next` returns nil when no tail remains.
Strings yield one character per exact UTF-16 code unit: supplementary characters
produce two elements and isolated surrogate units remain intact. Maps yield
actual map-entry values and sets yield their stored members; iteration order for
both remains unspecified. List tails share existing nodes. Finite snapshot tails
share an `Rc` slice with an offset rather than copying the remaining elements.
`rest`/`next` can skip malformed `keys`/`vals` entries without projecting them.
`cons` preserves SCI's distinction between a nil tail (proper list) and a non-nil
empty collection tail (Cons). A lazy Cons retains its head and deferred tail without forcing the tail.

No-init `reduce` uses the first element, calls its reducer with zero arguments
on empty input, and leaves a singleton untouched. Initialized reductions leave
an empty-input initializer untouched. Each reducer result is checked for
`reduced`; termination unwraps exactly one layer and does not request another
input. `reduce-kv` supplies vector indexes or map keys; its fallback consumes
sequences of actual map entries, including map-entry values themselves via that
fallback rather than vector indexes. Ordinary pair vectors in the fallback
fail when reached. `Context::reduced` and `RootedValue::reduced_value` provide
the corresponding owner-checked embedding operations. Markers use identity
rather than contained-value equality; `deref` and `unreduced` unwrap one layer.

`mapv` stops at the shortest input; `filterv` keeps the original selected values.
Search helpers short-circuit using Clojure truthiness. `some` returns the first
truthy callback result, including zero or empty collections, and returns nil
when all results are false/nil. `every?` and its negated variants return booleans.
Two/three-collection `mapv` validates sequence inputs before testing emptiness;
four-or-more-collection `mapv` follows the pinned short-circuit coercion order.

All callbacks use the common evaluator and preserve structured exceptions,
source frames, host failures and cancellation. `apply` combines fixed operands
with its final seqable argument. Interpreted variadic calls preserve the unused
argument tail, so counting or ignoring a malformed deferred projection need not
force it. Fixed arguments are demanded before the body. SCI multi-method dispatch also
demands up to four leading arguments before selecting its method. Combinators also retain
this applied-call behavior: `constantly` forces no applied arguments; `complement`
forces up to two; `comp`, `juxt` and partials with one through three bound arguments
force up to three; partials with four or more bound arguments force none before
forwarding to their target. These are pinned Clojure 1.12.4 entry-arity semantics.
Applied `partial` and `juxt` constructors retain their deferred capture tails;
internal finite concatenation views join partial captures and later arguments.
Traversal/count of the captured left side realizes its members; an unused rest
tail remains deferred. Applied variadic dispatch preserves JVM bounded next-link
lookahead, and applied mapv stops reading collection arguments at an empty input. Language `concat`, including syntax-quote expansion, uses genuine deferred nodes as described below.
Host native callbacks still receive a fully materialized checked argument slice.
`comp` runs right to left, `juxt` runs left to right, and one-argument `comp` or
`partial` returns its operand unchanged. Captured values/functions and reduced
contents are raw traced edges, keeping closures/code alive without Rust cycles.

Iteration boundaries poll through `PollPoint::Iteration`, including callback-free
consumers such as `into` and native `apply` argument collection. Interruption is
a terminal control failure and existing bounded `finally` cleanup still runs.
Native calls also participate in the shared call-depth guard, so nested `apply`
and reentrant native callbacks fail with a terminal depth error before the host
stack can overflow. No heap/store borrow crosses a callback. The evaluator reuses indexed local and
argument scratch; no lexical name environment is built per element. Rust tests
reduce, map and traverse 100,000 elements, sample constant stack/live roots/code
during reduction, and exercise collection during callbacks, retained tails,
combinator/Var cycle reclamation, owner reset and terminal cancellation. This is
a stack/lifetime contract, not a throughput benchmark. The shared budgets below
also cover these consumers.

The 108 `sci-seq-*` cases cover values, callback order/errors, exact UTF-16 units,
empty/no-init/early-reduced behavior, deferred applied tails and combinator entry
arities. Three adaptations use `core_test.cljc::core-test`, `fn-test` and
`try-catch-finally-throw-test`; the manifest names exact assertions and changes.
All 556 oracle cases regenerated successfully from pinned SCI on 2026-10-08.


## Host resource controls and cancellation

`Context::set_resource_limits(ResourceLimits { .. })` configures the next
outermost interpreter entry. Fuel, cumulative allocation and output default to
unlimited (`None`); zero permits no charged work in that category. Call depth
and active expression depth default to 32 and 128. Hosts can lower these guards;
larger settings are capped at those native-stack safety maxima. Parser/semantic
reader nesting limits remain separate. A call-depth limit is not a recur count.

```rust
use light_cljs::interpreter::{CancellationToken, Context, ResourceLimits};
let mut context = Context::new();
context.set_resource_limits(ResourceLimits {
    fuel: Some(100_000),
    allocation: Some(200_000),
    output: Some(8_192),
    call_depth: 16,
    ..ResourceLimits::default()
});
let cancellation = CancellationToken::default();
context.set_cancellation_token(Some(cancellation.clone()));
// Another thread may call cancellation.cancel(); Context itself stays on this thread.
let value = context.eval_string("(reduce + [1 2 3])")?;
# Ok::<(), light_cljs::interpreter::EvalError>(())
```

One accounting scope spans reading, analysis, top-level forms, execution, direct
`invoke`/`invoke_native`, and callbacks that reenter the same context. Standalone
analysis, semantic lowering and each pull-reader operation also create a scope
when no outer entry exists. Reusing an analyzed function avoids reading/analysis
cost but still pays its runtime costs. Configuring new limits or replacing the
cancellation token during a callback affects the next outer entry; it cannot
refill the active allowance. Context reset changes the value owner while keeping
the active accounting scope. A subsequent independent host evaluation starts a
fresh budget. A different Context remains isolated.

`ResourceUsage` reports successfully admitted fuel, allocation and output units
for the current or most recent outer entry; refused charges and separate cleanup
allowances are excluded. Fuel counts semantic work rather than CPU instructions:
reader/analyzer/expression steps, function entries, recur, sequence/equality/hash
traversal and native argument processing. Bulk work can be charged before it
starts. Allocation counts source bytes, UTF-16 payload units, names, namespace/Var
records, form/expression/member storage, collection snapshots and fresh root
records. Root clones share a record; new roots are charged even for scalar values.
Persistent structures share their old members, and charge logical new work rather
than copying the old graph. GC does not refund cumulative allocation. These units
are implementation accounting, not exact allocator bytes, live-value counts or
process RSS; counts may change as the implementation changes. Retained old values,
host allocations, arena capacity and error-reporting storage are not a hard memory
cap. Hosts still schedule collection and isolate untrusted processes as needed.

`CancellationToken` is a one-way, thread-safe signal. Cancellation or exhaustion
latches in the active scope and becomes an `ErrorClass::Control` failure even if a
host callback discards a nested failure and returns a value or ordinary exception.
Reader/analysis limits preserve their phase while retaining Control classification.
Source-backed operations report retained file/byte spans and call frames; direct
host dispatch has no synthetic source. Language exception data, including guessed
interrupt keys, cannot create or catch these signals. A throwing finalizer cannot
replace them. Successful effects preceding exhaustion are retained, but evaluation
never reports semantic success for exhausted work. Forwarded callback errors retain
their original inner source and call frames.

Cleanup has separate `cleanup_fuel` and `cleanup_allocation` allowances. It masks
the admitted cancellation signal to let finite cleanup run, and covers expressions,
native calls, nested eval, collection work and allocations through the same API.
Once either allowance fails, cleanup stays exhausted throughout that unwind.
The pending terminal body error wins; ordinary language exceptions retain normal
Clojure finally replacement semantics. Output continues to use its original quota,
so cleanup cannot write beyond it.

Native callbacks must cooperate. `checkpoint(PollPoint::Host)` charges a step and
checks cancellation; `charge_allocation(units)` reserves host storage work and
`charge_output(bytes)` reserves encoded bytes **before** writing to a sink. Return
these errors through `NativeError` (its conversion preserves Control), and forward
nested `InvokeError` structurally. Infallible host data constructors can only latch
exhaustion for the surrounding entry; reserve large payloads before constructing
them. The interpreter cannot preempt a blocking, looping or arbitrarily allocating
native callback, and it does not roll back host side effects. When no interpreter
scope is active, these host data construction/accounting helpers do not establish
a quota by themselves.

`PollPoint::Macroexpansion` charges every macro invocation through this same
accounting boundary. Macro bodies, callbacks, result conversion and generated
analysis share the outer budget. Language printing, captures and explicit host
streams use these same accounting methods rather than per-call budgets.

`tests/interpreter_limits.rs` adapts pinned SCI interruption assertions and adds
native contract tests for source attribution, reentry, ignored terminal errors,
independent fuel/allocation thresholds, scalar allocation, GC without refunds,
reader/analysis boundaries, native argument admission, output reservation and
bounded cleanup. Existing successful corpus results and effect order remain
unchanged. These host policy tests do not add default-context oracle rows.


## User and host macro expansion

`defmacro` supports fixed, multiple and variadic parameter vectors, including ordinary destructuring,
optional documentation and attribute maps, lexical captures, and implicit
`&form`/`&env` parameters. Macros can construct their results with ordinary
list/quote/vector/map operations. The native `defmacro`
expander lowers directly to `def` and `clojure.core/fn`; its exposed expansion shape differs
from SCI's bootstrap through `defn`, while definition behavior is compared to SCI.

Call dispatch gives primitive specials precedence, then lexical bindings, then
macro Vars. Qualified macro calls bypass an unqualified local of the same name.
The raw `fn*` spelling can itself be lexically shadowed, matching SCI. A bare
macro Var in value position fails analysis; Var quote and explicit dereference
still expose its root. `(def ^:macro m (fn* [&form &env x] ...))` works as well
as `defmacro`. Embedders may mark an existing Var with `set_var_macro` or install
a `NativeDefinition` using `register_macro`. Native arities count `&form` and
`&env` in addition to raw source arguments; both macro kinds use the ordinary
checked invocation ABI, including owner checks, GC/reentry and terminal errors.

`&form` is the complete unevaluated call with user metadata and source line/column.
`&env` is a map of currently visible lexical symbols to opaque generated symbols;
it contains no runtime local values, compiler objects or stable public slot IDs.
Top-level invocation receives an empty map. Parameter/self bindings and earlier
sequential let bindings are visible; later bindings are absent. Generated symbol
spellings and map traversal order are not compatibility promises.

Both `(macroexpand-1 form)` and `(macroexpand form)` also accept `(env form)`.
The explicit map merges over the current macro invocation's environment; keys
mapped to nil still suppress macros. A runtime call outside macro execution
starts with an empty environment, including when it occurs inside a lexical let.
Only the outer form is expanded. Repeated expansion stops on identity, so a macro
returning its own `&form` terminates in `macroexpand`; analyzing such a recursively
expanding call eventually reaches the depth/fuel limit. The host APIs take owned
data plus an optional environment and reject foreign-owner inputs.

Expansion executes during analysis, including in unused branches and function
bodies. It is never cached by input text. Reanalyzing the same `Form` repeats
macro effects; executing a retained `AnalyzedForm` does not. Generated top-level
`do` forms return to incremental evaluation, allowing emitted definitions,
macro definitions and `in-ns` calls to affect later children. Explicit
`analyze_form` continues to analyze its whole expression before execution.

Macro result values become forms directly, with no printer or parser round trip.
New forms inherit the invocation's source span and originating macro name;
unchanged heap-backed subforms passed through the macro retain their original
span/origin. Invocation failures use `EvalPhase::Macroexpansion` at the call site,
with macro-body failure locations retained in frames. Malformed expanded syntax
and unresolved generated symbols fail Analysis at their attributed locations.
The pinned SCI transport labels untagged macro arity/body failures Execution;
native tests compare their categories/effects and separately assert this explicit
phase difference. The oracle driver recognizes SCI's exact macro-value rejection
message as Analysis, reflecting its actual analyzer origin.

Macro execution and result conversion share the existing fuel, allocation,
cancellation and call-depth controls. Expansion chains, recursive analysis and
incremental generated top-level forms also obey the configured evaluation-depth
cap (at most 128). Host callbacks remain cooperative. Tests sweep every small
fuel/allocation boundary, exercise recursive result shapes and reset during host
macro invocation, and preserve terminal failures.

`set_macro_diagnostics(true)` enables `macro_expansion_stats()`, aggregated by
qualified macro name and originating definition. Counts include admitted user/native invocations; generated
form counts include metadata nodes actually lowered for analysis. Macroexpand-only
results are data and do not add analyzed form nodes. Elapsed time includes macro
invocation and successful form conversion, with nested macro time included in its
caller. Disabling diagnostics skips clock reads and report updates; reset clears
reports. `set_diagnostics(true)` additionally attributes repeated analysis and
generated expression/function counts to the source form. Reports use owned
snapshots without charging language allocation budgets or retaining interpreter
roots. See [measurement and diagnostic details](../benchmarks/interpreter.md).
These are attribution counters, not a performance claim or a cache.


## Context-aware syntax quote

Backticks lower the existing parser nodes directly into ordinary owned forms.
Generated forms use `sequence`, `seq`, `concat`, `list`, `vec`, `apply`, map/set
constructors and `with-meta`; no source printing/reparsing or alternate parser is
involved. Qualification happens when each form is read, using the current
namespace, explicit refers, core Vars and reserved core macro symbols. Reader
cursors share the context's live namespace registry, so top-level namespace and
alias changes affect subsequent reads. A completed reading retains its resolved
names. Locals do not affect syntax-quote qualification; use unquote for locals.
Default JVM class spellings such as `Exception` are qualified as symbol data only;
this does not introduce Java classes or interop.

`alias`/`ns-unalias` and embedding `alias_namespace`/`unalias_namespace` manage
qualifiers for existing namespaces. A real namespace name takes precedence over
an alias during reader resolution. Replacing a conflicting alias requires first
removing it. These APIs neither discover nor load a namespace; namespace loading
and the `ns`/`require` macros remain outside this implementation.

Each backtick reads/expands its nested backticks before creating its own gensym
table. Repeated full spellings such as `x#` share one unqualified generated symbol;
`x#` and `other/x#` are separate keys. Independent quote scopes/readings have fresh
names, including across contexts and reset. Reexecuting the same analyzed quote
reuses that reading's symbols, as does a retained macro definition. Unquote
expressions execute once per occurrence in source order. Qualified unquote marker
lists have the same meaning as `~`/`~@`; an unqualified `(unquote ...)` inside a
quote is ordinary data. Outside syntax quote, those reader prefixes produce their
ordinary core call forms. `#=` remains disabled in every context.

Lists, vectors, maps and sets retain their result categories. Spliced map/set
values are supplied to ordinary constructors, which accept duplicates just as
SCI does. Nonseqable splices fail when demanded: `first` of `` `(1 ~@7) `` succeeds,
while `next`, `count`, equality with `[1]`, or full realization fails. All unquoted
operands run before collection assembly; metadata expressions run afterward.
Explicit metadata is syntax quoted, including its symbols/unquotes. Metadata
containing only reader location keys is omitted; user metadata on lists retains
its source line/column. Metadata attached to an unquote marker used as a
collection member is bypassed, matching the reader's marker extraction.

The supporting `concat` implementation accepts seqable values, retains its inputs
in traced deferred nodes, and demands one head at a time. It preserves its
0/1/2/variadic sequence-category distinctions and permits unused invalid tails.
Existing strings, vectors, maps and sets are converted to finite sequence snapshots
when demanded. Syntax-quoted lists can therefore require mutable Context
realization before their elements are available through immutable handles.
`sequence` currently supports its one-argument collection arity; transducer arities
remain excluded. `array-map` has the constructor's value/arity behavior using the
existing hash-trie map storage, so it makes no small-map iteration-order promise.
`with-meta` exposes the existing owned metadata operation for eligible values.
Host `count()`/`inspect()` expose stored finite sizes, not language realization.
Use mutable Context sequence APIs, language `count`, and language equality to
observe deferred concatenation values and failures.

The 75 `sci-syntax-quote-*` fixtures include thirteen direct adaptations from
`syntax-quote-test`, `meta-on-syntax-quote-test`, and `resolve-unquote`. Every typed
result, error phase and completed effect matches the pinned SCI oracle (681 total
rows). The driver recognizes SCI's exact unquote-splice-outside-collection reader
error because the upstream raw exception omits its read phase. Native tests also
verify source/closure/deferred-tail ownership, live readers, macro call/body
provenance, allocation exhaustion and reclamation. Generated reader forms carry
`Origin::SyntaxQuote`; unquoted source expressions keep their own spans, and
macro diagnostics retain their invocation origin and expansion-body frames.


## Ordinary binding and function source forms

`let`, `loop`, `fn`, `defn`, `defn-` and `letfn` are registered core macro Vars.
They expand owned form data into the existing `let*`, `loop*`, `fn*`, `letfn*`,
`def`, `if` and `throw` execution path. Native macroexpand results can be passed
directly through `form_from_data` and executed. No expansion is printed or reparsed,
and the compiler's separate macro limitations remain unchanged.

Sequential patterns support nested vectors/maps, missing positions as nil, `&`
rest patterns and final `:as`. Associative patterns support explicit computed
lookup keys, `:keys`, `:strs`, `:syms`, namespaced keys/directives, `:as`, and
`:or` defaults. Defaults can use local binding names or lookup keys, as in the
pin; conflicting defaults for the same lookup fail. Qualified named bindings
inside a destructuring pattern become unqualified locals. An initializer is
evaluated once, and every earlier binding is visible to later initializers.
Source maps retain their ordered children through expansion, so explicit map
binding defaults can refer to earlier bindings as well as `:keys` siblings.
Host-created map data continues to have unspecified traversal order.

Ordinary `:or` defaults are eager arguments to `get`, so their expressions run
once even for a present key. Present nil and false are preserved; only absence
selects the default result. Unused default entries are neither analyzed nor
executed. Map destructuring converts sequence inputs with
`seq-to-map-for-destructuring`: alternating pairs accept duplicate keys, an
optional trailing map overrides earlier entries, a singleton yields its element,
and empty sequences produce an empty map. Trailing nil contributes no entries. The trailing value also accepts a vector pair,
map entry, or sequence of map entries through ordinary map `conj` semantics.
`:as` sees that converted value. Vectors retain ordinary indexed lookup behavior.
These semantics also support variadic keyword arguments to functions and macros.

A destructured `loop` establishes one recur slot per original binding, performs
its initial bindings sequentially, and destructures again in each iteration.
Initial defaults therefore run both during outer binding initialization and at
the first loop body entry, matching SCI. Function methods likewise recur over
original parameters and then reapply destructuring. Named `fn` captures its own
closure identity; `defn` emits an anonymous function and recursion resolves its
Var, including after that Var is rebound. Its `&env` therefore has no synthetic
binding for the function name. `letfn` uses the traced primitive closure group for
mutual recursion and escaping functions. Its expansion shape intentionally differs
from SCI's local Var bootstrap while ordinary calls and captures agree.

`defn` supports docstrings, symbol metadata, leading attribute maps, trailing
attribute maps on multiple arities, and quoted `:arglists`. Attribute expressions
run as definition metadata, with the pinned override order. `defn-` supplies
`:private true`; normal definition attributes can override it. Macro function
parameters also pass through `fn`, preserving their implicit `&form` and `&env`.

A leading body condition map is recognized only when another body expression
follows; a sole map is the return value. Otherwise, parameter-vector metadata may
supply `:pre` and `:post`. Destructuring precedes preconditions, followed by the
body once, then postconditions with `%` bound to its result. Preconditions preserve
tail recur; a postcondition's result binding makes body recur non-tail. Conditions
lower to the one-argument `assert` macro and portable `AssertionError.` constructor.
The assertion category is caught by `AssertionError`/`java.lang.AssertionError` or
portable `Throwable`, but not `Exception`. Assertions are always enabled in this
profile and use the fixed message `Assert failed`. Both assert arities work: the optional
message expression is analyzed normally and evaluated once only after a failed test;
its effects/errors are preserved, but its value and the printed test expression are
not interpolated into the fixed message. The dynamic `*assert*` switch is not implemented.
This introduces no Java class access.

Malformed declarations fail explicitly before binding initializers execute.
Expansion validation uses the dedicated Macroexpansion phase; errors in expanded
primitive syntax still use Analysis, and runtime lookup/assertion errors use
Execution. This profile rejects dangling/repeated `&`, extra function parameters
after rest, and extra vector forms after `:as`, even where pinned SCI's raw forms
permissively ignore them. SCI extensions `:keys!`/`:strs!`/`:syms!`, `&` inside
map directive vectors, `:select`, `:all`, `:excess`, and `:missing` are explicitly
unsupported. General function-form metadata remains unsupported.

Expansion work uses the shared fuel/allocation/depth/cancellation accounting and
macro diagnostics. Repeated execution of analyzed code performs no expansion.
Original initializer subforms retain their spans; generated lookups and assertions
carry the binding/function invocation and macro origin. Tests cover retained code,
mutually recursive cycle reclamation, every small resource boundary and 100,000
iterations of a destructured loop. The 114 `sci-binding-*` cases (113 new plus the
original binding-order case) match pinned values, errors and effects; all **794**
oracle rows regenerated successfully on 2026-10-08. Provenance anchors and the
assertion-message/expansion-shape boundaries are recorded in the reference corpus.


## Conditional, threading and case macros

The registered core macro Vars are `and`, `or`, `if-not`, `when`, `when-not`,
`if-let`, `when-let`, `if-some`, `when-some`, `when-first`, `cond`, `condp`, `->`,
`->>`, `as->`, `cond->`, `cond->>`, `some->`, `some->>`, `comment`, `assert` and
`case`. They construct owned data for the existing macro analyzer, indexed local
bindings and primitives; no generated source is printed or parsed.

`and`/`or` return the original selected value and evaluate each admitted operand
once. Only nil and false are false; zero and empty strings are true. Conditional
bindings support ordinary destructuring and confine the binding to the selected
body. `if-some`/`when-some` and `some->`/`some->>` distinguish nil from false.
`when-first` binds the first element of a nonempty seq, including false or nil.
Conditional threading checks every test in order, even after a false test; nil
threading skips subsequent step expressions after nil. Generated temporaries use
the same fresh identities as the binding macros; runtime locals remain indexed.
`as->` deliberately binds the supplied name, including sequential shadowing.

`condp` evaluates predicate then input once, calls `(predicate test input)` in
clause order, and passes the actual truthy predicate result to a `:>>` function.
Like the pin, a dangling two-form `test :>>` clause returns the keyword on a match.
`if-let` and `if-some` also retain SCI's ignored forms beyond the optional else.
`comment` returns nil without analyzing its body; normal reader validation still
applies. Other skipped branches remain subject to analysis.

`case` evaluates input once and dispatches through an immutable constant-to-branch
map, using the existing checked value equality and hashing independently of any
binding of `=`. Symbols, keywords, nil, booleans, supported numbers, UTF-16 strings
and characters, and nested collection data are constants; they are never evaluated.
List/seq tests denote groups, including quote-shaped lists; empty groups discard
their result. Duplicate constants, including duplicates within groups and equal
vector/list values, fail during macro expansion before input execution. Integers
and doubles remain distinct under the common equality contract. The optional
unpaired final form is the default; no match without a default raises the catchable
`NoMatchingClause` language error (portable Exception/Throwable category).

The analyzer checks input, then default, then each expanded constant's result
in source order, including repeated analysis of grouped results. This matches the
SCI cases backed by its small array map. Above eight expanded constants SCI may
analyze results in host hash-map traversal order; this profile deliberately keeps
source order rather than copying JVM map layout, so analysis-time macro effects
may differ for those wider cases. Runtime evaluates only the selected result.
Branches/default preserve tail recur. Constants and the dispatch table
are rooted in owned executable units, so returned closures and analyzed forms
survive source drops and garbage collection without repeating expansion.

The native internal primitive is `(case* input [constant result ...] default?)`;
it deliberately does not reproduce the JVM case* shift/mask/hash-switch layout.
`macroexpand` output remains directly executable via `form_from_data`. Raw malformed
case* data is rejected explicitly. Conditional binding vectors require exactly two
forms, including `when-first`; SCI's when-first helper is more permissive about
extra/nonvector binding declarations. Malformed conditional threading clauses use
an explicit macro diagnostic instead of a host AssertionError. Assertion message
formatting and the always-enabled policy are described above; no Java exception
class access is introduced.

All generated work uses the common fuel, allocation and depth accounting and
optional per-macro attribution. Source subforms retain their spans; generated
operations use their invocation's location and macro origin. The 132 pinned
`sci-control-*` rows cover values, errors and completed effects. Additional native
tests cover malformed clauses/arity, source attribution, macro registration and
owned expansion, retained case closures, wide-case analysis order, 100,000
case/recur iterations, and small
resource thresholds. Native macro validation uses Macroexpansion even when SCI's
untagged error is transported as Execution; this known phase difference is explicit.

## Delays and memoized lazy producers

`delay`, `delay?`, `force`, `deref`, `lazy-seq` and `realized?` execute natively.
Their bodies are analyzed once as zero-argument closures. Creation captures
lexical values and code; Var reads go through the active context at demand time.
There is no creation-time snapshot of Var roots or dynamic bindings. Producers
observe the active dynamic scope when realized, then retain their memoized result.
`force` passes non-delay values through unchanged, including an unrealized lazy
sequence. Nil and false results are successfully memoized.

`seq`, `first`, `rest`, `next`, `nth`, `count`, `reduce`, finite eager consumers,
variadic `apply`, lazy `concat` and entry projections consume producer nodes
incrementally. `cons` preserves an unrealized lazy tail; `conj` on a LazySeq
establishes its sequence, as in SCI. An early `reduced` result stops before the
next tail is demanded. Equality compares heads in order and stops on the first
mismatch. Hashing a lazy collection key demands its finite contents; the existing
hash-trie versus JVM small-array-map timing difference also applies here. An
infinite key, count, or full conversion needs host fuel/allocation/cancellation
limits. The higher-order lazy core surface is described in the next section.

Realization has explicit unrealized, evaluating, realized and failed states.
Successful lazy body results also have an internal produced-but-not-yet-sequenced
state, needed when one lazy wrapper unwraps another. Pinned SCI on Clojure 1.12.4
has these observable behaviors:

- A delay caches a successful value or a failure. A failed delay reports realized
  and rethrows the retained exception, including its identity and source frames.
- Direct `lazy-seq` syntax consumes its body once. A body exception or invalid
  sequence result fails the first demand and leaves `realized?` false; the next
  demand yields nil and makes it true. `macroexpand` exposes a constructor whose
  closure retries body failures, matching SCI's distinct expansion replay path.
  Successful bodies are shared in both paths; conversion failures consume their
  produced result rather than reinvoking the body.
- Unwrapping an inner lazy value realizes the outer wrapper while leaving the
  inner's `realized?` false until it is independently demanded. A produced invalid
  inner result still fails conversion when that inner value is demanded later.
- Lazy self-return and mutual return cycles resolve to empty. Reentrant demand
  during a direct lazy body sees empty; its outer invocation can then supply a
  result. Delay forcing is reentrant and the outer successful invocation wins.
  Unbounded delay recursion stops at the ordinary call-depth limit.
- Terminal cancellation propagates through cleanup and bypasses language catches.
  A delay retains that failure. A direct lazy body becomes empty on a later
  admitted evaluation, as in pinned SCI interruption probes. Cancellation before
  invoking a body leaves it unconsumed. Active-scope terminal errors stay latched.

No store borrow crosses producer invocation. Deferred heap records retain raw
closure/result/exception/source IDs, never public rooted handles. Completed
realization releases the producer and its captures. Lazy Cons tails point forward;
walking and discarding prefixes allows collection of their old nodes. Consecutive
lazy wrappers are unwrapped iteratively; 20,000 wrappers and 50,000 discarded
infinite-sequence elements have native bounded-stack/storage tests. Cycles through
Vars, results and cached exception data are collected by the same tracing heap.
The collector retains reusable arena capacity; it does not promise RSS shrinkage.

For embedding, `ValueView::Delay` and `ValueView::LazySeq` do not execute code or
invent a finite length. `RootedValue::deferred_status` inspects realization state.
`Context::force`, `Context::seq`, `Context::sequence_elements` and
`Context::value_eq` supply the mutable evaluation boundary. Immutable handle
`count()` returns no stored length for these values, and fallible element/equality/
hash operations report `RequiresRealization` when code would need to run. Already
realized finite sequences remain inspectable after context drop/reset; forcing an
old-owner value through a reset or foreign context is rejected. Runtime internal
constructor names used in macro expansions are not a public SCI namespace promise.

`tests/interpreter_deferred.rs` compares the deferred fixture group with the pinned
oracle and separately checks ownership, expansion replay, cached failures, cycles,
resource termination and live-storage bounds. The focused SCI source/test anchors
and exact adaptations are recorded in the reference manifest.


## Core lazy transformations, producers and traversal

The core registry implements `map`, `map-indexed`, `mapcat`, `filter`, `remove`,
`keep`, `keep-indexed`, `take`, `drop`, `take-while`, `drop-while`, `concat`,
`range`, `repeat`, `repeatedly`, `iterate`, `cycle`, `partition`, `partition-all`,
`interpose`, `interleave`, `take-nth`, `doall` and `dorun`. Traversal also includes
`second`, `ffirst`, `nfirst`, `fnext`, `nnext`, `last`, `butlast`, `nthnext`,
`nthrest`, `split-at` and `split-with`. Collection arities are supported;
transducer arities remain excluded.

Each pending node captures traced values and invokes callbacks through the common
Context evaluator. Realized heads are memoized exactly once. Infinite ranges,
repeats, iterations and cycles allocate only demanded prefixes; take, short-circuit
search, reduced reduction, equality mismatch and destructuring can terminate
without exhausting the source. Multi-input map/interleave stop at the shortest
input, including the fixed-arity versus variadic input-establishment order of
the pinned host. Applied concat retains its argument sequence after the host's
bounded lookahead, so finite prefixes of infinitely many concatenated collections
also work. All producer loops and consumers share fuel, cancellation, allocation
and call-depth checks; doall/dorun/count/into/reduce never bypass those checks.
Materializing an infinite sequence requires a finite bound or configured limit.

Native sequences are deliberately **unchunked**. In particular, first of map over
a vector runs one callback, whereas the JVM can execute an entire input chunk.
Chunk classes, chunk APIs and chunk-dependent effect/exception traces are excluded.
The fixture effect comparisons use unchunked lists or explicit lazy inputs.
Host concrete Range/Repeat/Iterate classes, their constant-time count/storage
optimizations, and their IPending/realized? distinctions are not represented:
these native producers use the same deferred sequence representation and immutable
handles do not report a fabricated finite length. Fully traversed nodes remain
inspectable after context detachment; unrealized nodes require their Context.

Non-chunk lookahead remains observable and matches the reference. `mapcat`
establishes up to four mapped inputs while constructing its result through the
host-style apply/concat boundary. `cycle` establishes its source head at
construction. Partition computes its next starting sequence before returning a
group, so it can realize one element beyond that starting offset. Both partition
and partition-all realize the complete current group before computing that offset. `dorun`/`doall` establish the source even for a
nonpositive count and establish the following link after consuming the bounded
prefix. `doall` returns the original input; `dorun` returns nil. `butlast` is eager.

Counts retain the pinned arithmetic behavior: take/drop/repeatedly and traversal
counts decrement numerically, so positive fractional counts round up through
iteration; repeat converts its count to a checked long, truncating fractions.
Partition checks exact integer equality with its realized group length (2.0 is
not equal to integer 2). Nonpositive sizes/steps can produce repeated empty groups,
which still obey consumer budgets. Range supports positive, negative and zero
steps, ratios and doubles; zero step repeats the start when bounds differ.
NaN/infinite double bounds and steps follow the pinned comparison behavior.
Integer/ratio arithmetic retains the existing bounded number profile: no BigInt
promotion occurs, and a demanded out-of-range value fails explicitly. Crossing a
finite exclusive integer endpoint does not overflow if no such value is emitted.

Host once-only callback failures consume the captured inputs of map, indexed
map/keep, filter/remove, take-while, partitions, interleave and take-nth; the next
demand yields empty. Repeatedly/iterate retry their callbacks. Invalid take counts
remain retryable, while invalid take sources become empty. Drop/drop-while and
interpose preserve subsequent type failures without replaying completed callback
effects; JVM NullPointerException messages/classes are not exposed. Concat consumes
its failing current source and permits a subsequent demand to continue with later
sources. Variadic concat groups its initial two inputs, so a failure establishing
the first consumes that pair; mapcat and applied concat use the same grouping. These behaviors are covered by repeated-demand fixtures; callback source
provenance is retained by the common invocation/error path.

`tests/interpreter_lazy.rs` compares 268 `sci-lazy-*` cases against the pinned
reference and adds shared-budget, owner/root, source provenance, GC-in-callback,
unchunked-demand, checked-boundary and bounded live-storage tests. Walking 20,000
discarded map/range nodes has stable live heap/root and host-stack samples.
Two golden cases adapt upstream map and finite range-reduction assertions;
infinite materializer tests adapt the host producer/materializer interrupt tests
with Context resource limits. The manifest and reference README record the pin
and exact anchors.

## Comprehensions and iterative forms

`for`, `doseq`, `dotimes` and `while` are registered native macro Vars. They
construct owned forms for the existing indexed binding/loop/recur primitives;
`for` additionally uses traced lazy closures and Cons/concat tails. There is no
second evaluator or comprehension-specific runtime frame. Generated bindings
are fresh, helper calls are core-qualified, and required expansion work shares
fuel, allocation, depth, cancellation and per-macro attribution.

Comprehension vectors support nested generators, ordinary destructuring and
ordered `:let`, `:when` and `:while` modifiers. Earlier bindings remain visible
to later generator expressions and modifiers. `:when` skips the current element;
`:while` stops only its own generator. An inner `:while` therefore allows the
next outer element to start a new inner traversal. Modifiers execute in source
order, so a skipped `:when` also skips a following `:while` or `:let`.

`for` evaluates its outer input expression once at construction. Establishing
its sequence, inner generator expressions, modifiers and the body are deferred
until demand. Demanded results are memoized; direct lazy-body failures consume
the failing producer as described above. Empty inner results and rejected
candidates advance through a primitive loop, so searching an infinite source
uses bounded stack and remains interruptible. Finite prefixes of infinite
outer/inner generators work with ordinary consumers. As with the lazy core,
sequences are unchunked: first of a comprehension over a vector executes only
the first admitted body, rather than a JVM chunk.

`doseq` walks inputs and executes its body in source order, returning nil for
any declaration containing a generator. It creates no intermediate result
sequence. Pinned SCI's zero-generator exception is retained: `(doseq [] body)`
executes once and returns the final body value, as does a declaration containing
only leading `:let` modifiers. Leading `:when`/`:while` modifiers are also allowed
and return nil. `for` requires at least one generator before any modifiers.

`dotimes` evaluates its bound once and applies the checked `long` conversion;
positive fractional bounds truncate, and nonpositive bounds execute no body.
Indices start at zero. `while` reevaluates its condition before every body.
Both return nil and use primitive loop backedges, including empty bodies.
Unresolved expressions and invalid body recur positions still fail analysis
when runtime input would be empty or the condition false.

Malformed binding/modifier vectors, unknown modifiers and missing generators
produce source-attributed macro validation errors. This is deliberately clearer
than the pin's host NullPointerException/AssertionError behavior on some malformed
forms. Runtime errors in user subforms retain their original source spans.
Retained lazy results own their captures/code without keeping dead evaluation
frames; discarded prefixes and completed effectful iterations release old roots.

`tests/interpreter_iteration.rs` compares 90 `sci-iteration-*` reference rows and
checks owned expansion replay, diagnostics, GC during effects, detached values,
resource exhaustion and terminal cancellation with finally cleanup. Walking
20,000 single/nested comprehension elements and 20,000 doseq/dotimes/while
iterations keeps measured live heap/root counts and host-stack samples stable.
The complete 1,358-row corpus was regenerated against the clean SCI pin and
Clojure 1.12.4 on 2026-10-08; exact upstream adaptations are in the manifest.


## Explicit namespace source loading

`ns`, `require`, `refer`, and `refer-clojure` use the interpreter namespace/Var
registry. A new Context has no source provider and performs no filesystem search.
Hosts install `Context::set_source_provider` with a `SourceProvider` or a closure
receiving `LoadRequest`; its result is `Option<SourceInput>` containing owned
source text and a file identity. Requests identify the requested namespace,
requesting namespace/file and effective `Reload` mode. `None` means absent;
`LoadError::Provider` reports a host failure. Clearing the provider does not clear
already loaded namespaces. Reset clears both the provider and load records.

```rust
use light_cljs::interpreter::{Context, LoadRequest, SourceInput};
let mut context = Context::new();
context.set_source_provider(|request: &LoadRequest| {
    Ok((request.namespace == light_cljs::interpreter::Name::new(None, "example"))
        .then(|| SourceInput::new("example.clj", "(ns example) (def answer 42)")))
});
let answer = context.eval_string("(require '[example :as e]) e/answer").unwrap();
```

`FileSourceProvider::new` is the explicit filesystem adapter for embedding and
native CLI modes. Construction does no I/O. Installed providers search
only supplied roots, translate namespace dots to directories and hyphens to
underscores, and consider `.clj` and `.cljc`. Multiple distinct canonical files,
including both extensions, report `AmbiguousSources`; repeated roots naming the
same canonical file do not create ambiguity. Missing files return None; I/O and
UTF-8 decoding failures are host errors. Invalid path-like namespace names are
rejected. The adapter is not a filesystem sandbox (symlinks follow normal host
rules). Existing compiler CLI modes and the compiler's `.cljs` loader are unchanged.
Reader features remain context controlled and default to `:clj` for both source
extensions. Arbitrary provider callbacks must finish cooperatively: interpreter
limits can check before/after host calls, not preempt host filesystem/callback work.

Supported libspecs are a symbol, a vector `[lib options...]`, and prefix groups
such as `'[example [math :as m] data]`. Options are `:as`, `:as-alias`, `:refer`
(a sequential symbol collection or `:all`), `:rename` (symbol map), `:exclude`,
and boolean `:reload`/`:reload-all`. Global reload flags can occur anywhere in
require's arguments. `:as-alias` creates reader qualification without invoking
the provider; a subsequent normal require still loads the namespace. Macro Vars
are ordinary library Vars: aliases, explicit refers, `:all` and renaming work,
and following top-level forms can immediately expand them. Refer's filters also
accept `:only`. `:refer-clojure` supports `:exclude`, `:rename`, and SCI's current
`:only` behavior (explicitly refers selected names without removing the implicit
core fallback). Outside an ns clause its symbol vectors/maps must be quoted.
Exclusions affect resolution, namespace views and syntax-quote qualification.

Validation rejects unknown/duplicate options, odd option lists, nonsymbol names,
nonsequential refers, nonsymbol rename maps, conflicting aliases, simultaneous
`:as`/`:as-alias`, and dotted child names in prefix groups before loading that
require call's first dependency. This is deliberately stricter than SCI's
permissive/ignored malformed option cases. `:use`, `:import`, `:gen-class`,
`:require-macros`, `:refer-macros`, string/JavaScript libspecs, verbose flags and
namespace metadata maps are unsupported. Ns accepts an optional docstring but
does not store namespace documentation metadata yet. Ns expands to ordinary
sequential primitives; a nested ns expression changes namespace when executed,
where SCI's special analyzer may perform the namespace switch during analysis.
Top-level ns/source loading follows the SCI fixture contract.

`namespace_load_state` exposes Unloaded, Loading, Loaded or Failed. Dependencies
load sequentially in source order and normal require initializes each once.
An unfinished ns clause requiring an ancestor reports the complete cycle path;
self-require succeeds. As in SCI, a later require after a namespace's ns clauses
have completed may refer back to that namespace even while its remaining source
is executing. A successful provider source must establish the requested namespace.
Failed source evaluations remove that namespace from the registry and leave Failed,
so a later require retries. Completed effects are retained; loading is not a
transaction. Aliases follow a successful replacement namespace, while retained
host Var/namespace handles and old analyzed code keep their original identities.
Caller namespace and `*file*` are restored after success, diagnostics, terminal
limits or host failure. Source diagnostics retain the nested file and caller frames.

`:reload` rereads and executes the requested namespace, reusing dependencies.
`:reload-all` also propagates through its requires. Successful reloads preserve
existing Var identities, update their roots and analyze fresh source/macros;
normal require never replays initialization. Existing analyzed forms/closures
retain their already-expanded macro code but continue reading live Var roots.
As in SCI, definitions absent from the replacement source remain until explicitly
unmapped. Reload of an existing namespace with no available source keeps it.
The built-in `clojure.core` namespace is permanently preloaded: require/reload does
not ask a provider to replace it. This protects the native core registry.

The 51 `sci-loading-*` oracle cases share the explicit in-memory modules in
`tests/reference/sci/load-sources.tsv` between JVM SCI and native tests.
`tests/interpreter_loading.rs` additionally checks filesystem ambiguity, malformed
libspecs, request provenance, failure/retry identities, retained analysis, callback
reset, cancellation, and small resource limits. The complete 1,491-row corpus was
regenerated against the pinned SCI revision and Clojure 1.12.4 on 2026-10-08.


## Dynamic Vars and mutation policy

`^:dynamic` (including evaluated `:dynamic` metadata) enables `binding` and
`with-bindings`. Each context owns separate Var roots and active binding slots;
there is no process-global or Rust thread-local runtime environment. Indexed
Var reads, dereference, Var invocation, macros and deferred producers all consult
the most recent active slot before the root. Nil, false and the unbound sentinel
are valid bound values. `bound?` takes one Var and recognizes either a root or a
binding; `thread-bound?` accepts any number of Vars and checks active slots only.

All `binding` initializers run before installing the scope and see outer values.
Duplicate binding targets evaluate every initializer and keep the last value.
`set!` and `var-set` return the assigned value and change only an existing slot;
they never establish a root. Inherited slots remain shared by nested scopes that
do not rebind the Var. Removing `:dynamic` metadata blocks future bindings but
does not disable an already active slot. Ordinary closures read dynamic values
when invoked; lazy sequences and delays read them when realized. Scope cleanup
runs after language errors, host errors and terminal resource failures, even when
the finalizer budget has been exhausted. `binding` introduces a recur boundary;
a loop inside its body can recur normally. `with-bindings*` accepts a Var-keyed
map, callable and optional arguments; `with-bindings` supplies the body closure.

`with-redefs` and `with-redefs-fn` temporarily replace raw roots while preserving
Var identity and active dynamic bindings. Unbound roots restore as unbound.
The same Var may be redefined recursively; body root changes are discarded when
the scope closes. Matching SCI, duplicate `with-redefs` targets discard earlier
initializer forms during expansion; list bindings and an unmatched trailing
target are accepted. `alter-var-root` passes the raw root plus any extra
arguments to its callable, commits its result and returns it. Callback failures
do not commit a replacement. `alter-meta!` and `reset-meta!` mutate Var metadata
without changing its identity. Metadata may be a map or nil; reset fully replaces
it. Source `def` replaces metadata as before. JVM SCI's unusual `set!` operand
handling is preserved: a missing value assigns nil and extra operands are ignored.

Builtin roots and metadata are read-only to interpreted code by default. Dynamic
builtin slots can still be bound or assigned. `allow_builtin_var_mutation(true)`
is an explicit host permission limited to that context; reset clears it and
independent contexts never inherit it. `alter-var-root` evaluates its callback
before checking the write, while `alter-meta!` checks before invoking the callback,
matching the SCI pin. A user Var marked `:sci/built-in true` gains the same guard.
Actual builtin identity remains protected even if a permitted metadata reset
removes that marker. Trusted embedding installation APIs `register_native`,
`set_var_root` and `set_var_macro` explicitly install host-supplied roots/macros;
they remain separate from permission-checked language mutation. Immutable native
arity/effect metadata continues to describe the actual native callback.

`*ns*` and `*file*` use these ordinary dynamic slots. Name resolution, reader
qualification and namespace introspection use active `*ns*`; `in-ns` changes that
slot when bound. A definition still targets the namespace chosen at analysis,
as in SCI. Namespace slots require Namespace values, an explicit native validation
boundary. `eval_string` inherits the active file binding (nil by default), including
Var `:file` metadata. String evaluations establish a fresh namespace slot; nested
evaluation restores its caller, while outermost Context evaluation retains the
final namespace for subsequent REPL calls (an explicit embedding difference from
SCI’s external namespace binding). Named `eval_source` and required module sources establish a
file scope and restore the previous slot. I/O and printer controls use the same
dynamic binding API and restoration guards.

Embedding `var_root` remains a raw-root read; `var_get` reads the effective value.
`bind_vars` validates all owner/Var/value pairs before installing them and returns
an owning `BindingGuard`. Dropping guards out of order safely removes only their
own slots. Guards keep unmapped Vars/values alive and restore their original owner
even after `Context::reset` or a Rust panic. `set_var_binding` assigns an active
slot. `with_var_redefs` brackets a Rust closure with raw-root restoration, and
`reset_var_metadata` applies the ordinary permission and owner checks. These APIs
hold no store borrow across host callbacks. Bindings, metadata, saved roots and
Var cycles participate in tracing; cleanup itself requires no budget allocation.

The implementation does not expose unmanaged push/pop thread bindings, binding
conveyors, bound-fn, with-local-vars, Var watches, concurrency or STM. Macroexpand
returns executable owned forms using the native scoped-call primitives; it does
not promise SCI's exact push/pop expansion structure. Malformed `binding` syntax
reports Macroexpansion/Analysis diagnostics rather than SCI's unclassified host
ExceptionInfo. Printed/inspected Var names remain their stable interned namespace
and name when arbitrary metadata changes `:name` or `:ns`.

`tests/interpreter_vars.rs` covers 74 pinned SCI assertions plus native guard,
owner/reset, reader/callback/macro reentry, GC cycles, file metadata, allocation,
fuel, and call-depth cases. Builtin metadata is persistent runtime data; GC tests
compare reclaimed user data against a warmed builtin baseline. The complete
**1,565-row** reference corpus was regenerated against the clean pinned checkout.

## Atoms and volatiles

`atom`, `deref`/`@`, `reset!`, `swap!`, `compare-and-set!`, `reset-vals!`,
`swap-vals!`, `add-watch`, `remove-watch`, `get-validator` and `set-validator!`
operate on traced atom identities. `volatile!`, `volatile?`, `vreset!` and
`vswap!` provide simpler mutable cells. These are single-threaded operations;
there is no cross-thread synchronization, transaction or STM guarantee. New
contexts have independent state. Foreign-owner cells and callback results are
rejected by the same embedding boundary as other runtime values.

Atoms and volatiles compare and hash by their cell identities, independently of
current contents or metadata. CAS compares the expected value with stored
representation identity, not structural equality: an equal separately allocated
vector or string does not match. Immediate native scalars compare their exact
representation, including double bits, as with `identical?`; JVM boxed-number
caches and allocation-dependent scalar identity are deliberately not reproduced.
A cell can be its own value or map key without recursing through its contents.

`atom` accepts key/value options `:meta` and `:validator`; duplicate options use
the last value and unknown options are ignored. The pinned setup function does
not accept a single options map. Nil/false options are ignored. Atom metadata
supports `meta`, `reset-meta!` and `alter-meta!` using the existing common APIs;
it is map/nil data, and changing it invokes neither validators nor watches.
`with-meta` does not copy a cell. Volatiles have no metadata, validator or watch
operations. `get-validator`/`set-validator!` extend the pin's default exported
SCI surface with Clojure's corresponding ARef behavior. Their reference profile
explicitly installs the Clojure 1.12.4 host functions into SCI.

Validation runs on the initial state, on every proposed mutation, and before
installing a replacement validator against the current state. A false/nil result
raises the catchable `Invalid reference state` error. Rejection or a callback
error prevents that operation's commit; effects already performed by callbacks
remain. Failed validator installation preserves the previous validator. Thrown
`ex-info` retains identity/data/cause. A portable checked `Exception.` from a
validator is wrapped with `Invalid reference state` and its original cause;
ordinary runtime errors and already wrapped failures pass through unchanged.
Watch and swap-function errors preserve their original exception identities.

Swaps pass the current value and extra arguments through the shared invocation
path, validate the result, then retry when callback reentry changed the stored
identity. An identity change followed by restoration does not force a retry.
CAS validates its proposal even when the expected value does not match. `reset!`
captures its old value before validation, while `reset-vals!` captures it after
validation; validator reentry makes that distinction observable. The `*-vals!`
functions return `[old new]` for their own committed transition even when a watch
subsequently changes the atom again. Pinned SCI's `vswap!` is a macro that
evaluates its target expression twice: target, function, target for dereference,
then extra arguments. It writes the result to the first target and never retries.
Use a stable local target when those repeated effects are undesirable.

Watches receive key, atom, old value and new value after commit, including an
identity-preserving update. Registration replaces a structurally equal key;
removal and registration return the atom. Nil keys and nil callbacks are accepted;
nil callbacks are skipped. Notification uses a watch-map snapshot taken after
validation/commit, so watches added by a validator run immediately and edits by
a watch affect subsequent notifications. Multiple-watch iteration order is
unspecified, as with hash-map traversal. Watch registration/removal captures its
map before forcing a lazy key; a successful outer update overwrites watch-map
edits made by key reentry. Removal from an empty or nil-key-only map does not force an
unrelated key. An error stops the remaining callbacks
without rolling back state. Callbacks can collect garbage or reenter evaluation:
no store borrow crosses invocation, and atom, old/new values, validator and watch
snapshots remain rooted. Fuel/allocation/depth/cancellation limits share the outer
evaluation; an exhausted watch leaves its already committed transition intact.

`Context::atom` and `Context::volatile` construct owner-checked cells;
`RootedValue::state_value` takes a rooted snapshot without evaluation. Snapshots,
cell metadata and stored values survive context reset/drop, while further
language mutation requires the original owning context. All cell edges use
unrooted arena IDs, allowing cycles through closures, metadata, watches,
validators, other cells and collections to be reclaimed after roots disappear.
Tests collect such cycles and keep live heap/root counts bounded across 2,000
updates. Native callbacks that reset their Context fail the old operation safely.

`tests/interpreter_state.rs` checks 94 pinned reference cases plus native
ownership, callback reentry/GC, retained source diagnostics, checked exception
wrapping, bounded storage and shared resource controls. Upstream atom metadata
and vswap assertions retain exact anchors in the provenance manifest. The
complete **1,659-row** corpus was regenerated against the clean pinned SCI
checkout and Clojure 1.12.4 on 2026-10-08.

## Readable printing and literal string operations

`printing_definitions()` exposes `pr-str`, `prn-str` and `str`.
`Context::print_value(&value, PrintOptions)` uses the same printer and returns an
owned UTF-16 string. `PrintOptions` selects readable output, metadata, collection
length and nesting level. Language printing reads the ordinary dynamic Vars
`*print-length*`, `*print-level*`, `*print-meta*` and `*print-readably*`, so nested
bindings, reentry and cleanup have the existing Var semantics. `prn-str` appends
one LF; multiple pr/prn arguments have spaces between them. `str` concatenates,
uses an empty string for nil and literal string/character units, and prints
collection members readably. As in Clojure, `str` ignores the print bindings.

Length limits apply independently to each collection (a map entry counts as
one); level zero prints a collection as `#`. Zero length preserves empty
collections and prints `...` inside nonempty ones. Negative or fractional lengths
do not truncate; negative levels act as zero. Nil/false disables either limit.
Integral floating lengths and fractional levels follow the pinned host's numeric
counter behavior. Other types fail explicitly. Metadata prints only in readable
mode and never changes the value, equality or hash. A sole truthy `:tag` uses the
reader's `^tag` shorthand. Map/set order follows native unordered iteration;
namespace-map abbreviation and JVM-specific float typography are not promised.
Finite doubles retain their type and exact IEEE bits through a print/read cycle,
including signed zero; NaN payload bits are not a readable representation.

Readable strings escape quotes, backslashes, standard named controls, remaining
C0 controls, DEL and **unpaired** surrogate units. Valid surrogate pairs remain
literal pairs. The additional `\uXXXX` escapes for controls/lone surrogates differ
from JVM raw-unit printing but produce valid UTF-8 source without replacement.
Reader-representable finite data round-trips through `FormReader` with its value
and scalar/collection category preserved. Sequence views print as lists, retaining
sequence equality rather than their internal view category. Arbitrary host-created
symbol/keyword names need not have legal reader spellings. A host-only surrogate
character prints explicitly as `\uXXXX`; the existing UTF-8 parser, like SCI's
escaped-character reader, rejects that character literal. Use a UTF-16 string for
lossless source interchange of such units. Raw `str` never replaces a surrogate.

Functions, namespaces, unbound values, delays, atoms, volatiles, reduced markers
and exceptions have documented opaque `#<...>` representations; Vars print
`#'namespace/name`. These are display values, not identity serialization. State
objects include a snapshot of their contents; a recursive edge prints `#<cycle>`.
Repeated shared values on separate branches are printed normally. Delays report
status without executing their body. Lazy sequences execute under the current
context and print settings; length truncation demands one extra member to detect
`...`. A level cutoff does not demand the hidden collection. Infinite sequences
need a finite print length or a configured fuel/allocation/output budget or
cancellation, as do other unbounded consumers. Recursive printer traversal also
honors the common evaluation-depth cap before growing the Rust stack.

Every appended printer unit reserves allocation and output **before** growing its
buffer. Output accounting uses two bytes per UTF-16 unit, including unmatched
surrogates. String operations reserve temporary and returned storage, and their
scans/callbacks poll common fuel and cancellation. Stringification inside join,
escape and case coercion uses the common printer and its output accounting. All
public/native entry points share the active budget; host callbacks cannot reset
it. Explicit output sinks are described below; no implicit filesystem/process
capability is installed.

`string_core_definitions()` adds `subs`, `name`, `namespace`, `symbol`, `keyword`
and `gensym`. Indices count UTF-16 units; slices may split a surrogate pair.
Numeric indices use JVM-style i32 narrowing/truncation; invalid substring bounds
return `NativeError::StringBounds` with start, end and length, while wrong types
retain the offending argument index. Names retain exact UTF-16 components, split
one-argument strings at the first slash (except `/`), and intern in the context.
`symbol` also accepts keywords and Vars; one-argument `keyword` returns nil for
unsupported types. `gensym` uses a process-wide monotonic counter with a default
`G__` prefix or `str` of the supplied prefix; generated spellings are not stable
across runs.

`clojure.string` is a preloaded native namespace. Qualified calls, require aliases,
refers/renames and namespace introspection work without a source provider. Its
built-in Vars retain normal mutation protection. Reload/reload-all preserve this
native registry, as for clojure.core; the pin's source-less clojure.string reload
can instead fail. `string_definitions()` is the precise supported subset:

- `join` (one/two arguments), `split-lines`, `trim`, `triml`, `trimr`, `trim-newline`,
  `blank?`, `starts-with?`, `ends-with?`, `includes?`;
- `index-of` and `last-index-of` with optional numeric start and string/character
  needle, returning nil when absent;
- `replace` and `replace-first` for literal string/string and character/character
  operands; empty string matches occur at every UTF-16 boundary;
- `upper-case`, `lower-case`, `capitalize`, `reverse` and `escape`.

Split-lines recognizes LF and CRLF, drops trailing empty lines and returns `[""]`
for empty input. Trim/blank use the Java whitespace set recorded by the fixtures
(including U+2003, excluding NBSP/NUL); trim-newline removes only trailing CR/LF.
Reverse preserves existing surrogate pairs. Escape invokes its map/set/function
once per UTF-16 character; nil/false keeps that unit and truthy results use `str`.
Literal replacement treats `$` and backslashes literally and rejects replacement
callbacks. Regex values/overloads, regex `split`, `re-quote-replacement`, locale
selection, normalization and grapheme indexing are outside this subset.

Case conversion uses Rust's Unicode tables and full per-scalar mappings, preserves
unpaired surrogates and has no locale dependency. Unicode version, locale rules
and contextual mappings can differ from Java: native lower-case `ΟΣ` is `οσ`,
where Java's contextual final sigma yields `ος`; Turkish locale selection is not
available. Capitalize uppercases the first UTF-16 unit and lowercases the suffix,
matching the host's substring boundary behavior. No third-party dependency or JVM
is used by these operations.


## Explicit host I/O and incremental read APIs

`io_definitions()` supplies `print`, `println`, `pr`, `prn`, `newline`, `flush`,
`read-line`, `read`, `read-string`, `with-in-str` and `with-out-str`. The dynamic
Vars `*in*`, `*out*` and `*err*` default to nil, so a fresh library context cannot
read ambient stdin or write stdout/stderr. `*flush-on-newline*` defaults to true.
No slurp/spit, filesystem, network or process function is installed. The existing
compiler CLI is unchanged; native interpreter CLI modes explicitly supply
streams through this API.

Hosts create opaque owned values with `Context::input_stream(name, reader)`,
`string_input(name, utf16)` and `output_stream(writer)`. `HostInput` supplies
complete UTF-16 lines including terminators, with None for EOF; a final line may
be unterminated. Empty chunks are rejected. Closures implement the input and
output callback traits; `Utf8Input<R: BufRead>` and `Utf8Output<W: Write>` adapt
explicit standard Rust streams. Invalid UTF-8 input and unpaired UTF-16 output
fail without replacement characters. `OutputBuffer` is a cloneable exact UTF-16
capture with `units()` and `take()`; it contains application data, not runtime
roots. Example:

```rust
use light_cljs::interpreter::{Context, OutputBuffer};
let mut context = Context::new();
let captured = OutputBuffer::default();
let out = context.output_stream(captured.clone());
let guard = context.bind_io(None, Some(&out), None)?;
context.eval_string("(println 42)")?;
assert_eq!(captured.units(), "42\n".encode_utf16().collect::<Vec<_>>());
drop(guard); // restores the previous output binding
# Ok::<(), Box<dyn std::error::Error>>(())
```

`bind_io` installs only supplied channels and returns the usual owning
`BindingGuard`. Bindings restore after success, exceptions, cancellation and Rust
unwinding; reset creates fresh unconfigured Vars. Language `(binding [*out* *err*]
...)` redirects output to the explicit error stream. Host errors propagate with
`ErrorClass::Host`, outside language catches. Arbitrary host code remains
cooperative and cannot be preempted; a callback may already have performed an
external effect before returning an error or noticing cancellation. Callback
objects must not capture interpreter handles (which would create host-owned
reference-count cycles); keep application state in them instead.

Print functions reuse the common printer and dynamic readable/length/level/meta
controls. `print`/`println` force non-readable printing, with spaces between
arguments; `pr`/`prn` respect `*print-readably*`. All return nil. A print call renders
and reserves its whole UTF-16 buffer before forwarding it once to its sink, so
formatting/budget failures do not emit a partial buffer. A host write itself can
partially succeed and then fail; it is never retried. This buffered-per-call
behavior differs from JVM streaming printers during lazy realization failures.
`println` and `prn` append LF and flush when `*flush-on-newline*` is truthy;
`newline` appends LF without flushing. LF is portable on all native platforms,
unlike the JVM's platform newline. Output counts two bytes per UTF-16 unit;
string captures do not charge those same output units a second time.

`with-out-str` captures only its body output and returns the exact string,
ignoring the body's return value. `with-in-str` evaluates its string once, binds
an independent cursor and returns its body result (nil for no body). Nested
scopes resume the outer cursor/output, including after failures. Its temporary
input is closed when the scope exits; escaped handles report a closed-stream
host error. `read-line` removes LF, CRLF or lone CR, preserves blank lines and
returns nil repeatedly at EOF. Line and form reads share one cursor, so reading
`1` from `"1\n2"` leaves an empty line for the next `read-line`.

`read` supports arities zero (dynamic input), one (explicit input), two
(`opts input`), three (`input eof-error? eof-value`) and four (the same plus an
ignored recursive flag). `read-string` accepts a string or `opts string` and
reads only the first form. The supported options map contains `:eof` only;
unknown options fail explicitly. Context configuration controls feature and tag
policy, and `#=` always fails. Per-call feature/read-cond overrides, dynamic
reader resolvers and reader-eval overrides are outside this profile. An explicit
EOF value can be nil, false or any other value, including `:eofthrow` as in the
pinned SCI runtime. Missing EOF overrides produce the catchable execution error
`NativeError::EndOfInput`. Literal nil is a successful value, never EOF.
Malformed/incomplete input retains the existing read diagnostic, original source
and read phase; like other native reader diagnostics it is outside language
catches. A form cursor fuses after a read failure, preventing callback replay.

Streaming form reads use the original borrowing `Parser` with syntax lookahead
and the same semantic lowerer. Lookahead can request additional lines for an
incomplete collection or a prefix whose conditional operand vanished; it does
not invoke data readers. Closed collections and selected completed forms do not
wait for a later form. Reader byte/storage, fuel, cancellation and output charges
share the surrounding evaluation scope. Input retains consumed text for stable
source offsets; source snapshots and conversion/probe work are charged before
semantic reading. Hosts can call `compact_input` between operations to release
fully consumed lines while retaining original line/byte-column locations, or
rotate streams/configure allocation limits when retaining history is undesirable.
Raw reader source must be representable as UTF-8, matching the existing front
end; use source escapes for lone surrogates. `read-line` and string capture still
preserve arbitrary UTF-16 units exactly.

Rust callers can keep the borrowing `Context::reader(&Source)` or use
`Context::source_reader(Source)` for an owned `SourceReader`. Both distinguish
`Ok(None)` (EOF), `Ok(Some(form))` (including nil) and `Err(ReadError)`; they fuse
at EOF/error. The owned cursor takes `&Context` on every `read_form` call, observes
namespace/alias/features/tag changes between forms and rejects another/reset
owner. `position()` is the next UTF-8 byte offset. `read_form_with_source` also
returns a `SourceSpan` covering the exact consumed bytes, including leading
whitespace/comments/discards; the ordinary form span identifies the form itself.
Forms, data and captured original spelling survive dropping the cursor, input
or context. Source capture does not print/reparse data or execute it.

`tests/interpreter_io.rs` compares 49 focused SCI fixtures plus prior stdout,
error-ordering and EOF cases, and tests owned source/cursor lifetime, host failure,
flush counts, stream allocation/output budgets, cancellation, exact UTF-16,
reader policies and callback-once behavior. Adapted upstream assertions and exact
anchors are recorded in `tests/reference/fixture-origins.tsv`.

## Set, walk, and EDN data libraries

`clojure.set`, `clojure.walk`, and `clojure.edn` are native namespaces available
in every context. Qualified calls, aliases, `:refer`, namespace introspection,
and host registration through `set_definitions`, `walk_definitions`, and
`edn_definitions` share the native library registry. Like `clojure.string`, these
libraries do not need a source provider and `:reload` does not replace them with
classpath code.

The set surface is `union`, `intersection`, `difference`, `subset?`, `superset?`,
`select`, `rename-keys`, `project`, `index`, and two-/three-argument `join`.
Set algebra accepts sets or nil; nil acts as the empty input while operations
that retain their first/selected input can return nil. Other inputs fail
explicitly, including the otherwise accidental JVM behavior where a one-argument
set function simply returns an arbitrary non-set. Relations may be finite
seqable collections of maps or nil rows. `project` removes duplicate projections;
`index` groups rows by projected key maps; absent keys differ from present nil
values. Natural joins use common keys, mapped joins use a left-key/right-key map,
and joins build an index on the smaller relation. Both original key names are
retained by mapped joins. For conflicting non-join keys the larger relation's
value wins (the right relation wins equal sizes). A natural join stops when
either relation is empty without consuming the rest of the other relation.

`rename-keys` reads values from the original map, so swaps and chains are
simultaneous. Set algebra, select and rename-keys retain the appropriate input
metadata. Union uses the larger set and intersection the smaller set; two-input
ties use the first, variadic ties select the last tied largest/smallest input,
matching Clojure 1.12.4. Projection retains each row’s metadata and original equal key representatives,
using the same select-keys operation as core. Project retains relation metadata; index/join return
fresh results without relation metadata. Map/set iteration order and ambiguous
key-collision order are unspecified. `rename`, `map-invert`, sorted sets and
custom collection implementations are outside this exported surface.

The walk surface is `walk`, `prewalk`, `postwalk`, `prewalk-replace`,
`postwalk-replace`, `keywordize-keys`, and `stringify-keys`. Walk calls `inner`
once on each immediate member, then `outer` on the rebuilt result. Prewalk calls
the function before descending into its returned value; postwalk calls it after
rebuilding children. Maps expose map-entry nodes between the map and each key
and value. Map entries remain entries; lists, vectors, sets and maps keep their
collection types; seqs are realized eagerly, including an empty seq value.
Lists, seqs, vectors, sets and maps retain metadata in the pinned Clojure 1.12.4
manner; map-entry reconstruction has no metadata. Metadata itself is not walked.
Atoms, delays, functions, streams and other opaque identity values are leaves;
walk does not dereference or force them. Infinite seqs and expanding prewalk
replacements need a fuel/allocation limit; the explicit traversal stack has the shared
evaluation-depth guard, including callback reentry.

Replacement maps use key presence, preserving nil/false replacements; nil is an
empty replacement map. Prewalk traverses new replacement children, whereas
postwalk does not. Key conversion is a postwalk on maps only. Keywordize converts
string keys (including namespace components); stringify uses the local name of
keyword keys, dropping their namespace. These two operations rebuild maps and
lose map metadata, while preserving metadata of surrounding other collections.
All callbacks use ordinary invocation, keep live values rooted during collection,
share resource accounting, and stop subsequent callbacks when a failure occurs.
Existing native unchunked lazy-sequence behavior still applies.

EDN is a restricted semantic mode over the same `Parser`, forms, numeric reader,
source storage and incremental input cursor. Lists are returned as data and never
evaluated. It accepts nil/booleans, bounded integers/doubles/ratios, exact UTF-16
string and character literals, symbols/keywords, lists/vectors/maps/sets,
metadata, discards, explicit namespaced maps and symbolic floating values. It
rejects all executable reader prefixes, anonymous functions, regexes, reader
conditionals, auto-resolved keywords/maps, and `#!` dispatch comments. `#=` is
always disabled, including discarded input and calls with custom readers.
Tagged names must immediately follow `#` and begin with a letter. Invalid EDN
constituents such as `a@b`, `a~b` and an embedded backtick fail instead of returning
a partial symbol. The existing front end's identifier and Unicode classification
rules remain in force: apostrophe-prefixed symbols and multi-slash names accepted
by JVM EDN are deliberately rejected rather than interpreted as code-reader
syntax. Other existing parser-profile differences still apply.

`clojure.edn/read-string` reads one form only. Its one-argument form returns nil
for empty/comment/discard-only input; nil or false input returns nil. Its
`opts string` form defaults to an EOF error, overridden only by an explicit
`:eof` value (which may itself be nil, false or `:eofthrow`). Literal nil is never
EOF, and truncated collections fail even with `:eof`. `clojure.edn/read` has
zero, one (`input`) and two (`opts input`) arities. Zero-arity read uses the
explicit dynamic native `*in*`, including `with-in-str`; this intentionally
avoids the pin's JVM host-`*in*` discrepancy. Default contexts expose no ambient
input. Code, EDN and line reads share the input cursor.

Only `:eof`, `:readers` and `:default` are supported options; unknown options,
non-map option dictionaries, and non-symbol reader keys fail explicitly.
This validation is deliberately earlier/stricter than JVM EDN's incidental
handling of malformed options. `:readers` is a map of tag symbols to callables
receiving the already-read data. A nil mapping falls through to `:default`,
which receives tag and data. These options are local to the call/reader and do
not inherit code-reader handlers, namespace aliases, features or dynamic reader
configuration. Discarded tags still invoke their explicitly supplied handlers.
Missing handlers, unsupported numeric extensions/overflow and unsupported host
results have explicit diagnostics. Built-in `#inst`/`#uuid` objects are not
provided; an explicit reader or fallback can map such tags to supported data.
Tag results must recursively contain supported eager data, not functions,
identity objects, streams or lazy seqs. Reader callbacks are explicit execution;
source data itself is never evaluated. Runtime EDN failures are catchable language
errors, with source spans and callback exception identity retained; cancellation,
resource limits and host failures retain their terminal/host classifications.
Reading another input from a callback is supported. As with the code reader,
same-stream callback reentry reports a host error while that stream is in use;
this deliberately retains the explicit host I/O borrowing boundary instead of
JVM PushbackReader reentry. Collision winners when several rename mappings share
a destination follow the unspecified native map traversal order.

Hosts can use `Context::edn_reader(&Source, EdnOptions)` or
`Context::edn_source_reader(Source, EdnOptions)` with rooted per-reader handlers.
Both return ordinary `Form` values and distinguish EOF from literal nil. Options
are owner-checked and shared without cloning the handler map per form. EDN
semantic nesting plus outstanding callback evaluation is capped at 64 frames
(or a lower configured evaluation-depth limit); this bounds native reader-stack
use during callback reentry. This is a stricter EDN limit than the parser's
128-form cap. These direct host readers preserve the ordinary `ReadError`
boundary and fuse after error/EOF; returned forms and spans retain their source
after reset or cursor destruction.

`tests/interpreter_data.rs` checks every export against 178 new pinned SCI
fixtures, including relational examples, traversal effects and metadata, EDN
options/restrictions, and arity failures. Native tests cover the explicit stream
API, direct reader lifetimes/owners, callback collection and exception identity,
unsupported values, namespace initialization and shared resource limits. The
provenance manifest distinguishes the adapted upstream stringify assertion from
independent probes. Normal tests use committed fixtures without JVM/research.

## Embedding configuration, permissions and forks

Run `cargo run --example embedding` for a complete native Rust embedding: it
registers a function and a macro, supplies a data binding, retains and calls a
returned interpreted function across evaluations, reports an error and continues,
and evaluates an owned form. No external runtime is used.

`Context::new()` is an isolated default owner. `Context::init(ContextOptions)`
adds explicit initial configuration; `merge_opts(ContextOptions)` applies a patch
without discarding definitions, aliases, load records, features, or stream state.
Omitted fields preserve their previous values. Native functions/macros, bindings,
namespaces, aliases and data-reader entries merge by name. A supplied Var binding
replaces its root while preserving the existing local Var identity. Explicit
aliases replace only that alias; supplying policy, features or limits replaces
that setting. `Some(None)` removes a loader, fallback reader, cancellation token
or execution poll; a nil stream root disconnects that channel. To supply owned
values, create the context first and then merge values constructed in it.

All option values/names/arity ranges and stream types are validated before the
patch mutates state. Fresh-context handles are rejected as `OptionsError::WrongOwner`.
Configuration patches and forks return `ActiveEvaluation` while any evaluation in
the shared owner is active, including callback reentry. This is a host boundary,
not an interpreted configuration operation. Unsupported SCI options such as JVM
classes, reflection, imports, async and proxy/reify capabilities have no typed
field and fail to compile instead of being silently ignored. Defaults install no
OS, process, network, filesystem or ambient standard-I/O capability. A
`FileSourceProvider`, arbitrary Rust callback or stream must be explicitly supplied.

`eval_string_plus` and `eval_form_plus` return `Evaluation { value, namespace }`.
On failure, `EvaluationError` contains the final namespace and the complete owned
`EvalError` diagnostic (phase, kind, source spans, macro origin, frames and exception
identity). Previous effects and successfully evaluated forms remain committed.
`eval_string`, `eval_source`, `eval_form`, `analyze_form`, `eval_analyzed` and
`invoke` remain available with their existing result types. Language `eval`
converts owned data directly to a form, then expands/analyzes/executes it in the
current namespace. It does not capture caller lexical locals or print/reparse
its input. Synthetic eval-data diagnostics identify `<eval-data>`; embedded source
text still retains its original source diagnostics when evaluated as a Form.

`NamePolicy` is a SCI-style name-access policy. Unqualified option names mean
`clojure.core/name`; qualified names are exact canonical defining names, never
alias spellings. Deny wins over allow. An absent allow list allows all otherwise
supported names; an empty allow list allows no exposed names. All host-installed
Vars (including `intern_var`/`set_var_root`), core/native functions, host data-reader
callbacks and primitives are subject to allow. Vars introduced by language
`def`/`defn` are exempt from allow and remain subject to deny. Primitive and
synthetic definition names use the `clojure.core` policy namespace. Macro-generated
forms undergo the same checks, without a metadata escape hatch. This is deliberately
stricter than SCI's privileged expansion exceptions for some core iteration macros:
a restrictive allow list must include operations in the expanded form, including
internal helper names when the selected macro uses them.

Checks apply to canonical resolved Var identities before access, including aliases,
qualified/unqualified names, renamed refers, resolve/ns-resolve and Var quotation.
Execution rechecks retained analyzed Var references and primitive requirements.
Var dereference, invocation, mutation and language dynamic binding check retained
handles obtained through namespace maps. Native invocation checks its definition's
canonical identity even when a callable is stored under another name, called via
`apply` or invoked directly by the host. Macro expansion, language eval, code/EDN
reader handlers, loader source evaluation and host callback eval/invoke reentry
use the active policy. Existing FormReader cursors share live policy updates.
Failures report `PermissionDenied`; they are diagnostics and cannot be caught as
ordinary interpreted language exceptions.

The policy governs name access and native capability invocation, not information
flow or revocation of previously obtained ordinary data/interpreted closures.
Already copied data remains available; a retained interpreted closure still runs
its code, whose Var/native/primitive accesses are checked. A previously expanded
macro is not rerun when its resulting code executes. Evaluation of an analyzed
unit conservatively checks its recorded references, including inactive branches.
Low-level host data access and trusted administrative installation methods are
not a sandbox against the embedding Rust program. A callback that performs an
external action itself remains a deliberately granted capability; effect metadata
is descriptive. `unrestricted: true` disables name checks only: it never grants
host access, enables `#=`, disables owner/resource checks or grants builtin Var
mutation. `reset` starts a fresh owner and clears language/capability state, while
preserving the configured name policy and existing resource configuration.
Arbitrary host code remains cooperative and cannot be forcibly cancelled.

`fork()` copies namespace maps, aliases, excludes, current namespace selection,
reader configuration, load-state records and name policy. Existing ordinary Vars,
roots, metadata, user dynamic binding slots, atoms, volatiles, lazy/delay caches,
closures and other values remain shared. Redefining an existing user Var in a fork
therefore changes what the parent sees. New namespace mappings and newly interned
Vars exist only in the branch that creates them; unmapping and reinterning produces
a new branch-local identity. Namespace handles carried in shared Var metadata are
routed through the receiving context's namespace registry. Code defining a Var in
a namespace absent from the receiving branch fails with a structured namespace
error. Passing a same-family handle explicitly shares access to that value.

The builtin dynamic Vars currently mapped in core (`*ns*`, `*file*`, I/O, print
controls and REPL history) receive copied slots, preserving the visible value but not active host
BindingGuard tokens. Retained code redirects their original Var IDs to stable,
rooted copies; later unmapping does not retarget those identities. Ordinary user
Vars keep their shared bindings. Already-unmapped Vars are retained values outside
the copied namespace maps and continue to be shared. Stream objects and loader
provider objects themselves are shared capabilities: changing a child stream Var
does not redirect its parent, but writing to an inherited stream affects the same
sink. Successful load records are copied so an already loaded module is not rerun
solely because of a fork. Later loads/reloads mutate only that branch's maps, while
updates to existing shared Vars remain visible across branches.

Forks share one owner, garbage-collected store and resource accounting, including
limit/poll/token configuration and usage. A settings change affects the whole fork
family. Independent `Context::new` owners reject each other's values; fork-family
values are compatible. GC in any branch traces live roots from every branch.
Dropping/resetting one branch does not invalidate another's retained roots. A fork
is neither deep isolation nor a separate resource sandbox. Use a fresh context
and explicit data transfer when independent ownership is required.

`tests/interpreter_embedding.rs` adapts the pinned SCI permission, context and
merge-options assertions and adds adversarial paths for retained code, namespace
handles, callback reentry, wrong owners, removed capabilities and fork collection.
Eleven `sci-embedding-*` fixtures compare eval and context state against the exact
SCI pin. JVM class options in upstream merge tests are intentionally excluded.

## Native script and REPL commands

The compiled `light-cljs` binary now executes the supported interpreter subset
with no Node, JVM, SCI checkout, generated JavaScript, or runtime dependency.
Existing token, `--parse`, `--analyze`, and `--compile` commands retain their input
syntax and behavior. Native interpretation is selected explicitly:

```sh
cargo build --release
./target/release/light-cljs --eval --text '(reduce + (range 1 6))'
# 15
printf '(let [x 21] (* 2 x))' | ./target/release/light-cljs --eval -
# 42
./target/release/light-cljs --eval --source-root examples/native examples/native/demo.clj
# Hello from native Rust
# {:squares [1 4 9 16 25], :sum 15}  (map order is unspecified)
printf '(+ 20\n 22)\n(* *1 2)\n' | ./target/release/light-cljs --repl
# 42
# 84
./target/release/light-cljs --repl
```

`--eval` accepts exactly one file, `-`/omitted input for stdin, or `--text SOURCE`.
Options can precede or follow the input. `--` ends option parsing for filenames
beginning with a dash. Input and filenames support UTF-8. `*file*` is the supplied
filename, `<text>`, or `<stdin>` during script evaluation. A failed read, analysis,
execution, or result rendering exits nonzero and prints a source filename,
one-based line and byte-column, phase, and byte span on stderr. Loaded-module
failures retain the module's source location and call frames. Unfinished syntax
also identifies its opening delimiter/prefix. No intermediate or failed final
result is printed; completed program effects are not rolled back. Empty scripts
successfully return `nil`.

The CLI explicitly supplies UTF-8 stdin/stdout/stderr capabilities. Language
`print`/`println` output goes to stdout; a binding to `*err*` writes stderr. After
successful evaluation, the CLI buffers a readable rendering of the last value
and writes it with a newline to stdout. Script output therefore precedes the
result (`(println "hello") 42` produces `hello`, then `42`). A script file or
`--text` can call `read`/`read-line` on stdin. For a stdin script, stdin is consumed
as source before evaluation, so runtime reads encounter EOF. Readable string
results escape lone UTF-16 surrogates; raw printing of an unpaired surrogate to
a UTF-8 channel reports an error instead of replacement characters.

Repeated `--source-root DIR` flags opt into the existing filesystem provider.
Roots are searched for `.clj` and `.cljc`; ambiguous canonical candidates fail.
The current directory, script directory, classpath, and `.cljs` files are not
implicit loader roots. The CLI grants only the standard streams and these
requested sources. Arbitrary filesystem, process, network, JVM class, reflection,
JavaScript interop, thread/async, full numeric tower and other excluded APIs
remain unavailable. This is the portable interpreter profile documented above,
not full Clojure or full SCI. In particular, `clojure.repl` doc/apropos/pst helpers,
terminal editing/completion, and persistent disk history are not implemented.

### REPL state and recovery

The REPL accepts stdin only. Each top-level form is read, executed and displayed
before the next form is semantically read, even when several share one line.
Definitions, macros, aliases and the current namespace persist; auto-resolved
keywords and later forms see namespace changes immediately. The underlying
stream cursor is shared with interpreted `read` and `read-line`, including bytes
already buffered after the current form. No separate lexer, delimiter counter,
or print-and-reparse pass decides when input is complete: the shared parser
and contextual reader handle missing operands, discarded/conditional forms,
strings, comments and delimiters. Read callbacks execute once after the complete
form is available.

For terminal stdin, the current namespace prompt and `...=>` continuation prompt
go to stderr; redirected input has no prompts or banner. EOF ends the session;
a final incomplete form reports one reader diagnostic. A successful session
exit has status zero even if recoverable form errors occurred. Host errors or
resource exhaustion while fetching a REPL form exit nonzero, as do CLI option
and result-output I/O failures.

Every successfully evaluated **and rendered** form shifts `*1` to `*2`, `*2` to
`*3`, and stores the new value in `*1`; initial values are nil. Failed reads,
evaluation and rendering leave those three values unchanged and update `*e`.
A thrown language exception is retained directly, preserving its `ex-data` and
identity. Other diagnostics are wrapped in a portable ExceptionInfo with
`:phase`, `:file`, `:line`, and `:column` data and a descriptive message. Success
leaves the last `*e` unchanged. The history names are ordinary builtin dynamic
Vars visible from other namespaces, with REPL-owned bindings. Host evaluation
APIs do not rotate them automatically; forked contexts copy their slots like the
other builtin dynamic controls.

Read errors discard the remainder of currently buffered input, normally the
bad line, before requesting fresh input. Execution/analysis/rendering errors
preserve subsequent forms, including later forms on the same line. A failed
interpreted `read` also recovers its shared stream at the buffered line boundary,
including an EDN read failure that the program handled with `catch`.
Definitions and effects completed before failure remain visible. Unreachable
interpreter objects are collected between interactions. Fully consumed input
lines are compacted while reusing the buffer capacity; the current partial line
stays available to shared reads. This avoids copying and charging old history
against each new form's resource budget. Retained forms/functions and diagnostics
own the source text they need. Original line numbers and byte-columns survive
compaction; diagnostic byte spans are relative to each retained source window.
Allocation limits remain per operation, not a global live-memory cap.

The embedding API exposes this same boundary as
`Context::read_input_form(&RootedValue) -> Result<Option<Form>, EvalError>`.
It shares the stream with language reads, returns `None` at EOF, and retains
source diagnostics. Errors fuse the stream until an explicit
`Context::recover_input` discards the buffered suffix; recovery does nothing to
a healthy cursor. Both operations reject streams from another owner.
`Context::compact_input` explicitly releases fully consumed lines while retaining
original source line locations; ordinary embedding streams keep their text until
the host opts into compaction. The REPL calls it between interactions.

### Native CLI limits

All numerical options accept nonnegative integers. Zero is a real limit, never
an unlimited sentinel. Builtin hard stack-safety caps still apply.

| Flag | Default | Meaning |
| --- | ---: | --- |
| `--fuel N` | 1,000,000 | Cooperative work units |
| `--allocation-limit N` | 16,000,000 | Cumulative semantic storage units, not bytes/RSS |
| `--output-limit N` | 1,000,000 | UTF-16 bytes formatted/emitted by interpreter operations |
| `--call-depth N` | 32 | Nested calls, capped at 32 |
| `--evaluation-depth N` | 128 | Nested expressions, capped at 128 |
| `--print-length N` | 100 | Result elements per collection |
| `--print-level N` | 20 | Result collection nesting |

Script evaluation shares one budget across all top-level forms, loading, nested
eval, macros and native callbacks. REPL stream reading, each top-level evaluation,
and automatic result rendering each start their own budget; a failure cannot
reset the budget within an active operation. In particular, a result can cost
additional work beyond its evaluation, and an infinite result is truncated by
printing limits. Automatic result printing uses the CLI length/level settings;
language `*print-*` bindings govern explicit program printing. The CLI newline
and diagnostic/prompt text are outside interpreter output accounting. These
cooperative counters cannot preempt a blocking stdin read or arbitrary host
callback and do not replace operating-system process isolation.

`tests/native_cli.rs` exercises the executable with an empty PATH, including a
copied binary in a fresh directory with a cleared environment and only explicit
program sources. Coverage includes UTF-8 paths/input, source/phase diagnostics,
stdout/stderr/result separation, loader ambiguity, multiline/contextual reading,
EOF, shared language input, recovery, histories, resource exhaustion and bounded
lazy printing. Printing and input assertions adapt the pinned SCI
`test/sci/io_test.cljc` tests listed in the provenance manifest; REPL/CLI policy
is tested directly rather than attributed to an upstream terminal implementation.
The existing 2,028-row offline SCI corpus remains unchanged.


## Compiler macro host

The JavaScript compiler now uses this interpreter for user/native and selected
core macro expansion. Its runtime globals remain separate from interpreter Vars.
`Context::expand_macro_call` invokes an explicitly resolved macro Var with owned
`Form` input and the caller-supplied environment, preserving direct data conversion,
macro provenance, resource controls and optional expansion statistics.
`find_interned_var` performs exact namespace lookup without caller aliases/refers.
SCI `macroexpand` and its flat lexical environment remain unchanged; the compiler
builds a separate CLJS-shaped environment. See [the compiler contract](compiler.md#compiler-macros-and-embedding)
and [the complete example](../examples/macros) for the supported boundary.
