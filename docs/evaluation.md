# Compiler-backed evaluation and embedding

`EvalSession` is the supported interactive embedding API. Rust reads and resolves
each form, expands macros in a separate compilation heap, emits canonical JavaScript,
and executes only that form in the application's QuickJS heap. Definitions,
closures, atoms and protocol tables persist in this explicit session. Later forms
see earlier effects even when a later read, analysis or runtime operation fails.
Ordinary exceptions restore language `binding`/`with-redefs` scopes; they do not
undo already performed application or host effects.

```rust
use light_cljs::{EvalOptions, EvalSession};
let mut session = EvalSession::new(EvalOptions::default())?;
session.eval_string("(def hits (atom 0))")?;
let result = session.eval_string("(swap! hits inc)")?;
assert_eq!(session.number(result.value)?, 1.0);
session.release(result.value)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

`eval_source` takes compiler-owned `SourceInput` (`.cljs` or `.cljc`); `eval_form`
takes a compiler-owned `Form`. `read_forms`, `analyze`, `evaluate` and `macroexpand`
expose their corresponding stages. An `AnalyzedForm` can execute only in its
originating session. `EvalResult` reports the last value ID and final namespace.
`EvalDiagnostic` owns its message, supported finite exception data and optional source span and remains useful
after session teardown. Runtime `eval` and macroexpand calls return to these Rust
compiler services; there is no JavaScript language evaluator or native fallback.

`fresh_session` reuses immutable options and creates independent roots/heaps.
`reset` replaces the session and invalidates every previous ID. Sharing the same
session explicitly shares application state. There is no implicit cross-heap
clone and no native fork family. `ValueId` contains an owner generation; foreign,
released and dropped-owner IDs cannot be used in another session. `value` and
`data` convert finite compiler data using the structured form bridge, preserving
UTF-16 names/strings and metadata; arbitrary functions/atoms/objects remain IDs.
Release retained IDs when finished. The entire heap and its root registry are
reclaimed on drop, including cycles.

## Capabilities and configuration

`EvalOptions` supplies host functions, finite initial bindings, namespace/aliases,
arguments, source provider, source data-reader functions and a default handler, assertions, print options,
input/output/error callbacks and resource bounds. Configuration is immutable for
a session. `bind` installs a session value; `lookup` retrieves a root; `bindings`
lists compiler-known qualified binding names. `with_bindings` restores temporary
host root replacements on success, error and panic. Language Var metadata,
`meta`/`alter-meta!` and dynamic scopes use the canonical runtime.

`HostFunction` registers an explicit qualified name, `HostArity` and an `Rc`
callback. A callback receives a `HostCall` and typed value IDs. `HostCall::number`,
`string`, `data`, `value`, `print` and `invoke` use the already active engine
context, so recursive invocation does not acquire a second runtime lock.
Argument IDs and IDs allocated through the call expire on return. Copy finite
data if it must survive. Host closures must retain Rust data only; they must not
capture an owning session. Panics are caught before crossing the C boundary and
make the active session terminal. Returned errors are located at the source call
and ordinary host errors leave the session usable.

Sources load only through the explicit provider. `ns` and `require` use ordinary
`.cljs`/`.cljc` compilation, aliases/refers and dependency-once loading; standard
`clojure.string`, `clojure.set`, `clojure.walk`, `clojure.edn` and `cljs.reader`
sources are bundled. No ambient filesystem, npm, Node, DOM or JVM is introduced.
Compiled `data_readers` are one-argument source functions installed in the reader
compilation service and canonical runtime reader. `default_data_reader` receives
the tag and value for unknown tags; named and built-in readers take precedence. Source macros use `defmacro`;
native callback macros are replaced by compiler source macros.

`NamePolicy` checks resolved compiler names, including core references, aliases,
Vars, quoted `resolve`, macros and explicitly exposed host bindings. Deny wins;
allow restricts exposed capabilities/core names while permitting ordinary user
bindings. This is a binding-resolution policy, **not a sandbox for arbitrary
JavaScript interop**. ECMAScript intrinsic objects remain available.

## Limits and failure

`engine` specifies JS memory bytes, stack bytes, deadline and cooperative poll
budget. A public evaluation/invocation begins a new deadline/poll budget, shared
by nested compiler and application work. Nested `eval`, callbacks and macro
expansion cannot replenish it. Cancellation, memory exhaustion, interrupts and
host panic are sticky, even when JavaScript catches an exception; call `reset`
before using that session again. A separate clean compilation/session remains
usable. Long host callbacks must poll `HostCall::control()` cooperatively.

`compiler` bounds macro expansions, generated nodes and expansion depth.
`max_source_bytes` and `max_output_bytes` bound input and emitted/returned print
text. Conversion also has finite node/depth bounds. These controls do not retain
the native interpreter's fuel/allocation unit meanings. Rust compiler allocation
is not measured by the JS heap limit. Input providers return a nonempty chunk,
EOF or an owned error; canonical reader cursors handle parsing and line state.
No input/output capability is connected implicitly.

## Native API migration map

The `interpreter` module and `native_form_adapter` have been removed, along with
their native-only tests and benchmark harnesses. No feature flag restores them.
The CLI and embedding API use compiler-backed sessions; unsupported emitted code
reports an error without a native fallback. The table below maps retired API names
to the current surface; the left column is not available in this checkout.

| Legacy API or contract | Supported replacement or retirement |
| --- | --- |
| `Context::new/init`, `ContextOptions` | `EvalSession::new(EvalOptions)` |
| `merge_opts`, mutable provider/limit/policy setters | Configure `EvalOptions` and create a fresh session; `bind` updates runtime bindings explicitly |
| `eval_string`, `eval_source`, `eval_form`, plus variants | Corresponding session methods returning `EvalResult`/owned `EvalDiagnostic` |
| `analyze_form`, `eval_analyzed`, `AnalyzedForm` counters | `analyze`/`evaluate`; native expression/local counters retired |
| `invoke`, `invoke_native`, `register_native`, `native_function`, `NativeDefinition` | `invoke`, configured `HostFunction`/`HostCall` and `HostArity` |
| Native macro callbacks | Source `defmacro`; macroexpand services use the compiler |
| `MacroHost::context_mut`, `register_macro`, `load_namespace`/native `Reload` | Source `defmacro` and ordinary `:require`; `compile_namespace_with_macros` reloads source definitions into fresh roots on every clean compilation |
| Native reader callback registration | `MacroHost::register_compiler_data_reader`/`register_compiler_default_data_reader`, or session data-reader options; handlers execute compiled source functions |
| `RootedValue`, `ValueView`, scalar/collection constructors | Session `ValueId`, typed accessors and compiler `Datum`; no native heap types |
| `fork`, shared owner families, reset/reuse stats, collector tracing APIs | `fresh_session`/`reset`; native owner/GC/reuse counters retired |
| `SourceProvider`, filesystem roots and load state | Compiler-owned provider requests and explicit host implementation; `ns`/`require` load once per session |
| namespace/Var lookup, intern, dynamic/metadata APIs | `namespace`, `bindings`, `lookup`, `bind`, `with_bindings`; canonical language Vars/scopes/metadata |
| `bind_io`, native stream constructors and buffers | Configured input/output callbacks, canonical `read`/`read-line`/`with-in-str`/`with-out-str`; host owns buffers |
| Native reader cursor compact/recover methods | Canonical reader stream operations; native cursor representation retired |
| Native cancellation token/poll/checkpoint/charge methods | `ExecutionControl`, explicit engine/compiler/output limits; callbacks poll shared control |
| Native integer/ratio/class descriptors and builtin-write ownership flags | JavaScript Number and canonical CLJS/JS types; exact ratios, native descriptor identity and native ownership flags retired |

`(/ 1 2)` is Number `0.5`; `(+ 9007199254740992 1)` is
`9007199254740992`. This is the deliberate numeric migration, not an exact-native
compatibility claim. See the runnable [embedding example](../examples/embedding.rs)
and [execution provenance](../tests/reference/eval-sessions.md).

## Script and REPL commands

`light-cljs --eval [OPTIONS] [FILE|-]`, `--eval --text SOURCE`, and
`--repl [OPTIONS]` compile each form with Rust and execute emitted JavaScript in
embedded QuickJS. Tokenize/parse/analyze/compile commands retain their existing
behavior. No Node/JVM is needed by evaluation. `.clj` files receive a migration
error; use `.cljs` or `.cljc`, ordinary `:require` for helpers/macros, and `:cljs`
conditionals. Stdin and text use the portable CLJS dialect with conditionals.

Explicit source roots search `.cljs` before `.cljc`, reject ambiguous candidates
of the selected extension and never discover the script/current directory.
Bundled libraries need no root. Missing external sources report the namespace
and the requesting form. ECMAScript Math, strings, objects and RegExp are available;
Node/DOM/JVM/filesystem/network/process capabilities are not supplied implicitly.

Program print output goes immediately to stdout. Bind `*print-fn*` to
`*print-err-fn*` for stderr output. The CLI writes a buffered readable last value
and newline only after all script forms and result rendering succeed. Earlier
program effects remain on failure. Empty scripts return nil. File/text scripts
may read stdin; stdin scripts consume it as source before execution. `*file*`
names the script (or `<text>.cljc`/`<stdin>.cljc`). Strings preserve UTF-16 internally;
readable output escapes lone surrogates and raw UTF-8 output rejects them.

The REPL reads and evaluates one form before contextually reading the next,
including namespace changes on the same line. It uses the Rust parser and
compiler reader; its cursor is shared with canonical runtime `read`/`read-line`.
Consumed lines compact while retained forms keep their original source locations.
Terminal namespace prompts and `...=>` continuation prompts appear on stderr;
redirected input has no prompts. EOF exits, with one diagnostic for unfinished
syntax. Reader errors discard the buffered suffix; analysis/runtime/printing
errors preserve following forms. Runtime read failures recover at the same
buffered boundary, even when caught by the program.

Successfully evaluated and rendered forms rotate `*1`, `*2`, `*3` (initially nil).
Errors leave them unchanged and update `*e`. Application throws retain their actual
value and `ex-data`; read/analysis diagnostics become ExceptionInfo with phase and
available source location. Success preserves `*e`. Definitions return their CLJS
initializer value; native Var return/printing conventions are retired.

| Flag | Default | Unit/behavior |
| --- | ---: | --- |
| `--poll-budget N` | 1000000 | Cooperative engine/compiler polls per operation |
| `--memory-limit N` | 67108864 | JS heap bytes; minimum 1048576 for startup |
| `--stack-limit N` | 1048576 | JS engine stack bytes |
| `--timeout-ms N` | 10000 | Deadline per operation in milliseconds |
| `--source-limit N` | 4194304 | UTF-8 bytes per source/input window |
| `--output-limit N` | 1048576 | UTF-8 emitted/rendered bytes per operation |
| `--print-length N` | 100 | Result collection item bound |
| `--print-level N` | 20 | Result collection depth bound |

Numbers are nonnegative integers; zero is a real bound. Startup rejects a heap
limit below its supported minimum. Native `--fuel`, `--allocation-limit`,
`--call-depth` and `--evaluation-depth` are rejected with explicit replacement
controls; units are not equivalent. `--output-limit` retains its spelling but
now measures UTF-8 bytes. Engine poll/deadline/memory failures, uncaught stack failures or cancellation
terminate the command nonzero; a fresh invocation allocates a fresh session.
Language errors remain recoverable in the REPL. Source input/host stream failures
are fatal. Read, evaluation and result rendering are separate operations; nested
macros/callbacks share their enclosing budget. Blocking host IO is cooperative,
not preempted by an engine deadline. Native exact numbers, descriptors, JVM catch
classes and native allocator/fork/GC guarantees are explicitly retired.

See [CLI execution evidence](../tests/reference/eval-cli.md) for scope and commands.
