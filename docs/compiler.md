# Compiler reference

This is the reference for the light-cljs compiler: how source becomes JavaScript,
which forms and core functions are supported, and where behavior deliberately
differs from upstream ClojureScript. It describes the current implementation.

The compiler reads `.cljs`/`.cljc` source, analyzes it in Rust and emits one
self-contained JavaScript expression. Macros (both first-party core macros and
user `defmacro`s) are compiled by the same pipeline and executed in an embedded
JavaScript heap. Native clean compilations use V8 by default; interactive
sessions, WebAssembly and builds without the `v8-macros` feature use QuickJS.
Both engines execute the same emitted functions and runtime. Evaluation
sessions and the `--eval`/`--repl` commands use QuickJS; see
[evaluation](evaluation.md) and the [synchronous evaluator](evaluator.md).

Supported today: lexical closures, multi-arity/variadic functions, mutable
globals and Vars, source namespaces, JavaScript interop, immutable
vectors/lists/maps/sets (hash and sorted), transients, lazy sequences,
transducers, protocols/types/records, multimethods and a large part of
`cljs.core`, plus first-party `clojure.string`, `clojure.set`, `clojure.walk`,
`cljs.reader` and `clojure.edn`. Coverage of the full ClojureScript language
and core library is **partial**: core functions the compiler does not implement
are reported as unresolved symbols. No general third-party standard library is
bundled; caller-supplied library sources compile through the same language
surface. Package resolution and bundling are out of scope.

The [compiler cost report](../benchmarks/compiler.md) measures compiler phases,
emitted bytes and runtime operation costs.

## Pipeline and ownership

