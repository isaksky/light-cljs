# light-cljs

A small Rust compiler for an executable ClojureScript core, with a borrowing
lexer, handwritten syntax parser, semantic analyzer, and JavaScript emitter.
The parser pulls tokens into a node arena; analysis resolves local references
to binding IDs in a separate expression arena. Generated JavaScript includes
immutable vectors, lists, maps, sets, named values and a first-party core runtime
when needed. Synchronous evaluation uses pinned `rquickjs` 0.14.0; its small
allocator boundary delegates pointer ownership to the upstream Rust allocator.
Tokens never allocate text; syntax leaves keep byte spans wherever possible.

## Adopted evaluator architecture

The project uses **rquickjs execution of JavaScript emitted by this Rust
compiler**, including macro bodies and ordinary helpers. Rust keeps reading,
resolution, analysis, semantic IR, optimization and emission. Macro and application
execution share canonical runtime operations and JavaScript Number semantics,
with separate mutable state and demand-initialized compile-time helper roots.
Macros live in `.cljs`/`.cljc` and use ordinary `:require`.

The [synchronous evaluator](docs/evaluator.md) now executes compiler units with
one canonical runtime per session, bounded host controls and owned value IDs.
**Production consumers use compiled JavaScript.** Core compiler macros run as
compiler-emitted JavaScript in QuickJS. Ordinary helper initializers can be
compiled on macro demand, including nested helper/macro compilation; definition
snapshots and shared semantic IR prevent repeated source expansion. See the
[reentrant evaluator contract](docs/evaluator.md#deferred-compiler-units). Required namespaces
share this compiler path for macros and ordinary helpers. Scripts and the REPL use the same compiler-backed `EvalSession` path. The supported embedding
API is now `EvalSession`, which compiles forms incrementally and executes canonical
JavaScript in an explicitly owned QuickJS heap. See [session embedding](docs/evaluation.md). The existing commands
and API examples below describe that current checkout. See the
[evaluator migration design](docs/evaluator-migration.md) for the accepted phase
semantics, interpreter extraction, consumer migration and completion criteria.
Pellets holds the implementation queue; evaluator migration and full intended
language completion are separate milestones.
The [migration execution gate](tests/reference/evaluator-migration/README.md) runs
compiler, evaluation, CLI and runtime checks. The native interpreter, temporary
adapters, native-only tests and benchmark harnesses have been physically removed.
This removal does not claim complete ClojureScript support.

## Evaluate ClojureScript and use a REPL

```sh
cargo run --quiet -- --eval --text '(/ 1 2)'
# 0.5
cargo run --quiet -- --eval --source-root examples/native examples/native/demo.cljs
# Hello from compiled JavaScript
# {:squares [1 4 9 16 25], :sum 15}
printf '(+ 20\n 22)\n(* *1 2)\n' | cargo run --quiet -- --repl
# 42
# 84
```

`--eval` accepts a UTF-8 `.cljs`/`.cljc` script, stdin/`-`, or `--text`.
Stdin and text use this CLJS dialect, including `:cljs` reader conditionals.
Program output streams immediately to stdout; the readable last value appears
only after successful evaluation and rendering. ECMAScript intrinsics such as
`js/Math.sqrt` work in programs and macros. Execution uses embedded QuickJS,
with host-owned standard streams and no Node/JVM process.

`--repl` keeps definitions, macros, namespaces and `*1`/`*2`/`*3`/`*e`, accepts
multiline forms and recovers reader/language errors. Runtime reads share its
input cursor. Terminal prompts go to stderr. Cancellation or engine limits exit
nonzero; start a fresh command to reset the session.

Use explicit `--source-root DIR` for `.cljs`/`.cljc` namespaces; `.cljs` wins,
and no root is discovered implicitly. `.clj` scripts and legacy native controls
(`--fuel`, `--allocation-limit`, `--call-depth`, `--evaluation-depth`) receive
migration errors. Use `--poll-budget`, `--memory-limit`, `--stack-limit`,
`--timeout-ms`, `--source-limit` and `--output-limit` for the documented new units.
JavaScript Number replaces exact native integer/ratio arithmetic. Native class
and extension descriptors, JVM catches, shared-owner forks and GC counters are
retired; language extension forms use canonical CLJS runtime behavior.

The [command contract](docs/evaluation.md#script-and-repl-commands) describes
IO, histories and controls. The [compiled examples](examples/native) retain the
historical directory name and cover data processing, macros, lazy pipelines,
state and exceptions. [Interpreter documentation](docs/interpreter.md) preserves
the retired implementation as historical evidence; its APIs and commands are
unavailable in this checkout.

## Execute small programs

```sh
cargo run --quiet -- --compile examples/closure.cljs \
  | node --print 'eval(require("fs").readFileSync(0, "utf8"))'
# 10

cargo run -- --compile program.cljs > program.js
cargo run -- --analyze --text '(let* [x 10] x)'
```

`--compile` emits a self-contained JavaScript expression whose value is the
last source form. Supported forms include `nil`, booleans, JavaScript numbers, strings, reader
literals, `if`, `do`, sequential `let*`, mutually recursive `letfn`/`letfn*`, and fixed/multiple/variadic `fn*`
functions and calls. Characters, regexes, `#inst`, `#uuid`, `#js`, `#queue`, `#()`,
metadata, namespaced maps, syntax quote, dereference and Var quote execute through
the compiler reader/runtime boundary. Large integer spellings use Number rounding. Closures capture lexical bindings; named `fn*`
supports self-recursion. Letfn groups keep outer captures and independent lexical
identities, including nested and escaped functions; the [letfn audit](tests/reference/compiler-letfn.md)
records two explicit upstream slot-reuse differences. Empty bodies return `nil`. Only `nil` and `false` are
false in conditionals, including when tests produce `0` or `""`.

JavaScript interop includes explicit/shorthand dot calls, `new` and constructor
shorthand, property `set!`, positional/named `js*` templates, `this-as` and
`js-arguments`. Host arrays/objects, computed access, array traversal macros,
qualified method/constructor values and `:refer-global` work in generated browser
JavaScript. Unqualified host paths such as `console.log`, qualified dot members
and host-path definitions also work. Member names use CLJS munging; string keys
keep their spelling.
See the [interop audit](tests/reference/compiler-interop.md).

Compiled definitions support docstrings/metadata, `defonce`, forward `declare`,
`var`/`#'`, quoted-symbol `resolve`, and legal global `set!`. `binding` and
`with-redefs` evaluate initializers in parallel and restore roots through nested
calls and exceptions. Their scope is synchronous; later Promise callbacks see
restored globals. Explicit boolean hints retain CLJS's JavaScript truth behavior.
Vars preserve live dereference/invocation, symbol equality/hash, metadata and
function test callbacks. See the [definitions audit](tests/reference/compiler-definitions.md).
Core macros bootstrap from first-party `.cljs` sources through this compiler and
execute in a fresh QuickJS heap. `defmacro` is a compiler declaration with hidden
`&form`/`&env` arguments; it produces nil and no application function. Standalone
macro bodies can use canonical core functions and ECMAScript intrinsics. Ordinary
helpers initialize on actual value demand, including closures, higher-order calls
and Var consumption. Var identity/metadata and untaken branches preserve pending
roots. Compile-time scopes restore pending, ready and failed roots without forcing
them; clean host reuse creates fresh mutable state. Protocol, type, record and
extension declarations initialize as bundles, with protocol-specific extension
demand, source-visible dispatch frontiers and rollback of failed installations.
Reify and specify! inside executed helpers use ordinary expression semantics.
Ordinary `:require` loads both macros and ordinary definitions from `.cljs` or
`.cljc`; both phases select `:cljs` branches and keep separate mutable state.
Compiler source lookup never loads `.clj` companions. Legacy macro-import clauses
produce located migration diagnostics pointing to ordinary `:require`.
Declarations follow source order at top level, inside top-level `do`, and in
macro-generated declarations. Same-name macro redefinitions preserve earlier
expanded code; macro/value kind conflicts and runtime use of a declared macro
are located phase errors. Local functions shadow unqualified macro calls.
See the [compiler macro contract](docs/compiler.md)
for helper demand semantics and namespace imports.

```rust
use light_cljs::{analyze, emit_js, parse};

let source = "(let* [x 10 f (fn* [] x)] (let* [x 20] (f)))";
let syntax = parse(source).unwrap();
let program = analyze(source, &syntax).unwrap();
let javascript = emit_js(&program);
// Or use light_cljs::compile(source) for all three stages.
```

Ordinary `let`, `loop`, `fn`, `defn`, and `letfn` execute nested sequential/map
destructuring, rest/as bindings, named-key directives and eager defaults in generated
JavaScript. Function pre/post conditions and assertions execute there too; method
lowering retains receiver fields and captures. The
[destructuring evidence](tests/reference/compiler-language/evidence.md#destructuring-and-conditions-member--2026-10-09)
records the explicit release/checkout order difference. Named types use the
pinned field, scope and ordinary method-body-map behavior. Extended map
directives (`:keys!`, `:syms!`, `:strs!`, `:select`, `:all`, `:missing`,
`:excess`, and `:defaults`) and `selector` follow the pinned checkout, with
explicit release comparisons in the [destructuring audit](tests/reference/compiler-destructuring-completion.md).
Conditional bindings, short-circuit/threading forms, `cond`/`condp`, grouped
`case`, `while` and `dotimes` execute in generated JavaScript, including effects,
failures and per-iteration captures. `MacroHost::set_assertions(false)` omits
assertions and function pre/post checks. The [control audit](tests/reference/compiler-control.md)
records the pinned comparisons and remaining separate helper witnesses.
User macros use ordinary `:require`, including `:as`, `:refer` and `:rename`.
A namespace can supply both compile-time macros and runtime functions; macros
can call its ordinary helpers on demand. The complete example has two runtime
namespaces and one `.cljs` macro namespace:

```sh
cargo run --quiet -- --compile --entry demo.main --source-root examples/macros | node
# 12
```

The [collection example](examples/collections) combines three runtime namespaces,
a shared dependency and a `.cljs` macro. Compile it to one expression returning
`batch.main/run`, then invoke it from JavaScript:

```sh
cargo run --locked --quiet -- --compile --entry batch.main \
  --source-root examples/collections > /tmp/light-cljs-collections.js
node --check /tmp/light-cljs-collections.js
node -e 'const run=eval(require("fs").readFileSync("/tmp/light-cljs-collections.js","utf8")); const result=run(x=>x); if(result==null) throw Error("missing result");'
# data
# ops
# main
```

`run` returns `[[1 2] [1 2 3] :pair true]`; its first observation is the
`report` result `[6 batch/data true "batch.data"]`. Values retain their language
types. The host callback also receives generated nested data and ordered effects.
Core printing and explicit JavaScript conversion helpers are available. The expression
contains one private collection runtime and needs no native interpreter, JVM,
module import or installed `cljs.core` at execution time. See the
[complete program contract](docs/compiler.md#complete-collection-program).

See [compiler macros and embedding](docs/compiler.md#compiler-macros-and-embedding)
for imports, host macros, reload, `&env`, resource limits, and runtime restrictions.

Compilation errors include source locations and produce no partial JavaScript.
Definitions and source-language JavaScript interop are also supported. Compile the
included two-namespace example with explicit source roots:

```sh
cargo run --quiet -- --compile --entry demo.main \
  --source-root examples/namespaces > /tmp/light-demo.js
node /tmp/light-demo.js
# 4
```

Namespace loading searches `.cljs` before `.cljc`, selecting shared-source
`:cljs`/`:default` reader branches before loading dependencies. It supports namespace
docstrings/metadata, aliases, runtime and macro refers/renames, and
`:refer-clojure` exclusions/renames. The same source supplies both phases;
compile-time helper demand is separate from application initialization. A virtual `cljs.core` namespace provides first-class
`keyword`, `symbol`, `vector`, `list`, `hash-map`, `hash-set`, `conj`, `assoc`,
`dissoc`, `disj`, `seq`, `first`,
`rest`, `next`, `name`, `namespace`, `count`, `nth`, `get`, `contains?`, `=`,
`reduce`, arithmetic/comparisons, numeric coercions and bit operations, fundamental
boolean/nil/numeric/named/function/collection predicates, `reduced`, `reduced?`,
`unreduced` and bounded `deref`.
Quote lowers symbols and nested lists/vectors/maps/sets directly to immutable data; quoted
names never resolve or execute. Vectors use indexed calls; keywords,
symbols, maps and sets use one/two-argument lookup calls. Lists and finite seq views remain non-callable.
Sequential equality crosses list/vector/seq kinds, preserving their typed
identity; empty lists differ from nil. Maps accept structural keys (equivalent lists,
vectors and seqs find the same entry), distinguish absent keys from stored nil,
false and undefined, and preserve old versions during updates. `assoc nil`
creates a map; an odd trailing assoc key receives nil, while odd `hash-map`
arguments fail. Map `conj` accepts vectors of at least two values (using the first
two), maps, finite sequences of vector entries and nil. Map lookup and copying
updates are O(n), plus structural key costs; iteration order is unspecified. List/seq tails share storage in O(1),
list indexing is linear, and vector updates copy the owned array.
Sets deduplicate structural members and support `get`, `contains?`, `conj`,
`disj` and finite traversal. Lookup returns the first retained member, including
nil and false; nested maps and sets can serve as members or map keys. Set updates
preserve old versions, and no-op updates preserve identity. Duplicate readable
set-literal forms reject before execution; dynamic distinct forms with equal results throw
Error as in the pinned checkout, with the release difference retained explicitly.
Set count is O(1), lookup/update is O(n), and naive construction
and unordered equality are O(n²), before nested costs. Iteration order is unspecified.
Ordinary `reduce` accepts two/three arguments over finite language collections,
seq views, nil and UTF-16 strings. It calls the reducer with no arguments for an
empty input without init, skips callbacks for a singleton without init or an
empty input with init, and never calls completion. A step returning `reduced`
stops immediately and unwraps one layer; `deref` supports these private
immutable markers and quoted Vars. Numeric functions use JavaScript Number arithmetic,
rounding and 32-bit bit operations; saved functions, aliases and `apply` share
the same runtime. The [numeric audit](tests/reference/compiler-numeric.md) records
arities, coercions, predicates, the direct comparison macro distinction, and
verification against both pinned references. Functions support overload dispatch and arbitrary variadic fixed prefixes, including
metadata wrappers, compiled macro expansion and namespace boundaries. The
[function arity audit](tests/reference/compiler-functions.md) records callable
entry points, rest sequences and pinned comparisons. All `apply` prefixes,
callable data, metadata and captured `reify` implementations are supported, along with
`identity`, `constantly`, `complement`, `comp`, `partial`, `juxt` and `fnil`.
Variadic calls can consume finite prefixes of infinite argument tails. The
[apply audit](tests/reference/compiler-apply.md) records 143 selected upstream
assertions and 228 independent probes. Compiled sequences include lazy producers,
collection-taking `map`/`mapcat`/filter/keep families, partitions, bounded traversal,
and nested `for`/`doseq` comprehensions with destructuring and modifiers. Range
and vector chunks preserve observable realization, while reduced results stop
before requesting the next input. Saved helpers and namespace aliases use the
same runtime. The [sequence audit](tests/reference/compiler-sequences.md) records
314 imported assertions, 56 independent differential probes, and the complete
arity inventory. Canonical eager transduction supports `completing`, `transduce`,
`ensure-reduced`, transforming `into`, and the standard map/filter/keep, bounded,
partition, cat/mapcat, interpose, replace, dedupe/distinct, random-sample and
halt-when constructors. The [transducer audit](tests/reference/compiler-transducers.md)
records reduction order, state, completion and pinned reference comparisons.
Transducing `sequence` supports one or multiple input collections with memoized,
chunked lazy output and completion emission in both application and macro phases.
The [lazy transduction audit](tests/reference/compiler-sequence-transducers.md)
records exact demand traces and all public arities. Repeatable `eduction` views
reapply transforms with fresh state for each reduction or traversal, including
independent iterators, buffered completion and finite prefixes of infinite sources.
The [eduction audit](tests/reference/compiler-eductions.md) records public constructor,
protocol, metadata, completion and shared macro/application evidence.

Compiled `delay`, `force`, `delay?`, `realized?` and `lazy-seq` preserve deferred
execution, retry throwing thunks, and memoize successful values. Lazy metadata
wrappers retain independent pending state; infinite prefixes and reduced
termination consume only the requested inputs. The [deferred-value audit](tests/reference/compiler-deferred.md)
records 27 upstream assertions, 23 independent reference probes, deep traversal
and GC checks.

Compiled `hash`, `hash-ordered-coll` and `hash-unordered-coll` use the public
ClojureScript Number/Murmur3 contract for canonical values. Equal collection
representations remain interchangeable keys, collisions retain the saved key,
and `INamed`/`-name`/`-namespace` dispatch to user types. Signed zeros share a
normalized zero hash; NaN remains unequal to itself. Foreign objects and regexes
use identity, and record equality remains nominal. The
[equality audit](tests/reference/compiler-equality.md) records complete upstream
workflows and application/macro observations in Node and QuickJS.

Compiled `meta`, `with-meta` and `vary-meta` preserve metadata independently of
structural equality and hashing. Collection updates, seq tails, lazy sequence
coercion, function/IFn wrappers, reify and records follow their CLJS metadata
protocols. Atoms expose mutable metadata; Vars retain separate definition
metadata. Direct, quoted and macro-generated metadata share the compiler bridge.
The [metadata audit](tests/reference/compiler-metadata.md) records 32 upstream
assertions and 38 independent probes against both pinned references.
Compiled `loop`/`loop*` and function/method `recur` use constant stack space and
preserve per-iteration closure captures, simultaneous rebinding, and variadic rest
sequences. Declared protocol/reify methods preserve receiver conventions; `case`
and synchronous `try` support recurrence control flow. The
[recurrence audit](tests/reference/compiler-recur.md) records exact reference
comparisons; the [record audit](tests/reference/compiler-records.md) adds the
original Search1–Search4 record wrappers. `reduce-kv` supports owned
maps/vectors, custom `IKVReduce` receivers, and reducible map-entry sequences.
Compiled user protocols support marker and overloaded declarations, method Var
metadata, namespace aliases, saved method functions and public CLJS dispatch slots.
Dispatch checks metadata implementations first when opted in, then direct methods,
then native/default tables. `satisfies?` and `implements?` distinguish membership
from available methods. Named/native extensions share the core dispatch path;
redefinition preserves existing direct implementations and uses the current method
Var for fallback. The [protocol audit](tests/reference/compiler-protocols.md)
records source-level tests, negative cases, core identities and both pinned oracles.
`extend-type`, `extend-protocol`, `specify!` and `specify` support named types,
records, built-in values, native selectors and default dispatch. Existing instances
and saved functions observe later extensions; instance methods and metadata retain
CLJS precedence. The [extension audit](tests/reference/compiler-extensions.md)
includes the complete upstream native-extension tests, isolated reference runs
and core operation fast paths.
The [core protocol audit](tests/reference/compiler-core-protocols.md) covers public
lookup, sequence, collection, reduction, state and printing consumers, marker/partial
protocols, custom keys, iterator reduction and explicit release/checkout failures.

Compiled persistent helpers include nested updates, key selection, merging and
zipmap as first-class functions. Collection coercion, map entries, stack
operations and all `into` arities preserve snapshots, metadata, custom protocols
and early reduction. The [helper audit](tests/reference/compiler-collection-helpers.md)
records the pinned source assertions, differential probes and output measurements.
Additional helpers include `some-fn`, `every-pred`, `memoize`, `group-by`,
`frequencies`, `update-keys`, `update-vals`, `reductions`, `partitionv`,
`partitionv-all`, `splitv-at`, `iteration`, tagged literals, printing, and
`clj->js`/`js->clj`. The [integration evidence](tests/reference/compiler-language/evidence.md)
retains exact upstream assertions, bounded property inputs, and version differences.

Compiled exceptions support ordered typed and `:default` catches, rethrowing,
and `finally` in value, discarded and return positions. `ex-info`, `ex-data`,
`ex-message`, `ex-cause`, `ExceptionInfo`, `instance?` and `Throwable->map` retain
JavaScript Error identity and arbitrary thrown values. The
[exception evidence](tests/reference/compiler-language/evidence.md#exceptions-member--2026-10-09)
records 42 selected upstream assertions and 56 independent pinned comparisons.
The self-contained runtime is included once only when used; primitive output
keeps its existing small helpers. Transient vectors/maps/sets and their public
protocols execute in the shared runtime and during macro expansion; see the
[transient audit](tests/reference/compiler-transients.md). Sorted sets remain
intended language gaps. Rust application preparation removes unused inert
definitions while retaining ordered effects, entry results, explicit exports and
Var observations. It conservatively specializes proven immutable top-level calls;
[the preparation API](docs/compiler.md#application-preparation) also supports
optimizer-off comparisons and repeated emission. Package acquisition and bundling
are not migration goals,
and a general standard library is not currently bundled. Caller-supplied library
sources use the same supported language features. See the
[migration and language boundaries](docs/evaluator-migration.md#language-coverage-and-completion-boundaries)
and the
[executable core contract](docs/compiler.md) for exact limits and testing.

Compiled atoms and volatiles support dereference, all swap/reset arities,
old/new returns, validators, synchronous watches, and mutable metadata. Custom
`IDeref`, `ISwap`, `IReset`, `IVolatile` and `IWatchable` implementations use the
same public functions, including saved calls and namespace aliases. ClojureScript
`compare-and-set!` uses structural equality. The
[state audit](tests/reference/compiler-state.md) compares 56 upstream assertions
and 41 independent probes with both pinned references.

For example, the following executable program returns `2`:

```sh
cargo run --quiet -- --compile --text '(let [v [:demo/x 1]] (count v))' | \
  node --print 'eval(require("fs").readFileSync(0,"utf8"))'
```

## Lexer API

```rust
use light_cljs::Tokenizer;

let source = "(+ 1 2)";
for token in Tokenizer::new(source) {
    println!("{:?} {:?} {:?}", token.kind, token.span, &source[token.span.clone()]);
}
```

`Tokenizer` borrows an `&str` and implements `Iterator<Item = Token>` and
`FusedIterator`. `Token` holds a kind and a half-open `Range<usize>` of byte
offsets. Every span, including an error span, is nonempty and can slice the
original UTF-8 source. Calls after EOF continue returning `None`.

## Token contract

| Source | Kind | Treatment |
| --- | --- | --- |
| `(` `)` `[` `]` `{` `}` | `OpenParen` / `CloseParen`, `OpenBracket` / `CloseBracket`, `OpenBrace` / `CloseBrace` | Each delimiter is a token; matching belongs to the reader. |
| Symbols, keywords, numbers, booleans, `nil`, `%`, `%1`, `%&` | `Atom` | Raw text; classification and contextual interpretation belong to the reader. |
| `"text"` | `String` | Includes quotes; backslashes protect the next character from delimiter recognition. |
| `#"pattern"` | `Regex` | Includes `#` and quotes; regex escapes are preserved. |
| `\a`, `\)`, `\space`, `\u03bb`, `\o141` | `Character` | Includes the backslash and complete character spelling. |
| `;comment`, `#!comment` | `Comment` | Emitted, excluding the terminating CR/LF. `#!` is recognized wherever a dispatch form can start. |
| `'` / backtick / `~` / `~@` | `Quote` / `SyntaxQuote` / `Unquote` / `UnquoteSplicing` | Prefix only. |
| `@` | `Deref` | Prefix only. |
| `^`, legacy `#^` | `Metadata` | Prefix only; the metadata form follows. |
| `#{` | `SetStart` | Includes the opening brace. |
| `#(` | `AnonymousFnStart` | Includes the opening parenthesis. |
| `#'` | `VarQuote` | Prefix only. |
| `#_` | `Discard` | Does **not** discard the following form; all its tokens are emitted. |
| `#?`, `#?@` | `ReaderConditional`, `ReaderConditionalSplicing` | Does not select or skip branches. |
| `#:ns`, `#::alias`, `#::` | `NamespacedMap`, `AutoNamespacedMap` | Includes the adjacent namespace/alias spelling; the map brace is separate. |
| `##` | `SymbolicValue` | Prefix; `Inf`, `-Inf`, or `NaN` follows as an atom. |
| `#js`, `#inst`, `#uuid`, `#app/tag` | `Dispatch` then `Atom` | `#` and the tag are separate tokens; no tag handlers run. |
| `#=` | `ReadEval` | Recognized for reader compatibility; never evaluated. |
| `#<` | `Error(UnreadableForm)` | This dispatch sequence is explicitly unreadable. |

An unterminated string/regex returns `Error(UnterminatedString)` or
`Error(UnterminatedRegex)`, spanning from its opening delimiter to EOF. A lone
backslash or hash at EOF returns `Error(MissingCharacter)` or
`Error(MissingDispatch)`. Errors always advance; `#<` permits scanning the rest
of the input, while unterminated literals consume the remainder. Missing forms
after prefixes such as `#_` or `'` are reader errors, not lexical EOF errors.

Quotes, commas, semicolons, and brackets inside literals are literal content.
Multiline strings and regexes are accepted lexically. Character literals consume
a literal first character even if it is whitespace or a delimiter; remaining
named/Unicode/octal spellings run to a symbol terminator. A character consisting
of CRLF or CRFF is kept together to match the indexing reader's normalization.

## Boundaries and Unicode

Whitespace follows the **JVM** compiler's vendored `tools.reader`, not Rust's
`char::is_whitespace` or the JavaScript host's whitespace set. Commas, ASCII
`09`–`0D`, `1C`–`20`, U+1680, U+2000–2006, U+2008–200A, U+2028–2029, U+205F,
and U+3000 are skipped. NBSP, NEL, figure space, narrow NBSP, zero-width space,
and BOM remain atom content. Inside comments, only CR or LF ends the comment.

Atoms end at whitespace or a macro-terminating character:
`" ; @ ^` backtick `~ ( ) [ ] { } \`.
The characters `#`, `'`, `:`, and `%` can appear **inside** symbol-like atoms:
`foo#`, `foo'`, and `foo:bar` each stay whole. At the beginning of a form, the
relevant reader prefixes are recognized. Number candidates additionally end at
all macro characters: `1:kw` produces atoms `1` and `:kw`, whereas `x:kw` is one
atom. Digit detection matches the reader's UTF-16 `Character.isDigit(char)`
classification, including BMP decimal digits; that does not validate numbers.

The independent [reader class snapshot](tests/reference/reader_classes.txt) is
regenerated using [the reference script](tests/reference/reader_classes.clj).
Compatibility is based on the local ClojureScript checkout at
`a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e` and its vendored reader, especially
`macro-terminating?`, `read-token`, `read-number`, `read-char*`, `read-string*`,
`read-regex`, `macros`, and `dispatch-macros` in
`research/clojurescript/src/main/clojure/cljs/vendor/clojure/tools/reader.clj`.

## Lexer versus parser

Lexical coverage does not mean a complete reader or source validator. The lexer
preserves escape sequences and character spellings without decoding or validating
them; for example, `"\q"` is a `String` and `\unknown` a `Character` whose values
a reader must reject. Regex compilation, numeric/symbol/keyword validation,
namespace resolution, tag resolution, metadata application, map arity, nested
anonymous-function restrictions, and delimiter matching are also later work.
The [parser](docs/parser.md) now handles spelling validation, string/character
decoding, delimiters, ordinary map arity, and anonymous-function syntax. Reader
expansion and namespace-sensitive meanings remain deferred.

Context-dependent `%` notation and syntax-quote gensyms remain raw atoms. For
example, `%&rest` and `%1:kw` each remain one atom; inside `#()` the reader must
interpret the raw span as argument shorthand followed by another form. Outside
`#()` the same text can be a symbol. This lexer intentionally keeps no nesting
or reader context, and never executes reader evaluation.

## Parse syntax

```rust
use light_cljs::parse;

let source = "(+ 1 2)";
let tree = parse(source).unwrap();
let root = &tree.nodes[tree.roots[0].0];
assert_eq!(root.span, 0..7);
```

`SyntaxTree` owns a `Vec<Node>` and root IDs; retain the original source alongside
it. Collections store `Vec<NodeId>` children. `Parser::read_form()` also supports
reading and inspecting one top-level form at a time. It uses one-token lookahead,
skips comments and whole-form `#_` discards, and limits nesting to 128 levels.

```sh
cargo run -- --parse --text '(+ 1 2)'
cargo run -- --parse tests/fixtures/parser_forms.cljs
cargo run -- --parse research/clojurescript/src/main/cljs/cljs/core.cljs
```

Parser output is a flat arena listing with child IDs, byte spans, and escaped
source text. Parse errors include line/byte-column, the failing span, and an
opening span when available. See [the parser contract](docs/parser.md) and
[measured construction costs](benchmarks/parser.md).

## Inspect tokens

```sh
cargo run -- --text '(+ 1 2)'
cargo run -- tests/fixtures/reader_forms.cljs
printf '(café,λ)' | cargo run --quiet
```

```text
OpenParen   0..1   "("
Atom        1..2   "+"
Atom        3..4   "1"
Atom        5..6   "2"
CloseParen  6..7   ")"
```

The CLI reads a UTF-8 file, `-`, or stdin, then prints tab-separated kinds, byte
spans, and escaped source text. It owns the input buffer; the scanner borrows it.
Lexical errors are printed as tokens and cause a nonzero exit status, as do
invalid UTF-8 and I/O errors.

## Verify and measure

```sh
cargo test
cargo test --release
cargo clippy --all-targets -- -D warnings
cargo bench --bench tokenizer -- --samples 9 --ms 150
cargo bench --bench parser -- --samples 9 --ms 100

# Optional: requires the gitignored reference checkout.
cargo test --release --test corpus -- --ignored --nocapture
cargo test --release --test parser_corpus -- --ignored --nocapture
# Required CI conformance, run explicitly locally (Java/Clojure CLI).
cargo test --test execution -- --ignored --nocapture
cargo bench --bench tokenizer -- --samples 9 --ms 150 \
  research/clojurescript/src/main/cljs/cljs/core.cljs \
  research/clojurescript/src/main/clojure/cljs/analyzer.cljc
```

Compiler execution and compiler CLI tests require Node.js on `PATH`, or `NODE`
set to its executable path. Evaluator CLI tests need only Rust; run
`cargo test --test eval_cli` for the subprocess suite. The compiler differential
test uses the same tiny programs and compares
values and observable effects; it may download the pinned Maven dependencies.

The [compiler language assertion inventory](tests/reference/compiler-language/evidence.md)
reports compiled source-language behavior separately from successful parser
and historical native-interpreter results. It preserves complete upstream bodies and original
expectations; diagnostics and version differences have separate dispositions. The
[integration evidence](tests/reference/compiler-language-completion.md) covers the
combined source program, deterministic interactions and completion checks. Python 3.10+ runs its accounting tests with `cargo test --test compiler_language`.

```sh
# Inspect missing capabilities without running or claiming any passes.
python3 -B tests/reference/compiler-language/report.py --show-missing
# Focus implementation on one section; rejected features remain missing.
python3 -B tests/reference/compiler-language/report.py --run --feature functions-apply
# Full intended-language gate: missing source/API/arity/phase evidence fails.
python3 -B tests/reference/compiler-language/report.py --run --strict
```

Execution defaults to both Maven 1.12.134 and the pinned research checkout.
`--reference none` supports a quick local run; it cannot establish conformance.
The default [full language ledger](tests/reference/compiler-language/full-scope.md)
retains every runtime test/support file from the pinned checkout, independent
public core/library names and arities, syntax, and SCI/API provenance. It exposes
parse, analysis/lowering, removed native execution, canonical JavaScript,
QuickJS macro eligibility, and meaningful execution evidence separately.
Remaining obligations have responsible Pellet links. Transients, eager transducers,
and [multimethods and hierarchies](tests/reference/compiler-multimethods.md) have
separate executing audits. `--strict` currently fails
for the full scope; parser acceptance and passing historical subsets cannot
establish completion. `--scope historical-selected` reproduces the earlier
18-file batch and its scoped execution gate.

The benchmark prepares eight fixtures before timing and accepts additional UTF-8
files. It consumes full tokens through `black_box`, emits raw CSV on stdout, and
reports configuration and medians on stderr. See the
[measurements and methodology](benchmarks/README.md). Historical subset scanner
experiments are preserved separately under `cargo bench --bench subset` and
must not be described as throughput for this expanded scanner or a complete
ClojureScript reader/compiler.

For the supported compiler-backed embedding API, run `cargo run --example embedding` or read the
[embedding example](examples/embedding.rs) and the documented
[session options, capability policy and migration map](docs/evaluation.md).

Compiled `reify` uses a stable anonymous type per source site and fresh captured
fields per evaluation. Protocol/IFn/Object methods support destructuring,
overloads, closures and recurrence. Metadata copies retain type and dispatch;
captured atoms share state intentionally while independent factories do not.
A three-namespace update/effect/watch event example executes entirely in the
emitted JavaScript. See [reify evidence](tests/reference/compiler-reify.md).

Compiled `deftype` and `deftype*` support positional constructors, live receiver
fields, permitted mutable assignments, nested closures, shadowing, method
recurrence and user/core protocols. Custom callable, lookup, sequence and reduction
objects work through normal language operations. Namespace-qualified types,
redefinition, saved factories and existing instances retain constructor identity.
The [type audit](tests/reference/compiler-types.md) records 30 upstream assertion
sites, independent differential probes, diagnostics and output measurements.

Compiled records support positional/map construction, extension entries, metadata,
cloning, nominal equality, structural map/set keys and full method dispatch. Removing
a declared field returns a plain map; extension-only updates preserve record identity.
Raw `defrecord*`, constructor aliases, redefinition and external extensions execute
in the generated JavaScript. See the [record audit](tests/reference/compiler-records.md)
for source assertions, pinned comparisons and output measurements.

Canonical sorted maps and sets support custom comparators, immutable updates,
lookup, reduction, reverse traversal, `ISorted`, `subseq`, and `rsubseq` in both
application and macro execution. The [sorted collection audit](tests/reference/compiler-sorted-collections.md)
retains full upstream bodies and pinned-reference evidence.
