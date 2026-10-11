# Executable core

This reference describes the **current implementation**. Macros and demanded
ordinary helpers execute JavaScript emitted by this compiler through rquickjs.
The [evaluator migration](evaluator-migration.md) also covers the remaining
native evaluation consumers. Explicit native embedding adapters remain available
until that migration removes them.

The compiler supports small programs with lexical closures, mutable globals,
source namespaces, and JavaScript interop. The two-namespace example
prints exactly `4\n`. Source macros and first-party core macro algorithms use
the compiled JavaScript host; compiler-owned declaration/binding transforms
remain in Rust. Immutable vectors/lists/maps/sets, symbols/keywords,
quoted data, lazy sequence sources and a finite set of first-class core functions
execute through a first-party JavaScript runtime. Sorted sets and transients
remain missing intended-language features. Explicit compilation-use records and
conservative application reachability now live in Rust. A bundled general standard library
is not currently provided. See the
[language coverage boundaries](evaluator-migration.md#language-coverage-and-completion-boundaries)
for feature ownership and the distinct completion milestones.
Caller-supplied library sources compile through the same language surface.

The [compiler cost report](../benchmarks/compiler.md) measures compiler phases,
emitted bytes, fresh CLI/Node processes and resident immutable operations
separately. It confirms one embedded runtime per combined collection unit and
byte-identical output for three unchanged primitive fixtures. Its large map/set
construction and collision measurements expose the costs of the deliberate
linear representation; Node process timings are not browser startup estimates.

## Pipeline and ownership

Compiler forms and source provenance now live in `compiler_data`: exact UTF-16
`Name`, `Source`/`SourceSpan`, origins, immutable `Form`/`FormId` nodes and an owned
finite `Datum` tree. `Form::data()` returns a compiler datum and retains no native
`Context`, `RootedValue`, executable unit or collector owner. Source text and
located reader errors remain usable after the macro host and input buffers drop.
`Form::from_data` constructs supplied data without an evaluator; the staged
`MacroHost::read_compiler_form` API returns the same owned representation.

Map form children alternate keys and values in source evaluation order, separately
from semantic map storage. Form spans retain lexical spelling; datum scalars keep
reader number categories and all floating bits, while JS emission converts numbers
to Number. Metadata and opaque reader literal payloads are owned datum/form edges.

`compiler_reader` constructs semantic forms directly from syntax and immutable
`NamespaceView` snapshots. A view retains source identity, aliases, refers,
excludes, resolved binding IDs/kinds and the visible declaration frontier.
Syntax quote, unquote/splicing, anonymous functions and namespaced maps produce
ordinary canonical runtime construction calls without entering an evaluator.
`core_metadata` queries the pinned core table directly. Namespace metadata and
reflection snapshots also use owned compiler data.

The pure compiler seed in `compiler_binding_macros` and
`compiler_destructuring` expands binding/function declarations and their extended
map patterns. Protocol/type/record analysis remains compiler-owned; method forms
use the same seed. Diagnostic assertion formatting uses compiler data directly.
Reading, seed expansion and namespace/core metadata inspection need neither a
native Context nor a JavaScript runtime. `source_provider` owns the load request,
input and error contracts separately from eager native loading.

`native_form_adapter` remains an explicit temporary macro execution bridge.
It converts structured data directly and retains source identity/nested expansion
origins. Explicit native custom reader callbacks also use this adapter during
migration. Filesystem compiler imports use the same `.cljs`/`.cljc` namespace
loader for both phases and never read `.clj` companions. Native public source/form
APIs remain available until `light-cljs-115` deletes the adapter and native
compatibility storage. The evaluator boundary executes compiled macro/helper units.
Compile-time emission uses the same semantic IR and canonical runtime operations;
it may add deferred-root access and bypass optional application optimization.
See [session ownership and reentrancy](evaluator-migration.md#sessions-ownership-and-reentrancy).

`parse(source)` constructs the existing syntax arena. `analyze(source, &syntax)`
constructs a separate `Program`; `emit_js(&program)` prepares an application in
Rust, then recursively writes one JavaScript expression into a `String`. `compile(source)` composes these stages
and distinguishes parser errors from analysis errors. `--compile` accepts files,
stdin, or `--text`; `--analyze` prints the resolved representation. Existing token
and parser CLI modes retain their behavior.

`Program` owns `Vec<Expr>`, lexical/global binding records, JavaScript paths, and
UTF-16 string/name values. `NamedLiteralId` identifies an owned namespace/name pair;
`CoreFnId` selects a finite runtime function independently of interpreter Vars.
`ExprId`, `LocalId`, `GlobalId`, `JsGlobalId`, and `StringId`
are integer indices into those arrays. Expression and binding records retain
original byte spans and a source index (zero for standalone analysis; the
namespace's dependency-order index for a combined program). Analysis requires the exact source
used to parse the syntax; the resulting program no longer borrows that source.
IDs are local to a single compilation unit, and the emitter expects an
analyzer-produced program.

### Application preparation

`prepare_application(&program, ApplicationOptions::default())` is the public
pre-emission boundary. `emit_js` uses this default; callers can retain the
`PreparedApplication` and call its `emit_js()` repeatedly without rerunning the
pass. `ApplicationOptions { optimize: false, exports: vec![] }` provides an
unoptimized comparison, while `exports: Vec<GlobalId>` explicitly keeps runtime
binding identities and their source generations. Exports are retention roots;
this API does not add a JavaScript module export wrapper.

The entry result, all standalone expressions, unknown or effectful initializers,
throws, definition tests, and protocol/type/extension declarations retain their
source/dependency order. Discarded top-level definitions are removable only when
their initializers are proven inert (primitive literals, function construction,
core function values, inert lists/vectors, and maps/sets with scalar keys). Runtime references and escaped
Vars retain their definitions transitively, including metadata and all source
generations. Non-scalar hashing, reader literals, arbitrary calls, and metadata operations
are conservatively effectful. Retained raw JavaScript prevents elimination and
specialization; raw code inside an otherwise discarded function is not a root.
This is deliberately conservative reachability, not a general optimizer.

Specialization currently covers eager top-level calls after a sole, non-dynamic
initialization, with no value/Var escape or mutation: fixed entries of multi-arity
functions and aliases to already-resolved core operations. Fixed entries retain
`.call(null)` and source-order argument evaluation. Missing arities, mutable
roots, deferred function-body calls and unknown protocol targets keep ordinary
shared lowering. Single fixed-arity functions already have no dispatcher.

`Program::compilation_uses` records deferred ordinary-unit names, generations,
actual analysis counts and compile-time emission counts. Analysis for application
output does not imply compile-time execution; macro demand does not create a
runtime root. Macro/bootstrap functions remain outside application IR. Deferred
unit emission bypasses this pass, caches each generation's emitted code, and
shares canonical runtime operations. Applications have no deferred-unit compiler
callback or demand-slot dependency. Statistics and optimizer on/off execution
checks are recorded in [application evidence](../tests/reference/compiler-optimization.md).

The environment is a stack of scopes containing binding IDs. Lookup walks scopes
and their bindings from newest to oldest. Binding records survive scope exit.
Each `let*` initializer is analyzed before its binding is installed; previous
bindings are visible, including earlier occurrences of the same spelling.
`letfn*` allocates every group binding before analyzing its function initializers.
Each entry creates independent lexical slots, including nested groups and loop
iterations. `fn*` installs its optional self-name and parameters before analyzing its body.
A parameter may shadow the self-name. Every emitted local name derives only from
its ID (`l0`, `l1`, …), so shadowing, Unicode names, and JavaScript reserved words
do not change binding identity.

## Supported syntax and values

| Construct | Behavior |
| --- | --- |
| `nil`, booleans | JavaScript `null`, `true`, `false`. |
| Integer literals | Signed decimal, hexadecimal, octal, radix and BigInt spellings use JavaScript Number rounding, including beyond the safe-integer range. Integer `-0` is ordinary zero. |
| Floating-point literals | IEEE-754 doubles and decimal literals; negative zero, overflow to infinity and underflow follow the pinned ClojureScript behavior. |
| Strings | Exact decoded UTF-16, including lone surrogates. JavaScript escaping preserves quotes, backslashes, controls, and Unicode. |
| Keywords | Immutable named values, distinct from strings; qualified and auto-resolved names preserve namespace identity. |
| Vectors, maps and sets | Immutable owned collections; element and key/value expressions evaluate once, left to right. |
| `'data`, `(quote data)`, `()` | Supported scalars, symbols, nested lists/vectors/maps/sets and empty lists become data without name resolution or execution. Nested quote retains its `quote` symbol. |
| `(if test yes no?)` | Evaluate the test once and only the selected branch. The omitted branch is `nil`. |
| `(do forms...)` | Evaluate in order, returning the last value, or `nil` when empty. |
| `(let* [name init ...] body...)` | Sequential simple-symbol bindings and an implicit `do` body. Repeated names create distinct bindings. |
| `(letfn* [name function ...] body...)`, `letfn` | Mutually visible lexical functions with outer captures, overloads, variadic methods and independent group instances. |
| `(loop* [name init ...] body...)`, `recur` | Stack-safe nearest-target iteration with simultaneous left-to-right rebinding and fresh lexical captures. |
| `try`, `catch`, `finally`, `throw` | Ordered typed and `:default` catches, arbitrary thrown values and synchronous cleanup in every expression context. Recurrence cannot cross a try boundary; nested loops/functions establish their own targets. |
| `case`, `case*` | Grouped constants, optional defaults, validated test forms, single selector evaluation and tail recurrence. Scalar and keyword dispatch use JavaScript switch; general constants use ClojureScript equality. |
| Conditional/threading macros | Nil-aware and truth-aware bindings, short-circuit operations, ordered threading, `cond` and both `condp` clause forms preserve lexical scope and selected effects. |
| `while`, `dotimes` | Executable nil-returning iteration with fresh captures, one dotimes bound evaluation and nearest-target recur validation. Fractional bounds retain CLJS comparison semantics. |
| `(fn* name? [params...] body...)` | A lexical closure with one parameter list and optional `& rest`; optional self-name supports recursion. |
| `(fn* name? ([params...] body...) ...)` | Multiple fixed methods and at most one variadic method; fixed methods take precedence at their exact argument count. |
| `(callee args...)` | Evaluate callee first, then arguments left to right, then call. |
| Multiple source forms | An implicit `do`; the file's result is its last value, or `nil` when empty. Top-level lexical scopes do not leak into following forms. |

Primitive `let*`, `loop*`, and `fn*` bindings must be simple symbols: no namespace slash, dot, or destructuring. Ordinary `let`, `loop`, `fn`, `defn`, and `letfn` lower nested vector/map bindings, `:as`, `&`, `:keys`, `:syms`, `:strs`, qualified keys and `:or` to these primitives. `&` introduces a final rest binding.
The standalone symbol `/` is unqualified and is supported, including as a global
whose qualified spelling is `namespace//`. Embedded ampersands, such as `a&b`,
are ordinary name characters. Repeated function parameters are legal; the last
binding wins, including a rest binding that shadows a fixed parameter. Each
overload has a separate scope and shares the optional function self-name.
Calls to a single fixed method do
not check argument counts: missing parameters receive JavaScript `undefined`,
and extra arguments still evaluate before the body, which ignores them. This
matches the pinned standard compiler's runtime behavior; known local/global
arity mismatches warn there, while light-cljs has no arity-warning API.
Multiple fixed methods throw `Error("Invalid arity: N")` for unmatched counts.
When a variadic method is present it supplies the default branch, including
underfilled calls: missing fixed parameters receive `undefined`, and an absent
rest receives nil. Direct rest arguments are immutable indexed sequences, not
JavaScript arrays; `apply` can retain its existing sequence tail. Prefix lengths
are unrestricted. The pinned `apply` helper has a separate 20-argument limit
when it selects the bounded fixed-call path of a variadic function; direct calls
and the variadic sequence entry support larger prefixes and rests.

`letfn` takes named function declarations; its primitive `letfn*` takes an even
vector of simple names and function initializers. Named, overloaded, variadic
and macro-generated functions share the ordinary function ABI. Empty groups and
bodies return nil. Repeated group names retain their last declaration. Invalid
bindings and non-function initializers produce located errors. Function metadata
expressions run during initialization, before the body; an initializer exception
stops the remaining initializers. Mutual calls begin after the group is initialized.

Stable local identities preserve escaping functions across same-spelled nested
groups and loop iterations. Both pinned upstream emitters reuse JavaScript `var`
slots in two such cases; these explicit differences are independently tested and
[documented](../tests/reference/compiler-letfn.md), outside matching conformance
counts. This preserves the existing lexical identity guarantee.

The emitter exposes `cljs$core$IFn$_invoke$arity$N`, the variadic delegate,
`cljs$lang$maxFixedArity`, and `cljs$lang$applyTo` where ClojureScript does.
Metadata functions use callable-object entries that invoke their wrapped function
with the pinned receiver rules. Duplicate fixed methods, multiple
variadic methods, and a fixed method wider than the variadic prefix produce
located declaration errors. The reference warns for duplicate/incompatible
signatures (and can emit invalid JavaScript for multiple variadics); the exact
stage differences are recorded in the diagnostic matrix.

Vector, keyword, symbol, map and set calls support `.call`, `.apply` and language
`apply`. Fixed `reify IFn` methods supply custom callable receivers. Declared
`defprotocol` methods and `reify` implementations dispatch fixed and overloaded
methods with stable namespace-qualified keys. Marker and partial implementations
retain protocol membership independently of method availability. Declarations
retain docs, custom metadata, method arglists and protocol identities. Saved,
aliased and qualified method calls use the same runtime dispatch path.

With `:extend-via-metadata true`, metadata keyed by the qualified method symbol
wins before a direct implementation. Public CLJS method slots dispatch next,
followed by the current method Var's native type table and default table.
`satisfies?` includes native extensions; `implements?` checks direct membership.
Both predicates resolve the protocol symbol, including namespace aliases.
Redefining a protocol leaves existing direct slots usable and resets its native
method tables. Saved methods still consult the current method root, including
inside `with-redefs`. Protocol declarations return JavaScript `undefined`.

`extend-type` and `extend-protocol` install named-constructor or native/default
implementations through the same mechanism. Core collection, metadata and state
protocol methods retain their first-class runtime identities. Small `js-obj`
macro expansions retain insertion order for native-object traversal, and reduction
accepts a protocol-provided lazy sequence. Fixed signatures include large arities;
the accepted-but-warned upstream variadic protocol expansion retains its literal
ampersand/packed-tail behavior rather than becoming an ordinary variadic function.
See the [protocol audit](../tests/reference/compiler-protocols.md) for the pinned
contract, executable assertion mapping and measured output cost.

The binding bridge also constructs `deftype`/`defrecord` receivers,
including positional/map record constructors, receiver fields, mixed IFn/custom
protocol methods, Object methods and `ISeqable`/`IIndexed`/`ILookup` access needed
by destructuring. Record/reify metadata copies retain their methods and captures.
Metadata protocols, their `satisfies?` predicates and record metadata updates
are implemented. Record collection operations, generated protocols, construction
and nominal identity now have the [record audit](../tests/reference/compiler-records.md). A literal `nil` callee is
rejected during analysis. Dynamic non-callables produce JavaScript TypeError.
Ordinary calls first access the callee's `.call` property: a nullish callee fails
before argument effects, while numbers, booleans, and plain host objects evaluate
arguments before the invocation fails. Explicit `recur` is stack-safe; ordinary
self-calls continue to use JavaScript calls.
`apply` supports all fixed prefixes and a sequence tail. The standard function
combinators `identity`, `constantly`, `complement`, `comp`, `partial`, `juxt`, and
`fnil` retain their public arities, evaluation order, and lazy variadic tails.
`this-as`, `js-arguments`, and `copy-arguments` preserve receiver and argument
observations, including value-context function scopes. See the
[apply audit](../tests/reference/compiler-apply.md) for exact evidence.
The [function audit](../tests/reference/function-evidence.md) records executable
evidence for these boundaries and both accepted signature spellings.

Core functions outside the finite whitelist remain unresolved. Reader characters
become one-unit strings. Symbolic numbers, large integers, BigInt and decimal
spellings, and integral ratios use the pinned compiler's Number semantics.
Non-integral ratios are rejected because the JVM reference cannot emit Ratio
constants. A safe-integer bound is not imposed. Unresolved symbols remain errors
in unselected branches. JavaScript globals use `js/` qualification or an explicit
namespace `:refer-global` declaration.

## Atoms, volatiles and state protocols

`atom`, `volatile!`, `deref`/`@`, `reset!`, `reset-vals!`, `swap!`,
`swap-vals!`, `compare-and-set!`, `set-validator!`, `get-validator`, `add-watch`,
`remove-watch`, `volatile?`, `vreset!` and `vswap!` execute in the embedded
JavaScript runtime. `Atom`/`Volatile` constructors and factories are available.
`reset-meta!` and `alter-meta!` change reference metadata without changing state
or notifying watches. `update` supports state transformations within collections.

Validation runs synchronously before reset captures its old value. Failed
validation preserves state unless the validator itself changes it; a thrown
validator value escapes unchanged and failed installation retains the previous
validator. Construction does not validate the initial state. Watches run after
state changes, from the watch map captured by that notification, and may reenter
state operations. Their failures do not roll back state. `reset-vals!` and
`swap-vals!` retain the proposed new value even if a watch changes state again.

The pinned ClojureScript `compare-and-set!` uses structural `=`, including equal
but distinct collections. It does not use the JVM atom identity rule. Public
functions and their state protocol methods dispatch consistently on custom
reify/type implementations. Custom swap-vals/reset-vals dereference before
calling the custom method, preserving dereference side effects. Protocol
predicates retain marker and partial declarations; native atoms implement
`IAtom`, `IDeref`, `IMeta` and `IWatchable`, not `ISwap` or `IReset`.
`vswap!` preserves the upstream macro's repeated target evaluation and callback
order. None of these runtime operations needs the native interpreter.

The [state audit](../tests/reference/compiler-state.md) records the upstream
assertion mapping, independent outcome checks, namespace persistence, and
runtime size/startup observations.

## Reader forms and runtime literals

The compiler expands `#()`, `%`, `%n`, `%&`, `@`, `#'`, metadata, namespaced maps,
and syntax quote/unquote through its native semantic reader boundary. Reader
aliases include runtime and explicit macro aliases; macro aliases take precedence.
Anonymous arguments have fresh identities, nested shorthand is rejected, and
rest arguments are nil when empty. Syntax quote qualifies symbols and preserves
unquote/splice evaluation, gensym identity and owned UTF-16. `#'` resolves global
Vars even beneath local shadows; deref observes their current roots, and Vars
are callable. Quote traverses the expanded data without executing it.

Regex literals construct JavaScript RegExp values; `regexp?`, `re-pattern`,
`re-find`, `re-matches` and lazy `re-seq` use the JavaScript regex engine. `#inst`
validates Gregorian timestamps and constructs Dates; `#uuid` validates and
canonicalizes UUID literals. Dates and UUIDs compare by value, and separate
literals preserve distinct object identities. `#js` vectors/maps construct host
arrays/objects, retaining nested language values and evaluating expressions once
in source order. Object keys must be strings or unqualified keywords. `#queue`
expands to `into` and an empty persistent queue, including when quoted.
`pr-str` exposes tagged printing; metadata is retained on literal collections
and quoted symbols through `meta`, `with-meta` and `vary-meta`. Collection
updates preserve metadata; ordinary seq tails discard it while queue seq tails
retain it. `sequence` retains lazy seq identity; `vec` strips metadata. Atoms
support mutable reference metadata, Vars support immutable metadata copies,
and reify/records retain their methods. Runtime metadata maps evaluate their
expressions; quoted metadata is data. Reader locations are elided from runtime
metadata while definition metadata remains available to macros and Vars.
The [metadata audit](../tests/reference/compiler-metadata.md) records full
interface inputs, source provenance, 32 upstream assertions and 38 independent
probes against both pinned references.

Explicit embedding handlers registered with
`MacroHost::context_mut().register_data_reader` receive already-read native
values and return semantic forms. Unknown tags fail without dynamic class,
filesystem or namespace lookup. Reader handlers execute once in source order,
including discarded forms and unselected runtime branches. Compiler-only opaque
values keep regex/Date/UUID/JavaScript literals intact through macro expansion;
they are never treated as strings or printed and reparsed. Source/argument
payloads retain their original spans. Reader evaluation (`#=`) stays disabled.
Runtime `.cljc` namespace files select `:cljs` or `:default` reader branches,
including collection splicing, before dependency discovery. `.cljs` and standalone
`compile(source)` input continue to reject reader conditionals.

The [reader evidence](../tests/reference/compiler-reader.md) records full upstream
anchors, literal and macro probes, numeric behavior, exact checks and output cost.

## Numeric operations and fundamental predicates

The resolved core includes arithmetic and variadic comparisons, `inc`/`dec`,
`max`/`min`/`abs`, integer/floating/character and unchecked coercions, quotient,
remainder, modulo and every public bit operation. Boolean, nil, number, string,
named-value, function and collection predicates are first-class functions.
They preserve JavaScript Number, UTF-16 and private collection-brand semantics.
`int` wraps to 32 bits; `long` truncates without wrapping. No native interpreter
arithmetic executes in generated code.

Saved functions, aliases, shadowing and `apply` use the ordinary callable ABI.
Malformed direct numeric macro arities are rejected during analysis, while
saved function values preserve upstream's missing/extra argument behavior.
All local calls evaluate arguments once in order. Upstream direct comparison
macros may skip or repeat expressions; the pinned differential effect tests use
saved comparison functions to test the ordinary function ABI explicitly.

The [numeric evidence](../tests/reference/compiler-numeric.md) lists the complete
API families, imported assertion mappings, independently expected runtime checks,
full differential outcomes and the observed release/checkout set difference.
External Closure Long/Integer construction remains separate from JS Number
predicates. Additional collection families and unrelated
libraries are not implied by these predicate names.

## Immutable collection data and the bounded core

`keyword`/`symbol` (1/2 arguments), `vector`/`list`/`hash-set`/`conj` (0+), `hash-map`
(0+ paired arguments), `assoc` (3+), `dissoc`/`disj` (1+), `seq`/`first`/`rest`/`next` (1), `name`, `namespace`, `count`,
`nth`/`get` (2/3), `contains?` (2), `=`, `reduce` (2/3), `+` (0/1/2/variadic),
`reduced`/`reduced?`/`unreduced`/`deref` (1) are actual callable values. Locals and
current namespace globals take precedence over unqualified core fallback.
Explicit `cljs.core/F` and `clojure.core/F`, or an alias for `cljs.core`, select
only this whitelist. Same-named members of user namespaces remain ordinary
globals. Saved core references retain their function identity after shadowing.

Keyword conversion is bounded to strings/keywords/symbols and a nil namespace;
two-argument inputs outside that domain fail with `Error`, even where the
pinned JavaScript implementation permits arbitrary values. Keyword literals own
UTF-16 namespace/name information. `::x` uses the source
namespace (`cljs.user` for standalone compilation); `::alias/x` uses the existing
runtime/macro reader alias view, with macro aliases taking precedence. Unknown
aliases fail with a source location. Native macros can return supported vectors
and keywords directly; generated names retain semantic data, including lone
surrogates, rather than being sliced from the macro call's text. Plain literal
analysis does not initialize the native interpreter.

The original `src/js_runtime.js` factory is embedded once inside the output IIFE
when the resolved program contains a collection, named literal or core value. It has no
imports, external global core, interpreter fallback, host runtime dependency or
production package dependency. Primitive-only output retains its previous
shape except that its enclosing scope no longer forces strict mode. Owned
arrays/cells and base callable storage are frozen; per-type extension prototypes
and public value wrappers remain extensible for protocol and JavaScript property writes. Those properties do not replace the
private collection payloads;
private brands prevent host arrays or objects from impersonating language values.
Nested immutable values and opaque host references are shared; caller-owned host
objects are never frozen or converted into collections.

Vector count/indexed lookup is O(1); constructing a vector is O(n). Equality
compares sequential contents across list/vector/seq kinds, with an identity fast
path for the same immutable value. Keywords and symbols compare by full qualified
spelling within their own kind.
Host objects/functions compare by identity; nil and undefined compare equal,
although the observation protocol distinguishes them. `[]` differs from nil,
and keywords/symbols differ from strings and from each other. Internal hashes
are consistent across equal sequential values and nil/undefined; foreign objects
use weak identity bookkeeping. There is no public `hash` function.

Vector `.call` uses `nth` semantics for its first index; extra arguments evaluate
and are ignored. Missing or out-of-range indexes throw ordinary `Error`.
Keyword/symbol `.call` accepts a collection and optional default, using `get` semantics;
dynamic unsupported arities throw `Error`. Invalid literal keyword calls are
rejected with a located analysis diagnostic. This deliberate boundary avoids
depending on the pinned compiler's invalid-call inlining artifacts.

Core wrappers follow pinned JavaScript fixed/overloaded/variadic behavior:
`(count)` returns zero and fixed-argument extras evaluate before being ignored;
`(=)` is true. Vectors accept fractional numeric `nth`/`get` indexes by truncation,
but `contains?` requires an integer index. String lookup follows the reference's
coercible-index rules and UTF-16 units; these rules do not turn strings into
vectors. `nth` requires a numeric index even for nil: `(nth nil "0")` fails,
`(nth nil 0)` yields nil, and `(nth nil 0 :missing)` yields the default.
False, nil and undefined stored in collections remain present values. This original collection milestone is extended by the runtime protocols and deferred-value support documented below.
The reader supplies `apply`, `vec`, `concat`, one-argument `sequence` and
two-argument `into` for its expanded forms. The native
interpreter has a separate target and stricter call/index rules; its support is
not evidence for generated JavaScript.

Quote traverses the source syntax or macro-owned semantic data directly into
owned `Symbol`, `List`, `Vector`, `Map`, `Set` and scalar expressions. No interpreter is started
for literal construction, and no printer/reparser participates. Quoted unknown
names and call-shaped forms are data, even when a macro of that name exists.
Quoted characters, syntax quote, metadata, tags and namespaced maps retain their
reader semantics. Built-in tagged values retain their runtime types. Macro-generated quoted symbol names retain exact UTF-16,
including lone surrogates that remain invalid as executable symbol names.

One-argument `symbol` accepts strings, symbols (identity) and keywords (parsing
full spelling), and throws `Error` for other inputs. It splits a string at the
first slash only when that slash follows a nonempty namespace: `"a/b/c"` has
namespace `"a"` and name `"b/c"`, while `"/x"` remains unqualified. Two-argument
symbol construction is bounded to string name and string/nullish namespace;
arbitrary host values accepted by upstream in these slots are excluded. Keyword
conversion of a symbol preserves its namespace/name and full spelling. Neither
kind is interned globally. Symbols are callable through ordinary `.call` lookup;
lists and seqs evaluate argument effects before failing with `TypeError`.

Lists retain frozen private cons cells with an empty singleton and cached counts. `conj`
prepends list elements, appends vector elements, and constructs lists from nil;
its zero-argument result is `[]`, and its one-argument result is unchanged.
Vector `assoc` replaces an existing numeric index or
appending exactly at count. In-range positive fractions truncate like the pinned
runtime; invalid indices throw `Error` without changing the old vector. An odd
extra key receives nil. Vector updates copy their array in O(n); list prepend,
first and tail are O(1), and list construction/indexing are O(n).

`seq` of an empty value is nil; nonempty lists can reuse their identity, and
vectors/strings produce views with frozen private offset/storage descriptors.
`rest` at the end (including nil) is the empty list, while `next` is nil. Each tail
is O(1), so complete traversal is linear without repeated suffix copying. String
traversal produces UTF-16 one-unit strings. `conj` of a generic seq produces a
seq-cons, distinct from a list in typed observation. The reference adapter uses
concrete List/EmptyList versus Cons/other seq classes: upstream `list?` also
accepts Cons and alone cannot preserve this distinction.

List `nth` requires an exact reachable integer; fractions default or throw.
Pinned indexed seq views (strings and vectors of at most 32 elements) instead
use absolute storage offsets: fractional properties can yield undefined, and a
negative index into a tail can reach preceding storage. Larger vector seqs use
linear traversal; this observable boundary is covered by direct runtime tests.
`get` on nonassociative lists/seqs returns the supplied default (nil when omitted),
and `contains?` returns false. Typed observation preserves list/vector/seq/empty
list/nil and symbol/keyword distinctions independently of structural equality.

Maps store frozen entry arrays and private structural key hashes. Lookup compares
both hash and language equality, so collisions do not merge unequal keys, and a
list/vector/seq with equal contents can retrieve the same entry. Nested map keys
use unordered equality/hash. Keyword, symbol and string keys remain distinct;
`"__proto__"` is ordinary data. Map count is O(1); lookup and copying updates are
O(n), plus key hash/equality costs. Building n distinct entries and naive unordered
map equality are O(n²), before nested costs. New versions retain the first
equivalent key and share unchanged entry cells and nested immutable values.
No host object is frozen or used as a JavaScript property table.

Maps implement ordinary `.call` with a key and optional default, alongside keyword
and symbol lookup calls. Missing is a private sentinel: stored nil, false and
undefined remain present, and `contains?` returns true for them. Nil/undefined keys
are language-equivalent; this runtime consistently retains whichever equivalent
key was inserted first. Upstream hash maps can canonicalize an undefined key to
nil while array maps retain it; that storage-specific key presentation is excluded.
Stored undefined **values** always remain observable as undefined.

`assoc` accepts maps or nil and supplies nil for an odd trailing key. `hash-map`
requires pairs and throws ordinary `Error` for an odd argument count. `dissoc`
removes zero or more keys from a supplied map, preserving all old versions.
Map `conj` uses the first two items of a vector of length at least two, accepts
maps and finite sequences of vector entries, and treats nil as a no-op; shorter
vectors or invalid entry sequences fail without mutating the input. These odd
assoc/long-entry behaviors intentionally differ from the stricter native subset.
Map seq materialization costs O(n), produces vector-like two-element entries,
and then advances tails in O(1). `seq {}` is nil; `rest {}` is the empty list.
Map traversal order is unspecified and is never sorted by the runtime.

Literal duplicate reader data is rejected at the repeated key's source span.
Reader identity distinguishes integer from floating tokens and equates list/vector
contents; it is checked directly from owned syntax/Form children. Lowering then
uses JavaScript Number equality: `{1 :int 1.0 :float}` has one runtime entry whose
value is `:float`. All literal key/value expressions run once in their original
order, including dynamically duplicate keys. Quote constructs map data without
executing nested expressions, and macro expansion retains original owned children.

Two pinned compiler optimizations are deliberately excluded: direct `hash-map`
calls can evaluate all keys before values, and unchecked literal construction can
retain duplicate JavaScript Number keys that were reader-distinct (`1` versus
`1.0`). This compiler consistently uses ordinary call order and checked map
construction. Independent assertions cover those exact sources; differential
constructor cases save `hash-map` as a function to exercise the shared runtime ABI.
The full typed user acceptance example passes the pinned compiler comparison.

The map milestone passes **410** typed fixture outcomes, **48** generated cases,
and **124** analyzer boundary classifications. Independent map transport probes
check counts, malformed entries, duplicate encoded keys, cycles, host objects and
nil/false/undefined distinctions. Runtime checks exercise actual hash collisions,
immutable escaped storage, shallow sharing and 512 retained update versions.
[Resident map measurements](../benchmarks/results/compiler-2026-10-09/maps-smoke/README.md)
record sizes 8/64/512 without claiming a speedup. See
[map reference evidence](../tests/reference/compiler-collections-runtime.md#structural-maps-2026-10-09)
for runtime probes and the deliberate source-emission distinctions.

### Immutable structural sets

Set literals (including quoted data), `hash-set` (zero or more arguments), and
`disj` (a collection and zero or more members) use private immutable member entries.
They share the map hash/equality search; native JavaScript `Set` is never language
set storage. `count`, `get`, `contains?`, `conj`, `seq`, `first`, `rest`, `next` and
`=` accept sets. `assoc`, `dissoc` and direct `nth` do not. `disj nil` remains nil;
its one-argument path preserves the supplied value, and its permissive zero-argument
runtime path returns nil. Removing a member from undefined returns nil, while
one-argument `disj` preserves undefined. Callable sets take a member and optional default (1/2
arguments); unsupported arities throw ordinary `Error` after evaluating arguments.

Membership returns the first retained equal member, including false, nil or an
undefined member; presence uses a private sentinel. Nil and undefined compare/hash
alike, as do equal list/vector/seq contents. Set equality and hashes ignore member
order, while sets remain unequal to sequences, maps, and host arrays/sets. Nested
maps and sets work as members and map keys. Unequal colliding hashes remain distinct.
Weak identity bookkeeping handles foreign object/function members without examining,
freezing or copying host data. No strong global hash or intern cache retains values.

Set count is O(1), lookup/copying updates O(n), construction of n distinct members
and naive unordered equality O(n²), before structural costs. New versions share
unchanged entry cells and nested immutable values. Initial seq materialization is
O(n), subsequent tails O(1); empty `seq`/`first`/`next` are nil and empty `rest` is
the empty list. Traversal order is unspecified. Tests check membership and visit
counts rather than requiring CLJS hash-trie order.

Duplicate readable set member forms report the repeated member's source span,
including executable forms such as `#{(observe 1) (observe 1)}`. Reader identity
still distinguishes integer and floating tokens. Dynamic distinct forms evaluate
once in source order; structurally equal results then throw Error, matching the pinned
checkout. Maven 1.12.134 instead deduplicates these dynamic literal results, and
the reference comparison retains both outcomes explicitly. The compiler macro
bridge checks readable duplicates before projecting native input data.

Ordinary `hash-set` and `set` constructors deduplicate structural members.
No-op `conj`/`disj` preserve the original set, metadata, and stored equal member.
Two other pinned distinctions remain explicit: upstream source set storage can
reorder operand effects, and direct `hash-set` inlining can retain reader-distinct
but Number-equal members (for example `1` and `1.0`). This compiler preserves source
operand order and structural constructor deduplication. Reader-distinct numeric constants that
round to one JavaScript Number retain the documented local deduplication policy. Exact divergent sources
have independent execution assertions; saved first-class constructors exercise
the shared reference ABI.
Undefined retained-member presentation follows the map distinction above.

The shared observer uses a separate `{"$set":[...],"count":N}` tag, checks count and
duplicate complete encoded members, and sorts only the encoded set members.
Sequences created from sets keep their sequence tag and order. Runtime assertions
cover collisions, hostile prototype/storage mutation, nested sharing and 512 saved
versions. This set implementation does not bundle `clojure.set`, sorted sets,
transients or mutable host-set conversion. Protocol implementations and selected
caller-supplied library functions are covered separately. See the
[set reference evidence](../tests/reference/compiler-collections-runtime.md#structural-sets-2026-10-09)
and [resident scaling samples](../benchmarks/results/compiler-2026-10-09/sets-smoke/README.md).

The [initial smoke measurements](../benchmarks/results/compiler-2026-10-09/vectors-keywords-smoke/README.md)
record a 7,335-byte runtime source, 104-byte primitive output, and 7,477-byte
`[1 2]` output. Fresh process compiler/startup samples are retained without a
speedup claim or threshold. At that milestone, focused verification passed 297 typed fixture cases
(244 retained plus 53 additions), 48 generated cases and 119 reference boundary
classifications against Maven ClojureScript 1.12.134, plus the independent
transport/runtime checks and a 140,000-element literal execution.
The subsequent symbol/list milestone passes 353 typed fixture outcomes, the same
48 generated and 119 boundary cases, and 19,771 direct runtime comparisons.
Its runtime source is 15,243 bytes; primitive output remains 104 bytes, and
repeated nested quoted literals add 111 bytes each with one embedded prelude.
See [runtime evidence](../tests/reference/compiler-collections-runtime.md#quote-symbols-and-finite-sequences-2026-10-09)
for exact sources, representation edge cases and reproduction.

### Delays and lazy sequence values

`delay` and `lazy-seq` compile their bodies as zero-argument closures.
`force`, `delay?`, `realized?`, dereference and `IPending/-realized?` are
first-class runtime operations; `Delay`/`->Delay` and `LazySeq`/`->LazySeq`
constructors retain their CLJS contract. Successful results are cached exactly,
including nil and undefined; throwing thunks retry. Reentrant realization uses
ordinary call/write behavior. Returned chains of lazy wrappers flatten
iteratively without growing the stack.

Metadata wrapping stays lazy and creates an independent pending state, sharing
source realization and cached hashes. `seq`, `first`, `rest`, `next`, `count`,
`nthrest`, `nthnext`, and `reduce` traverse lazy input on demand. An early
`reduced` result does not request another element. Empty/nested values,
content equality versus sequence identity, custom ISeqable effects, public
constructors/fields, host iteration/index methods and delay printing follow the
pinned runtime. See [the deferred-value evidence](../tests/reference/compiler-deferred.md)
for the exact source selection, reference runs, GC checks and measurements.

### Ordinary finite reduction and Reduced markers

`reduce` accepts a reducer and collection, or a reducer, initial accumulator and
collection. Other argument counts raise ordinary `Error` after argument effects.
It traverses nil/undefined, immutable vectors/lists/maps/sets, finite seq views
and strings. String elements are one UTF-16 code unit, including lone surrogates.
Maps supply vector entries; map/set traversal order remains unspecified. Later
core protocol and deferred-value support also admits host arrays, lazy/unbounded
sources and custom reduction protocols; reduced termination bounds traversal.

Without an initial accumulator, an empty source calls the reducer's zero arity,
a singleton returns its element without a call, and other sources start with
their first element. With init, an empty source returns that exact value without
calling the reducer. Each step invokes `(rf).call(null, accumulator, item)`;
keyword/symbol/map/set/vector callbacks and fixed user functions keep their
ordinary callable behavior. Missing fixed user parameters receive undefined,
and extra parameters are ignored. There is no one-argument completion call.
The function argument is evaluated once; global redefinition during a step does
not replace it. Its `.call` property is read afresh for each invocation. Source
and callback exceptions propagate unchanged and prevent later steps.

`reduced` creates a private immutable marker; `reduced?` recognizes only those
markers, `unreduced` removes one layer if present, and `deref` removes one layer
from a marker. The same public `deref` also dispatches on Vars, atoms, volatiles
and custom `IDeref` implementations, or throws `Error` for unsupported values. These are fixed one-argument functions:
missing arguments bind undefined and extra arguments evaluate then are ignored.
Only a marker returned **by a step** stops reduction and loses one layer. An
explicit init, singleton element or empty-source zero-arity result is retained
exactly, including nested markers. Markers compare by identity, keep their brand
and payload slot private, and are observed through these operations; raw markers
in returned values or traces are harness errors, never implicit maps.

`+` is an ordinary first-class function: zero arguments yield 0, one returns the
argument unchanged, and two or more use left-to-right JavaScript addition.
Loop and method recurrence has a resolved target independent of value use. Each
iteration exposes fresh lexical constants and evaluates every recur argument before
updating its next state. This preserves swaps, argument effects, and closures
retained from earlier iterations. Nested functions/loops cannot target each other;
non-tail positions and cross-try recurrence are located analysis errors. Variadic
methods recur with one explicit rest sequence, without re-packing it.

Protocol methods accept either all parameters or all non-receiver parameters;
the receiver expression still runs when supplied, while the method's `this`
identity stays fixed. IFn must satisfy both upstream `.call` and direct entry
signatures and therefore excludes the receiver from recur arguments. The pinned
IFn `.call` capture-slot failure after recurrence is retained, separately from
the direct invocation entry. Method recurrence also updates observable supplied
`js-arguments` slots as upstream's non-strict parameters do. See the
[recurrence audit](../tests/reference/compiler-recur.md) for probes and limits.

Number rounding, overflow, NaN, infinities and signed zero follow the host;
source literals use the same versioned Number rounding and symbolic values. It does not
use the native interpreter's checked integer arithmetic. There is no special
source-spelling dispatch or optimizer for direct calls.

Compiler-emitted macros use those same canonical runtime operations in QuickJS.
For example, `(/ 1 2)` returns `0.5`, `(= 1 1.0)` is true, and
`(inc 9007199254740992)` stays `9007199254740992` during expansion and in an
application. Computed expansion data retains Number rounding at each operation,
including signed zero and non-finite values. Reader integer/decimal distinctions
remain source data for diagnostics and duplicate checks; wide radix integers
round once at execution, consistently across the reader/form bridge and emitter.
Non-integral ratio source literals remain rejected; ordinary division produces
fractional Numbers. No Rust exact/ratio arithmetic profile serves JS macros.
The remaining native embedding adapter is transitional and is retired by
`light-cljs-115`. The [numeric phase audit](../tests/reference/compiler-numeric.md#compile-time-and-application-number-semantics)
records the shared bodies, pinned key behavior and verification limits.

Traversal is iterative, uses direct immutable storage offsets and allocates no
captured closure or seq wrapper per step. Map/set traversal may materialize O(n)
entry/member storage once; vectors/lists/seq views need O(1) traversal state.
Temporary storage stays local and private brands use weak maps. Runtime execution
uses host resources/GC and the existing test subprocess timeout; the current
native macro host has separate fuel/cancellation limits. `reduce-kv` dispatches
`IKVReduce` and supports owned maps and vectors. `transduce` supports both
three- and four-argument forms; `completing` adds completion to a step function,
and `ensure-reduced` preserves an existing Reduced marker. Without an explicit
initial value, `transduce` calls the original reducer's zero-argument form before
applying the transform. Reduction removes one Reduced layer, then invokes the
transformed reducer's completion once. Throwing initialization, transforms,
source realization or steps prevent subsequent completion. Engine cancellation
remains terminal for the session.

Transducer constructors include `map`, `map-indexed`, `keep`, `keep-indexed`,
`filter`, `remove`, `take`, `drop`, `take-while`, `drop-while`, `take-nth`,
`partition-all`, `partitionv-all`, `partition-by`, `cat`, `mapcat`, `interpose`,
`replace`, `dedupe`, `distinct`, `random-sample` and `halt-when`. Their state is
fresh each time a constructor receives a reducer. Reducer initialization forwards
to the downstream reducer without resetting existing state. Buffered partitions
clear before emitting, including during completion and reentry; completion
unwraps one marker from a final partial partition before forwarding. `cat`
preserves downstream Reduced layers through each nested reduction and stops
before realizing another inner item. `halt-when` preserves the upstream public
`::cljs.core/halt` return convention and optional completed-result callback.

All reducers, mapping functions and source reductions use the ordinary canonical
IFn/IReduce ABI in JavaScript. Transforming `into` uses persistent destinations
and retains metadata. Compiled macros execute these same helpers and can export
persistent expansion forms. The [transducer audit](../tests/reference/compiler-transducers.md)
records exact reference and local verification; native exact-number and
JVM-specific constructor differences are superseded by CLJS Number semantics.
`sequence` transducer arities and `eduction` remain separately assigned.
Reader and language support add
`apply`, recurrence, multiple/variadic functions and Vars; `@` uses the shared
reference and `IDeref` dispatch described above.

The [reduction audit](../tests/reference/compiler-collections-runtime.md#finite-reductions-2026-10-09)
records the pinned differential outcomes and resident prebuilt-vector measurements.

## Exceptions

`throw` evaluates one value and throws that exact JavaScript value, including
nil, undefined, booleans, numbers and strings. `try` evaluates its body in order;
typed catches test their classes in source order using JavaScript `instanceof`.
The first match runs with a fresh lexical exception binding. A final `:default`
catch accepts any thrown value; an unmatched value is rethrown unchanged.
Catch bodies may be empty, and closures retain their exception binding.

`finally` runs after successful bodies, caught failures, unmatched failures and
throwing catches. Its ordinary result is discarded, preserving the pending
return value or exception. A throw from `finally` replaces that pending result.
These rules apply to initializers, callbacks, method bodies, discarded forms,
function returns and value expressions. A single clause-free `try` adds no
JavaScript argument scope; a handled value expression uses the same IIFE scope
as upstream. Recurrence cannot cross any try/throw boundary, while loops and
functions declared inside a body, catch or finally have their own targets.
Malformed clause ordering, missing catch bindings, invalid binding names and
wrong throw argument counts produce located analysis errors.

`ex-info` accepts message/data or message/data/cause and constructs an
`ExceptionInfo` that is also a JavaScript `Error`, with the original payload
identities intact. Its two-argument form supplies a nil cause; an explicit
undefined cause stays undefined. `ExceptionInfo.`, qualified core classes,
namespace aliases and saved core function values use the same resolved class.
`ex-data` and `ex-cause` return fields only for that class; `ex-message` returns
the message of any JavaScript Error, with nil for other inputs. Class identity
also survives syntax quoting and macro expansion. `instance?` supports both
direct calls and saved function values, preserving upstream class/value
evaluation order. `pr-str`, `str` and `.toString` print the CLJS `#error` form.
`Throwable->map` exposes the cause chain, type/message/data maps, root cause,
optional phase and nil trace used by the upstream exception tests.

The [exception evidence](../tests/reference/compiler-language/evidence.md#exceptions-member--2026-10-09)
records exact source assertions, independent outcome probes, negative syntax
tests and output measurements. The state audit additionally verifies unchanged
exception identity through validators, watches and state protocol callbacks.

## Context, truthiness, and the macro boundary

Expressions carry `Value`, `Discard`, or `Return` context during analysis. The
emitter propagates those contexts recursively: conditionals can be ternary
expressions or statements; bodies discard earlier values and return their last
value when needed. `let*`, `letfn*`, `loop*`, `try`, `case`, and `do` in value position use small immediately invoked
functions. This keeps evaluation order and avoids hoisting expressions
out of branches or call arguments.

The generated unit has a local truth helper, testing `value != null && value !==
false`. Thus `0`, negative zero, and empty strings are true. The nullish check
also handles `undefined` when a compiled function is invoked by a host harness.
Evaluating output produces the program's value; explicit host calls such as
`js/console.log` can print.

List analysis dispatches primitive special forms before touching binding vectors
or other operands. The macro hook then resolves and executes macros in the native
Rust interpreter, with the current lexical environment and expression context,
before ordinary call analysis. It expands each call once, then analyzes its result
and any nested calls. Lexical bindings shadow unqualified macros. Explicit macro
imports take precedence over same-named runtime definitions; runtime definitions
shadow the fallback core macro names. Qualified macro calls bypass simple lexical
shadowing. `cljs.core/` and `clojure.core/` select the same supported native core
expanders; this does not expose interpreter functions to the JS runtime.
Unknown names remain unresolved.

The parser remains unchanged. Original calls are lowered from their retained
`SyntaxTree` into shared owned `Form` data. Returned data is appended directly to
a private copy of the syntax arena, with indexed spelling/value/Form side tables;
there is no printing, reparsing, JVM macro process, or JS macro executor. Generated
forms retain their call-site spans and macro attribution. Original input objects
retained in the result keep available source provenance. Scalar/generated leaves
may use the entire call span. Analysis never mutates the caller's syntax tree.
A source is retained once per analyzed runtime namespace when a macro needs it.

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
emits invalid JavaScript for this shape. Definitions accept an unqualified name
(including `&`), an optional docstring and initializer, and definition metadata.
A dotted definition such as `(def host.value 1)` assigns the actual host path;
the host root must already exist. Its Var and subsequent `set!` retain that live
host storage. `defonce`
initializes only undefined roots; existing nil/false roots suppress its initializer.
`declare` supports forward calls and preserves existing definition metadata.

Resolution is source ordered: a definition is visible to its own initializer
and later analyzed forms. The default API rejects undeclared/forward references,
including inside functions and unselected branches. The explicit warning API
retains the standard compiler's warning-and-emit behavior for these forms. An explicit preceding
`(def name)` or `declare` permits a later assignment to that identity. No namespace/global
state survives across separate calls to `compile` or `compile_namespace`.

## Definition metadata and temporary roots

`binding` and `with-redefs` save current global roots, evaluate their initializers
in parallel, install temporary roots, and restore them in reverse order through
`finally`. Nested calls, exceptions, core roots, aliases and namespace readers
share the same storage. Initializer failures occur before the restoration region,
so initializer side effects remain. `set!` returns its assigned value; constants
and lexical locals are rejected. Binding a non-dynamic global remains permitted,
as in the pinned compiler's warning-and-emit behavior.

These scopes are synchronous. A callback invoked inside a binding sees the
current roots; a later Promise callback sees restored roots. Captured lexical
values remain snapshots. Explicit boolean tags on globals preserve the pinned
JavaScript truth behavior; untagged dynamic globals retain CLJS truthiness.

`var`/`#'` creates an explicit callable object with a live getter, qualified symbol,
symbol-based equality/hash and captured definition metadata. Repeated Var
expressions compare equal without being identical. `with-meta` preserves the
getter while replacing metadata. `cljs.core/Var.`, its public fields and
`isMacro`, `var?`, `hash`, `deref`, invocation and `test` are supported.
`resolve` requires a quoted symbol and handles namespaces, aliases and locals;
its runtime existence check can observe later definitions. Unknown-at-analysis
names retain nil metadata. Core argument lists and docs come from a checked,
pinned data table and do not load an external cljs.core in generated programs.
Definition test callbacks follow the pinned value/return context behavior.

`print` and `with-out-str` support the original Var-through-case/apply regression,
including rebound print sinks, nested capture and cleanup. Unused definition
metadata does not pull the collection runtime into primitive-only output.
The [definitions audit](../tests/reference/compiler-definitions.md) records full
upstream witnesses, independent probes, diagnostics and reproduction commands.

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
hyphens become underscores. `.cljs` takes precedence over `.cljc` across all
explicit roots, matching upstream extension precedence. Canonical duplicate
roots/files are deduplicated. Two distinct files at the winning extension remain
an ambiguity error under the existing explicit-root API; root order never silently
selects one. This deliberately differs from upstream classpath first-match behavior.

Each file must begin with `(ns NAME ...)` and declare exactly its requested
namespace. Names and aliases contain dot-separated ASCII components matching
`[A-Za-z_][A-Za-z0-9_-]*`; `js` is reserved. Supported require clauses are:

```clojure
(ns demo.main "Example" {:author "team"}
  (:refer-clojure :exclude [+] :rename {count size})
  (:require demo.side [demo.math]
            [demo.util :as u :refer [value calculate] :rename {calculate compute}]))
```

Aliases and full names of directly required namespaces can qualify globals.
Only one `:require` clause may appear in a namespace declaration.
The current namespace can qualify its own globals. Transitive-only and unknown
qualifiers are errors in the strict API. Repeated identical imports coalesce; aliases that conflict with another target or redirect a
required/current full namespace name are rejected. `cljs.core` is a virtual dependency: `(:require [cljs.core :as c])`
never reads `cljs/core.cljs`. Reserved `cljs.core` and `clojure.core` qualifiers
cannot be redirected to user libraries. Ordinary `:require` imports both macros
and values from the same `.cljs` or `.cljc` source. `:refer` and `:rename` retain the original namespace
identity, including saved values and syntax quotes; lexical bindings and own
definitions can shadow unqualified imported values. `:refer-clojure` exclusions
and renames affect both core functions and core macros; explicit core qualifiers
remain usable. Missing and conflicting refers fail even when unused. The equivalent
`:use [library :only [names] :rename {...}]` clause is
supported, with `:only` required. Namespace
metadata merges symbol metadata with the optional attribute map; docstrings are
available separately at `&env :ns :doc`. `:as-alias` is a reader-only alias and
does not load a source. A missing `clojure.*` runtime source can resolve an
existing `cljs.*` source while preserving both qualifiers. `:import` binds actual
host constructors and `:require-global` binds supplied host globals. Top-level
quoted require/use/import and macro equivalents, `refer-clojure`, and
`refer-global` retain their namespace effects. Explicit `goog.*` dependencies
can use host namespaces when no caller source exists; no Closure implementation
is synthesized. JavaScript package acquisition remains outside this loader.
Validation occurs before
dependency analysis, with diagnostics in the originating file.

The [namespace evidence](../tests/reference/compiler-namespaces.md) records
upstream anchors, differential checks, diagnostics and output cost.

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

### Complete collection program

[examples/collections](../examples/collections) and its identical differential
fixture contain `batch.data`, `batch.ops`, `batch.main`, and `batch.macros.cljs`.
The ClojureScript oracle uses a preserved dialect copy under
`tests/fixtures/reference-namespaces/collections`.
`main` requires both `data` and `ops`; `ops` also requires `data`, so the shared
dependency initializes exactly once. The macro-generated original map prints
`data`, followed by the `ops` and `main` initializers, before the exported `run`
function is called. The collection runtime is initialized before all three.

```sh
cargo run --locked --quiet -- --compile --entry batch.main \
  --source-root examples/collections > /tmp/light-cljs-collections.js
node --check /tmp/light-cljs-collections.js
node -e 'const run=eval(require("fs").readFileSync("/tmp/light-cljs-collections.js","utf8")); const result=run(x=>x); if(result==null) throw Error("missing result");'
```

`batch.macros/append-item` binds its map and key once, then emits explicitly
qualified `cljs.core/assoc` and `cljs.core/conj` with a keyword lookup call.
`batch.ops/total` uses the `cljs.core` alias and first-class `reduce`/`+`.
`run` returns `[[1 2] [1 2 3] :pair true]`, preserving the original map/vector;
`report` returns `[6 batch/data true "batch.data"]`. The host `observe` callback
receives `report`, quoted generated nested data, its equality with authored data,
then `"map"`, `"key"`, `"item"`, and the equality of the authored/generated update.
These observations use the existing typed test encoder; program source needs no
printing/JSON library. Lists, symbols, keywords, vectors, maps and sets keep their
types through macro conversion and JavaScript execution.

The single expression contains one runtime prelude plus the program, with no
module imports, global installation or native runtime callback. Two emitted
units have separate runtime brands/globals even when compiled with one reused
`MacroHost`; host macro roots/reload behavior remains as documented below.
Macro diagnostics attribute each expansion to its definition; the tests count
one initializer expansion per compilation and preserve compile-time limits and
cancellation. Macro-generated `delay` and `lazy-seq` bodies execute as deferred
closures. Malformed deferred bodies and record/type forms retain macro identity
and located empty-output errors. Multiple/variadic macro-generated
functions execute; reader values,
including regexes and large numeric literals, now survive macro conversion.

The reference launcher adds explicit macro source roots to `-Sdeps :paths` for
both Maven and selected-checkout builds. Its optional program export argument
loads the entry and exposes `batch.main/run` with the same shallow typed adapter
used by individual cases. Program loading happens inside the result runner,
so namespace stdout and run effects are checked together. Each observed program
runs in a fresh Node process, twice; release/settings validation, strict result
comparison and retained failure artifacts still apply. The
[collection runtime evidence](../tests/reference/compiler-collections-runtime.md)
records measured output size and commands.

## Compiler macros and embedding

### Macro phase contract

Macros are compile-time bindings in `.cljs`/`.cljc`, imported alongside runtime
functions through ordinary `:require`. Unmarked helpers initialize only on actual
compile-time execution demand, including higher-order and Var paths. Both phases
select `:cljs` and use canonical JavaScript Number operations, with separate roots
and state. Ordinary ECMAScript operations become available during expansion;
external host facilities remain explicit. A reused host starts each clean
compilation with fresh source-defined mutable roots.

The [migration design](evaluator-migration.md) specifies source order, declaration
bundles, bootstrap, structured forms, reentrancy and cleanup. Compiler namespace
loading now uses this shared source contract. `:require-macros`, `:use-macros`,
`:refer-macros` and `:include-macros` produce located migration diagnostics. Move
macro declarations and helpers to `.cljs`/`.cljc` and use ordinary `:require`;
compiler source lookup never loads `.clj` companions.

### Compiled core bootstrap

Core macro algorithms are first-party `.cljs` declarations compiled by the Rust
analyzer/emitter and executed in QuickJS. A fresh heap is created for each clean
compilation; only immutable emitted unit strings are reused in memory. Pure
binding/destructuring, syntax quote, declarations and namespace reflection remain
compiler services. `MacroHost::disable_native_evaluation()` checks this boundary.

Same-file source `defmacro` bodies in `.cljs` and selected `:cljs` branches of
`.cljc` execute compiler-emitted functions in that session. They can call
canonical core operations and ECMAScript intrinsics. Fixed, multiple, variadic
and destructured methods receive hidden `&form` and `&env` before user arguments;
docs and name/leading/trailing metadata are retained. A declaration returns nil
and emits no application function, Var or initializer.

Declarations are processed in source order at top level and inside top-level
`do`, including declarations generated by macros. Runtime conditionals and
lexical/function bodies reject macro declarations with a located placement error.
A later same-kind declaration creates a new macro generation. Already analyzed
functions and macro bodies retain their expanded code from earlier generations.
Undeclared forward macro calls fail, including in untaken branches. `declare`
reserves an ordinary Var and conflicts with a later macro of the same name;
macro/value kind changes likewise require distinct names.

Lexical functions shadow unqualified macro calls; explicit namespace-qualified
macro calls preserve macro identity. Runtime value, `apply`, Var, `resolve`,
dereference and assignment uses are phase errors with the suggestion to invoke
the macro directly in source. During expansion, `&env :light-cljs/macros` maps
qualified symbols to descriptive declaration metadata, including user `:arglists`
and `:light-cljs/generation`; it provides no callable macro or runtime Var.
A clean compile refreshes source macro state even when its `MacroHost` is reused;
assertion settings, evaluator options and expansion limits persist.

Ordinary top-level `def`, `defn`, `defonce`, `declare` and macro-generated
equivalents register compile-time roots without executing their initializers.
Value reads, calls, closures, collection-held functions, `apply`, Var dereference
and invocation share a JS demand getter. Quote, Var identity/metadata, `resolve`
without consumption and untaken branches do not initialize roots. Functions are
ready before their bodies execute, so self/mutual recursion through preceding
ordinary declarations works. A declaration without an encountered initializer
reports a located unavailable-definition error when consumed.

Root initialization is pending → initializing → ready or failed. Initializer
cycles report the root path. Failure and effects remain cached within that
generation; an ordinary redefinition or fresh session permits retry. Saved
function values retain code/captures and read active ordinary global generations.
`defonce` preserves pending/ready/failed roots, including nil, false and a pending
initializer that eventually yields undefined. Declaration-only roots admit their
first `defonce`; application `defonce` retains its undefined-root check.

`binding` and `with-redefs` evaluate all initializers in the outer environment,
install overlays and restore them in reverse order on success or exception.
Saving/restoring a compile-time root preserves its exact pending/ready/failed
state without forcing it. `set!` writes an overlay or demands and changes the
ordinary root. Executed nested definitions affect only that phase. Standalone
source expressions run only in the application, and application initialization
retains source order in its own heap. See the [executing demand evidence](../tests/reference/compiler-helper-demand.md).

Top-level `defprotocol`, `deftype`, `defrecord`, `extend-type` and
`extend-protocol` are demand-initialized declaration bundles, including forms in
top-level `do` and macro-generated declarations. Protocol descriptors/method Vars,
type descriptors/factories and extension registrations share one bundle state.
Method bodies expand and analyze once using the declaration-site snapshot; the
same IR supplies compile-time and application emission. Demanding any product
installs the whole bundle. Compiler-emitted descriptor/factory linking can use
staged identity; ordinary unfinished value reads report a bundle cycle.

Protocol invocation (including saved functions and higher-order calls), marker
membership, applicable core operations and Object member access demand relevant
source-visible extensions. Known descriptor/native selectors narrow that index.
Computed target aliases remain candidates for their resolved protocol and are
demanded only when a query needs their applicability. An unrelated protocol's
extensions do not initialize. Earlier deferred macro expansions retain their
extension frontier, even after newer dispatch has executed. Dispatch uses the
canonical phase-local tables; replacement generations and frontier changes apply
owned registration patches without rerunning user initializers.

Failure rolls back staged products and dispatch patches and stays sticky for that
bundle generation. A matching new extension declaration or clean compilation can
repair it; user/host effects already performed remain performed. Cancellation is
terminal for the compilation heap, and reused hosts allocate fresh roots/tables.
Application declarations still execute in source/dependency order in their own
heap. See the [executing bundle evidence](../tests/reference/compiler-declaration-demand.md).

`reify`, `specify!`, nested declarations and low-level `deftype*`/`defrecord*`
inside executed code use ordinary canonical expression semantics. Arbitrary
standalone expressions, including top-level `specify!`, are application effects
and are never replayed as macro setup. They are retained in application output.
This classification is syntactic declaration admission, not an effect inference.

An ordinary binding shadowing a core name cannot silently call the core
implementation from a macro body.

### Shared source namespace imports

```clojure
;; demo/macros.cljs (or .cljc, selecting the :cljs reader feature)
(ns demo.macros)
(defmacro twice [x]
  `(let [value# ~x] (js/Math.imul value# 2)))

;; A runtime .cljs source
(ns demo.math (:require [demo.macros :as m :refer [twice]]))
(defn scale [x] (m/twice (twice x)))
```

Run the complete two-runtime-namespace example:

```sh
cargo run --quiet -- --compile --entry demo.main --source-root examples/macros > /tmp/macros.js
node /tmp/macros.js
# 12
```

Each ordinary `:require` library may be a bare symbol or a vector with
`:as`, `:refer [simple-names...]`, and `:rename {original local}`. Aliases and full
imported namespace names qualify both values and macros. A referred or renamed
macro retains its declaration identity, and a namespace can export macros beside
ordinary runtime definitions. Macro bodies can require and call ordinary helpers
from their own or another namespace; helpers initialize only when compilation
actually demands their values. Source-declared macros have no application Var,
function or initializer. Runtime value, `apply`, Var and assignment uses receive
located phase errors.

The compiler's explicit roots prefer `.cljs` over `.cljc`; multiple distinct
canonical matches at the winning extension are ambiguous. Both phases select
`:cljs` and the usual `:default` branch before loading dependencies. Inactive
`:clj` branches cannot add dependencies. Same-name `.clj` companions are never
consulted, and no companion self-import or macro inference syntax is needed.
Missing macros and helpers therefore report the selected source location rather
than falling back to a native namespace.

Dependency namespaces are analyzed before their dependents. Their shared semantic
IR supplies compile-time demand units and application emission. Ordinary helpers
and declaration bundles retain the demand, generation and failure rules above;
standalone source expressions run only during application initialization. Macro
and application heaps keep separate roots, mutations and dispatch tables. A clean
compilation starts fresh source-defined mutable state even with a reused host.

Core expansion supports `let`, `fn`, `defn`, `defn-`, `defmacro`, `if-not`, `when`, `when-not`, `when-first`, `cond`, `condp`,
`and`, `or`, `->`, `->>`, `as->`, `cond->`, `cond->>`, `some->`, `some->>`,
`if-let`, `when-let`, `if-some`, `when-some`, `loop`, `case`, `letfn`, `while`,
`dotimes`, and `assert`. This is an
expansion surface with executable binding, condition and recurrence support.
Destructuring evaluates each initializer once, preserves indexed-access versus
sequence traversal, and supplies emitted `seq-to-map-for-destructuring` for
keyword argument lists, singleton maps and trailing maps. `:or` is an eager
`get` argument: missing keys use defaults, while present nil and false are retained.
Sequential bindings support nil, lazy inputs, host arrays and custom sequence or
indexed protocol receivers. Mapped long vectors realize bounded chunks.

`fn`/`defn` pre/post maps and parameter metadata check conditions in declaration
order around one body evaluation. Postconditions bind `%` to that result; only
nil and false fail. `assert` throws a JavaScript Error with the condition text;
its optional message runs only on failure. The same lowering handles all function
arities and destructured protocol/IFn/Object methods, retaining receiver fields
and lexical captures. Postcondition bodies cannot recur across the result binding.

Assertions are enabled by default. Call `MacroHost::set_assertions(false)` before
`compile_with_macros` or `compile_namespace_with_macros` to omit assertion
conditions and messages entirely, including unresolved forms and pre/post checks.
The setting persists on a reused host, can be enabled again, and is reflected in
the macro interpreter's dynamic `clojure.core/*assert*` Var so source macros can
choose whether to generate checks. Macro-side `assert` and explicit `macroexpand`
also honor that dynamic Var. It is a compilation setting, not an emitted
runtime toggle. The default CLI and convenience compilation functions enable it.
`cljs.core` aliases and referred/renamed macros retain their resolved core identity.

`case` checks duplicate constants after compiled macro conversion. JavaScript
Number treats integer and floating spellings such as `1` and `1.0` as the same
constant; that pair is rejected. Keyword-only no-match errors use the keyword's full name
without its colon (or the empty string for a non-keyword selector), matching the
upstream optimization. Grouped branches expand once. Public `case*` requires a
symbol selector, vectors of test groups and branches, and number/string or
`:const` global tests; omitting its default produces nil. The
[control audit](../tests/reference/compiler-control.md) records executable
coverage, diagnostics and output sizes. Lazy `for` and `doseq` traversal remain
part of the separate sequence feature.

Ordinary defaults follow the Maven release evaluation order. The pinned checkout
moves certain explicit-map defaults before their initializer. Named types resolve only their fields, parameters and namespace Vars; they do
not capture enclosing lexical bindings. Their initial method map is ordinary
body code, matching both reference compilers. This supersedes the earlier bridge's
local condition-checking/capture behavior; the reify-specific historical probe
remains separate. See the [type audit](../tests/reference/compiler-types.md) and
[destructuring evidence](../tests/reference/compiler-language/evidence.md#destructuring-and-conditions-member--2026-10-09).
Extended map directives `:keys!`, `:syms!`, `:strs!`, `:select`, `:all`,
`:missing`, `:excess`, and `:defaults` preserve their checkout binding,
evaluation-order and failure behavior. `selector` uses the same compiler macro
bridge. Release differences are executed and retained explicitly in the
[destructuring audit](../tests/reference/compiler-destructuring-completion.md). The full native macro runtime is not exposed to emitted programs.
The finite sequential/named-value core does not expose the interpreter's full collection
or arithmetic runtime. Metadata/docstrings consumed by
macros remain available during expansion; runtime function metadata is evaluated
and attached when the function is created.

Collections, keywords, quoted forms, and shared semantic reader constructs may
be passed as unevaluated macro data. Generated vectors, keywords and quoted
symbols/lists/vectors/maps/sets execute; unsupported generated reader data still fails analysis. Auto-resolved keywords and qualified syntax-quote symbols use the
shared namespace alias view. `.cljs` reader conditionals are rejected, including inside macro calls;
`.cljc` branches select `:cljs`/`:default` before macro lowering in both phases. Syntax quote resolves current runtime globals,
refers, renames, exclusions and macro imports through the compiler's namespace
view without changing interpreter namespace aliases. `#=` is never enabled.

`compile`, `analyze`, and `compile_namespace` create fresh isolated macro hosts.
The host is initialized lazily, so primitive programs do not initialize an
interpreter. Use `compile_with_macros`, `analyze_with_macros`,
`compile_namespace_with_macros`, or `analyze_namespaces_with_macros` for an explicit
`MacroHost`. Runtime global storage is always fresh per compilation, even with a
reused host. Namespace compilation installs a file provider for its explicit
roots. Each clean compilation re-reads its required sources and creates fresh
compile-time roots. Definition snapshots preserve already-expanded code inside
that compilation; edits, import changes and conditional selection cannot reuse
stale application IR from a previous compilation.

The separate native embedding adapter remains available to explicit host callers.
Embedders may use `host.context_mut()` to evaluate/register macro code or configure
native limits/effects, then `set_imports(caller, Vec<MacroImport>)` to validate
already-loaded imports. `host.register_macro(&NativeDefinition)` installs a host
macro using the same ABI: callback arguments are `&form`, `&env`, then unevaluated
source arguments; declared arity includes those two implicit arguments. Native
namespaces registered this way do not require filesystem sources. The lower-level
`Context::expand_macro_call` accepts an explicitly resolved macro Var and a custom
environment without applying SCI's flat lexical-map convention.

The compiler's supported `&env` map follows the pinned CLJS analyzer:

| Key | Supported value |
| --- | --- |
| `:locals` | Symbol-to-binding map for visible lexical names. Each record contains `:name`, `:form`, `:op :binding`, `:local :let/:arg/:fn`, and stable per-program `:light-cljs/id`. |
| Binding details | Let/argument records have `:binding-form? true`; arguments have zero-based `:arg-id`; shadowed bindings appear under `:shadow`. Fn self bindings omit `:binding-form?`. |
| `:context` | `:expr`, `:statement`, or `:return`. Fn bodies and value-position let bodies return; a multi-form value-position `do` also has a returning tail. |
| `:ns` | `:name` caller symbol and `:doc`; `:requires` maps ordinary required namespaces and aliases to their namespace identities; `:uses` maps referred and renamed macro names to their declaring namespaces. `:require-macros` and `:use-macros` remain empty compatibility descriptions, not accepted import syntax. |
| `:line`, `:column` | One-based call-site location, also attached to `&form` metadata. |

`*ns*` is dynamically bound to the caller during expansion and restored afterward,
including after failures. This is a bounded CLJS environment contract, not the
entire upstream analyzer AST: `:defs`, inferred types, compiler internals,
`:info`, binding source locations and other keys are absent. This compiler emits
an expression with a returning final form; standalone/entry tails consequently
have `:return`, rather than CLJS's usual top-level statement context. The
[reference evidence](../tests/reference/compiler-macros.md) records the source
anchors and observed distinctions from SCI's environment.

Default `MacroLimits` allow 1,024 expansions and 100,000 generated form nodes per
compilation, with 64 nested macro expansions (also the hard cap). Generated
runtime analysis has a 128-frame nesting bound. `evaluator_options` configures
QuickJS memory, stack, timeout and shared polling bounds. Failures discard the
JavaScript bootstrap session; the next clean compilation creates a new one.
Native macro calls/load operations
also obey interpreter limits; the default host uses 1,000,000 fuel units and
2,000,000 allocation units per interpreter operation. Embedders may lower these
limits or supply cancellation/poll callbacks through the context. Failures are
explicit, and no partial JS is emitted. Arbitrary native callbacks remain subject
to the interpreter's cooperative cancellation limitations.

Enable `host.context_mut().set_macro_diagnostics(true)` to retrieve
`macro_expansion_stats()`: macro name, definition location when available,
expansion count, generated node count and elapsed native expansion/conversion
time. Diagnostics default off; no expansion clock reads occur in that mode.

## JavaScript interop

| Form | Behavior |
| --- | --- |
| `js/Math`, `js/Math.sqrt` | Resolve a host global or property path. |
| `(js/Math.sqrt 256)` | Call a JavaScript path, retaining its final receiver. |
| `(.-length "hello")` | Evaluate the receiver once, then read a property. |
| `(.slice "hello" 1)` | Evaluate receiver, look up method, evaluate arguments left to right, and invoke with the receiver as `this`. |

Host roots are properties of `globalThis`; missing roots throw `ReferenceError`.
Missing properties produce `undefined`; accessing a nullish intermediate has
normal JavaScript failure behavior. Dot members and JS paths use CLJS munging
(`a-b` becomes `a_b`); computed string keys and `#js` object keys retain their
spelling. Unicode member names are emitted safely as quoted properties. `js/x`
resolves to a same-spelled lexical `x`; a dotted `js/Math.sqrt` keeps its host root.
Namespace `:refer-global :only` and `:rename` expose explicit host globals.
Unqualified dotted names such as `console.log` also resolve host paths when
the root is not a lexical binding. Such host calls retain the final receiver;
a lexical root instead uses ordinary CLJS function-call receiver behavior.
Munging precedes path splitting, including repeated dots, and qualified dot
members such as `(.a/b object x)` invoke `b` on `object.a`. A qualified
`(.-a/value object)` likewise invokes `object._a.value()`; the leading hyphen
belongs to the munged namespace path, whereas unqualified `.-value` reads a field.

All three explicit dot forms work: `(. object -field)`, `(. object method args)`
and `(. object (method args))`. `..`, `memfn`, `new`, constructor shorthand,
property `set!` and its three-operand spelling are supported. A method read keeps
its receiver until invocation; receiver evaluation and getters precede arguments,
and each source operand runs once. Saved ordinary function values retain their
existing `.call(null, ...)` behavior. Qualified `Type/.method` values call the
prototype method with their first argument as receiver; `Type/new` uses
`Reflect.construct`. Namespace/global identity is retained through returned values.

`js*` accepts literal JavaScript with positional placeholders and expressions, or
named placeholders resolved in the current lexical/namespace context. Analysis
checks the template type, closed placeholders and exact positional arity before
emission. Literal segments remain literal and generated identifiers are hygienic
against those segments. Statement templates emit in statement context; value
and return contexts retain their actual function scope. The compiler does not
parse JavaScript source strings. `this-as`, `js-arguments`, function metadata and
`apply` preserve the pinned function-context behavior, including non-strict
parameter/arguments aliasing. See [the interop audit](../tests/reference/compiler-interop.md)
for the reference's constructor-expression precedence quirk.

Host helpers include `array`, `aget`, `aset`, `alength`, `aclone`, `make-array`,
`into-array`, `to-array`, `amap`, `areduce`, `unchecked-get`, `unchecked-set`,
`js-obj`, `js-keys`, `js-delete`, `js-in` and `js-invoke`. Saved array/object
function values and `apply` use the same host operations; direct indexing keeps
getter effects between computed indices. Private checked-array helpers retain
value/error modes used by the upstream array-access tests. `js-obj` retains its
macro's computed-key/literal/symbol grouping and duplicate-key behavior.
`#js` keeps array holes and ordinary object-literal prototype-key behavior.
Generated type fields use munged host properties; writes remain visible to method
field reads and record lookup, with independent fields on metadata copies.

All helpers use browser JavaScript APIs. Npm/package resolution and bundling
remain outside the compiler. Literal `nil` dot receivers are rejected during
analysis as in the reference; receiver expressions returning nil fail at runtime.

## Verification

### Executable language inventory

This is the existing selected-scope gate. Its strict result does not establish
full intended-language completion: the current inventory excludes, among other
families, transients, transducers, sorted collections and multimethods. Those
remain assigned implementation work. The
[broader coverage contract](evaluator-migration.md#language-coverage-and-completion-boundaries)
requires separate parsing, lowering, native, canonical JS, macro-execution and
execution-evidence accounting; that ledger expansion is planned in
`light-cljs-116`, with language completion in `light-cljs-125`.

The [assertion inventory](../tests/reference/compiler-language/README.md) retains
complete bodies and assertion-level source spans from eighteen pinned upstream
files. Its [executable report](../tests/reference/compiler-language/evidence.md)
distinguishes **covered**, **missing**, **verified rejections**, **verified warnings**,
verified **version differences**, and
**unrelated upstream behavior**. The [arity audit](../tests/reference/compiler-functions.md)
adds 49 passing assertion witnesses for multiple/variadic methods and their finite
`apply` interactions. The [integration audit](../tests/reference/compiler-language-completion.md)
combines the selected feature sections; each assertion requires its exact witness
to pass. Verified invalid-source rejections have a separate disposition and never
count as runtime passes.
The [apply audit](../tests/reference/compiler-apply.md) extends this to all 143
selected apply/combinator/invocation assertions, including infinite argument
tails, metadata, IFn receivers, and 228 independent outcome/effect probes.
Genuinely malformed-source diagnostics remain separate negative tests.

```sh
python3 -B tests/reference/compiler-language/extract_inventory.py --check
python3 -B tests/reference/compiler-language/report.py --run --feature functions-apply
python3 -B tests/reference/compiler-language/report.py --run --strict --output /tmp/compiler-language.json
cargo test --locked --test compiler_language --test provenance --test execution
```

The first command verifies exact pinned source and inventory completeness.
The focused command permits missing capabilities to stay visible while a feature
is implemented. The strict command fails on any missing in-scope assertion or
unverified supplemental public-boundary contract and is required by the
conformance job. The 27 supplemental contracts preserve their original source
and explicit public API, exact diagnostics, and runtime observations; they add
no imported assertion passes. Lexer/parser acceptance,
native results, generated JavaScript, Maven 1.12.134 and the pinned checkout each
have separate result columns. Coverage requires generated JavaScript and both
references to match the unchanged expected outcome, effects and stdout. No
reference fallback, missing output, skip, rejection or timeout can add a pass.
Python 3.10+ is required for these inventory commands and the fast accounting
test; the existing Rust/Node execution fixture formats remain unchanged.

### Existing execution and diagnostic regressions

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
including inside trace arrays. Backend-specific shallow adapters additionally
preserve typed named values, sequential kind/order, and map key types/counts.
Only complete encoded map entries are sorted, after duplicate-key/count checks;
ordinary sequence and trace order is preserved.
Full spelling distinguishes the pinned `(keyword "a/b/c")` edge case from
`(keyword "a")`, although both report name `"a"` and nil namespace. Host arrays
and tag-shaped objects remain distinct from language values; cyclic graphs
through either representation fail transport. Case wrappers share explicit
`semantic.caseN` namespaces across backends, while standalone APIs retain
`cljs.user`. Unsupported source literals remain unsupported;
host callbacks supply special values. Independent JavaScript probes check the
serializer and runner against literal expectations, so a shared harness error
cannot count as compiler agreement. The [bounded runtime audit](../tests/reference/compiler-collections-runtime.md)
also records independent immutable-storage checks and pinned runtime probes. See the [outcome protocol and evidence](../tests/reference/outcome-evidence.md).
Separate tests cover complete files, CLI diagnostics, invalid binding forms,
and resolved IDs.

`tests/fixtures/execution-literals.tsv` carries byte-exact UTF-8 sources as hex
for actual CR/CRLF, nulls and other control characters. Both compilers decode
these bytes before compilation and use the same result/effect protocol. An
independent Node probe checks lone surrogate and control-unit serialization.
The [numeric and UTF-16 audit](../tests/reference/literal-evidence.md) records
**230** passing runtime cases against both baselines, exact diagnostic spans,
upstream provenance and intentional numeric restrictions.

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

The [function/invocation audit](../tests/reference/function-evidence.md) adds
36 cases for **179** passing runtime programs against both baselines. It covers
empty and multiple-expression bodies, returned/self-capturing closures,
higher-order callee/argument effects, saved function values versus live globals,
redefinition during argument evaluation, fixed-method argument-count behavior,
and dynamic non-callable failures. This historical audit is extended by the
[function arity audit](../tests/reference/compiler-functions.md), which executes
multiple/variadic functions and their callable conventions. The
[recurrence audit](../tests/reference/compiler-recur.md) adds stack-safe loop and
function/method recurrence. The [letfn audit](../tests/reference/compiler-letfn.md)
adds mutually recursive lexical groups and all nine selected upstream letfn
assertions. The [record audit](../tests/reference/compiler-records.md) executes
the original Search1–Search4 record wrappers; remaining library assertions retain
their separate inventory status.

The [control-flow audit](../tests/reference/control-flow-evidence.md) brings the
runtime corpus to **202** passing programs against both baselines. Its matrix
checks if/do/let* in Value, Discard and Return contexts, including nested tests,
initializers, bodies, callees and arguments. Ordered traces and host failures
expose duplicated evaluation, eager branches, dropped discarded effects and
premature returns. Existing namespace comparisons verify dependency/source order;
method probes verify exactly-once receiver/getter/argument evaluation and `this`.

The [analyzer boundary audit](../tests/reference/boundary-evidence.md) checks
119 rejection/subset cases through the analyzer/API and CLI, with exact UTF-8
byte locations, unselected branches and empty output on failure. A required
reference comparison distinguishes analyzer errors, emitter errors, invalid
JavaScript, warnings and accepted syntax. Release and checkout classifications
remain distinct where upstream changed. Eight positive execution cases and a
fourth namespace comparison bring the passing runtime corpus to **238** cases;
source-ordered globals, macro-name shadowing, slash bindings and explicit host
references remain supported. The retained slash statement-context reproducer
records a reference emission limitation outside the passing corpus.

The [secondary repository audit](../tests/reference/secondary-repositories-evidence.md)
maps the bounded Clojure, Shadow CLJS, Closure Compiler and Closure Library
survey. Six additional primitive/interop regressions bring runtime coverage to
**244** programs against both standard baselines. These check numeric receivers,
receiver capture across argument effects, detached methods and null/digit string
emission; secondary runtime/tooling semantics remain explicit exclusions.

The [bounded generated audit](../tests/reference/generated-evidence.md) adds
**48** programs (12 fixed seeds with original, alpha-renamed, identity-binding
and single-form-do variants), separately from the 244-case fixture corpus.
Generation tracks lexical scope and callable arity, forbids recursion, and caps
source size, form depth, case count and subprocess time. Ordered observations
expose effect changes even when returned values agree. Fast tests compare
metamorphic counterparts; the required selectable reference job also compares
all variants with standard ClojureScript. The audit documents single-seed replay,
retained-source replay, a reduction procedure and separate generated origins.


## Reify objects

`reify` uses the compiler's protocol/type machinery, with one anonymous
constructor per source site and a new instance for every evaluation. Visible
locals become receiver fields, including shadowed names resolved to the visible
binding. Methods read their own receiver's fields; nested closures retain that
receiver, and nested reify objects capture a fresh environment. The generated
runtime has no native-interpreter dependency.

Methods support destructured parameters, separate fixed overloads, Object
methods (including large and variadic signatures), nested functions and loops,
method recurrence and arbitrary thrown values. Protocol markers and partial
implementations retain ordinary dispatch behavior. `IFn` supports saved calls
and `apply`; `IHash` and `IEquiv` implementations participate in hashing, equality,
maps and sets. `get-in` preserves the two- versus three-argument `ILookup`
dispatch needed by upstream reify regression tests.

Reify always supplies `IMeta` and `IWithMeta`. Source-form metadata initializes
the instance directly, even with an explicit `IWithMeta` override. `with-meta`
normally returns a fresh identity with the same anonymous type and methods,
copies current captured field values, and shares referenced values such as
atoms. Changing a copied receiver's scalar field does not change the original.
Explicit metadata protocol methods may replace those defaults.

The pinned CLJS Object dispatcher loses the instance receiver when selecting a
variadic overload beside fixed overloads; the generated implementation preserves
that observable behavior. A single variadic Object method retains its receiver.
The existing IFn call-versus-direct-entry recurrence behavior is also retained.
Grouped signatures remain available to `extend-type`; reify overloads use
separate method forms, matching its upstream macro grammar.

[Reify evidence](../tests/reference/compiler-reify.md) records complete imported
assertions, independent edge probes, a Potok-style event object across namespace
aliases, direct host expectations, and size/startup measurements.


## User-defined types

`deftype` creates a fresh constructor on each evaluation, a positional `->Type`
factory, `getBasis`, and factory Var documentation. Fields initialize in argument
order. Mutable fields accept `:mutable`, `:unsynchronized-mutable` and
`:volatile-mutable` metadata, with ClojureScript truthiness. `set!` on immutable
fields and local parameters is rejected at the source location; explicit host
property writes follow JavaScript interop. Parameters and lexical bindings can
shadow fields. Field reads and writes inside returned functions remain attached
to the method's actual receiver and observe later mutation.

Methods support protocols, Object members, destructuring, overloads and recurrence.
`IFn`, `ILookup`, `ISeqable`, `IReduce` and `IKVReduce` participate in calls,
`apply`, keyword lookup, `get`, sequence traversal, `reduce`, `into` and
`reduce-kv`. Collection protocols also dispatch ordinary collection operations;
custom indexed/vector implementations support the original Subvec regression.
Native method tables retain the existing precedence over owned collection storage.

The low-level `deftype*` accepts fields, protocol-mask data and an optional body.
Macro-style annotated extension methods retain field receivers. Unannotated
low-level field methods reproduce the pinned missing receiver failure. The IFn
`.call` recurrence adapter can clear its hidden field receiver, while the direct
invocation slot keeps it; both paths have differential coverage.

Types use resolved namespace identities. Alias-qualified constructors and protocol
methods do not collide with same-named types in other namespaces. Redefinition
creates a new constructor and prototype; old instances keep their old methods,
and saved positional factories read the current type binding. Named types do not
capture enclosing lexical locals. Method body maps are evaluated normally, rather
than interpreted as function pre/post conditions.

[Type evidence](../tests/reference/compiler-types.md) retains complete imported
source bodies, assertion mappings, independent probes and actual verification.


## Records

`defrecord` and `defrecord*` support compiled nominal data, positional and map
construction, declared fields, extension entries, metadata, cloning and user/core
protocol methods. Assoc and extension-only dissoc preserve a record; removing a
declared field produces a plain map with its metadata. Record equality distinguishes
plain maps and other constructors, and equal records work as structural map/set keys.
Sequence and find return map entries; empty returns nil. Records are callable only
when they implement IFn. External extensions affect existing instances.

Saved factories and generated copy/update methods follow the current constructor
Var after redefinition, while old instances retain their original methods and type.
Named record methods use namespace Vars for enclosing names and retain receiver
fields in escaped closures. Raw `defrecord*` exposes internal fields and installs
only the methods supplied by its body. `pr-str` prints tagged record data; default
`str` follows JavaScript object stringification, matching the pinned reference.

The [record audit](../tests/reference/compiler-records.md) maps 59 selected upstream
assertions, independent differential probes, direct host checks and namespace
programs, and reports generated output/startup measurements.

## Protocol extensions

`extend-type` and `extend-protocol` extend named types and records, JavaScript
constructors, built-in collection/named-value types, `nil`, `object`, `boolean`,
`number`, `string`, `array`, `function`, `symbol`, `bigint`, and `default`.
Built-in types have distinct public extension prototypes while their immutable
payloads and private brands remain protected. Extensions affect existing instances.

`specify!` evaluates its target once and installs instance methods, returning that
same value. `specify` clones through `ICloneable` before installing methods. The
macro-style `^{:extend :instance}` target annotation is supported. Metadata methods
on opted-in protocols precede instance/prototype methods; native/default dispatch
follows direct methods. Saved method functions and namespace aliases keep these
rules. Replacing a method during a call preserves that selected invocation and
changes subsequent dispatch. Native replacement replaces that method's overload
function; prototype replacement preserves other arity slots.

Extension method `recur` retains the full explicit parameter count and rebinds
the receiver to JavaScript `this` on each iteration.

Core predicates observe native membership. Ordinary core operations preserve the
upstream nil/string/array fast paths: for example, a native `-count` extension can
change `(-count "ab")` while `(count "ab")` remains 2. Raw protocol methods and
native object `seq`, `get`, `reduce`, `contains?`, `empty`, and printing dispatch
through extensions. Grouped IFn extension signatures follow the upstream macro
shape; ungrouped forms are rejected. Primitive `specify!` property writes have the
upstream silent no-op behavior, while nil and unsupported clone operations fail.

The [extension audit](../tests/reference/compiler-extensions.md) maps all 31
assertions in `extend_to_native_test.cljs` plus all four CLJS-414 `specify`
assertions. Native mutations execute in separate Node processes; namespace
separation alone cannot isolate upstream core tables. It also records independent
outcome/effect/stdout probes, syntax classifications, and output measurements.


## Public core protocol integration

Public core operations distinguish protocol membership from available methods.
A marker or a partial implementation satisfies its predicate but still fails
when a required method is missing. Custom sequence types work without `INext`;
counting stops at a counted tail, and reductions honor custom implementations,
iterator protocols and early `Reduced` results. `reduce-kv` supports its fallback
through reducible map entries. `key`/`val` and `keys`/`vals` use entry protocols.

Built-in collections accept custom keys and compare with compatible custom map,
set and sequential interfaces. `Fn` and `IList` are public marker identities.
`ILookup` supports contains/destructuring callers without requiring an associative
implementation. Ordinary arrays retain host identity and support public indexed
lookup. Custom printers and writers support raw write/flush methods; default
printing passes nil options, as in upstream.

See the [core protocol audit](../tests/reference/compiler-core-protocols.md) for
the family inventory, full source witnesses, independent literal tests, reduction
and extension precedence, exact commands, and versioned missing-method errors.


## Persistent collection helpers

`assoc-in`, `update-in`, `select-keys`, `merge`, `merge-with` and `zipmap` are
ordinary callable core values, including through `apply` and macro expansions.
They preserve persistent snapshots, structural keys, metadata and record update
semantics. Paths and collection inputs traverse custom sequence protocols;
lookup defaults distinguish absent entries from present nil/false.

`cons`/`list*`, `vec`/`set`, map construction and entry helpers, `empty`/`empty?`,
`peek`/`pop`, and `keys`/`vals` retain their public collection behavior. `MapEntry`
supports its constructor, indexed invocation/defaults, stack operations and
conversion to an ordinary vector by updates or metadata attachment. `empty?`
uses a custom count protocol before sequence traversal. `keys`/`vals` advance
the underlying entry sequence when their tail is requested.

`into` accepts zero, one, two or three arguments. The transforming arity accepts
a supplied reducer transformer, stops on `Reduced`, invokes completion once,
and preserves destination metadata. Built-in construction and custom collection
protocol dispatch follow their respective reference paths. The separate
transducer constructors and transient collection APIs remain outside this helper
member. See the [collection helper audit](../tests/reference/compiler-collection-helpers.md)
for 164 imported assertion sites, independent probes and measured output costs.


## Lazy sequence operations and comprehensions

`range`, `repeat`/`repeatedly`, `iterate`/`cycle`, `map`, `map-indexed`, `mapcat`,
`filter`/`remove`, `keep`/`keep-indexed`, take/drop and while variants, `concat`,
`partition`/`partition-all`, `dorun`/`doall`, and ordinary sequence traversal
helpers are callable core values. Their collection-taking public arities include
multiple input sources, finite prefixes of infinite producers, and empty inputs.
`map` stops at the shortest source. Its four-or-more-source overload preserves
lazy argument tuples and `apply` receiver behavior. `mapcat` retains the upstream
bounded argument probing at construction.

Integer and floating ranges and larger vector sequences expose chunks. Unary
map/filter/keep families and the innermost `for` generator preserve those chunks,
including predicate/body effect counts, rather than uniformly forcing one element.
Unchunked inputs remain incremental. Repeated traversal shares realized lazy
contents; `iterate` reduction follows the producer directly without caching a
new sequence traversal. Producer pending states and metadata copies follow the
pinned runtime. Reduced results and exceptions do not request later inputs;
effects already required by a source chunk remain observable.

`for` remains lazy. `doseq` performs effects in order and returns nil. Both support
nested generators, sequence/map destructuring, `:let`, `:when`, `:while`, and
escaped per-iteration closures. First-party compiled macro expansions and core
namespace aliases use the same compilation path. The separate native interpreter
retains its comprehension implementations until evaluator consumer migration.

The [sequence evidence and arity inventory](../tests/reference/compiler-sequences.md)
distinguishes collection overloads from transducer constructors. Eager transducer constructors are implemented by the
[canonical transduction contract](../tests/reference/compiler-transducers.md);
`sequence` transducer arities and `eduction` have separate lazy output contracts.

### Public compiler warnings

Prefix a compiler command with `--warnings` to emit located warnings on stderr,
for example `light-cljs --warnings --compile source.cljs`. This mode permits
undeclared Vars/namespaces and diagnosed function overloads to proceed through analysis;
ordinary commands retain their strict unresolved-symbol and invalid-overload
errors. `--checked-arrays-warn` also enables receiver/index diagnostics for
`aget` and `aset`. Both flags work with namespace compilation through `--entry`
and `--source-root`. Successful commands retain their normal stdout format.

Embedding callers can use `analyzer::analyze_with_warnings`,
`analyzer::analyze_with_macro_warnings`, or
`compiler::compile_namespace_with_warnings` with `analyzer::AnalysisOptions`
and a mutable `Vec<analyzer::AnalysisWarning>`. Each diagnostic contains a kind,
message, source index, and byte span. Diagnostics already collected remain
available when a later error stops compilation. Warning collection includes
private Var access (Var literals bypass that warning), JavaScript local
shadowing, declared/defined argument lists, function and record arities,
non-dynamic earmuffed names, arithmetic operand types, checked array access,
single-segment namespaces, and the reserved `js` namespace alias. Type diagnostics use bounded inference
over analyzed expressions, including loop recurrence and returning `try` paths.

The source coverage report tracks these assertions as `verified-warning`,
separately from executed JavaScript. It checks complete original probes, ordered
warning kinds/messages, acceptance categories, and effective settings against
both pinned references. Original `static-fns=true` contexts are also checked
in both references alongside the mandatory `static-fns=false` baseline.