`parse(source)` constructs the syntax arena. `analyze(source, &syntax)`
constructs a separate `Program`; `emit_js(&program)` prepares an application in
Rust, then recursively writes one JavaScript expression into a `String`.
`compile(source)` composes these stages and distinguishes parser errors from
analysis errors. `--compile` accepts files, stdin, or `--text`, and
`--optimize none|prepare|advanced` selects the [application
preparation](#application-preparation) level; `--analyze` prints the resolved
representation.

`Program` owns an `Arena<Expr>`, lexical/global binding records, JavaScript paths, and
UTF-16 string/name values. `NamedLiteralId` identifies an owned namespace/name pair;
`CoreFnId` selects a runtime core function. `ExprId`, `LocalId`, `GlobalId`,
`JsGlobalId`, and `StringId` are integer indices into those arrays. Expression and
binding records retain original byte spans and a source index (zero for standalone
analysis; the namespace's dependency-order index for a combined program). Analysis
requires the exact source used to parse the syntax; the resulting program no longer
borrows that source. IDs are local to a single compilation unit, and the emitter
expects an analyzer-produced program.

Compiler forms and source provenance live in `compiler_data`: exact UTF-16
`Name`, `Source`/`SourceSpan`, origins, immutable `Form`/`FormId` nodes and an owned
finite `Datum` tree. `Form::data()` returns a compiler datum with no runtime
handles. Source text and located reader errors remain usable after the macro host
and input buffers drop. `Form::from_data` constructs supplied data without an
evaluator; `MacroHost::read_compiler_form` returns the same owned representation.

Map form children alternate keys and values in source evaluation order, separately
from semantic map storage. Form spans retain lexical spelling; datum scalars keep
reader number categories and all floating bits, while JS emission converts numbers
to Number. Metadata and opaque reader literal payloads are owned datum/form edges.

Reader identity checks resolve keyword and symbol names without inserting them
into the program's executable literal tables. Duplicate-key checks, collection
ordering and case validation use temporary reader data; only lowering an actual
literal adds its name and UTF-16 strings to the program. This keeps discarded
validation data out of deferred snapshots and fragment copies.

`compiler_reader` constructs semantic forms directly from syntax and immutable
`NamespaceView` snapshots. A view retains source identity, aliases, refers,
excludes, resolved binding IDs/kinds and the visible declaration frontier.
Syntax quote, unquote/splicing, anonymous functions and namespaced maps produce
ordinary runtime construction calls. Core binding identities, kinds and metadata
come from a pinned public declaration table (`src/core_var_metadata.tsv`,
`src/core_public_bindings.tsv`) filtered by the executable compiler registry.
Source macro metadata remains inspectable without exposing its closure.

The analyzer's private reader view resolves declared binding spellings only
when syntax quotation reads them, using the current definition snapshot's name
index. It does not rebuild binding-kind tables by scanning earlier expressions.
The public `NamespaceView` still exposes its complete binding introspection.

The pure compiler seed in `compiler_binding_macros` and `compiler_destructuring`
expands binding/function declarations and their extended map patterns.
Protocol/type/record analysis is compiler-owned; method forms use the same seed.
Reading, seed expansion and namespace/core metadata inspection need no JavaScript
runtime. `source_provider` owns the load request, input and error contracts.

Macros use a structured compiler/engine form bridge (`form_bridge`, `v8_bridge`); configured
compiler readers execute JavaScript functions over immutable data. Filesystem
imports use the same `.cljs`/`.cljc` namespace loader for both phases and never
read `.clj` companions. Compile-time emission uses the same semantic IR and
runtime operations as application emission; it may add deferred-root access and
bypass optional application optimization. See
[deferred compiler units](evaluator.md#deferred-compiler-units).

### Work scheduler

`scheduler` is the compiler's work pool. It is built on the standard library
alone: scoped worker threads, one mutex over per-phase ready queues and
per-worker deques, one condition variable for workers waiting for work and one
for the coordinator waiting for outcomes. It claims batches, steals the far
half of a peer's deque, ages phases so that none starves, bounds admission by
job count and estimated bytes, and stops the build on a worker panic. It is
generic over a job's phase, cost, bytes and priority, and knows nothing about
the compiler. The calling thread coordinates: it submits ready jobs, consumes
outcomes and alone declares the build done. With one slot it runs every job
inline without spawning. `--jobs N` (experimental, default 1) counts these
slots, the coordinator's included, and reaches the compile entry as
`EntryRecorders::slots`; no compiler phase runs on the pool yet. The design,
the as-built details and what the tests cover are in
[compiler-concurrency.md](compiler-concurrency.md#the-scheduler).

### Namespace reflection and reading

`ns-publics`, `ns-interns`, `ns-imports` and `ns-refers` take quoted namespace
symbols and expand from compiler-visible source declarations. Core public maps
include supported core Vars, preserving their source names and metadata. Saved
Vars remain callable; quoted Var identity and metadata inspection do not demand
an ordinary helper initializer during macro compilation. Dereferencing or calling
an ordinary Var demands its root. Source macro Vars expose `:macro` metadata;
consuming their root reports a phase error.

`exists?` observes unavailable, renamed or excluded ordinary bindings safely
during macro execution. An existing root initializes lazily once to distinguish
JavaScript `undefined` from existing `nil`/`false` values; initializer errors and
real dependency cycles still propagate. `resolve` separately observes Var
identity without demanding its value.

`Namespace`, `create-ns`, `find-ns`, `ns-name`, `the-ns` and `all-ns` use explicit
session identities. Emitted units register their source namespaces, including
namespaces without definitions; an `--optimize advanced` application does so
only when it reaches this reflection (see "Application preparation"), and a
session's unit registers those whose reader context or definitions changed
since its last unit ([Actions](evaluation.md#actions)).
Creating an existing named namespace reuses its identity. `create-ns` accepts
an optional host object for `.findInternedVar`; no browser global namespace
traversal is assumed. On source symbols retained in
Var metadata, `ns-name` returns the symbol's string name field, as upstream does.
These snapshots describe the supported source surface and do not emulate upstream
analyzer AST or cache structures.

Runtime `read` and `read-string` use the active dynamic `*ns*`: `cljs.user` for
standalone compilation, the source namespace during namespace initialization,
and the configured/current namespace in an evaluation session. `::x` and
`#::{:x 1}` qualify to that name; `::alias/x`, `#::alias{...}` and syntax-quoted
alias-qualified symbols use explicitly registered source aliases. Missing
auto-resolved aliases report reader source, line and column. Reading never loads
a namespace or evaluates input. Nested tag reads and `*ns*` bindings preserve the
enclosing view. Macro computations bind the caller's reader context and restore
it afterwards. `&env :light-cljs/reader-context` exposes descriptive refers,
exclusions and declaration names: `:bindings` names the macros and the Vars
the expanding analysis itself declared, and `:declared` registers the
namespace's source declarations with the macro session once each, in source
order, of which the first `:visible` are declared at the expansion. Strict
`clojure.edn` and `cljs.reader` profiles reject auto-resolved/syntax-quote forms.

### Application preparation

`prepare_application(&program, ApplicationOptions::default())` is the public
pre-emission boundary. `emit_js` and `emit_esm` use this default; callers can
retain the `PreparedApplication` and call its `emit_js()` or `emit_esm()`
repeatedly without rerunning the pass. `ApplicationOptions::optimize` is an
`OptimizationLevel`, ordered so each level includes the previous one:

| Level | CLI | Effect |
| --- | --- | --- |
| `None` | `--optimize none` | Emit every definition and call as analyzed. This output is the oracle that the other levels are tested against. |
| `Prepare` | `--optimize prepare` (default) | Remove discarded definitions with inert initializers and specialize eager top-level calls, as described below. |
| `Advanced` | `--optimize advanced` | Release mode from [advanced-optimization.md](advanced-optimization.md), with the semantic differences listed in its "Advanced-mode contract". It currently prepares like `Prepare`, except that it also removes definitions with pure initializers and does not treat retained `js*` as an escape (below), reads runtime bindings directly instead of through the runtime's `core` object, embeds only the runtime units the program reaches (runtime slicing, below), declares each distinct keyword, quoted symbol and constant collection literal once (the constant table, below), registers its namespaces only when the program reaches reflection, gives collections a printing `toString` only when the program can print, infers types to write truth tests and core calls as JavaScript, drops the methods and types of the program that no instance can use (rapid type analysis, below) and the runtime types it cannot build, and calls known functions through the method each call selects, without a dispatcher for functions whose value nothing else sees (call shapes, below), and folds constants, dead branches and lookups in constant collections, substitutes `^:const` and `goog-define` values and inlines single-use and small pure functions (folding and inlining, below). |

`ApplicationOptions::new(level)` builds options without exports; it also takes
the former boolean (`false` is `None`, `true` is `Prepare`).
`exports: Vec<GlobalId>` explicitly keeps runtime binding identities and their
source generations. Exports are retention roots; this API does not add a
JavaScript module export wrapper. For namespaces,
`compiler::analyze_namespace_entry` returns the combined program to prepare at a
chosen level; the CLI's `--compile --entry` (with or without `--esm`) and
single-file `--compile` both accept `--optimize LEVEL`. The REPL, `--eval`,
macro expansion and deferred units never enter this boundary and always share
the full runtime.

Below `Advanced`, any output that reads the runtime embeds all of it and reads
it through the runtime object: `rt.keyword`, and every core operation through
the `core` map, `(rt.core.add).call(null, 1, 2)`. At `Advanced`, `emit_js` and
`emit_esm` read runtime bindings directly. The runtime's core table (the
`coreCells` and `core` units of `src/js_runtime.js`, parsed by
`src/runtime_manifest.rs`) resolves each core operation to the binding that
holds it, a member of one, or a mutable cell. The output embeds only the units
reachable from those bindings over the unit manifest, in runtime order, returns
the bindings it reads from the runtime factory and destructures them into
`rt$<binding>` constants, and calls core operations that have a binding of their
own as `rt$add(1, 2)`. Dynamic Vars (`*print-length*`, `*print-fn*`, `*ns*`) and
the operations the runtime itself reads through a cell (`rand`, `flush`) are
read and assigned as `rt$coreCells.printLength`. A core operation the program
may replace (a `set!`, `binding` or `with-redefs` target, or a `js*` hole) is
read and written through an output-level cell, `let core$inc = rt$inc;`, for the
whole program (`PreparedApplication::replaced_core()` lists them); every other
operation stays a direct read. Runtime effects follow demand: a builtin type's
prototype members and `EMPTY` constants are embedded with its constructor, a
core function the program only calls (`rt$add(1, 2)`) is embedded without the
variadic arity properties that `apply` reads, which a value use such as
`(apply + xs)` keeps, and an `extend-type` of a core protocol keeps the
protocol method's dispatcher. A generic operation's branch for one kind of
value is embedded only with a unit that builds that kind, and its protocol
dispatch only with `installMethods`, which every protocol implementation goes
through, so a program keeps of the runtime's collection, keyword and seq types
only those it can build (see "Phase 2 builtin types extension points" in
[advanced-optimization.md](advanced-optimization.md)). Core protocol
dispatchers and protocol tests answer natively without protocol
implementations, vectors, maps and sets answer their host-facing methods
from their own items, the printer reads options only where the program can
pass them, `(atom x)` is a plain constructor call, a map literal of at most 8
entries builds an array map only and a set literal of at most 8 constants an
array set only, so none of these keeps the protocol machinery, seqs, hash
maps or hashing by itself ("Phase 2 builtin types, second pass" and "Phase
2 builtin types, sets"). The runtime's own values defer to protocol
implementations only where `extend-type` or `specify!` can give them some,
and the generic operations find a program type's methods on its values
without the protocol tables, so a record next to a vector or a keyword keeps
about 10 KB instead of 35 KB (light-cljs-246, "Phase 2 builtin types,
program types"); their protocol tests (`counted?`, `count`, `seq`) read the
protocols a value declares by name, so `(counted? (T.))` of a `deftype`
keeps about 6 KB instead of 12 KB, and `count` walks a seq only where the
program builds a value that needs it, so `(count (T.))` keeps about 8 KB
instead of 27 KB and `count` of a host array 1.4 KB (light-cljs-251). Each distinct keyword
and quoted symbol literal the output reads is declared once after the
runtime, `const k0 = rt$keyword(null, "a");`, and its sites read `k0`;
keyword `case` switches on
`rt$keywordFqn(caseValue)`, the keyword's full name, instead of printing it
(see "Phase 1 constant table extension points" in
[advanced-optimization.md](advanced-optimization.md)). The same table holds
each distinct constant collection literal, reader metadata on one and `#uuid`
literal, built once from the entries of its parts, with the hash the runtime
would compute for it: `const k1 = rt$hashed(rt$vectorFromArray([1, k0]),
-1713415597);` (`src/value_hash.rs` ports the runtime's hashing; a program
that extends a core protocol beyond its own types embeds no hashes). Equal
literals at different sites are therefore one value, which the advanced-mode
contract allows. A constant
of 1,024 or more nodes is built on its first read instead,
`let c2; const k2 = () => c2 ??= ...;`, read as `k2()`. A literal with a
local, global or call inside, a set literal whose members are not all
scalars, keywords or symbols, `#js`, regexes and `#inst` are built where they
are evaluated, as at every other level (see "Constant collection literals"
in [advanced-optimization.md](advanced-optimization.md)). A map literal whose
keys are distinct constants is built there from a key shape in the table,
`const k3 = rt$mapShape([k0, k1, k2], [1174270348, 2099068185, -1757859776]);`,
and its site passes only the values, `rt$mapFromShape(k3, [k4, l0, l1])`, so
no evaluation looks up, compares or hashes its keys; past 8 entries the shape
also holds the hash trie. A set literal whose first members in reader order
are distinct scalars, keywords or symbols adds its other members to a shape
of those, with the same duplicate check. The maps and sets are the ones the
builders make, down to their entries and trie nodes. At every level,
empty `[]`, `{}`, `#{}` and `()` literals are the runtime's `EMPTY`
singletons, as upstream, so `(identical? [] [])` is true.

Compiler machinery in the runtime (the compiler form bridge, declaration
demand and transactions, the namespace registry, the macro reader context,
`inspect` and the `eval` stub) is marked `compile-time` in the unit manifest,
and an `Advanced` application keeps it only when the program reaches
reflection: when it reads, sets or binds `*ns*`, calls `find-ns`, `the-ns`,
`all-ns`, `create-ns`, the reader (`read-string`, `read`, and the
`cljs.reader` and `clojure.edn` readers, which share it) or `eval`, uses a
macro defined in the program, or quotes a `#js` literal. Only such a program
registers its namespaces with the runtime and switches `*ns*` as each
namespace loads, and its registered namespaces list only the definitions the
output keeps, so a removed definition no longer shadows a `cljs.core` name in
the reader's syntax quote. Any other program has no namespace registration:
`(js/console.log (+ 1 2))` reads no runtime at all and is about 200 bytes.
`resolve`, `ns-publics`, `ns-interns`, `ns-imports` and `ns-refers` expand to
Vars at compile time and are not reflection in this sense. The protocol,
multimethod and type paths of the runtime call the declaration hooks of the
compiler (demand for deferred declarations, journaling of declaration
writes) through plain versions that the compiler replaces in the shared
runtime, so application output carries neither. See "Phase 1 compile-time
extension points" and "Advanced-mode contract" in
[advanced-optimization.md](advanced-optimization.md).

An `Advanced` application keeps the printer only when it can print: when it
calls `str`, `pr-str`, `prn`, `print`, `println` or another operation that
prints (`clj->js`, `gensym`, `munge`, `tagged-literal`, the reader). The
printer installs the printing `toString` of collections and `ExceptionInfo`
when it loads, so in a program that never prints `(.toString [1 2])` is
`[object Object]` and an `ExceptionInfo` gives `Error: message`, like
upstream's `:elide-to-string`. Error messages format values without the
printer: those that show a collection (`No value supplied for key`, `No
matching clause`, `assert` messages) do so through its `toString`, and
multimethod errors print nil, booleans, numbers, strings, keywords and
symbols as the printer does. With the printer every message is the same as
at `None`; `[1 2 3]` alone is about 120 KB instead of 132 KB. See "Phase 1
print extension points" in [advanced-optimization.md](advanced-optimization.md).
Since light-cljs-238 the printer keeps a branch only for the kinds of value
the program builds, and walks arrays, vectors, maps and sets without seqs
where the program has no protocol implementation, so `(println "hi")` is
about 4.8 KB and `(println [1 2 3])` about 9.7 KB; the generic seq
operations look up protocol implementations only in a program that has
some ("Phase 2 builtin types, seq operations and the printer").

At `Advanced` the output is terse (light-cljs-215; "Phase 3 terse emission
extension points" in [advanced-optimization.md](advanced-optimization.md)).
`build.rs` strips the runtime units of comments and whitespace and gives
their locals short names, using the lexer and scope analysis of
`src/js_syntax.rs`; the slice names its top-level bindings by frequency; and
`src/terse.rs` gives every name the emitter binds a short name, dots member
reads with identifier keys, drops redundant parentheses and joins the tokens
without whitespace. Runtime constructors keep their names. Raw `js*` text
keeps each of its tokens and comments (a `js-comment` too), losing only
whitespace, and the short names avoid every identifier it contains
(light-cljs-294), so `clojure.string/join`, whose body is `js*`, no longer
keeps a program in the emitter's layout. Only a program whose raw text puts
a hole inside one of its strings, templates or regexes (or a hole with
compiler names inside one of its comments), or whose raw text the lexer may
read otherwise than an engine (a byte outside ASCII outside its string
literals, an HTML-like comment, a `/` after `}`), keeps its layout, with
names after the hygiene prefix.
`ApplicationOptions::pseudo_names` (`--compile --pseudo-names`) keeps the
readable output instead. Terse output changes engine-generated error
messages and function names, as the Advanced-mode contract says.

`PreparedApplication::retention()` reports what was embedded: each
runtime name the output reads with the expression (source index and byte span)
that first demanded it, and each embedded unit with its byte size and the first
of those names whose dependencies reach it. `--compile --size-report` prints
that report as a table on stderr, for single files and `--entry` builds, at any
level; a root the output only calls prints as `rt$add(...)`, and a dispatcher
kept for an `extend-type` prints in parentheses. At `Advanced` the line under
the summary gives the constant table's entries, how many are collections and
lazy cells, and its bytes, and a unit's bytes are its terse bytes in the
slice.

The entry result, all standalone expressions, unknown or effectful initializers,
throws, definition tests, and protocol/type/extension declarations retain their
source/dependency order. Discarded top-level definitions are removable only when
their initializers are proven inert (primitive literals, function construction,
core function values, inert lists/vectors, and maps/sets with scalar keys). Runtime
references and escaped Vars retain their definitions transitively, including
metadata and all source generations. Non-scalar hashing, reader literals, arbitrary
calls, and metadata operations are conservatively effectful. Below `Advanced`,
retained raw JavaScript prevents elimination and specialization; raw code inside an otherwise
discarded function is not a root. This is deliberately conservative reachability,
not a general optimizer.

`Advanced` also removes a discarded definition whose initializer binds locals,
branches, adds metadata, calls core operations that the core operation table
(`src/core_operations.tsv`) marks `pure` or `allocates` for the argument types
without running code the arguments supply (`(str "n" (+ 1 2))`,
`(hash-map :a 1)`, `(count [1 2])`), or constructs a program type with `new`
or its positional factory after the type's only definition. `mapv`, `filterv`,
`every?`, `not-every?`, `not-any?`, `some`, `group-by` and `run!` over literal
collections of plain data are removable when their function, a `fn` literal or a
core operation, is itself inert on the elements (`(mapv inc [1 2])`). Operations
that may throw (`into`, `conj`, `reduce`, a `hash-map` call missing a value),
that call other function arguments, or that are called outside their documented
argument types stay, as do calls of an operation the program `set!`s,
`with-redefs` or passes to
a `js*` hole, and every core call in a program that extends a core protocol to a
type it does not define (`extend-type number IHash`, `PersistentVector`, `default`,
`specify!`). Retained `js*` is an opaque leaf at `Advanced`: hygiene keeps raw
text from naming compiler bindings (terse names that no raw text spells, or the
hygiene prefix), so it can reach only its `~{}` holes and host globals, and it
no longer prevents elimination or specialization.

Specialization covers eager top-level calls after a sole, non-dynamic
initialization, with no value/Var escape or mutation: fixed entries of multi-arity
functions and aliases to already-resolved core operations. Fixed entries retain
`.call(null)` and source-order argument evaluation. Missing arities, mutable
roots, deferred function-body calls and unknown protocol targets keep ordinary
shared lowering. Single fixed-arity functions already have no dispatcher. At
`Advanced` the call shapes below choose the method at every call site.

`Advanced` infers a type for every expression of the prepared program
(`src/inference.rs`, `PreparedApplication::inference()`): from literals, the
core operation table, known functions, program types, definitions made once,
and the tests of enclosing `if`s (`string?`, `nil?`, `instance?`, `satisfies?`,
truthiness, through `and`, `or` and `not`). A parameter has the types of the
arguments its function's calls pass when the program makes every call of the
function itself: the function is only ever called, through its global, a
`let` or `letfn` local or its own name, or called in place, and never passed
as a value (`apply`, `partial`), exported or reassigned. Passing it to a core
operation whose row in `src/core_operations.tsv` describes how it calls its
function argument (`map`, `filter`, `reduce`, `swap!` and 28 more) adds that
operation's calls: `(map f xs)` calls `f` with an element of `xs`, `(reduce f
init xs)` with the accumulator and an element, `(swap! a f x)` with the state
and `x`. Inference tracks the elements of sequences, lists, vectors and sets
for this, and what an atom holds when the program makes every use of the atom
itself: an `(atom x)` without options bound by `let` or by a global's only
`def`, read only as the atom of `deref`, `reset!`, `reset-vals!`,
`compare-and-set!`, `swap!` and `swap-vals!` calls, holds `x` and every value
those calls store, including what the functions given to `swap!` return. On
such an atom, `deref` returns what it holds, `reset!` its argument, `swap!`
what its function returns, and `reset-vals!` and `swap-vals!` vectors of what
it holds. The state of an atom that goes anywhere else (a function, host code, a
collection, `add-watch`, `set-validator!`) is unknown, as is every atom's in a
program that names a prototype (`prototype`, `__proto__`, `getPrototypeOf`,
`setPrototypeOf`). A method that a `deftype`, `defrecord`, `reify`,
`extend-type` of a program type or `specify!` implements for a program protocol likewise has the
types of the protocol method's calls whose receiver may run it, when those
calls are all the method's: its global is only ever called, never passed as a
value, exported or reassigned, its signature is not variadic, nothing names its
slots (`.cljs$user$P$m$arity$2`, a `js/` path, a string, `js*`) and no other
method's name munges to them, and the value of an `extend-type` or `specify!`
holding it, its last method, is not used. A receiver that may be a host
object, which may inherit any object's slots, may run every implementation,
as may every receiver when the program names a prototype (`prototype`,
`__proto__`, `setPrototypeOf`). Other parameters, and those that `js*` text or
`(js-arguments)` may assign, are unknown. Emission uses the types:

- An `if` tests a value directly instead of through `truth()` when it cannot
  be a number, string or host value (`(< a b)`, `(seq xs)`, a keyword), and
  `(and a b)`/`(or a b)` become `(a && b)`/`(a || b)` when `a` is such a value.
- `+ - * /`, `< <= > >=`, `inc`, `dec`, `identical?`, `nil?`, `some?`,
  `zero?`, `pos?` and `neg?` become the JavaScript operators that their runtime
  definitions are, whatever the operand types.
- `(not x)` becomes `!x`, `(= a b)` becomes `===` when a non-nil primitive
  decides it, and `(= x :k)` compares `x` with the table constant and then the
  keyword's spelling (`rt$keywordFqn`).
- `(count x)` reads `.length` of a string, array or vector literal, `(nth v i)`
  on a vector literal reads its payload, and `(:k rec)` on a record of a known
  type reads the field.

A program that `set!`s an operation keeps its calls, and one that extends a
core protocol to a type it does not define keeps every lowering that dispatches
(`=`, `count`, `nth`, record lookups). See "Phase 2 type inference extension
points" in [advanced-optimization.md](advanced-optimization.md).

`Advanced` then runs rapid type analysis over the program's types and
protocols (`src/rta.rs`). A `deftype`, `defrecord`, `reify` or `extend-type`
method stays only when an instance can run it: its type is constructed by
reachable code, or its constructor escapes as a value, and the program calls
the method (or tests the protocol with `satisfies?` for its marker). Naming
a slot (`cljs$user$P$m$arity$2`) anywhere in a property, method, `js/` path,
string or `js*` text calls the method (light-cljs-255). It also does the
following:

- A type nothing constructs loses its methods, its factories and, unless
  `instance?` or `extend-type` names it, its definition.
- A `defprotocol` defines only the protocol value and the dispatchers the
  program reads.
- A call whose receiver inference types as an instance of one program type
  reads the method from the receiver's slot instead of calling the
  dispatcher. An argument it cannot read twice, and a local read before
  one, is evaluated once, in order, into a local of the function it is in,
  `(operand0 = g3(i, 2))["cljs$user$Area$area$arity$1"](operand0)`
  (light-cljs-262).
- A type gets its static `getBasis` only when the program names it.
- The runtime's types get their upstream fields and methods (`first`, `cnt`,
  `meta`, `sval`, ...) only when the program names one, or calls
  `reset-meta!` or `alter-meta!` (light-cljs-238); their host-facing
  methods (`equiv`, `indexOf`, iteration) stay.

The runtime's record methods are units of their own, kept only where an
operation that dispatches them is, or where the program names a method's
slot (`cljs$core$ILookup$_lookup$arity$3`) anywhere in a property, method,
`js/` path, string or `js*` text (light-cljs-250, light-cljs-255), as it can
name a program protocol's slot, so a record that is only constructed and
read keeps about 11 KB of runtime instead of 145 KB. Collections still keep
the shared core (light-cljs-227). See "Phase 2 rapid type analysis extension
points" and the "Advanced-mode contract" in
[advanced-optimization.md](advanced-optimization.md).

`Advanced` last decides the shape of calls (`src/call_shapes.rs`,
`PreparedApplication::call_shapes()`). A function is *known* when a single
`def` binds it and nothing else assigns its global: not a second `def`,
`defonce`, `set!`, `binding`, `with-redefs`, a `js*` hole or `:dynamic`
metadata (a `declare` assigns nothing). Calls of a known function name the
method their argument count selects, at every call site:

- A function with one fixed method is called plainly, `g3(a, b)`, and so is a
  local or parameter bound to one, or a `fn` literal, unless it may be nil.
- Any other known function binds each method to a top-level entry at its
  `def`, `g3_0`, `g3_1`, ..., and calls name the entry, `g3_1(a)`. A variadic
  method receives the rest sequence its dispatcher would build,
  `g3_2(a, rt$fnRest([b, c], 0))`.
- The `arguments.length` dispatcher and the `cljs$core$IFn$_invoke$arity$N`
  properties exist only when the function's value escapes: when the program
  reads the global other than to call it (`apply`, `var`, a higher-order
  argument), uses the value of its `def`, exports it, or calls it with an
  argument count that selects no method. They then call the same entries; a
  `def` that may run more than once gives its dispatcher the entries of its
  own evaluation.
- A named function, `(def f (fn self ...))`, calls the entries through its
  name too, when its `def` runs once at top level and the name is only
  called; otherwise it keeps its usual shape.
- A function with several methods or rest arguments bound by `let` or
  `letfn`, whose local and name are only called with counts that select a
  method (no `reify` in their scope), has no dispatcher either. A method that
  captures nothing is a function entry of the constant table, created once
  (`k4(a)`); one that reads its scope is a `const` next to the binding,
  `const l5_1 = function (...) {...}`, created at each evaluation as before.
- A method whose body holds `js*` keeps `.call(null, ...)`, since a plain call
  passes `undefined` as `this`. Unknown callees keep `.call(null, ...)`.
- The positional and map factories of a `deftype` or `defrecord` (`->T`,
  `map->T`) and the method dispatchers of a `defprotocol` are called plainly
  too, `g3(a, 2)`, when only their definition defines them, nothing
  assigns them and the call is in a later top-level form than the definition:
  a factory is an arrow and a dispatcher reads no `this` (light-cljs-262).
- A `do` in value position is a comma expression instead of a function, so a
  `def` initializer is no longer wrapped, unless one of its forms reads
  `arguments` or holds `js*`.

A call that runs before the definition still throws a `TypeError`, whose
message now names the output binding. Calls of a known function do not
read its `call` property, except those of a method holding `js*`, and
inference types the calls of a program function from its bodies, so a
program that reassigns `call` is unsupported (light-cljs-257). See "Phase 2 call shape extension
points" and the "Advanced-mode contract" in
[advanced-optimization.md](advanced-optimization.md).

Before those analyses, `Advanced` folds, inlines and removes what that
leaves unread, in up to four rounds (`src/folding.rs`, `src/inlining.rs`):

- A read of a `^:const` or `goog-define` Var defined once by a top-level
  `def` of a literal is that literal in later top-level forms, which run
  after the definition (see "Definitions and globals").
- Core calls on literals that the core operation table marks `pure` for those
  arguments are their results, computed as the runtime does: arithmetic,
  comparisons, `=`, predicates, `not` and `str` (`(str "n=" 1.5)` is
  `"n=1.5"`). Leading number literals of `+ - * /` combine, `(+ 1 2 x)` being
  `(+ 3 x)`. A call outside the operation's argument types, of an operation
  the program replaces, or of `=` when the program extends a core protocol to
  a host or runtime type, stays.
- An `if` on a literal is its branch, a `let` binding of a literal is
  substituted (so `and`, `or` and `when-let` over constants fold), and forms
  without effects disappear where their value is discarded. The definitions
  only a removed branch read are removed in turn.
- Lookups, counts and items of constant collections are their results
  (light-cljs-221): `get`, keyword and collection calls, `get-in`, `nth`,
  `count`, `empty?`, `contains?`, `seq`, `first`, `second`, `last`, `peek`,
  `rest`, `next` and the collection predicates, on a literal, a local bound
  to one or a `^:const` global, so destructuring a constant binds its locals
  statically. A call that would throw, such as `nth` out of range without a
  default, stays and throws. A map literal with constant keys answers
  `count`, `contains?` and lookups of absent keys whatever its values, and
  a lookup of a present key reads that key's entry by index. Membership in
  a set literal of at most eight keywords, or of strings and numbers,
  compares the value with each member: an `if` on it is a `switch` on the
  value's spelling (or the value), and elsewhere it is a conditional
  expression that gives the boolean or the member.
- A call of a known function is replaced with the body of the method it runs
  when the function is called only there (and does not escape), or when its
  body is a small pure computation (at most ten expressions of reads,
  branches, bindings, literals and pure core calls) and copying it shrinks
  the program. The function's definition must be a top-level form that runs
  before the call. Bodies with nested functions, loops, `recur`, `try`,
  `throw`, `case`, assignments, `js*`, `js-arguments` or `this-as` stay
  functions, and so do `^boolean`-tagged ones. Arguments are evaluated once
  and in order: literals and locals are read in place, an argument with an
  effect is bound to a local first or, when the body reads it before doing
  anything else, moved there.

`ApplicationStats::folded_expressions` and `inlined_calls` count the
rewrites. `ApplicationOptions::no_inlining` keeps every call (folding still
runs), as Closure's `inlineFunctions` option does, and
`ApplicationOptions::no_folding` turns folding off (inlining still runs). A debug path behind `(when DEBUG ...)` with `(goog-define DEBUG
false)` leaves nothing in the output. See "Phase 2 folding and inlining
extension points", "Constant folding extension points" and the
"Advanced-mode contract" in [advanced-optimization.md](advanced-optimization.md).

`Program::compilation_uses` records deferred ordinary-unit names, generations,
analysis counts and compile-time emission counts. Analysis for application
output does not imply compile-time execution; macro demand does not create a
runtime root. Macro/bootstrap functions remain outside application IR. Deferred
unit emission bypasses this pass, emits a generation when its root first needs it, and
shares runtime operations. Applications have no deferred-unit compiler callback.

### Code-splitting contract

A chunked build (`--out-dir`; [Chunked builds](#chunked-builds)) is
one compilation of the program at the chosen level, whose output is split
into chunks that load on demand. Its deliberate semantic differences from
the single file, stated the way the "Advanced-mode contract" in
[advanced-optimization.md](advanced-optimization.md) states those of
`advanced`:

- A lazy namespace's top-level effects run when its chunk evaluates, not at
  startup: `extend-type`, `defmethod`, `set!`, registry `swap!`s,
  `enable-console-print!`, printing. Until then the program behaves as the
  program without that chunk's code: a protocol method implemented only in a
  feature reports no implementation (at `advanced`, the `TypeError` the
  existing contract allows for a devirtualized call) when called on a value
  before the feature loads.
- `load` resolves asynchronously, after the chunk and its prerequisites have
  evaluated, with the target Var's current value; a resident loadable
  resolves on a later microtask with no request. `loaded?` is synchronous.
  `@l` throws before the first successful load. Rejections carry the host's
  error object.
- A movable definition in a shared namespace may run later than its source
  position, in the chunk of its first reader. This is unobservable through
  the program: only its readers can see it, and they are in that chunk or
  later. It is observable through reflection, so a program that reaches
  reflection moves nothing. (Definitions move at `advanced` since
  light-cljs-277, Phase 3 of docs/code-splitting.md; below it every form
  stays in its namespace's chunk.)
- Eager code may be larger or less specialized because of lazy code: every
  whole-program decision (which types exist, which methods are called, which
  core operations the program may replace, whether printing is reachable)
  counts the lazy chunks. It is never incorrect.
- Globals assigned from another chunk are assigned through a setter the owner
  exports; `set!`, `binding` and `with-redefs` semantics are unchanged, and
  engine messages that name the binding differ as they already do between
  levels.
- Chunk file names are stable functions of root namespace names. A bundler
  may rename or regroup them; the manifest describes the compiler's output.
- `js*` raw text cannot name another chunk's bindings; it never could name
  compiler-owned names.
- Everything else prints what the single-file build prints, at every level.

### Source maps

`PreparedApplication::emit_js_mapped` and `emit_esm_mapped` return the text of
`emit_js` and `emit_esm`, unchanged, with a `source_map::SourceMapping`, at
every level. `SourceMapping::to_json(file, sources, include_sources)` writes it
as a Source Map v3 (ECMA-426) against the program's sources by source index;
`compiler::analyze_namespace_entry_with_sources` returns those for a namespace
build (paths and texts in dependency order), and `source_map::data_url` makes
an inline `sourceMappingURL`. Generated and original columns count UTF-16 code
units. `--compile` writes the map with `--source-map PATH` or
`--source-map-inline`, and `--source-map-include-sources` adds
`sourcesContent` (see the README's "Compile to JavaScript").

The emitter records where the text of each expression starts, at the byte span
start the IR carries, and after a nested expression's text the expression
around it again, so every output position maps to the innermost expression
whose text it is. Macro-expanded code carries the span of the macro call.
Folding and inlining keep the spans of the expressions they copy; a form that
takes over a nested form's code (a `do` of one form, a `let` whose bindings
all go) keeps its own span, so at `advanced` such code maps to the enclosing
form. The `function` keyword of each method is named (`names`) after its
`fn` name, else the definition or local it initializes, which is the name
Node's `--enable-source-maps` shows for a frame; each local and definition
name the emitter writes (`l7`, `g3_1`) is named after its source name. The
marks move with the text inserted before the body (the runtime, the host
helpers, the constant table, whose entries map to the literals they pool),
through the hygiene renaming of programs with `js*` and through the terse
rewrite, where each mark moves to the token written for the one it was
before. A mapping holds only on its generated line, so each line that starts
inside mapped text (raw `js*` of several lines, say) starts with the same
origin. The host helpers, core cells and namespace registration map to no
source.

The embedded runtime maps to `src/js_runtime.js`, the source
`light-cljs/js_runtime.js`: the whole runtime and a readable slice line by
line (each is the annotated source's text without its headers), and a terse
slice by the marks `build.rs` writes for each unit (`TerseUnit::lines`: where
the first token of each source line starts in the unit's terse text), with
each unit's start named after the first binding it declares. The REPL, the
macro host and deferred units have no source maps.

### Lexical environment

The environment is a stack of scopes containing binding IDs. Lookup walks scopes
and their bindings from newest to oldest. Binding records survive scope exit.
Each `let*` initializer is analyzed before its binding is installed; previous
bindings are visible, including earlier occurrences of the same spelling.
`letfn*` allocates every group binding before analyzing its function initializers.
Each entry creates independent lexical slots, including nested groups and loop
iterations. `fn*` installs its optional self-name and parameters before analyzing
its body. A parameter may shadow the self-name. Every emitted local name derives
only from its ID (`l0`, `l1`, …), so shadowing, Unicode names, and JavaScript
reserved words do not change binding identity.

## Supported syntax and values

| Construct | Behavior |
| --- | --- |
| `nil`, booleans | JavaScript `null`, `true`, `false`. |
| Integer literals | Signed decimal, hexadecimal, octal, radix and BigInt spellings use JavaScript Number rounding, including beyond the safe-integer range. Integer `-0` is ordinary zero. |
| Floating-point literals | IEEE-754 doubles and decimal literals; negative zero, overflow to infinity and underflow follow ClojureScript. |
| Strings | Exact decoded UTF-16, including lone surrogates. JavaScript escaping preserves quotes, backslashes, controls, and Unicode. |
| Keywords | Immutable named values, distinct from strings; qualified and auto-resolved names preserve namespace identity. |
| Vectors, maps and sets | Immutable owned collections; element and key/value expressions evaluate once, left to right. |
| `'data`, `(quote data)`, `()` | Scalars, symbols, nested lists/vectors/maps/sets and empty lists become data without name resolution or execution. Nested quote retains its `quote` symbol. |
| `(if test yes no?)` | Evaluate the test once and only the selected branch. The omitted branch is `nil`. |
| `(do forms...)` | Evaluate in order, returning the last value, or `nil` when empty. |
| `(let* [name init ...] body...)` | Sequential simple-symbol bindings and an implicit `do` body. Repeated names create distinct bindings. |
| `(letfn* [name function ...] body...)`, `letfn` | Mutually visible lexical functions with outer captures, overloads, variadic methods and independent group instances. |
| `(loop* [name init ...] body...)`, `recur` | Stack-safe nearest-target iteration with simultaneous left-to-right rebinding and fresh lexical captures. |
| `try`, `catch`, `finally`, `throw` | Ordered typed and `:default` catches, arbitrary thrown values and synchronous cleanup in every expression context. Recurrence cannot cross a try boundary; nested loops/functions establish their own targets. |
| `case`, `case*` | Grouped constants, optional defaults, validated test forms, single selector evaluation and tail recurrence. Scalar and keyword dispatch use JavaScript switch; general constants use ClojureScript equality. |
| Conditional/threading macros | Nil-aware and truth-aware bindings, short-circuit operations, ordered threading, `cond` and both `condp` clause forms preserve lexical scope and selected effects. |
| `while`, `dotimes` | Nil-returning iteration with fresh captures, one dotimes bound evaluation and nearest-target recur validation. Fractional bounds retain CLJS comparison semantics. |
| `(fn* name? [params...] body...)` | A lexical closure with one parameter list and optional `& rest`; optional self-name supports recursion. |
| `(fn* name? ([params...] body...) ...)` | Multiple fixed methods and at most one variadic method; fixed methods take precedence at their exact argument count. |
| `(^:async fn ...)`, `(defn ^:async name ...)`, `(await x)` | An `async function` whose call returns a Promise; `await` only in its body. See [Async functions and await](#async-functions-and-await). |
| `(callee args...)` | Evaluate callee first, then arguments left to right, then call. |
| Multiple source forms | An implicit `do`; the file's result is its last value, or `nil` when empty. Top-level lexical scopes do not leak into following forms. |

Primitive `let*`, `loop*`, and `fn*` bindings must be simple symbols: no namespace
slash, dot, or destructuring. Ordinary `let`, `loop`, `fn`, `defn`, and `letfn`
lower nested vector/map bindings, `:as`, `&`, `:keys`, `:syms`, `:strs`, qualified
keys and `:or` to these primitives. `&` introduces a final rest binding.
The standalone symbol `/` is unqualified and is supported, including as a global
whose qualified spelling is `namespace//`. Embedded ampersands, such as `a&b`,
are ordinary name characters. Repeated function parameters are legal; the last
binding wins, including a rest binding that shadows a fixed parameter. Each
overload has a separate scope and shares the optional function self-name.

### Function arity

Calls to a single fixed method do not check argument counts: missing parameters
receive JavaScript `undefined`, and extra arguments still evaluate before the
body, which ignores them. This matches upstream runtime behavior; upstream warns
about known local/global arity mismatches at compile time, while the light-cljs
strict analyzer does not. Multiple fixed methods throw `Error("Invalid arity: N")`
for unmatched counts. When a variadic method is present it supplies the default
branch, including underfilled calls: missing fixed parameters receive `undefined`,
and an absent rest receives nil. Direct rest arguments are immutable indexed
sequences, not JavaScript arrays; `apply` can retain its existing sequence tail.
Prefix lengths are unrestricted. Upstream's `apply` helper has a separate
20-argument limit when it selects the bounded fixed-call path of a variadic
function; direct calls and the variadic sequence entry support larger prefixes
and rests.

The emitter exposes `cljs$core$IFn$_invoke$arity$N`, the variadic delegate,
`cljs$lang$maxFixedArity`, and `cljs$lang$applyTo` where ClojureScript does.
Metadata functions use callable-object entries that invoke their wrapped function
with upstream receiver rules. Duplicate fixed methods, multiple variadic methods,
and a fixed method wider than the variadic prefix produce located declaration
errors. (Upstream only warns for duplicate/incompatible signatures and can emit
invalid JavaScript for multiple variadics.)

`letfn` takes named function declarations; its primitive `letfn*` takes an even
vector of simple names and function initializers. Named, overloaded, variadic
and macro-generated functions share the ordinary function ABI. Empty groups and
bodies return nil. Repeated group names retain their last declaration. Invalid
bindings and non-function initializers produce located errors. Function metadata
expressions run during initialization, before the body; an initializer exception
stops the remaining initializers. Mutual calls begin after the group is initialized.

Stable local identities preserve escaping functions across same-spelled nested
`letfn` groups and loop iterations. **Dialect difference:** both upstream
emitters reuse JavaScript `var` slots in two such cases, so an escaped function
can observe a later group's binding; light-cljs keeps each lexical identity
distinct.

### Callables and dispatch

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

The binding bridge also constructs `deftype`/`defrecord` receivers, including
positional/map record constructors, receiver fields, mixed IFn/custom protocol
methods, Object methods and `ISeqable`/`IIndexed`/`ILookup` access needed by
destructuring. Record/reify metadata copies retain their methods and captures.
A literal `nil` callee is rejected during analysis. Dynamic non-callables produce
JavaScript TypeError. Ordinary calls first access the callee's `.call` property:
a nullish callee fails before argument effects, while numbers, booleans, and plain
host objects evaluate arguments before the invocation fails. Explicit `recur` is
stack-safe; ordinary self-calls continue to use JavaScript calls.

`apply` supports all fixed prefixes and a sequence tail. The standard function
combinators `identity`, `constantly`, `complement`, `comp`, `partial`, `juxt`, and
`fnil` retain their public arities, evaluation order, and lazy variadic tails.
`this-as`, `js-arguments`, and `copy-arguments` preserve receiver and argument
observations, including value-context function scopes.

Reader characters become one-unit strings. Symbolic numbers, large integers,
BigInt and decimal spellings, and integral ratios use ClojureScript's Number
semantics. Non-integral ratios are rejected because the JVM ClojureScript compiler
cannot emit Ratio constants either. A safe-integer bound is not imposed.
Unresolved symbols are errors, also in unselected branches (unless `--warnings`
is used). JavaScript globals use `js/` qualification or an explicit namespace
`:refer-global` declaration.

### Async functions and await

As in upstream since CLJS-3470 (pinned a5e94c98), `:async` metadata makes a
function an `async function`: on the `fn` symbol, `(^:async fn [x] ...)`; on
a `fn`'s self-name, `(fn ^:async f [x] ...)`, which is also how `letfn`'s
`(^:async f [] ...)` reads it; and on a `defn` or `defn-` name,
`(defn ^:async f [x] ...)`. **Dialect difference:** upstream also passes a
`def`'s Var name to its `fn*` initializer, so `(def ^:async f (fn ...))` is
async there; light-cljs reads only the three places above. Every arity is
async, fixed or variadic, and so is the `arguments.length` dispatcher of a
multi-arity or variadic function, as upstream's front function is: an arity
error rejects the call's Promise instead of throwing. `apply` entries stay
plain functions returning the method's Promise, as upstream's. A call returns
a Promise of the body's value, and a throw in the body rejects it. `await` is
upstream's `cljs.core/await` and resolves like any core macro:
`clojure.core/await`, a `cljs.core` alias, `:refer`/`:rename`, and a macro's
syntax quote all name it, and a local or definition named `await` shadows
it. It emits JavaScript `(await x)`.

`await` is legal only in the body of an `^:async` function. Any other
function boundary clears that, as upstream's `fn*` sets `:async` from its own
metadata: a plain `fn`, `#()`, the thunks of `delay`, `lazy-seq` and `for`,
and `reify`, `deftype`, `defrecord` and `extend-type` methods. Elsewhere,
including at the top level of a file, `--eval` and `--repl` (upstream has no
top-level `await` either), it is upstream's failed assertion, a located
analysis error: `Assert failed: await can only be used in async contexts`.
A source macro expanded in an `^:async` body sees `:async true` in `&env`, as
upstream's macros do.

Inside an `^:async` method, a function the emitter wraps around expressions
that `await` (see [Context, truthiness, and the macro
boundary](#context-truthiness-and-the-macro-boundary)) is itself an awaited
`async` one, as upstream's `iife-open` writes it: `let`, `letfn`, `loop`,
`try`, `throw`, `case`, `do`, `binding`, `with-out-str`, `this-as`, `assert`
and type definitions in value position. An `await` therefore never leaves
the async function it is in, at any level. Each such wrapper costs a
microtask turn or two, as upstream's do; a wrapper around expressions that
do not await stays a plain function. **Dialect difference:** the wrapper
returns its value boxed, `(await (async function () { ... return [v]; })())[0]`,
so it never adopts a Promise its expressions produce:
`(let [x (await a)] p)` is `p` itself. Upstream awaits every wrapper in an
async function, which unwraps such a `p`, but only where its emitter happens
to write a wrapper. In light-cljs a value is the same at every level, where
`Advanced` folds a `let` away or writes a `do` as a comma expression.

A nested `def` that awaits in an `^:async` method of a namespace the macro
heap compiles evaluates its initializer before registering it, since that
registration takes a synchronous thunk; a nested `defonce` first asks
whether it keeps an earlier value, and evaluates nothing when it does.

`Advanced` never inlines an `^:async` method (its call is a Promise and its
body may `await`), types such a call as `any` instead of its body's type,
treats it as unknown when deciding whether an unused definition is inert,
and never moves an argument that awaits to its parameter's read inside an
inlined body: other code runs while it waits, and could change what the body
reads first. Folding treats `await` as an effect. Terse emission renames
names only and keeps `async`/`await` as written.

Under `--eval` and `--repl` the session runs promise jobs after each
top-level form ([evaluation](evaluation.md#script-and-repl-commands)), so by
the next form an async call has run the rest of its body after each `await`
and its Promise's reactions have run; a call whose Promise then stays
rejected with no handler is a warning under `--repl` and the script's error
under `--eval`. Compiled output runs in the host's
event loop at every level and with `--esm`. `tests/compiler/async_await.rs` runs upstream's
`async_await_test.cljs` there, with `clojure.test`'s `deftest`/`is` and
`goog.object/equals` supplied as small sources, and its `cljs.test`
reporting test replaced by one that awaits the failing test.

## Atoms, volatiles and state protocols

`atom`, `volatile!`, `deref`/`@`, `reset!`, `reset-vals!`, `swap!`,
`swap-vals!`, `compare-and-set!`, `set-validator!`, `get-validator`, `add-watch`,
`remove-watch`, `volatile?`, `vreset!` and `vswap!` execute in the embedded
JavaScript runtime. `Atom`/`Volatile` constructors and factories are available.
`reset-meta!` and `alter-meta!` change reference metadata without changing state
or notifying watches.

Validation runs synchronously before reset captures its old value. Failed
validation preserves state unless the validator itself changes it; a thrown
validator value escapes unchanged and failed installation retains the previous
validator. Construction does not validate the initial state. Watches run after
state changes, from the watch map captured by that notification, and may reenter
state operations. Their failures do not roll back state. `reset-vals!` and
`swap-vals!` retain the proposed new value even if a watch changes state again.

ClojureScript's `compare-and-set!` uses structural `=`, including equal but
distinct collections; it does not use the JVM atom identity rule. Public
functions and their state protocol methods dispatch consistently on custom
reify/type implementations. Custom swap-vals/reset-vals dereference before
calling the custom method, preserving dereference side effects. Protocol
predicates retain marker and partial declarations; native atoms implement
`IAtom`, `IDeref`, `IMeta` and `IWatchable`, not `ISwap` or `IReset`.
`vswap!` preserves the upstream macro's repeated target evaluation and callback
order.

## Reader forms and runtime literals

The compiler expands `#()`, `%`, `%n`, `%&`, `@`, `#'`, metadata, namespaced maps,
and syntax quote/unquote through its semantic reader boundary. Reader aliases
include runtime and explicit macro aliases; macro aliases take precedence.
Anonymous arguments have fresh identities, nested shorthand is rejected, and
rest arguments are nil when empty. `%-1` and `%&` share the same rest binding;
`%0` and indices below `-1` retain reader symbols in the body without positional
parameter bindings. Such unbound symbols remain visible in quoted reader data
and are diagnosed at their original placeholder when analyzed as code.
Syntax quote qualifies symbols and preserves unquote/splice evaluation, gensym
identity and owned UTF-16. `#'` resolves global Vars even beneath local shadows;
deref observes their current roots, and Vars are callable. Quote traverses the
expanded data without executing it.

Regex literals construct JavaScript RegExp values; `regexp?`, `re-pattern`,
`re-find`, `re-matches` and lazy `re-seq` use the JavaScript regex engine in both
application and macro phases. `#inst` validates Gregorian timestamps and
constructs Dates; `#uuid` validates and canonicalizes UUID literals. Dates and
UUIDs compare by value, and separate literals preserve distinct object
identities. `#js` vectors/maps construct host arrays/objects, retaining nested
language values and evaluating expressions once in source order. Object keys
must be strings or unqualified keywords. `#queue` expands to `into` and an empty
persistent queue, including when quoted.

`pr-str` exposes tagged printing; metadata is retained on literal collections
and quoted symbols through `meta`, `with-meta` and `vary-meta`. Collection
updates preserve metadata; ordinary seq tails discard it while queue seq tails
retain it. `sequence` retains lazy seq identity; `vec` strips metadata. Atoms
support mutable reference metadata, Vars support immutable metadata copies,
and reify/records retain their methods. Runtime metadata maps evaluate their
expressions; quoted metadata is data. Reader locations are elided from runtime
metadata while definition metadata remains available to macros and Vars.

Embedding handlers registered with
`MacroHost::register_compiler_data_reader(tag, function_source)` execute compiled
JavaScript over immutable compiler-owned reader data. Only finite form-domain
results cross the typed boundary; functions, mutable references, opaque host
objects and cycles are rejected at the original tag span. Handler closures keep
state within a compilation and reset before the next compilation. Unknown tags
fail without dynamic class, filesystem or namespace lookup. Handlers read resources only through `light.build` ("Compile-time services: light.build"). Reader handlers run
once in source order, including discarded forms and unselected runtime branches.
Evaluation sessions can instead register source data readers (see
[evaluation](evaluation.md)). Compiler-only opaque values keep regex/Date/UUID/
JavaScript literals intact through macro expansion; they are never printed and
reparsed. Reader evaluation (`#=`) is always disabled.

Runtime `.cljc` namespace files select `:cljs` or `:default` reader branches,
including collection splicing, before dependency discovery. `.cljs` and standalone
`compile(source)` input reject reader conditionals.

## Compiler-owned standard libraries and stream reading

`clojure.string`, `clojure.set`, `clojure.walk`, `cljs.reader`, `clojure.edn`,
`light.lazy` and `light.build` are embedded first-party sources (`src/compiler_libraries/`) compiled by the
ordinary namespace resolver. Explicit source roots take precedence, as for other
namespaces. Runtime imports, aliases, refers and unmarked macro helpers use the
same sources and JavaScript runtime.

`clojure.string` includes all 20 public functions, with UTF-16 text and JavaScript
regular expressions. Set projection drops relation metadata, retains row metadata
and uses the requested equivalent keys, matching `select-keys`. Walks preserve
metadata, map entries and records, visit callbacks in order and use bounded
iterative traversal state through callback reentry.

**Dialect difference:** `clojure.string` follows the documented contracts where
the pinned upstream implementation has JavaScript-level quirks:

- Literal replacement strings are always literal, including `$&` and `$1`
  (upstream passes them to JavaScript `replace`, which interprets them).
- Regex replacement callbacks receive the full match string, or a vector of the
  full match and capture groups, in both `replace` and `replace-first`
  (upstream `replace-first` passes JavaScript's raw callback arguments).
- `replace-first` replaces exactly one occurrence even with a global regex.
- Regex flags such as dotAll and Unicode are preserved; global/sticky flags are
  normalized to the requested operation.
- Positive-limit `split` uses the match index and full match length, so regexes
  with capture groups split at the correct offsets.

`clojure.edn/read` and `read-string` implement a strict data profile.
`cljs.reader` exposes code-data reading, built-in tags and compiler-emitted tag
parser registration. Reader cursors live in JavaScript. `cljs.reader/input-provider`
and `Evaluator::input_provider` accept explicit synchronous chunk providers;
`string-push-back-reader` needs no host capability. Core `read`, `read-string`,
`read-line` and `with-in-str` share those cursors. There is no ambient stdin or
filesystem. Provider failures, malformed input and same-cursor reentry are
located; input, node, depth and execution limits bound reading and tag handlers.
Nested input scopes restore and close their streams after success or failure.

## Numeric operations and fundamental predicates

The resolved core includes arithmetic and variadic comparisons, `inc`/`dec`,
`max`/`min`/`abs`, integer/floating/character and unchecked coercions, quotient,
remainder, modulo and every public bit operation. Boolean, nil, number, string,
named-value, function and collection predicates are first-class functions.
They preserve JavaScript Number, UTF-16 and private collection-brand semantics.
`int` wraps to 32 bits; `long` truncates without wrapping.

Numbers are JavaScript Numbers everywhere — in applications and during macro
expansion. For example, `(/ 1 2)` returns `0.5`, `(= 1 1.0)` is true, and
`(inc 9007199254740992)` stays `9007199254740992`. Rounding, overflow, NaN,
infinities and signed zero follow the host. Reader integer/decimal distinctions
remain source data for diagnostics and duplicate checks; wide radix integers
round once at execution. Non-integral ratio literals are rejected; ordinary
division produces fractional Numbers. There is no exact integer or ratio
arithmetic.

Saved functions, aliases, shadowing and `apply` use the ordinary callable ABI.
Malformed direct numeric macro arities are rejected during analysis, while
saved function values preserve upstream's missing/extra argument behavior.
All local calls evaluate arguments once in order. **Dialect difference:**
upstream's direct comparison macros may skip or repeat operand expressions;
light-cljs always evaluates each operand once.

Closure `goog.math.Long`/`Integer` construction requires an explicit embedding
that loads those classes. Once initialized, `int?`, `pos-int?`, `neg-int?` and
`nat-int?` recognize their instances and use their sign/zero methods. `integer?`
retains its JavaScript Number contract, including negative zero and
exponent-notation limits. No ambient Closure constructors or general Closure
library loading are supplied.

## Immutable collection data and the core runtime

Core functions are first-class callable values. Locals and current namespace
globals take precedence over unqualified core fallback. Explicit `cljs.core/F`
and `clojure.core/F`, or an alias for `cljs.core`, select the core
implementation. Same-named members of user namespaces remain ordinary globals.
Saved core references retain their function identity after shadowing.

Keyword conversion is bounded to strings/keywords/symbols and a nil namespace;
two-argument inputs outside that domain fail with `Error`, even where upstream
permits arbitrary values. Keyword literals own UTF-16 namespace/name information.
`::x` uses the source namespace (`cljs.user` for standalone compilation);
`::alias/x` uses the runtime/macro reader alias view, with macro aliases taking
precedence. Unknown aliases fail with a source location. Macros can return
vectors and keywords directly; generated names retain semantic data, including
lone surrogates, rather than being sliced from the macro call's text.

`src/js_runtime.js` is embedded once inside the output IIFE when the resolved
program contains a collection, named literal or core value. It has no imports,
external global core, host runtime dependency or package dependency.
Primitive-only output omits it. The source file is partitioned into units by
`//@unit` header comments that declare each unit's runtime dependencies;
`src/runtime_manifest.rs` documents the format and validates the headers
against the code, and `build.rs` removes the header lines, so the embedded
runtime is the file without them. Owned arrays/cells and base callable storage are
frozen; per-type extension prototypes and public value wrappers remain extensible
for protocol and JavaScript property writes. Those properties do not replace the
private collection payloads; private brands prevent host arrays or objects from
impersonating language values. Nested immutable values and opaque host
references are shared; caller-owned host objects are never frozen or converted
into collections.

A vector is upstream's persistent 32-way trie with a tail of up to 32 items:
`count` is O(1); `nth`, `get`, `assoc` at an index and `pop` are O(log32 n),
with the tail read and copied directly; `conj` is amortized O(1). Constructing
a vector of n items is O(n). Its brand and store are a private class field
rather than a WeakMap entry, which no host object can have or copy either, and
its trie nodes stay private: `.-root` of a vector is an opaque token, and
`.-tail` a copy. `subvec` is O(1), as upstream's Subvec: its store is a view of
the vector's (a subvec of a subvec views the first vector), which `nth`, `get`,
`count`, `reduce`, `pop` and `assoc` read or update through; a seq or iterator
of one copies 32 items at a time, and an operation that needs a trie of its own
(`conj`, `transient`, the public fields) builds one once, in O(n). The public
Subvec constructor takes the same view where its source is a vector holding
every index it names. `rseq` of a vector copies its items leaf by leaf, in
O(n), where upstream's RSeq is O(1). Equality
compares sequential contents across list/vector/seq kinds, with an identity fast
path for the same immutable value. Keywords and symbols compare by full qualified
spelling within their own kind. Host objects/functions compare by identity; nil
and undefined compare equal. `[]` differs from nil, and keywords/symbols differ
from strings and from each other. Internal hashes are consistent across equal
sequential values and nil/undefined; foreign objects use weak identity
bookkeeping. For the unusual keywords `(keyword nil "a/b")` and
`(keyword "a" "b")`, spelling equality is true while namespace/name hashing
differs. Small literal/array maps merge them and retain the first key with the
last value; explicit hash maps retain both.

Vector `.call` uses `nth` semantics for its first index; extra arguments evaluate
and are ignored. Missing or out-of-range indexes throw ordinary `Error`.
Keyword/symbol `.call` accepts a collection and optional default, using `get`
semantics; dynamic unsupported arities throw `Error`. Invalid literal keyword
calls are rejected with a located analysis diagnostic (upstream's invalid-call
inlining artifacts are not reproduced).

Core wrappers follow upstream fixed/overloaded/variadic behavior: `(count)`
returns zero and fixed-argument extras evaluate before being ignored; `(=)` is
true. Vectors accept fractional numeric `nth`/`get` indexes by truncation, but
`contains?` requires an integer index. String lookup follows upstream's
coercible-index rules and UTF-16 units. `nth` requires a numeric index even for
nil: `(nth nil "0")` fails, `(nth nil 0)` yields nil, and `(nth nil 0 :missing)`
yields the default. False, nil and undefined stored in collections remain present
values.

Quote traverses the source syntax or macro-owned semantic data directly into
owned `Symbol`, `List`, `Vector`, `Map`, `Set` and scalar expressions; no
printer/reparser participates. Quoted unknown names and call-shaped forms are
data, even when a macro of that name exists. Quoted characters, syntax quote,
metadata, tags and namespaced maps retain their reader semantics. Built-in tagged
values retain their runtime types. Macro-generated quoted symbol names retain
exact UTF-16, including lone surrogates that remain invalid as executable symbol
names.

One-argument `symbol` accepts strings, symbols (identity) and keywords (parsing
full spelling), and throws `Error` for other inputs. It splits a string at the
first slash only when that slash follows a nonempty namespace: `"a/b/c"` has
namespace `"a"` and name `"b/c"`, while `"/x"` remains unqualified. Two-argument
symbol construction is bounded to string name and string/nullish namespace;
arbitrary host values accepted by upstream in these slots are excluded. Symbols
are not interned. Keyword literals are: at every optimization level each literal
site, and each constant keyword of the runtime itself, yields the one keyword
for its namespace and name, so `(identical? :a :a)` is true. Keywords built at
run time (the `keyword` function, symbol conversion, the reader, keywordized
`js->clj` keys, the positional `Keyword.` constructor) are fresh, as upstream:
they equal a literal with the same full name, and hash like it when their
namespace and name match, but are not identical to it. Symbols are callable through ordinary
`.call` lookup; lists and seqs evaluate argument effects before failing with
`TypeError`.

### Lists, vectors and seqs

Lists retain frozen private cons cells with an empty singleton and cached counts.
`conj` prepends list elements, appends vector elements, and constructs lists from
nil; its zero-argument result is `[]`, and its one-argument result is unchanged.
Vector `assoc` replaces an existing numeric index or appends exactly at count.
In-range positive fractions truncate like upstream; invalid indices throw `Error`
without changing the old vector. An odd extra key receives nil. Vector updates
copy the tail, or the trie's path to one leaf; list prepend, first and tail are
O(1), and list construction/indexing are O(n).

`seq` of an empty value is nil; nonempty lists can reuse their identity, and
vectors/strings produce views with frozen private offset/storage descriptors.
A vector of more than 32 items gives upstream's ChunkedSeq over its trie's
leaves and tail, which a seq shares frozen. Cons cells, views, lazy seqs and
chunks keep their payloads in class-private fields, as vectors do, rather than
in `WeakMap` entries (light-cljs-259): each step of a lazy pipeline builds new
values, and their entries, which the collector traces as ephemerons, cost more
than the step. A lazy seq or chunked cons built with nil metadata stores none;
`meta` and its `meta` field read nil either way.
`rest` at the end (including nil) is the empty list, while `next` is nil. Each tail
is O(1), so complete traversal is linear. String traversal produces UTF-16
one-unit strings. `conj` of a generic seq produces a seq-cons, distinct from a list.

`seq`, `first`, `rest`, `next` and `count` read a value's kind once, instead of
probing each payload store in turn (light-cljs-237). The prototypes of the lazy
seq type and of the cell types (`List`, `Cons`, `ChunkedCons` and the two range
types) hold a kind number under a private symbol, which picks the kind's
branches from a table that only the runtime can reach. Each operation is
wrapped, inside its protocol wrapper, by a function that calls its kind's branch
and else the operation; a value with no kind number calls no branch. A branch
takes the value apart from its store and payload and gives a private `missing`
for anything it leaves to the operation, so a host object that inherits a
prototype or holds a kind number is still no language value. A lazy seq's
branches realize it and answer with the cell branch of what it realized. Where
protocol implementations exist, the branches of `first`, `rest` and `next`
leave every value to the operation, whose path finds an `ISeqable` slot
JavaScript gave a value where it did before; `extend-type` of a type with a kind
or of `Object`, and `specify!` of a value with one, empty the tables. The views
of vectors and arrays (`IndexedSeq`, `ChunkedSeq`) are a third kind with a
branch of `first` only, and take their number only where a program keeps the
wrappers. At `Advanced` the wrappers and branches are effects of the kinds'
types that wait for their operations, so a program that builds no lazy seq or
cell keeps none of them. The same pellet made a step of a seq cheaper: a
range's cell is a class instance instead of a frozen literal with a getter
closure, a chunked cons's cell is its own hash holder, list and cons cells are
not frozen (their payloads are private and never written), a store's `set`
assumes a new value (`reset` replaces a payload), and where no protocol
implementation exists a chunked cons and `map`, `filter` and the indexed
sequence functions read a runtime chunk's store instead of calling `nth` and
`count`.

List `nth` requires an exact reachable integer; fractions default or throw.
Indexed seq views (strings and vectors of at most 32 elements) instead use
absolute storage offsets, as upstream does: fractional properties can yield
undefined, and a negative index into a tail can reach preceding storage. Larger
vector seqs use linear traversal. `get` on nonassociative lists/seqs returns the
supplied default (nil when omitted), and `contains?` returns false.

### Hash maps

An array map (at most 8 keys) stores a frozen entry array. A hash map, which an
array map becomes at its ninth distinct key and which `hash-map`, larger literals
and transients build, stores upstream's structure (light-cljs-235): a hash array
mapped trie of its entries (bitmap nodes, array nodes past 16 children, collision
nodes for keys of equal hash), its nil entry beside the trie, and its count. Keys
hash with the runtime's `hash`, so -0 and 0, nil and undefined, and equal
collections land together. Lookup compares both hash and language equality, so
collisions do not merge unequal keys, and a list/vector/seq with equal contents
can retrieve the same entry. Nested map keys use unordered equality/hash.
Keyword, symbol and string keys remain distinct; `"__proto__"` is ordinary data.
Map count is O(1). Hash-map lookup, `find`, `contains?`, `assoc` and `dissoc`
are O(log32 n): an update copies one path of trie nodes and shares the rest,
plus key hash/equality costs; array-map ones are O(n) in their at most 8
entries. Building n distinct entries one `assoc` at a time is O(n log32 n), and
map equality looks up each entry of one map in the other. A transient hash map
starts from the map's trie in O(1) and edits it in O(log32 n) per `assoc!` or
`dissoc!`: as upstream's, it copies a node of the persistent map once, the
first time an edit passes through it, and then changes that copy, which no
other version shares, in place, so a run of edits allocates amortized O(1)
nodes each. Each node keeps its count of occupied slots, so an insertion counts
none, and a bitmap node the capacity of upstream's array for it, which decides
when an edit turns it into an array node, including after a removal or an array
node's packing. That array node gets new slots, as upstream's, so a key whose
hash throws during it leaves the transient as it was. `assoc!` and `dissoc!`
change the transient's root, nil entry and count in place. Builders (`hash-map`, larger literals) edit their
unpublished trie the same way. New versions retain the first equivalent key and share
unchanged entry cells, trie nodes and nested immutable values. A hash map keeps
its trie in a class-private field rather than a `WeakMap` entry: as unforgeable
a brand, without a `WeakMap` registration per version. `seq`, printing and
reduction walk the trie in upstream's order (nil first, then by hash bits), and
a builder's map also keeps its entries in literal order, which advanced
constant lookups read.

Maps implement ordinary `.call` with a key and optional default, alongside keyword
and symbol lookup calls. Missing is a private sentinel: stored nil, false and
undefined remain present, and `contains?` returns true for them. Nil/undefined keys
are language-equivalent; this runtime consistently retains whichever equivalent
key was inserted first. (Upstream hash maps can canonicalize an undefined key to
nil while array maps retain it; that storage-specific presentation is not
reproduced.) Stored undefined **values** always remain observable as undefined.

`assoc` accepts maps or nil and supplies nil for an odd trailing key. `hash-map`
requires pairs and throws ordinary `Error` for an odd argument count. `dissoc`
removes zero or more keys, preserving all old versions. Map `conj` uses the first
two items of a vector of length at least two, accepts maps and finite sequences
of vector entries, and treats nil as a no-op; shorter vectors or invalid entry
sequences fail without mutating the input. Map seq materialization costs O(n),
produces two-element map entries, and then advances tails in O(1). `seq {}` is
nil; `rest {}` is the empty list. Hash-map and array-map traversal order is
unspecified. Sorted maps traverse in comparator order; see
[sorted collections](#sorted-collections).

Literal duplicate reader data is rejected at the repeated key's source span.
Reader identity distinguishes integer from floating tokens and equates list/vector
contents. Lowering then uses JavaScript Number equality: `{1 :int 1.0 :float}`
has one runtime entry whose value is `:float`. All literal key/value expressions
run once in their original order, including dynamically duplicate keys.

**Dialect differences:** two upstream compiler optimizations are deliberately not
reproduced: direct `hash-map` calls can evaluate all keys before values, and
unchecked literal construction can retain duplicate JavaScript Number keys that
were reader-distinct (`1` versus `1.0`). light-cljs consistently uses ordinary
call order and checked map construction.

`PersistentHashMap.fromArrays` constructs a hash map from separate JavaScript key
and value arrays in both application and macro phases. A missing value throws
`Error` with `No value supplied for key: ...`; extra values are ignored.

### Hash sets

Set literals (including quoted data), `hash-set` and `disj` use private immutable
member entries, sharing the map hash/equality search; native JavaScript `Set` is
never language set storage. `count`, `get`, `contains?`, `conj`, `seq`, `first`,
`rest`, `next` and `=` accept sets. `assoc`, `dissoc` and direct `nth` do not.
`disj nil` remains nil. Callable sets take a member and optional default;
unsupported arities throw ordinary `Error` after evaluating arguments.

Membership returns the first retained equal member, including false, nil or an
undefined member. Nil and undefined compare/hash alike, as do equal
list/vector/seq contents. Set equality and hashes ignore member order, while sets
remain unequal to sequences, maps, and host arrays/sets. Nested maps and sets work
as members and map keys. Weak identity bookkeeping handles foreign object/function
members without examining, freezing or copying host data. No strong global hash or
intern cache retains values.

A set of at most 8 members stores a frozen member array, as an array map. At its
ninth distinct member it becomes a hash set, which stores what upstream's
PersistentHashSet does through its PersistentHashMap (light-cljs-236): the hash
map's trie of its members (each with a nil value), its nil member beside the
trie, and its count, in a class-private field of its own as a hash map's (see
[Hash maps](#hash-maps)). The members go into the trie as upstream's transient
array map promotes to a transient hash map, so traversal order and the calls
that hash members are as before. Set count is O(1). Hash-set lookup (`get`,
`contains?`, a call, `.has`), `conj` and `disj` are O(log32 n): an update
copies one path of trie nodes and shares the rest, plus member hash/equality
costs; array-set ones are O(n) in their at most 8 members. Building n distinct
members one `conj` at a time, or with `set`, `hash-set`, `into` or a literal,
is O(n log32 n). Equality looks up each member of one set in the other, in the
order `seq` walks them (as upstream's `reduce-kv` over its map), so it is O(n)
lookups, and so are `clojure.set`'s `union`, `intersection`, `difference`,
`subset?` and `superset?` in the members they visit. A transient hash set,
as upstream's TransientHashSet over a transient hash map, starts from the set's
trie in O(1) and edits it as a transient hash map does. Initial seq
materialization is O(n), subsequent tails O(1). Traversal order is unspecified;
a hash set walks its trie as a hash map does (nil first, then by hash bits).
A set the public `PersistentHashSet` constructor builds from a map copies the
map's keys, O(n), at that and each later update.

Duplicate readable set member forms report the repeated member's source span,
including executable forms such as `#{(observe 1) (observe 1)}`. Dynamic distinct
forms evaluate once in source order; structurally equal results then throw Error,
matching the pinned ClojureScript checkout (the Maven 1.12.134 release instead
deduplicates them). `hash-set` and `set` deduplicate structural members. No-op
`conj`/`disj` preserve the original set, metadata, and stored equal member.
**Dialect differences:** upstream set literal storage can reorder operand effects,
and direct `hash-set` inlining can retain reader-distinct but Number-equal members
(`1` and `1.0`); light-cljs preserves source operand order and deduplicates.

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
`reduced` result does not request another element.

### Reduction and Reduced markers

`reduce` accepts a reducer and collection, or a reducer, initial accumulator and
collection. Other argument counts raise ordinary `Error` after argument effects.
It traverses nil/undefined, immutable collections, seq views, strings (one UTF-16
code unit per element), host arrays, lazy/unbounded sources and custom reduction
protocols. Maps supply map entries; hash map/set traversal order is unspecified.

Without an initial accumulator, an empty source calls the reducer's zero arity,
a singleton returns its element without a call, and other sources start with
their first element. With init, an empty source returns that exact value without
calling the reducer. Each step invokes `(rf).call(null, accumulator, item)`.
Missing fixed user parameters receive undefined, and extra parameters are ignored.
There is no one-argument completion call. The function argument is evaluated
once; global redefinition during a step does not replace it. Source and callback
exceptions propagate unchanged and prevent later steps.

`reduced` creates a private immutable marker; `reduced?` recognizes only those
markers, `unreduced` removes one layer if present, and `deref` removes one layer
from a marker. The same public `deref` also dispatches on Vars, atoms, volatiles
and custom `IDeref` implementations, or throws `Error` for unsupported values.
Only a marker returned **by a step** stops reduction and loses one layer. An
explicit init, singleton element or empty-source zero-arity result is retained
exactly, including nested markers.

Traversal is iterative, uses direct immutable storage offsets and allocates no
closure or seq wrapper per step. `reduce-kv` dispatches `IKVReduce` and supports
owned maps, vectors and reducible map-entry sequences.

### Transducers

`transduce` supports both three- and four-argument forms; `completing` adds
completion to a step function, and `ensure-reduced` preserves an existing Reduced
marker. Without an explicit initial value, `transduce` calls the original
reducer's zero-argument form before applying the transform. Reduction removes one
Reduced layer, then invokes the transformed reducer's completion once. Throwing
initialization, transforms, source realization or steps prevent completion.

Transducer constructors include `map`, `map-indexed`, `keep`, `keep-indexed`,
`filter`, `remove`, `take`, `drop`, `take-while`, `drop-while`, `take-nth`,
`partition-all`, `partitionv-all`, `partition-by`, `cat`, `mapcat`, `interpose`,
`replace`, `dedupe`, `distinct`, `random-sample` and `halt-when`. Their state is
fresh each time a constructor receives a reducer. Buffered partitions clear before
emitting, including during completion and reentry. `cat` preserves downstream
Reduced layers through each nested reduction and stops before realizing another
inner item. `halt-when` preserves the upstream `::cljs.core/halt` return
convention and optional completed-result callback. Transforming `into` uses
persistent destinations and retains metadata.

`sequence` supports coercion, one-source transduction and variadic multi-source
transduction. Construction applies the transform without pulling inputs; each
returned sequence memoizes output chunks and keeps independent reducer state.
Demand fills at most 32 outputs per chunk and probes one extra transformed output
for lookahead, including source effects. Completion runs once on exhaustion or
Reduced termination and can emit more values. Multi-source input stops at the
shortest source.

`eduction` creates repeatable views that reapply transforms with fresh state for
each reduction or traversal, including independent iterators, buffered completion
and finite prefixes of infinite sources.

### Public core protocols

`Inst`, `IDerefWithTimeout`, `IUUID`, and `ASeq` are public protocol identities.
`inst-ms*` dispatches the `Inst` method, including native Date, user types, reify,
and native/default extensions; `inst-ms` calls that method and `inst?` uses
`satisfies? Inst`. `-deref-with-timeout` dispatches its three-argument protocol
method directly. `IUUID` and `ASeq` are markers. `uuid?` checks direct `IUUID`
implementation, so a native marker extension can satisfy `IUUID` while `uuid?`
remains false. `ASeq` marks IndexedSeq, nonempty List, Cons, ChunkedCons, and the
runtime's NodeSeq representation; it does not mark every sequence. Protocol
instances and protocol functions are unsupported macro expansion results.

Public core operations distinguish protocol membership from available methods.
A marker or a partial implementation satisfies its predicate but still fails
when a required method is missing. Custom sequence types work without `INext`;
counting stops at a counted tail, and reductions honor custom implementations,
iterator protocols and early `Reduced` results. `key`/`val` and `keys`/`vals` use
entry protocols. Built-in collections accept custom keys and compare with
compatible custom map, set and sequential interfaces. `Fn` and `IList` are public
marker identities. `ILookup` supports contains/destructuring callers without
requiring an associative implementation. Ordinary arrays retain host identity and
support public indexed lookup. Custom printers and writers support raw
write/flush methods; default printing passes nil options, as upstream does.

### Recurrence

Loop and method recurrence has a resolved target independent of value use. Each
iteration exposes fresh lexical constants and evaluates every recur argument before
updating its next state. This preserves swaps, argument effects, and closures
retained from earlier iterations. Nested functions/loops cannot target each other;
non-tail positions and cross-try recurrence are located analysis errors. Variadic
methods recur with one explicit rest sequence, without re-packing it.

Protocol methods accept either all parameters or all non-receiver parameters;
the receiver expression still runs when supplied, while the method's `this`
identity stays fixed. IFn must satisfy both upstream `.call` and direct entry
signatures and therefore excludes the receiver from recur arguments. Upstream's
IFn `.call` capture-slot failure after recurrence is reproduced, separately from
the direct invocation entry. Method recurrence also updates observable supplied
`js-arguments` slots as upstream's non-strict parameters do.

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
function returns and value expressions. Recurrence cannot cross any try/throw
boundary, while loops and functions declared inside a body, catch or finally have
their own targets. Malformed clause ordering, missing catch bindings, invalid
binding names and wrong throw argument counts produce located analysis errors.

`ex-info` accepts message/data or message/data/cause and constructs an
`ExceptionInfo` that is also a JavaScript `Error`, with the original payload
identities intact. Its two-argument form supplies a nil cause; an explicit
undefined cause stays undefined. `ex-data` and `ex-cause` return fields only for
that class; `ex-message` returns the message of any JavaScript Error, with nil for
other inputs. Class identity survives syntax quoting and macro expansion.
`instance?` supports direct calls and saved function values. `pr-str`, `str` and
`.toString` print the CLJS `#error` form (at `Advanced`, `.toString` only in a
program that can print). `Throwable->map` exposes the cause chain,
type/message/data maps, root cause, optional phase and nil trace.

## Context, truthiness, and the macro boundary

Expressions carry `Value`, `Discard`, or `Return` context during analysis. The
emitter propagates those contexts recursively: conditionals can be ternary
expressions or statements; bodies discard earlier values and return their last
value when needed. `let*`, `letfn*`, `loop*`, `try`, `case`, and `do` in value
position use small immediately invoked functions. This keeps evaluation order and
avoids hoisting expressions out of branches or call arguments.

The generated unit has a local truth helper, testing `value != null && value !==
false`. Thus `0`, negative zero, and empty strings are true. The nullish check
also handles `undefined` when a compiled function is invoked by a host harness.
Evaluating output produces the program's value; explicit host calls such as
`js/console.log` can print.

List analysis dispatches primitive special forms before touching binding vectors
or other operands. The macro hook resolves source/core macros, compiles their
bodies through this compiler and executes the emitted JavaScript in its session
with the current lexical environment and expression context. Each valid generation
expands once; semantic IR is shared between compile-time and application emission.
Lexical bindings shadow unqualified macros; qualified macro calls retain explicit
identity. A namespace binding cannot be both macro and value. Runtime definitions
can shadow fallback core macro names. `cljs.core/` and `clojure.core/` select the
same compiler bootstrap operations.

Original calls are lowered from their retained `SyntaxTree` into shared owned
`Form` data. Returned data is appended directly to a private copy of the syntax
arena, with indexed spelling/value/Form side tables; there is no
printing/reparsing step, JVM macro process, or second language evaluator.
Generated forms retain their call-site spans and macro attribution. Analysis
never mutates the caller's syntax tree.

## Definitions and globals

`(def name initializer)` registers a namespace-qualified global before analyzing
its initializer. The initializer executes once whenever that expression runs;
the expression yields its assigned value. Redefinitions reuse `GlobalId` and
ordinary mutable JavaScript storage (`g0`, `g1`, …). A closure that reads a
global observes later redefinitions; a closure capturing a local retains that
lexical binding. Global functions can recurse and be passed as values. Globals
are stored directly, not in Var boxes (`var`/`#'` creates a Var object on
demand, see below). Locals take precedence over unqualified globals; qualified
`namespace/name` references identify globals even under lexical shadowing.
Standalone `compile`/`analyze` use namespace `cljs.user`.

`(def name)` declares a global without resetting existing storage. An initially
uninitialized global reads as JavaScript `undefined`. As a discarded expression
it has no effect; in tail position its result is `undefined`. Uninitialized
definitions in required value positions, such as `(let* [x (def a)] x)`, are
rejected (upstream emits invalid JavaScript for this shape). Definitions accept an
unqualified name (including `&`), an optional docstring and initializer, and
definition metadata. A dotted definition such as `(def host.value 1)` assigns the
actual host path; the host root must already exist. `defonce` initializes only
undefined roots; existing nil/false roots suppress its initializer. `declare`
supports forward calls and preserves existing definition metadata.

`(def ^:const name value)` defines a constant: a later `def` of the name is
rejected ("Can't redefine a constant") and so is `set!` ("Can't set! a
constant"), and `case` accepts it as a test. At `--optimize advanced` a read
of a constant whose top-level `def` has a literal value (`nil`, a boolean,
number, string or keyword), in a top-level form after the definition, is that
value, so `(if FLAG ...)` folds away; reads in forms before it, including
functions they define, read the global, which may not be set yet, as at every
other level. `(goog-define name default)` defines such a constant whose
default is a string, number or boolean literal and whose value a build may
replace, at every level: `ApplicationOptions::defines` takes `Define { name:
"ns/name", value }` with a `DefineValue` of the default's kind, and
`--define ns/name=value` after `--compile` reads `true`/`false`, a number or
the text as written, by the default's kind. A define that names no
`goog-define` of the program or has another kind is an error at the CLI and
is listed by `PreparedApplication::rejected_defines()` in the API, which
ignores it.

Resolution is source ordered: a definition is visible to its own initializer
and later analyzed forms. The default API rejects undeclared/forward references,
including inside functions and unselected branches. The warning API (`--warnings`)
reproduces the standard compiler's warn-and-emit behavior for these forms. An
explicit preceding `(def name)` or `declare` permits a later assignment to that
identity. No namespace/global state survives across separate calls to `compile`
or `compile_namespace`.

### Dynamic binding and Vars

`binding` and `with-redefs` save current global roots, evaluate their initializers
in parallel, install temporary roots, and restore them in reverse order through
`finally`. Nested calls, exceptions, core roots, aliases and namespace readers
share the same storage. Initializer failures occur before the restoration region,
so initializer side effects remain. `set!` returns its assigned value; constants
and lexical locals are rejected. Binding a non-dynamic global is permitted, as in
upstream's warn-and-emit behavior.

These scopes are synchronous. A callback invoked inside a binding sees the
current roots; a later Promise callback sees restored roots. Captured lexical
values remain snapshots. Explicit boolean tags on globals preserve upstream's
JavaScript truth behavior; untagged dynamic globals retain CLJS truthiness.

`var`/`#'` creates an explicit callable object with a live getter, qualified symbol,
symbol-based equality/hash and captured definition metadata. Repeated Var
expressions compare equal without being identical. `with-meta` preserves the
getter while replacing metadata. `cljs.core/Var.`, its public fields and
`isMacro`, `var?`, `hash`, `deref`, invocation and `test` are supported.
`resolve` requires a quoted symbol and handles namespaces, aliases and locals;
its runtime existence check can observe later definitions. Unknown-at-analysis
names retain nil metadata. Core argument lists and docs come from the pinned data
table and do not load an external cljs.core in generated programs. Unused
definition metadata does not pull the collection runtime into primitive-only
output.

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
roots/files are deduplicated. **Dialect difference:** two distinct files at the
winning extension are an ambiguity error; root order never silently selects one
(upstream uses classpath first-match).
`compiler::analyze_namespace_entry_with_dependencies` also returns the
compilation's dependency manifest. It records each namespace's dependencies by
kind and every candidate file each lookup tried, present or absent
([compiler cache design](compiler-cache.md), "Conservative keys and
invalidation").

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
The current namespace can qualify its own globals, and so can the full name of
any namespace the program has already loaded, as upstream resolves it (a macro
expansion names the macro namespace's own dependencies); unknown qualifiers
are errors in the strict API. Repeated identical imports coalesce;
aliases that conflict with another target or redirect a required/current full
namespace name are rejected. `cljs.core` is a virtual dependency:
`(:require [cljs.core :as c])` never reads `cljs/core.cljs`. Reserved `cljs.core`
and `clojure.core` qualifiers cannot be redirected to user libraries. Ordinary
`:require` imports both macros and values from the same `.cljs` or `.cljc` source.
`:refer` and `:rename` retain the original namespace identity, including saved
values and syntax quotes; lexical bindings and own definitions can shadow
unqualified imported values. `:refer-clojure` exclusions and renames affect both
core functions and core macros; explicit core qualifiers remain usable. Missing
and conflicting refers fail even when unused. The equivalent
`:use [library :only [names] :rename {...}]` clause is supported, with `:only`
required.

Namespace metadata merges symbol metadata with the optional attribute map;
docstrings are available separately at `&env :ns :doc`. `:as-alias` is a
reader-only alias and does not load a source. A missing `clojure.*` runtime source
can resolve an existing `cljs.*` source while preserving both qualifiers.
`:import` binds actual host constructors and `:require-global` binds supplied host
globals; string libspecs bind host-supplied [JavaScript packages](#javascript-packages). Top-level quoted require/use/import and macro equivalents,
`refer-clojure`, and `refer-global` retain their namespace effects. Explicit
`goog.*` dependencies can use host namespaces when no caller source exists; no
Closure implementation is synthesized. Validation occurs before dependency
analysis, with diagnostics in the originating file.

Raw `(ns* (:require '[library :as alias]))` directives use the same quoted
libspec validation and explicit source loading, including `:refer`/`:rename`
and reader-only `:as-alias`. A raw directive contains one literal clause. It adds
imports to the declared source namespace and has a nil application result, also
inside a top-level `do`. Nested namespace declarations and legacy JVM macro
imports (`:require-macros`, `:use-macros`, `:refer-macros`, `:include-macros`)
produce located errors.

A sequential depth-first walk uses Unseen/Loading/Loaded states. Dependencies
initialize before consumers, in require order; each namespace loads once,
including shared dependencies. Source forms execute in order within each
file. Loading an already-Loading namespace reports the cycle path. Missing
sources, mismatched declarations, analysis errors, and parse errors include the
originating file/location when available. No output is emitted on failure.
The result is one combined JavaScript expression whose value is the entry
namespace's final form (with `--esm`, an ES module whose default export it is; with `--out-dir`, a directory of ES modules, below). There is no package manager or parallel
scheduler. Builds are fresh unless `--cache-dir DIR` asks for the persistent
compiler cache: each namespace whose conservative key (its source and
resolution, the keys of every namespace its compilation depends on, its
`light.build` inputs, the analysis and macro options, the `--config` map and
the compiler's own identity) matches a stored artifact is installed from it
at its place in source order instead of being analyzed, so none of its macro
calls run, and every other namespace is analyzed and stored. Lookup itself
runs no macro; a corrupt, missing or mismatched artifact is a miss. For
macros that only compute their expansion from their arguments, declared
inputs and the namespaces they depend on, the output is byte for byte the
uncached build's. A macro whose effects reach other namespaces' expansions
(a counter in a macro namespace) is outside that contract: a hit does not
repeat its calls, so mark the macro namespace `^:dev/always` or name it with
`--cache-blocker`, which reach every namespace compiled with it.
`^:light-cljs/no-cache` covers only the namespace it marks, so on a macro
provider it does not make its consumers' calls run again; it must mark each
consumer whose calls must. The contract, the keys
and the store are in [compiler-cache.md](compiler-cache.md).

A lazy reference, `(light.lazy/loadable app.editor/start)` or
`(light.lazy/loadable app.editor)` (also through an alias of `light.lazy`),
is not a require (docs/code-splitting.md). Its argument is a literal symbol;
the namespace need not be required, and the Var need not be defined yet, since
analysis reserves its binding by name as `exists?` does. The site lowers to
`ExprKind::Loadable` and is recorded in `Program::lazy_targets`. Analysis is a
worklist: after a source is analyzed, the loader visits each namespace its
lazy references name, which appends that namespace's unvisited dependencies
and then the namespace itself; a namespace already loaded is resident, and a
target may require the namespace that names it. Sources reached only that way
follow the entry and run after it, and the program's value is still the entry
namespace's final form, which then stores its value in a global the program
returns at the end. Once every namespace is analyzed, a Var target its
namespace never defines is an error at the site ("lazy target app.editor/start
is not defined"), as are a non-literal argument, a namespace no source
provides, a target in the site's own namespace and `cljs.core`. The `Program`
also records the static require graph (`namespace_requires`, by source index)
and which sources only lazy references reach (`namespace_lazy_only`). The REPL
loads the namespace of a loadable it has not loaded, as a `require` would,
before a top-level form runs, or after the outermost namespace load when the
form belongs to a namespace being loaded, then checks the Var targets.

A loadable's value is a `light.lazy/Loadable`, of the bundled first-party
namespace `light.lazy` (`src/compiler_libraries/light/lazy.cljs`), which also
defines `load` and `loaded?`. The type is the runtime's (unit `Loadable`),
which `(light.lazy/loadable-type)` reads, so that an `advanced` program pays
for no type machinery: effects give it what a `deftype` implementing IDeref
has only where the slice keeps code that can observe it (`deref`, a printer of
types, `defineType` or a protocol lookup by mask or slot), or where reachable
code names what they add, as a property, method, string or in `js*` text: the
IDeref marker or slot, a protocol mask, `cljs$lang$type` or
`cljs$lang$ctorStr` (`LOADABLE_MEMBERS` in `src/runtime_slice.rs`,
light-cljs-296). Every target is resident in the one
output file (a chunked build, `--out-dir`, loads the others; see Chunked
builds, below): the site emits `new Loadable(null, () => g20)` (`() => null` for a
namespace target), `@l` reads the target's current value, `loaded?` is true,
and `load` returns a Promise that resolves with `@l` on a later microtask, as
the load of a chunk already loaded would. The site reads the `Loadable` type
like any global, so it needs no require of `light.lazy`: a source whose compiled forms spell
`light.lazy/loadable` (also under syntax quote, for a macro) requires
`light.lazy` after its own requires, and the
REPL loads `light.lazy` before a form holding a loadable runs, even while a
namespace is loading. A site a macro writes in a namespace that neither spells
the symbol nor loads `light.lazy` earlier is an error ("light.lazy/loadable
needs light.lazy loaded before app.main: require it there"). Every lazy Var
target is retained like an `ApplicationOptions::exports` entry, since its
loadable reads it only once loaded: it keeps its definition and arities, and
its parameters take any argument. A namespace target needs no root, since a
namespace's top-level forms are roots however the namespace is reached. A target namespace that only lazy
references reach runs after the entry, so during that synchronous startup
`@l` reads its Var before the definition has run (`nil`); `load` always
resolves after it. Under `--eval` and `--repl` the session runs promise jobs
after each top-level form, so a `load` Promise's reactions have run by the
next form.

Standalone file/stdin/text modes accept `def` and interop, but a standalone
`(ns ...)` form is rejected (`unsupported construct: ns`): select an entry with
`--entry` and explicit roots to request namespace loading.

### JavaScript packages

String libspecs name JavaScript (npm) packages. The compiler never reads
`node_modules` or `package.json`; package resolution, CommonJS conversion and
tree-shaking belong to the bundler or to the embedding host.

```clojure
(ns app.main
  (:require ["react" :as r :refer [useState] :rename {useState use-state}]
            ["@mui/material/Button" :default Button]
            ["lodash.debounce" :as debounce]))
```

`:as` binds the package object, `:refer` (optionally with `:rename`) binds named
exports and `:default` binds the `default` export. A package name is any
nonempty string without whitespace, including scoped names (`@scope/pkg`), dots
(`lodash.debounce`) and subpaths (`some-package/sub`). Every reference lowers to
a lookup in one host-supplied registry object, `globalThis.lightCljsPackages`
(`light_cljs::PACKAGE_REGISTRY`): `r/createElement` reads
`lightCljsPackages["react"]["createElement"]`, `use-state` reads
`["react"]["useState"]` and `Button` reads `["@mui/material/Button"]["default"]`.
Export and member names use host-member munging (`r/use-ref` reads `use_ref`), so
spell exports as JavaScript does; dotted members such as `r/a.b` extend the path.
Lookups happen at each use site rather than through a copied binding, so a host
that replaces the registry or one of its packages is seen by code compiled
earlier, including REPL definitions. A missing registry or package throws a
`ReferenceError` at first use naming both the package and
`globalThis.lightCljsPackages`; a missing export reads as `undefined`.

The host fills the registry before the program runs, for example from an ES
module (`import * as react from "react"; globalThis.lightCljsPackages = {react};`)
or, in an `EvalSession`, QuickJS runner or REPL, from a prelude such as
`(set! js/lightCljsPackages (js-obj "react" react-object))`. The REPL accepts the
same libspecs in `require` and `ns`; there a later binding of the same name
replaces an earlier package or namespace alias.

A string libspec without `:as`, `:refer` or `:default` binds nothing. It compiles,
and the warning APIs (`--warnings`, `compile_namespace_with_warnings`) report
`package-require-no-effect`, because the host rather than the compiler loads
packages. `:as-alias` on a string is reader-only, as for symbols. Package aliases
cannot reuse a namespace alias and package refers cannot conflict. `:default` on
a namespace symbol, and package strings in `:use` or `:require-global`, are
rejected.

#### Bundler ES modules

Add `--esm` to `--compile --entry NS` (library: `compiler::compile_namespace_esm`
or `emitter::emit_esm`) to emit one ES module for Vite, esbuild, Rollup or
webpack instead of the bare expression. Without the flag the output is
unchanged.

```js
import { createElement as $react$createElement, useState as $react$useState } from "react";
import { default as $_40mui_2fmaterial_2fButton$default } from "@mui/material/Button";
import * as $lodash_2edebounce from "lodash.debounce";
export default (() => { /* program */ })();
```

The registry does not exist in this mode. Each package export that surviving
code references is a named import bound to a module-local identifier, so the
bundler can drop unused exports and minifiers can rename the bindings; the
result matches hand-written JavaScript with named imports. When an alias object
itself is used as a value (passed around, `(.-x r)`, `aget`, `exists?` or
`set!`), that package also gets a namespace import (`import * as $react from
"react"`) that the reference uses directly. Imports are collected after
application preparation, so packages and exports referenced only by removed
definitions are not imported, and a string libspec without bindings imports
nothing. Binding names are `$`, the package name, `$` and the export, where
ASCII letters and digits are kept and every other byte is written as `_xx` hex:
distinct package names always get distinct bindings, and emitter locals never
begin with `$`. Declarations follow first-reference order; ES module evaluation
order loads every package before the program runs. The entry namespace's value
is the default export.

CommonJS packages are the bundler's concern. When Vite or esbuild cannot detect
a CommonJS package's named exports statically, `import * as p from "cjs-pkg"`
may expose everything only under `default`, and named imports read `undefined`.
That is bundler behavior, not compiler behavior; bind such a package with
`:default` and read its members from that binding.

### Chunked builds

Add `--out-dir DIR` to `--compile --entry NS --source-root DIR` (library:
`PreparedApplication::emit_chunks`) to write a chunked build into `DIR`
instead of printing one module: one ES module per chunk, which
`light.lazy/loadable` sites load with `import()`, and `chunks.json`, which
describes them (docs/code-splitting.md, "Phase 2"). `--out-dir` implies
`--esm`, so packages are static imports and the startup chunk's default
export is the entry namespace's value. It creates `DIR` when missing and
writes only the files the manifest names, so other files there, those of an
earlier build included, stay as they are. It works at `none`, `prepare` and
`advanced`, with `--pseudo-names`, `--define` and `--elide-asserts`. At
`advanced` each chunk embeds the runtime units only the roots that load it
need, as a function of the bindings it takes from the chunks before it
(docs/code-splitting.md, "Phase 3", light-cljs-279), and `--size-report`
prints each chunk's units, every unit of the build once, with the root and
expression that first demanded it, which may be another chunk's; below
`advanced` startup embeds the whole runtime and `--size-report` prints the
single file's report. The source map flags are refused, since chunks have no
source maps yet. Without `--out-dir` the output stays one file in which every
loadable is resident. With `--reloadable`, `--out-dir` names the directory of
a dev build instead (Dev builds, below).

```sh
cargo run --quiet -- --compile --optimize advanced --entry app.main \
  --source-root examples/features --out-dir /tmp/light-features
node -e 'import("/tmp/light-features/app.main.js").then(m => m.default())'
# menu: Editor | Reports
# editor: 4 documents, first Introduction, ids #{:api :faq :intro :setup}
# reports: tags Draft, Guide, Reference; drafts FAQ, INTRO; 1050 words
```

The chunks are planned per namespace, and at `advanced` per definition (`src/chunks.rs`). A *root*
is the entry namespace, startup, or a namespace a `loadable` site names; the
static reach of a root is the namespaces its namespace requires,
transitively. Each namespace goes to the chunk of the set of roots that
reach it, except that every namespace startup reaches is startup's. The
other chunks hold namespaces only lazy roots reach: those of one root are
`NS.js`, and those several roots share are named by the roots' namespaces,
sorted and joined with `+` (`app.editor+app.reports.js`), or
`shared-<16 hex digits>.js` past 80 characters. Below `advanced` every
top-level form stays in its namespace's chunk, in source order. At
`advanced` a definition that removal could drop when nothing reads it (an
inert initializer: a function, a literal, a pure core call, a constructor
of a type defined earlier), and a `defprotocol` or `deftype` form, goes
instead to the chunk of the roots whose code reads it, in function bodies
too: a helper of a shared namespace that only one feature reads moves to
that feature's chunk, and one that two features read but startup does not
to a chunk of its own those two import (docs/code-splitting.md, "Items and
signatures"). The other forms stay in their namespace's chunk: effects, a
`defmulti`, a `defmethod`, a raw `deftype*`, the definition of a
`loadable`'s target, a form holding a `loadable`, a Var defined twice.
At `advanced` a chunk that would write no code is not written: one that
holds no root and no Var another chunk reads, and whose namespaces' forms
all emit nothing there, because removal dropped their definitions,
inlining consumed them, they moved toward their readers or they are
`declare`s. Its importers import what it imported instead, and startup
lists its namespaces. Two features requiring a namespace whose one
function is inlined into both import startup alone. A program that reaches
reflection ("Namespace reflection and reading": `*ns*`, `find-ns`, the
reader, `eval`, ...) keeps every form in its namespace's chunk at `advanced`
too, so the namespaces it registers are whole, and keeps every chunk.
Items keep source order within a chunk. At `advanced` each entry of the
constant table (a keyword, a quoted symbol, a constant collection, a map or
set shape, a hoisted method) is declared once, in the chunk of the roots
whose code reads it, directly or through a collection holding it: a keyword
only the editor reads is the editor's, one both features read their shared
chunk's, and one startup reads startup's. When no chunk has those roots,
the constants get a chunk of their own, named as a shared chunk is, which
holds no namespace and no definition, only constant declarations that its
readers import; a hoisted method stays in its function's chunk. Below
`advanced` startup holds the whole runtime. At `advanced` each runtime unit
goes to the chunk of the roots whose code needs it (light-cljs-279): a unit
only the editor's code reaches is the editor's, and an `install:` or
`effect:` unit follows the roots whose code holds the value it extends or
builds the values it handles, so `apply str` in one feature installs `str`'s
variadic arity when that feature loads. A chunk's runtime is a function of
the bindings its units take from the chunks before it, which it imports,
`const [rt$a] = ((b, c) => { ... return [a]; })(rt$b, rt$c);`, and returns
the bindings its code and later chunks read. A unit that assigns another
unit's binding (`lookup.trie` wraps `lookup`), or reads one a function
assigns after loading, stays in that unit's chunk, since a chunk receives
other chunks' bindings as values.
Analysis and optimization run once over the whole program, so each chunk's
code is what the single file would hold. A chunk imports statically the
chunks holding what its namespaces require and the bindings its code reads,
each of which therefore evaluates first; startup imports none. A loadable
whose target's chunk has evaluated before the site's chunk runs is resident,
as in the single file; any other imports the target's chunk when it is first
loaded. examples/features is three files at `none` and `prepare`,
`app.main.js`, `app.editor.js` and `app.reports.js`: 441,976, 1,295 and
13,101 bytes, against 455,109 for the single file. At `advanced` it is
four: `app.shared/documents`, which both features read and the menu does
not, moves to `app.editor+app.reports.js` with the keywords, strings and
collections of its value, and `app.shared/index-by` to `app.editor.js`, so
the files are 9,046, 54,195, 5,453 and 4,513 bytes, against 70,032 for the
single file: startup's runtime is what the menu needs, the shared chunk's
the collections and printer both features use, the editor's the sorted
collections and the reports' the set operations. A program without a loadable whose
target another chunk holds, every program without a loadable among them, is
one file, byte for byte its `--esm` output.

`chunks.json` names the startup file and lists the files in plan order:
startup first, and every chunk after the chunks it imports. For
examples/features at `advanced`:

```json
{
  "startup": "app.main.js",
  "chunks": [
    {"name": "app.main", "file": "app.main.js", "imports": [], "namespaces": ["app.shared", "light.lazy", "app.main"], "definitions": [], "roots": ["app.main"], "loadables": [], "bytes": 9046},
    {"name": "app.editor+app.reports", "file": "app.editor+app.reports.js", "imports": ["app.main.js"], "namespaces": [], "definitions": ["app.shared/documents"], "roots": [], "loadables": [], "bytes": 53667},
    {"name": "app.editor", "file": "app.editor.js", "imports": ["app.main.js", "app.editor+app.reports.js"], "namespaces": ["app.editor"], "definitions": ["app.shared/index-by"], "roots": ["app.editor"], "loadables": [{"namespace": "app.main", "target": "app.editor/start"}], "bytes": 5453},
    {"name": "app.reports", "file": "app.reports.js", "imports": ["app.main.js", "app.editor+app.reports.js"], "namespaces": ["clojure.string", "app.reports"], "definitions": [], "roots": ["app.reports"], "loadables": [{"namespace": "app.main", "target": "app.reports/start"}], "bytes": 4509}
  ]
}
```

- `startup`: the file to load first, whose default export is the program's
  value.
- `name`: the chunk's name, its file name without `.js`.
- `file`: the file, which the other chunks import as `./FILE`, so the files
  stay together in one directory.
- `imports`: the files it imports statically, which evaluate before it, in
  the order of its import lines: those holding namespaces or moved
  definitions in the order the single file loads these, then those holding
  only constants.
- `namespaces`: the namespaces whose top-level forms it holds, in load order:
  the forms that stay in their namespace's chunk. Each namespace is listed
  once. At `advanced` startup also lists the namespaces of a chunk that
  was not written because it would hold no code; startup is the one chunk
  every root reaching them loads, and their forms write nothing there.
- `definitions`: the definitions it holds of namespaces another chunk holds,
  moved toward their readers at `advanced`, in program order, each by the
  first Var it defines (`app.shared/documents`); empty below `advanced`.
- `roots`: the namespaces of the roots it holds: the entry for startup, and
  for any other chunk the target namespaces whose loads load it.
- `loadables`: the `light.lazy/loadable` sites of the output whose target it
  holds, in program order, each pair once: `namespace` holds the site in its
  top-level forms, and `target` is the target as written, `app.editor/start`
  for a Var or `app.editor` for a namespace.
- `bytes`: the file's size in bytes.

The manifest describes the compiler's output for tools, such as a server
that preloads chunks. The program never reads it: the files carry no module
manager and no load markers, and a bundler that renames or regroups them
leaves the manifest behind (docs/code-splitting.md, "Open questions").
Library callers get the files as values: `emit_chunks` returns a
`ChunkedOutput` whose `files` are `ChunkFile { name, source, imports,
namespaces, definitions, roots, loadables }`, startup first (`ChunkedOutput::startup`),
and whose `manifest()` is `chunks.json`'s text; the library writes no file.
The playground's WebAssembly module offers the same as `compile_chunks`,
whose result holds the file names, their sources and the manifest.

The files are standard ES modules that load each other by relative paths,
so a browser loads them from any static server that sends `.js` as
`text/javascript`, and a bundler takes the startup file, or a page that
imports it, as its entry. `examples/features/index.html` is such a page:
placed beside the example's chunks, it shows what the program logs.
docs/code-splitting.md, "Verification", gives the commands that load it in
headless Chrome without a bundler and after a Vite build, at every level.

The loading API is the bundled namespace `light.lazy`
(`src/compiler_libraries/light/lazy.cljs`, docs/code-splitting.md, "The
loading API"):

- `(lazy/loadable app.editor/start)`, or `(lazy/loadable app.editor)` for a
  namespace, is a `light.lazy/Loadable` of the target. Its argument is a
  literal symbol, and the namespace need not be required (Namespace loading,
  above).
- `(lazy/load l)` is a promise of the target Var's current value (nil for a
  namespace) once its chunk and the chunks that chunk imports have
  evaluated. The first `load` of a chunk imports it with `import("./FILE")`;
  a resident target, or one whose chunk has loaded, resolves on a later
  microtask without a request. A failed import, or a chunk that throws
  while it evaluates, rejects with the host's error.
- `(lazy/loaded? l)` is whether the target's chunk has been loaded through a
  loadable of it, so that `@l` returns the value; a resident loadable is
  always loaded. Loading one target of a chunk loads every loadable of that
  chunk. A chunk that only another chunk's static import evaluated is not
  loaded yet (docs/code-splitting.md, "Open questions").
- `@l` is the target's current value, and throws before the first successful
  load.

What a chunked program can observe differently from the single file is
listed under [Code-splitting contract](#code-splitting-contract).

### Dev builds

Add `--reloadable --out-dir DIR` to `--compile --entry NS --source-root DIR`
(library: `dev_build::emit_dev_build`) to write a dev build into `DIR` instead
of printing one expression: a directory whose namespaces a running
application can replace one at a time (docs/dev-workflow.md, "The dev build").

```text
DIR/
  runtime.js           globalThis.__lightRuntime = (factory)(); the whole runtime, once
  client.js            the dev client: globalThis.__lightDev.define, and in a page
                         served by --watch --serve the reader of its event stream
  app.state.js         one unit per namespace, in dependency order,
  app.state.js.map       each with its source map
  ...
  manifest.json        the entry, the namespaces with their files, requires and macros, the hooks
  index.js             loads runtime.js, client.js and the units in manifest order
```

A unit's file is `NAMESPACE.js`, except that the namespaces `runtime`,
`client` and `index` get `runtime$.js`, `client$.js` and `index$.js`, since
no namespace name contains a `$`.

A unit is the namespace's forms emitted as an interactive unit, the storage
model of the session (docs/evaluation.md): every definition is
`globalThis.__lightApplicationRoots["ns/name"]`, read at each use, and nothing
is sealed in a closure. The unit is wrapped in a registration call,

```js
globalThis.__lightDev.define("app.view", ["app.macros", "app.state"], (__lightInteractiveRuntime) => { globalThis.__lightApplicationRoots ??= Object.create(null); return (() => {
  /* the namespace's registration and forms */
})()
; });
//# sourceMappingURL=app.view.js.map
```

whose second argument lists the namespace's source requires. The client's
`define` records the unit by namespace in `__lightDev.units` and runs it
against `globalThis.__lightRuntime` at once, keeping its value in
`__lightDev.values`. Evaluating a unit again replaces its namespace's
definitions for every caller, with the session's semantics: `def` values are
reinitialized and `defonce` values kept, a redefined record or type gets a new
constructor while old instances keep their fields and methods, and functions
saved in data keep their old code.

Units are emitted from the one whole-program `Program`, by namespace entry
(`Program::namespace_entries`), at `none` semantics with the whole runtime:
they never go through application preparation. Each registers its own
namespace's reader context and sets `*ns*` as it starts, as compiled output
does, so reflection works, and the first unit registers every namespace of
the build, as single-file output does before its first form runs, so a
namespace that loads early finds the later ones. The CLI analyzes a dev build in warning mode
(`--warnings`). Each unit's source map names every source relative to `DIR`
and maps the unit's text to its `.cljs` lines, columns and names, so
`node --enable-source-maps` reports a throw at the source.

`manifest.json` holds the entry namespace; the namespaces in load order, each
with its file, its source requires (`Program::namespace_requires`) and
whether it declares macros (`Program::declares_macros`), whose requirers a
change must recompile; and the functions marked `^:dev/before-load` and
`^:dev/after-load` (or their `-async` variants, `"async": true`, which take a
continuation), in definition order:

```json
{
  "entry": "app.main",
  "namespaces": [
    {"name": "app.state", "file": "app.state.js", "requires": [], "macros": false},
    {"name": "app.macros", "file": "app.macros.js", "requires": [], "macros": true},
    {"name": "app.view", "file": "app.view.js", "requires": ["app.macros", "app.state"], "macros": false},
    {"name": "app.main", "file": "app.main.js", "requires": ["app.state", "app.view"], "macros": false}
  ],
  "before-load": [
    {"name": "app.main/stop", "async": false}
  ],
  "after-load": [
    {"name": "app.main/reload", "async": false}
  ]
}
```

`index.js` loads the files in that order, as script elements after its own
in a page and with `require` in Node (CommonJS), and sets
`globalThis.__lightDevLoaded` to a promise of the entry namespace's value,
the value of the single-file output. A file that throws while it runs
rejects the promise with that error and leaves the later files unloaded; in a
page the error is the page's next `error` event before the script's `load`.
Hooks are listed by namespace in load order, and within a namespace in the
order of their (last) definitions, also when a `declare` named one earlier.
Namespaces that only lazy references
reach load after the entry, and their loadables are resident, as in a single
file.

```sh
cargo run --quiet -- --compile --entry app.main --source-root examples/dev \
  --reloadable --out-dir /tmp/light-dev
node -e 'require("/tmp/light-dev/index.js"); __lightDevLoaded.then(start => start())'
# start
# clicks: [1]
```

A dev build takes none of `--esm`, `--optimize`, `--size-report`,
`--pseudo-names`, `--define` and the source map flags, and `--reloadable`
requires `--out-dir` (which without `--reloadable` writes a chunked build,
above). The Node driver `tests/support/dev_execution.mjs`
runs a dev build or a single-file build, calls a start function, and applies
replacement units with the hook protocol (the before-load functions in
reverse order, the units, then the after-load functions); the dev builds of
the phase oracles, the size ratchet fixtures, examples/dev and
examples/features print through it what their `--optimize none` builds print
(`tests/compiler/dev_build.rs`). `--watch` writes the same build and then
rewrites a saved namespace's unit, its map and the manifest, whose hook lists
each step that succeeds finds again in the program it compiled
([Watch](evaluation.md#watch)); a rewritten unit is
`dev_build::namespace_unit` of the session's units of the namespace's forms,
the same `define` call around an interactive unit that registers the
namespace once (the first in load order every namespace of the build), makes
it current and runs each form's statements in turn, each in a scope of its
own. `--watch --serve PORT` also serves the build and pushes the units of each
step's reload set (its namespaces, which include the `^:dev/always` ones and
their compilation consumers that every step recompiles, never `^:dev/once`
ones, and with `--reload-dependents` those downstream) to the pages running
it, which `client.js` (src/dev_client.js) loads between the hooks
([Serve](evaluation.md#serve)).

### Complete collection program

[examples/collections](../examples/collections) contains `batch.data`,
`batch.ops`, `batch.main`, and `batch.macros.cljs`. `main` requires both `data`
and `ops`; `ops` also requires `data`, so the shared dependency initializes
exactly once. The macro-generated map prints `data`, followed by the `ops` and
`main` initializers, before the exported `run` function is called. The collection
runtime is initialized before all three.

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

The single expression contains one runtime prelude plus the program, with no
module imports or global installation. Two emitted units have separate runtime
brands/globals even when compiled with one reused `MacroHost`.

## Compiler macros and embedding

### Macro phase contract

Macros are compile-time bindings in `.cljs`/`.cljc`, imported alongside runtime
functions through ordinary `:require`. Unmarked helpers initialize only on actual
compile-time execution demand, including higher-order and Var paths. Both phases
select `:cljs` and use JavaScript Number operations, with separate roots and
state. Ordinary ECMAScript operations are available during expansion; external
host facilities remain explicit. A reused host starts each clean compilation with
fresh source-defined mutable roots. Macro declarations and helpers live in
`.cljs`/`.cljc`; compiler source lookup never loads `.clj` companions.

### Compiled core bootstrap

Core macro algorithms are first-party `.cljs` declarations
(`src/compiler_core_macros.cljs`, `src/compiler_sequence_macros.cljs`) compiled by
the Rust analyzer/emitter and executed in the session's engine. A fresh heap is created for
each clean compilation; only immutable emitted unit strings are reused in memory.
Pure binding/destructuring, syntax quote, declarations and namespace reflection
remain compiler services.

Same-file source `defmacro` bodies in `.cljs` and selected `:cljs` branches of
`.cljc` execute compiler-emitted functions in that session. They can call core
operations and ECMAScript intrinsics. Fixed, multiple, variadic and destructured
methods receive hidden `&form` and `&env` before user arguments; docs and
name/leading/trailing metadata are retained. A declaration returns nil and emits
no application function or initializer. Explicit Var/resolve reflection provides
declaration identity and metadata without a callable application root.

Declarations are processed in source order at top level and inside top-level
`do`, including declarations generated by macros. Runtime conditionals and
lexical/function bodies reject macro declarations with a located placement error.
A later same-kind declaration creates a new macro generation. Already analyzed
functions and macro bodies retain their expanded code from earlier generations.
Undeclared forward macro calls fail, including in untaken branches. `declare`
reserves an ordinary Var and conflicts with a later macro of the same name;
macro/value kind changes likewise require distinct names.

Lexical functions shadow unqualified macro calls; explicit namespace-qualified
macro calls preserve macro identity. Var and `resolve` inspection expose metadata;
runtime value, `apply`, root dereference/invocation and assignment uses are phase
errors with the suggestion to invoke the macro directly in source. During
expansion, `&env :light-cljs/macros` maps qualified symbols to descriptive
declaration metadata, including user `:arglists` and `:light-cljs/generation`; it
provides no callable macro or runtime Var. A clean compile refreshes source macro
state even when its `MacroHost` is reused; assertion settings, evaluator options
and expansion limits persist.

### Helper demand

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
retains source order in its own heap.

### Declaration bundles

Top-level `defprotocol`, `deftype`, `defrecord`, `extend-type` and
`extend-protocol` are demand-initialized declaration bundles, including forms in
top-level `do` and macro-generated declarations. Protocol descriptors/method Vars,
type descriptors/factories and extension registrations share one bundle state.
Method bodies expand and analyze once using the declaration-site snapshot; the
same IR supplies compile-time and application emission. Demanding any product
installs the whole bundle. Ordinary unfinished value reads report a bundle cycle.

Deferred ordinary initializers select declaration products from their source
frontier, even when a later record/type declaration has already replaced the
active products. Nested initializer demands restore the caller's view on both
success and failure. This view lasts only while an initializer executes: saved
record factories and generated assoc copies continue to read the active
constructor Var on later invocation.

Protocol invocation (including saved functions and higher-order calls), marker
membership, applicable core operations and Object member access demand relevant
source-visible extensions. Known descriptor/native selectors narrow that index.
Computed target aliases remain candidates for their resolved protocol and are
demanded only when a query needs their applicability. An unrelated protocol's
extensions do not initialize. Earlier deferred macro expansions retain their
extension frontier. Dispatch uses phase-local tables; replacement generations and
frontier changes apply owned registration patches without rerunning user
initializers.

Failure rolls back staged products and dispatch patches and stays sticky for that
bundle generation. A matching new extension declaration or clean compilation can
repair it; user/host effects already performed remain performed. Cancellation is
terminal for the compilation heap, and reused hosts allocate fresh roots/tables.
Application declarations still execute in source/dependency order in their own
heap.

### Multimethods and hierarchies

Top-level `defmulti` and `defmethod` use the same declaration-bundle policy.
`defmulti` preserves its existing value like `defonce`; docstrings, metadata,
`:default`, and atom/Var/custom dereferenceable `:hierarchy` options are supported.
Method-body and dispatch-function macros expand once at their declaration site.
Direct declaration targets narrow demand to that multimethod; aliases and computed
targets remain conservative candidates. Matching declarations install in source
order. Standalone `remove-method`, `prefer-method`, `derive`, and other mutations
remain application effects unless executed by a demanded helper or macro.

The JavaScript runtime implements `MultiFn`, `IMultiFn` and its public method
operations: `methods`, `get-method`, `prefers`, `prefer-method`, `remove-method`,
`remove-all-methods`, `dispatch-fn`, and `default-dispatch-val`. Methods receive
the original arguments, including `apply` and variadic calls. Keyword, arbitrary
equal-value, hierarchy and recursive vector dispatch support default methods and
explicit ambiguity/preference-conflict errors. Method and preference changes reset
cached dispatch; changes to the dereferenced hierarchy invalidate prior answers.
Compilation and application runtimes own their hierarchies, method tables and
caches independently. At `--optimize advanced` a program that cannot see the
tables or give a hierarchy ancestors (it reads no field holding them or the
hierarchy, calls neither `methods`, `prefers`, `prefer-method`,
`remove-method` nor `remove-all-methods`, calls neither `derive` nor
`underive`, reads `get-global-hierarchy` only as `defmulti` does and gives no
multimethod a hierarchy of its own, extends no core protocol to a runtime or
host type, implements neither `IEquiv`, `IHash` nor `IMultiFn` (nor names an
`IMultiFn` slot or a protocol mask), builds no
value with a hash of its choosing (a runtime type's public constructor, an
assigned `__hash` or `_hash`), and adds methods only in at most eight
top-level `defmethod` forms) keeps its method table and cache as arrays of
the entries their array maps would hold, which keeps no persistent map
operations, dispatches with `isa?` of a hierarchy without ancestors, calls no
`IMultiFn` dispatcher, and, unless the program can call `name` or `namespace`
on it or names the field `name`, names it by the text of its symbol, which
messages print as they print the symbol; see "Phase 2 multimethod extension
points" in [advanced-optimization.md](advanced-optimization.md).

`make-hierarchy`, `derive`, `underive`, `isa?`, `parents`, `ancestors`, and
`descendants` support global and explicit immutable hierarchies. `bases` and
`supers` expose JavaScript prototypes; there is no JVM class hierarchy. As in
upstream CLJS, preference matching uses effective direct preferences (the
recursive `prefers*` loop discards inherited matches).

`reify`, `specify!`, nested declarations and low-level `deftype*`/`defrecord*`
inside executed code use ordinary expression semantics. Arbitrary standalone
expressions, including top-level `specify!`, are application effects and are
never replayed as macro setup. This classification is syntactic declaration
admission, not effect inference. An ordinary binding shadowing a core name cannot
silently call the core implementation from a macro body.

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
actually demands their values.

Both phases select `:cljs` and the usual `:default` branch before loading
dependencies. Inactive `:clj` branches cannot add dependencies. Same-name `.clj`
companions are never consulted, and no companion self-import or macro inference
syntax is needed. Dependency namespaces are analyzed before their dependents.
Their shared semantic IR supplies compile-time demand units and application
emission. Macro and application heaps keep separate roots, mutations and dispatch
tables.

### Core macros, destructuring and assertions

Core expansion supports `let`, `fn`, `defn`, `defn-`, `defmacro`, `if-not`,
`when`, `when-not`, `when-first`, `cond`, `condp`, `and`, `or`, `->`, `->>`,
`as->`, `cond->`, `cond->>`, `some->`, `some->>`, `if-let`, `when-let`, `if-some`,
`when-some`, `loop`, `case`, `letfn`, `while`, `dotimes`, `for`, `doseq`, `assert`
and more. Binding and declaration forms such as `let`, `fn` and `defn` expand in
Rust (`compiler_binding_macros`, `compiler_destructuring`); control macros live in
`src/compiler_core_macros.cljs` and `for`/`doseq` in
`src/compiler_sequence_macros.cljs`. Destructuring evaluates each
initializer once, preserves indexed-access versus sequence traversal, and supplies
`seq-to-map-for-destructuring` for keyword argument lists, singleton maps and
trailing maps. `:or` is an eager `get` argument: missing keys use defaults, while
present nil and false are retained. Sequential bindings support nil, lazy inputs,
host arrays and custom sequence or indexed protocol receivers.

`time`, `simple-benchmark`, `locking`, `unchecked-max` and `unchecked-min`
expand as the pinned macros do (light-cljs-260; `js-debugger` and `js-comment`
are under [JavaScript interop](#javascript-interop)). `time` evaluates its
expression once, prints `"Elapsed time: <ms> msecs"` with `prn` (quoted, six
decimals, through `*print-fn*`, so compiled output needs
`enable-console-print!` or `set-print-fn!` as upstream's does) and returns the
value. It reads the clock through `system-time`: the session's
`EvalOptions::clock` when one is set, otherwise `performance.now` where the host
has it and `Date.now` where it does not (the embedded QuickJS).
`simple-benchmark` binds its bindings, runs its expression the given number of
times and calls `:print-fn` (default `println`, resolved where the macro is
used) with the bindings and expression as printed at expansion time, the count
and the elapsed whole milliseconds between two `Date` reads, returning what
that call returns. Its iteration expression is spliced twice, for the loop and
for the line, so it runs twice; options are keyword/value pairs, and a trailing
map is rejected as upstream's destructuring rejects it. Expansion-time printing
shows light-cljs macro data, so `2.0` prints as `2` and `\a` as `"a"` where
upstream's JVM printer keeps the reader spelling. `locking` is its body: the
lock expression is not evaluated, as upstream discards it. `unchecked-max` and
`unchecked-min` evaluate two operands once each, left to right, and compare
them with `>` or `<`, so NaN wins only as the second operand; more operands nest
`max` or `min`. As upstream, all of these are macros only, with no function
value.

Extended map directives `:keys!`, `:syms!`, `:strs!`, `:select`, `:all`,
`:missing`, `:excess`, and `:defaults`, and the `selector` macro, follow the
pinned ClojureScript checkout (`a5e94c98`); they are not in the 1.12.134 release.
**Dialect difference:** ordinary `:or` defaults follow the Maven release
evaluation order; the pinned checkout moves certain explicit-map defaults before
their initializer.

`fn`/`defn` pre/post maps and parameter metadata check conditions in declaration
order around one body evaluation. Postconditions bind `%` to that result; only
nil and false fail. `assert` throws a JavaScript Error with the condition text;
its optional message runs only on failure. The same lowering handles all function
arities and destructured protocol/IFn/Object methods, retaining receiver fields
and lexical captures. Postcondition bodies cannot recur across the result binding.

Assertions are enabled by default. Call `MacroHost::set_assertions(false)` before
`compile_with_macros` or `compile_namespace_with_macros` to omit assertion
conditions and messages entirely, including unresolved forms and pre/post checks.
The setting persists on a reused host and is reflected in the compile-time
runtime's dynamic `clojure.core/*assert*` Var (also read as `*assert*` and
`cljs.core/*assert*`), as upstream binds it while compiling: every heap
`MacroHost::new_evaluator` allocates starts with it, so source macros, the
functions they call while expanding and configured data readers can choose
whether to generate checks. It is a compilation setting, not an emitted runtime
toggle.
The default CLI and convenience compilation functions enable it. The CLI's
`--elide-asserts` (after `--compile`, or with `--eval`/`--repl`, where it sets
`EvalOptions::assertions`) disables it for the whole build, at every
optimization level, as upstream's `:elide-asserts` does. An omitted assertion
analyzes to `nil`, which `none` and `prepare` emit as a `null;` statement and
`advanced` removes. The flag is the only switch: `--define` names `goog-define`
Vars of the analyzed program, read after analysis, while this setting must hold
before it, so `--define cljs.core/*assert*=false` is rejected as naming no
`goog-define`.

`case` checks duplicate constants after macro conversion. JavaScript Number
treats integer and floating spellings such as `1` and `1.0` as the same constant;
that pair is rejected. Keyword-only no-match errors use the keyword's full name
without its colon (or the empty string for a non-keyword selector), matching the
upstream optimization. Grouped branches expand once. Public `case*` requires a
symbol selector, vectors of test groups and branches, and number/string or
`:const` global tests; omitting its default produces nil.

Named types resolve only their fields, parameters and namespace Vars; they do not
capture enclosing lexical bindings. Their initial method map is ordinary body
code, matching upstream. Metadata/docstrings consumed by macros remain available
during expansion; runtime function metadata is evaluated and attached when the
function is created.

Collections, keywords, quoted forms, and semantic reader constructs may be passed
as unevaluated macro data. Auto-resolved keywords and qualified syntax-quote
symbols use the shared namespace alias view. `.cljs` reader conditionals are
rejected, including inside macro calls; `.cljc` branches select `:cljs`/`:default`
before macro lowering in both phases. Syntax quote resolves current runtime
globals, refers, renames, exclusions and macro imports through the compiler's
namespace view.

### Macro hosts and limits

`compile`, `analyze`, and `compile_namespace` create fresh isolated macro hosts.
The host is initialized lazily, so primitive programs do not initialize a macro
execution heap. Use `compile_with_macros`, `analyze_with_macros`,
`compile_namespace_with_macros`, or `analyze_namespaces_with_macros` for an
explicit `MacroHost`. Runtime global storage is always fresh per compilation, even
with a reused host. Namespace compilation installs a file provider for its
explicit roots. Each clean compilation re-reads its required sources and creates
fresh compile-time roots; edits, import changes and conditional selection cannot
reuse stale IR from a previous compilation.

Embedding host functions and reader capabilities use
[`EvalOptions` and `EvalSession`](evaluation.md); source `defmacro` is the only
way to define macros.

The compiler's supported `&env` map follows the upstream CLJS analyzer:

| Key | Supported value |
| --- | --- |
| `:locals` | Symbol-to-binding map for visible lexical names. Each record contains `:name`, `:form`, `:op :binding`, `:local :let/:arg/:fn`, and stable per-program `:light-cljs/id`. |
| Binding details | Let/argument records have `:binding-form? true`; arguments have zero-based `:arg-id`; repeated shadowing retains each previous record under `:shadow`. Fn self bindings omit `:binding-form?`. |
| `:context` | `:expr`, `:statement`, or `:return`. Fn bodies and value-position let bodies return; a multi-form value-position `do` also has a returning tail. |
| `:ns` | `:name` caller symbol and `:doc`; `:requires` maps ordinary required namespaces and aliases to their namespace identities; `:uses` maps referred and renamed macro names to their declaring namespaces. `:require-macros` and `:use-macros` are empty compatibility entries, not accepted import syntax. |
| `:line`, `:column` | One-based call-site location; `&form` also retains original metadata and compiler source provenance. |
| `:light-cljs/unchecked-arrays` | Boolean compiler setting for the calling source file. |

This is a bounded environment contract, not the entire upstream analyzer AST:
`:defs`, inferred types, compiler internals, `:info`, binding source locations and
other keys are absent. Because this compiler emits an expression with a returning
final form, standalone/entry tails have `:return` context rather than CLJS's usual
top-level statement context.

Default `MacroLimits` allow 1,024 expansions and 100,000 generated form nodes per
compilation, with 64 nested macro expansions (also the hard cap). Generated
runtime analysis has a 128-frame nesting bound. `evaluator_options` configures
engine memory, stack, timeout and polling bounds. V8 starts with a collection
threshold of at most 256 MiB and allows live heaps to grow toward the configured
budget. Its cooperative storage budget combines JavaScript heap and buffer
backing stores, including resizable/shared buffers and Wasm memory; it is not a
process-RSS ceiling. V8 sessions require at least 16 MiB for startup. Failures discard the
JavaScript bootstrap session and produce no partial application output. A caught
host cancellation or terminal limit still aborts the compilation. Later clean
compilations allocate fresh roots, heaps and dispatch tables while retaining host
configuration.

### Compile-time services: light.build

The bundled namespace `light.build` (`src/compiler_libraries/light/build.cljs`)
gives macros, the helper code they run at compile time and custom data readers
explicit inputs that a compiler cache can key on
([compiler-cache.md](compiler-cache.md), "Explicit dependency APIs"):

```clojure
(ns app.macros (:require [light.build :as b]))
(defmacro inline [path] (b/resource path))          ; text, or an error when absent
(defmacro optional [path] (or (b/resource? path) "")) ; text, or nil when absent
(defmacro schema []
  (b/depend! {:resource "app/schema.edn"})            ; a dependency, not read
  (b/depend! {:namespace 'app.model})
  (b/depend! {:provider :git/head :fingerprint "0abc12"})
  nil)
(defmacro flavor [] (b/config :app/flavor))          ; --config app/flavor=:dev, or nil
(defmacro test-namespaces []
  (b/depend-on! :namespaces)                          ; a coarse broad view
  (list 'quote (map ns-name (all-ns))))
```

- **Paths resolve like namespaces.** A logical path such as `app/data.txt` is
  looked up in every source root, and a path present in two roots is an error,
  for `resource?` too. A path starting with `./` or `../` resolves against the
  directory of the file of the namespace whose macro expansion is running (the
  expansion site, as Shadow's `shadow.resource/inline` does): namespace
  `my-app.core` in `src/my_app/core.cljs` resolves `./x.txt` to
  `my_app/x.txt`, which is then searched in every root. A file the provider
  cannot place under the roots (REPL input, an entry file outside them)
  resolves by the namespace's name, to the same `my_app/x.txt`. In a REPL or
  watch session the site is the session's current namespace and the file of
  the form. A data reader resolves it against the namespace being read. Code
  that runs outside any expansion, such as a helper `def`'s initializer that
  a macro's first use triggers, has no site, and a relative path there is an
  error. Absolute paths, empty segments, backslashes and paths that leave the
  roots are errors, and so is an argument with a NUL or a lone surrogate.
- **Only at compile time.** In emitted application code every call throws
  `light.build/... is only available at compile time`. A helper namespace that
  the application also loads can test `(exists? js/lightCljsBuild)`.
- **Build configuration.** `(b/config :app/flavor)` returns the value the
  build gave the key with `--config app/flavor=:dev` (repeatable, with
  `--compile --entry`, `--reloadable` and `--watch`), or nil when the key is
  unset. KEY is a keyword written without its colon. EDN is read with the
  compiler's reader and must be keywords, decimal integer and floating-point
  numbers, strings, booleans, nil and lists, vectors, maps and sets of them;
  a symbol, a tagged literal, `1N`, `1M` or a radix or ratio number, a map
  key or set element repeated by value, a malformed value or a key given
  twice is a CLI error. The value is kept as canonical text (numbers as the
  JavaScript number they read as, strings re-escaped, elements single-spaced,
  comments and discards dropped) and read back as data at each call. Embedders pass the same map as a
  `BuildConfiguration` to `MacroHost::set_build_configuration` and
  `EvalOptions::configuration`; `BuildConfiguration::insert` takes a key and
  EDN text, `insert_text` a `KEY=EDN` text.
- **Broad views.** `(b/depend-on! view)` declares a coarse dependency for
  reflective macros such as test discovery, and returns nil. `:namespaces` is
  the build's namespace inventory, every namespace's name and resolved source
  identity; `:declarations` is the declaration views of every namespace
  earlier in source order; `:config` is the build configuration's
  fingerprint. They are deliberately coarse: a finer view, such as the
  declarations of the namespaces a macro actually lists, is a later
  optimization. Any other view is an error.
- **Data readers.** A handler registered with
  `MacroHost::register_compiler_data_reader` calls the same services through
  the host object `js/lightCljsBuild`: `.resource`, `.optionalResource`,
  `.dependResource`, `.dependNamespace` and `.dependProvider` (token and
  fingerprint strings), `.config` (a key's name without its colon, answering
  its canonical EDN text or null) and `.dependOn` (`"namespaces"`,
  `"declarations"` or `"config"`).
- **Embedding.** Resources come from `SourceProvider::resources`, a
  `ResourceProvider` that answers a logical path and appends each location it
  tried. `FileSourceProvider` serves the source roots with each file's
  canonical path as its identity. `ResourceProvider::directory` places a
  source file the provider named under the roots, for `./` paths; the
  default places none. A provider without resources makes every read an
  error (and the reading namespace non-cacheable, since a macro can catch
  it), and one whose `stable` is false (the default) or that answers
  without an identity makes the reading namespaces non-cacheable
  (`cache_policy::non_cacheable_inputs`).

Reading and recording are one operation. With a dependency manifest
(`analyze_namespace_entry_with_dependencies`), each read or declaration is an
`ExternalInput` in `DependencyManifest::inputs`: a resource's logical path,
every candidate location tried, present or absent, and, when found, its
identity, byte length and the SHA-256 of the exact text returned; a
namespace, which also gets an observed edge when that namespace loaded earlier; a provider token
with its fingerprint; or a broad view. Every namespace also depends on
`ExternalInput::Configuration`, the fingerprint of the whole build
configuration (`BuildConfiguration::fingerprint`, the SHA-256 of its canonical
text, `{}` when empty), whether or not it reads a key: changing any key,
read or not, changes every namespace's inputs. This first implementation
over-invalidates on purpose; recording the keys each namespace reads is a
later optimization. An input is recorded on the namespace being analyzed.
When a deferred helper unit is being analyzed or initialized, or a data reader
runs, it is also recorded on that unit's (or the read form's) namespace, and
then reaches every namespace whose dependency closure contains it. A helper
value computed during one namespace's compilation and reused in memory by a
later one is therefore a dependency of both. The records live in the
compilation's log, not in the session's unit state, so releasing a unit's
analysis does not lose them.

**Macro-author responsibilities.** The cache contract assumes that an expansion
depends only on its forms, the namespaces it uses and these recorded inputs.
Read files through `resource` or `resource?`, not through host access, and
declare anything else a macro consults with `depend!`: a provider token's
fingerprint must change whenever the provider's answer would. A macro that
needs clock or random values, other undeclared host state, or an effect on
every build must opt out of the cache (`^:light-cljs/no-cache` on the
namespaces that expand it, or a cache blocker on its namespace), as
[compiler-cache.md](compiler-cache.md), "Macro-author responsibilities",
explains.

For example, a timestamp macro that reads the clock keeps its first
expansion on every cache hit, so the build time it reports goes stale:

```clojure
(defmacro built-at [] (.toISOString (js/Date.)))     ; undeclared clock: stale on a hit
```

Pass the build time in as configuration instead, so that it is a keyed input
and a new value recompiles what uses it:

```clojure
(ns app.build-info (:require [light.build :as b]))
(defmacro built-at []
  (or (b/config :app/built-at)
      (throw (js/Error. "pass --config app/built-at='\"2026-10-18T12:00:00Z\"'"))))
```

```sh
light-cljs --compile --entry app.main --source-root src \
  --config "app/built-at=\"$(date -u +%Y-%m-%dT%H:%M:%SZ)\""
```

Every namespace depends on the whole configuration, so with the compiler
cache a new time recompiles the whole build, as an uncached build would, and
only a build that passes the same time can reuse its artifacts.

### Generated names and the compiler cache

Syntax-quote `x#`, `#()` parameters, `binding` and destructuring locals and
`gensym` give fresh names, which a macro may rely on being distinct, never on
their numbers. With the compiler cache ([compiler-cache.md](compiler-cache.md),
"Generated names"), a namespace installed from its artifact keeps the names its
capturing compilation allocated, including gensyms that escape into constants,
and every fresh allocation of the compilation is reserved above them. Numeric
suffixes therefore differ between a cached and an uncached build, and between
builds that hit different namespaces. A macro whose expansion needs the
allocation sequence itself to repeat, for example one that derives a persisted
identifier from a `gensym` suffix, must opt out of the cache: mark each
namespace that expands it `^:light-cljs/no-cache`, or make the macro's
namespace a cache blocker, so that it and every compilation consumer of it
recompile.

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
A referred global's `Class/member`, like a built-in class's (`String/fromCharCode`),
is the host path `js/Class.member`, as upstream resolves it, so a call such as
`(Promise/resolve x)` keeps the class as `this`; reading the member first, as in
`((.-resolve Promise) x)`, calls it without a receiver, as upstream does, and so
does a function called with the method as an argument, also where `advanced`
inlines that function.
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
`.call(null, ...)` behavior. Qualified `Type/.method` values call the prototype
method with their first argument as receiver; `Type/new` uses `Reflect.construct`.

`js*` accepts literal JavaScript with positional placeholders and expressions, or
named placeholders resolved in the current lexical/namespace context. Analysis
checks the template type, closed placeholders and exact positional arity before
emission. Generated identifiers are hygienic against the literal segments.
A local passed directly through a placeholder has a lexical, assignable binding.
When a template uses `var` to destructure into an existing local, the compiler's
JavaScript parser converts that declaration to assignment, retaining declarations
for any raw names in the same pattern. Nested-function declarations still shadow
normally, and loop closures retain their own iteration's binding. Templates the
parser cannot recognize are left unchanged. Statement templates emit in statement
context; value and return contexts retain their actual function scope.
`this-as`, `js-arguments`, function metadata and
`apply` preserve upstream function-context behavior, including non-strict
parameter/arguments aliasing.

Host helpers include `array`, `aget`, `aset`, `alength`, `aclone`, `make-array`,
`into-array`, `to-array`, `amap`, `areduce`, `unchecked-get`, `unchecked-set`,
`js-obj`, `js-keys`, `js-delete`, `js-in` and `js-invoke`. Saved array/object
function values and `apply` use the same host operations; direct indexing keeps
getter effects between computed indices. `js-obj` retains its macro's
computed-key/literal/symbol grouping and duplicate-key behavior. `#js` keeps array
holes and ordinary object-literal prototype-key behavior.

`js-fn?` evaluates its single operand once and checks whether its JavaScript type
is `function`. Metadata-wrapped callable values remain `fn?` and `ifn?` without
becoming `js-fn?`.

`js-debugger` emits a `debugger;` statement and `js-comment` a `/** ... */`
block, one ` * ` line per line of its string argument, with three leading
spaces dropped from later lines and trailing empty lines dropped, as upstream
(light-cljs-260). Both are nil. They are raw `js*` text, which every
optimization level keeps verbatim, so both stay in `advanced` output too.
Upstream's `js-comment` is a bare template, which is not JavaScript in a value
position such as a `let` initializer; here it is nil there as well.

`to-array-2d` converts ragged seqable rows to ordinary JavaScript arrays.
`int-array`, `long-array`, `double-array`, and `object-array` support size/seq and
size/initializer arities, without coercing elements. A numeric size alone fills
slots with nil. A seq initializer copies at most the requested count and leaves
unfilled slots as holes; other initializers repeat the original value.

Core `subs` uses JavaScript substring coercion, swapped/clamped bounds and UTF-16
indices. Parsing functions require primitive strings and throw on other types.
`parse-long` accepts complete signed decimal digits within the safe-integer range.
`parse-double` accepts upstream's decimal/exponent/suffix/ASCII whitespace grammar,
NaN and infinities; both preserve parsed negative zero. Invalid strings return nil.
`parse-uuid` validates the complete 36-character hexadecimal UUID spelling before
constructing the lowercase UUID; `parse-boolean` accepts exactly true/false strings.
`rand` supports zero/one arguments; `rand-int` floors Math.random times its bound,
and `rand-nth` indexes the collection using that result. No identical PRNG stream
across engines is promised. `js-symbol?` recognizes primitive and boxed ECMAScript
Symbols; `special-symbol?` checks upstream's special-form symbol set. `clone` uses
ICloneable, and aliases retain their own source Var metadata even when runtime
operations are shared.

Generated type fields use munged host properties; writes remain visible to method
field reads and record lookup, with independent fields on metadata copies.

All helpers use browser JavaScript APIs. Npm/package resolution and bundling
remain outside the compiler; [JavaScript packages](#javascript-packages) are
registry lookups or, with `--esm`, static imports for a bundler. Literal `nil` dot receivers are rejected during
analysis as upstream does; receiver expressions returning nil fail at runtime.

## Reify objects

`reify` uses the compiler's protocol/type machinery, with one anonymous
constructor per source site and a new instance for every evaluation. Visible
locals become receiver fields, including shadowed names resolved to the visible
binding. Methods read their own receiver's fields; nested closures retain that
receiver, and nested reify objects capture a fresh environment.

Methods support destructured parameters, separate fixed overloads, Object
methods (including large and variadic signatures), nested functions and loops,
method recurrence and arbitrary thrown values. Protocol markers and partial
implementations retain ordinary dispatch behavior. `IFn` supports saved calls
and `apply`; `IHash` and `IEquiv` implementations participate in hashing, equality,
maps and sets. `get-in` preserves the two- versus three-argument `ILookup`
dispatch.

Reify always supplies `IMeta` and `IWithMeta`. Source-form metadata initializes
the instance directly, even with an explicit `IWithMeta` override. `with-meta`
normally returns a fresh identity with the same anonymous type and methods,
copies current captured field values, and shares referenced values such as
atoms. Explicit metadata protocol methods may replace those defaults.

Upstream's Object dispatcher loses the instance receiver when selecting a
variadic overload beside fixed overloads; the generated implementation reproduces
that behavior. A single variadic Object method retains its receiver. Grouped
signatures remain available to `extend-type`; reify overloads use separate method
forms, matching its upstream macro grammar.

## User-defined types

`deftype` creates a fresh constructor on each evaluation, a positional `->Type`
factory, `getBasis`, and factory Var documentation. Fields initialize in argument
order. Mutable fields accept `:mutable`, `:unsynchronized-mutable` and
`:volatile-mutable` metadata, with ClojureScript truthiness. `set!` on immutable
fields and local parameters is rejected at the source location; explicit host
property writes follow JavaScript interop. Parameters and lexical bindings can
shadow fields. Field reads and writes inside returned functions remain attached
to the method's actual receiver and observe later mutation.

As upstream's constructor does, an instance holds each field as an own data
property under its munged name (a record's `__meta`, `__extmap` and `__hash`
slots follow), then its two protocol masks. The emitter writes the assignments
once per type and the positional factory passes its arguments on, so a
construction munges no name and writes no `WeakMap`: what makes the instance a
type instance, and a record's type, is a class private field set by
`TypeBrand` in `src/js_runtime.js`, which copying fields or inheriting the
prototype cannot forge. The `typeFields` and `recordTypes` stores read it, and
`maps` and `mapKinds` answer for a record from it until an update gives the
record entries of its own (light-cljs-258). Below `Advanced` the runtime's
`makeType` makes the constructor, one function for every type, around the
emitter's assignments. At `Advanced` the emitter writes each type's
constructor as a function of its own, `function Type(a0, a1) { this.x = a0;
this.y = a1; new TypeBrand(this); }`, which `defineType` decorates with the
methods, and the masks are properties of the type's prototype instead of each
instance (light-cljs-264): V8 then keeps every type's construction
monomorphic, where the shared constructor's inline caches go megamorphic as a
program adds types, and a type costs about its own text (benchmarks/compiler.md,
"Per-type output"). Where the output keeps no runtime unit that reads a brand
and reads no type other than in a `new`, that constructor writes none
and the output has no `TypeBrand` (light-cljs-262). An instance takes the brand
once, so calling a constructor as a plain function on an existing instance
(`(.call T t 2)`), which upstream and earlier light-cljs allowed, throws a
`TypeError`. A field named `__proto__` is an own field like any other. A
saved positional `deftype` factory passes all its arguments to the type's
current definition, and a record factory its declared fields. An instance
calls through its type's current `IFn` table, so an extension of `IFn`, even
one replacing the table, reaches existing instances.

Methods support protocols, Object members, destructuring, overloads and recurrence.
`IFn`, `ILookup`, `ISeqable`, `IReduce` and `IKVReduce` participate in calls,
`apply`, keyword lookup, `get`, sequence traversal, `reduce`, `into` and
`reduce-kv`. Collection protocols also dispatch ordinary collection operations.
Native method tables retain precedence over owned collection storage.

The low-level `deftype*` accepts fields, protocol-mask data and an optional body.
Macro-style annotated extension methods retain field receivers. Unannotated
low-level field methods reproduce upstream's missing-receiver failure.

Types use resolved namespace identities. Alias-qualified constructors and protocol
methods do not collide with same-named types in other namespaces. Redefinition
creates a new constructor and prototype; old instances keep their old methods,
and saved positional factories read the current type binding. Named types do not
capture enclosing lexical locals. Method body maps are evaluated normally, rather
than interpreted as function pre/post conditions.

## Records

`defrecord` and `defrecord*` support nominal data, positional and map
construction, declared fields, extension entries, metadata, cloning and user/core
protocol methods. Assoc and extension-only dissoc preserve a record; removing a
declared field produces a plain map with its metadata. Record equality
distinguishes plain maps and other constructors, and equal records work as
structural map/set keys. Sequence and find return map entries; empty returns nil.
Records are callable only when they implement IFn. External extensions affect
existing instances.

Saved factories and generated copy/update methods follow the current constructor
Var after redefinition, while old instances retain their original methods and type.
Named record methods use namespace Vars for enclosing names and retain receiver
fields in escaped closures. Raw `defrecord*` exposes internal fields and installs
only the methods supplied by its body. `pr-str` prints tagged record data; default
`str` follows JavaScript object stringification, as upstream does.

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
follows direct methods. Replacing a method during a call preserves that selected
invocation and changes subsequent dispatch. Native replacement replaces that
method's overload function; prototype replacement preserves other arity slots.
Extension method `recur` retains the full explicit parameter count and rebinds
the receiver to JavaScript `this` on each iteration.

Core predicates observe native membership. Ordinary core operations preserve the
upstream nil/string/array fast paths: for example, a native `-count` extension can
change `(-count "ab")` while `(count "ab")` remains 2. Raw protocol methods and
native object `seq`, `get`, `reduce`, `contains?`, `empty`, and printing dispatch
through extensions. Grouped IFn extension signatures follow the upstream macro
shape; ungrouped forms are rejected. Primitive `specify!` property writes have the
upstream silent no-op behavior, while nil and unsupported clone operations fail.

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
uses a custom count protocol before sequence traversal.

`into` accepts zero, one, two or three arguments. The transforming arity accepts
a transducer, stops on `Reduced`, invokes completion once, and preserves
destination metadata.

Additional helpers include `some-fn`, `every-pred`, `memoize`, `group-by`,
`frequencies`, `update-keys`, `update-vals`, `reductions`, `partitionv`,
`partitionv-all`, `splitv-at`, `iteration`, tagged literals, printing, and
`clj->js`/`js->clj`.

## Transient collections

`transient`, `persistent!`, `conj!`, `assoc!`, `dissoc!`, `disj!`, and `pop!`
are first-class functions for vectors, maps, and sets. Their
IEditableCollection/ITransientCollection/ITransientAssociative/ITransientMap/
ITransientVector/ITransientSet methods also dispatch to custom implementations.
Vectors provide indexed access; all three provide lookup, count, and IFn calls.
Transients own private mutable copies of persistent storage. Persistence drops
metadata, freezes the result, and prevents subsequent mutation or counting.
A transient vector, as upstream's TransientVector, copies only the tail when it
is made and the trie nodes it first changes, so `transient`, `conj!`, `assoc!`,
`pop!` and `persistent!` are O(1) amortized or O(log32 n), and `vec`, `into`,
`filterv` and `mapv` of one collection (a reduction, as upstream) build an
n-item vector in O(n). Where no protocol implementation exists, the `conj!` of
`into` (with or without a transducer), `mapv` and `filterv` onto their own
transient skips the protocol dispatch, and so does the `assoc!` that a transient
map's `conj!` of a map entry or pair makes, as upstream's TransientHashMap calls
its own `assoc!`. A transient hash set edits its set's trie as a transient hash
map does (see [Hash sets](#hash-sets)), so `conj!` and `disj!` are O(log32 n).
As upstream, lookup is allowed on persisted hash-map transients; array-map
transients and vectors reject it. Out-of-range three-argument vector nth returns
its fallback even after persistence. Transient sets return the query key during
lookup, while persisted sets retain the original representative. Array maps can
promote to a new transient hash map; callers must retain the operation's returned
value. Lists, subvecs, sorted maps, and records require an explicit editable
protocol implementation.

## Lazy sequence operations and comprehensions

`range`, `repeat`/`repeatedly`, `iterate`/`cycle`, `map`, `map-indexed`, `mapcat`,
`filter`/`remove`, `keep`/`keep-indexed`, take/drop and while variants, `concat`,
`partition`/`partition-all`, `dorun`/`doall`, `tree-seq`, `flatten`, and ordinary
sequence traversal helpers are callable core values. Their collection-taking
public arities include multiple input sources, finite prefixes of infinite
producers, and empty inputs. `map` stops at the shortest source. `mapcat` retains
upstream's bounded argument probing at construction.

Integer and floating ranges and larger vector sequences expose chunks. Unary
map/filter/keep families and the innermost `for` generator preserve those chunks,
including predicate/body effect counts, rather than uniformly forcing one element.
Unchunked inputs remain incremental. Repeated traversal shares realized lazy
contents; `iterate` reduction follows the producer directly. Reduced results and
exceptions do not request later inputs; effects already required by a source
chunk remain observable.

`for` remains lazy. `doseq` performs effects in order and returns nil. Both support
nested generators, sequence/map destructuring, `:let`, `:when`, `:while`, and
escaped per-iteration closures.

### Sorted collections

`sorted-map`, `sorted-map-by`, `sorted-set`, and `sorted-set-by` use the shared
runtime in application and macro execution. Comparators accept either numeric
comparison results or ordering predicates. Sorted membership uses a zero
comparator result; retained representatives keep their structural equality and
hashes. Numeric keys remain JavaScript Numbers, including rounding at 2^53 and
zero/-zero equivalence. Ordinary equal sorted/hash collections have equal hashes.
Comparators inconsistent with structural equality retain upstream's distinct
membership and structural-hash roles.

`assoc`, `dissoc`, `conj`, `disj`, lookup/callability, map entry finding, sequence,
reduction, and reverse traversal preserve sorted order and prior values. Metadata,
clone, and `empty` retain the comparator; no-op sorted updates preserve collection
identity. Sorted maps supply nil for an unmatched trailing constructor key. Applying
`sorted-map-by` or `sorted-set-by` without a comparator returns an empty collection;
adding multiple keys later exposes the invalid comparator as an ordinary error.
Sorted collections require an explicit editable protocol extension for transient use.

`subseq` and `rsubseq` support their three- and five-argument ranges with `<`, `<=`,
`>`, and `>=`, including saved function values and custom `ISorted` dispatch.
`-sorted-seq`, `-sorted-seq-from`, `-entry-key`, and `-comparator` honor the
collection's comparator and map/set entry distinction. Empty sorted-map traversals
are nil; sorted-set protocol traversals retain the upstream lazy-sequence result.

Sorted storage uses private immutable arrays with binary comparator search:
lookup is O(log n), updates copy O(n) entries, and traversal materialization is
O(n). This is not a red/black tree. `PersistentTreeMap`, `PersistentTreeSet`, and
`PersistentTreeMapSeq` retain public type identities and constructor arities;
opaque upstream node storage is not exposed.

### Positional factories for implemented core types

The public generated factories for IndexedSeq, UUID, Range, IntegerRange,
PersistentTreeMap, PersistentTreeSet, PersistentTreeMapSeq, Eduction, MultiFn,
SeqIter, RangedIterator, Namespace, TaggedLiteral, Cons, Reduced and Symbol
construct their runtime types. Saved factories and apply use the same path.
Supplied public metadata, hashes, counts, chunk caches, iterator state and backing
fields are retained. These values can be intermediate macro computations;
arbitrary types, functions and iterator objects receive unsupported
expansion-result diagnostics when returned as macro expansions.

### Public iterator helpers

`seq-iter [coll]` creates a SeqIter with lazy initialization. `ranged-iterator
[v start end]` creates a RangedIterator with explicit vector bounds and chunk
state. Both support saved and apply invocation in applications and intermediate
macro computations. As upstream, an exhausted SeqIter `next` throws, while
RangedIterator `next` is unchecked.

### Primitive Array-like sequences

`prim-seq` accepts one primitive JavaScript Array-like value and an optional
start offset. Direct, qualified, saved and applied calls use the same IndexedSeq
in applications and macro computations. Empty and exhausted inputs return nil;
strings traverse UTF-16 code units, and sequences retain their backing input and
public IndexedSeq metadata, indexing, reduction and iterator behavior. Negative,
fractional and nil offsets follow upstream JavaScript comparison and indexing
rules.

### Printing and string writers

Core printing supports `*print-length*`, `*print-level*`, `*print-meta*`,
`*print-readably*`, configurable output/error sinks and nested capture through
`with-out-str`. Public `pr-str*`, `StringBufferWriter`, and `->StringBufferWriter`
execute in applications and macro computations; `pr-str*` calls the printing
protocol with nil options.

## Compiler warnings

Prefix a compiler command with `--warnings` to emit located warnings on stderr,
for example `light-cljs --warnings --compile source.cljs`. This mode permits
undeclared Vars/namespaces and diagnosed function overloads to proceed through
analysis; ordinary commands retain their strict unresolved-symbol and
invalid-overload errors. `--checked-arrays-warn` also enables receiver/index
diagnostics and runtime validation for direct `aget` and `aset` calls. Invalid
runtime accesses report through `*print-err-fn*` and continue.
`--checked-arrays-error` throws instead. These flags work with namespace
compilation through `--entry` and `--source-root`.

Embedding callers can use `analyzer::analyze_with_warnings`,
`analyzer::analyze_with_macro_warnings`, or
`compiler::compile_namespace_with_warnings` with `analyzer::AnalysisOptions`
and a mutable `Vec<analyzer::AnalysisWarning>`. `checked_arrays: true` retains
static operand diagnostics; `array_checks: analyzer::CheckedArrays::Warn` or
`Error` additionally selects runtime validation. Each diagnostic contains a kind,
message, source index, and byte span. Diagnostics already collected remain
available when a later error stops compilation. Warning collection includes
undeclared Vars, private Var access (Var literals bypass that warning),
JavaScript local shadowing, declared/defined argument lists, function and record
arities, non-dynamic earmuffed names, arithmetic operand types, checked array
access, single-segment namespaces, the reserved `js` namespace alias, and
JavaScript package libspecs that bind nothing (`package-require-no-effect`, also
collected without `warning_mode`). Type
diagnostics use bounded inference over analyzed expressions.

The public `*unchecked-arrays*` Var has a false runtime root. In compiled files,
unqualified `(set! *unchecked-arrays* true)` and `false` change compiler state
and emit nil; computed values neither change that setting nor emit runtime
effects. This matches upstream's non-REPL intrinsic. The setting suppresses
checked array diagnostics and runtime checks until reset. Each loaded source file
and fresh compilation starts false; deferred definitions capture their source
setting. Qualified Var assignment remains an ordinary runtime assignment. Source
macros can inspect `(:light-cljs/unchecked-arrays &env)`.

## Testing

```sh
cargo nextest run                            # all fast Rust + Node tests, in parallel
cargo test                                   # the same through the standard runner
cargo nextest run --test execution           # shared tiny-program execution fixtures
cargo clippy --all-targets -- -D warnings
node tests/support/js_runtime.test.js        # direct runtime assertions
node tests/support/js_core_helpers.test.js
```

Compiler execution tests run emitted JavaScript in Node.js; it must be on `PATH`,
or `NODE` must name its executable. Evaluator tests (`--test evaluation`) need
only Rust, since they run QuickJS in-process. README "Testing" lists the test
binaries and how to run one suite or test.

`tests/fixtures/execution.tsv` contains shared tiny programs and their expected
JSON values/effect traces. The harness wraps each program in `(fn* [observe] ...)`
and supplies a host function that records its argument and returns it.
`tests/fixtures/execution-outcomes.tsv` adds runtime failures, stdout and host
boundary values; `tests/fixtures/execution-literals.tsv` carries byte-exact UTF-8
sources as hex (CR/CRLF, NULs, other controls). Each case produces one
`{id, outcome, trace, stdout}` record: an outcome either returns an encoded value
or throws a stable error category. Nil/null, `undefined`, negative zero, NaN, and
both infinities stay distinct. Only complete encoded map/set entries are sorted;
ordinary sequence and trace order is preserved. The bounded generated programs
in `tests/support/generated.rs` can be replayed with `LIGHT_CLJS_GENERATED_SEED`
or `LIGHT_CLJS_GENERATED_REPLAY_TSV`, and `LIGHT_CLJS_GENERATED_COUNT=n` runs
`n` seeds instead of the fixed twelve. Its namespace generator feeds
`tests/compiler/generated_optimization.rs`, which compiles multi-namespace
programs with tagged dead code at every `--optimize` level (see "Generated
programs" in [advanced-optimization.md](advanced-optimization.md)).

### Differential comparison with upstream ClojureScript

The `#[ignore]`d tests in `tests/execution.rs` compile the same fixtures with
upstream ClojureScript via `tests/reference/execution.clj` and compare returned
values or failures, effects and stdout with light-cljs. They need Clojure CLI,
Java and Node, and download the Maven ClojureScript **1.12.134** release on first
use. Options are pinned to `:optimizations :none` and `:static-fns false`.

```sh
# Maven 1.12.134 release (the default selection)
LIGHT_CLJS_REFERENCE=release cargo test --test execution -- --ignored --nocapture

# Release and an exact source checkout; reports release/checkout discrepancies first.
LIGHT_CLJS_REFERENCE=both \
LIGHT_CLJS_REFERENCE_CHECKOUT="$PWD/research/clojurescript" \
LIGHT_CLJS_REFERENCE_REVISION=a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e \
cargo test --test execution -- --ignored --nocapture --test-threads=1
```

`LIGHT_CLJS_REFERENCE` selects `release` (default), `checkout`, or `both`.
Checkout selections require a clean repository at the given full commit SHA; the
harness uses its `deps.edn` via `:local/root` and verifies that compiler resources
came from that checkout. `LIGHT_CLJS_REFERENCE_TIMEOUT_SECONDS` overrides the
per-build timeout. Missing tools, failed builds, timeouts and mismatches fail the
test; there is no passing skip. With `LIGHT_CLJS_KEEP_TMP=1` the printed temporary
directory is kept and holds the generated source, `invocation.sh`, build
stdout/stderr and (when execution was reached) `driver.js` and `results.txt`;
otherwise it is removed when the test ends, failed or not. JavaScript text is never
compared; only observable behavior is.

### Reader corpora

The lexer and parser corpus tests read the gitignored upstream checkout at
`research/clojurescript`:

```sh
cargo test --release --test frontend corpus::clojurescript_checkout_has_no_lexical_errors -- --ignored --nocapture
cargo test --release --test frontend parser_corpus::parse_clojurescript_checkout -- --ignored --nocapture
```
