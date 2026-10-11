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

The `interpreter` module is a temporary legacy adapter until **light-cljs-115**.
The CLI switches independently in light-cljs-113. It is never a fallback for
unsupported emitted code. Legacy interpreter tests retain historical native
behavior until the dedicated suite migration/removal steps.

| Legacy API or contract | Supported replacement or retirement |
| --- | --- |
| `Context::new/init`, `ContextOptions` | `EvalSession::new(EvalOptions)` |
| `merge_opts`, mutable provider/limit/policy setters | Configure `EvalOptions` and create a fresh session; `bind` updates runtime bindings explicitly |
| `eval_string`, `eval_source`, `eval_form`, plus variants | Corresponding session methods returning `EvalResult`/owned `EvalDiagnostic` |
| `analyze_form`, `eval_analyzed`, `AnalyzedForm` counters | `analyze`/`evaluate`; native expression/local counters retired |
| `invoke`, `invoke_native`, `register_native`, `native_function`, `NativeDefinition` | `invoke`, configured `HostFunction`/`HostCall` and `HostArity` |
| Native macro callbacks | Source `defmacro`; macroexpand services use the compiler |
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
