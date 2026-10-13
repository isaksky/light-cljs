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
compiler services; there is no separate language interpreter.

`fresh_session` reuses immutable options and creates independent roots/heaps.
`reset` replaces the session and invalidates every previous ID. Sharing the same
session explicitly shares application state. There is no implicit cross-heap
clone. `ValueId` contains an owner generation; foreign,
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
String libspecs such as `(require '["react" :as r])` bind JavaScript packages
from the host registry `globalThis.lightCljsPackages`, read at each use and never
loaded by the session: supply it first, for example with a prelude
`(set! js/lightCljsPackages (js-obj "react" ...))`. See
[JavaScript packages](compiler.md#javascript-packages).
Compiled `data_readers` are one-argument source functions installed in the reader
compilation service and canonical runtime reader. `default_data_reader` receives
the tag and value for unknown tags; named and built-in readers take precedence.
Macros are always source `defmacro` definitions; there are no host callback macros.

`load_file(path)` and language `load-file` use `EvalOptions.file_provider`, which
receives an opaque path and requesting namespace and returns `SourceInput`.
They reuse this session's compiler, heap and dependency cache. Loaded definitions
and effects before failure persist; diagnostics retain the returned file name,
and caught language exceptions retain their original identity and data. `.clj`
requests and returned sources are rejected. No filesystem access is implied.

`EvalOptions.clock` and `random` provide fallible Number callbacks for
`system-time` and random operations. Defaults use the host's ECMAScript clock
and `Math.random`; a clock returns finite milliseconds and random returns
`[0,1)`. `add-tap`, `remove-tap` and `tap>` use bounded synchronous delivery by
default, with a maximum of 1024 accepted events per drain. Emitted programs can
inject `rt.core.configureHostHooks({clock, random, tapSchedule, maxPendingTaps})`;
the scheduler owns admission and delivery timing. Dynamic `*exec-tap-fn*` can
also supply scheduling. Console printing requires a supplied host console;
process arguments remain explicit `EvalOptions.arguments`. Clock and random options
configure the application heap; the macro-expansion heap has its own host
environment.

`NamePolicy` checks resolved compiler names, including core references, aliases,
Vars, quoted `resolve`, macros and explicitly exposed host bindings. Deny wins;
allow restricts exposed capabilities/core names while permitting ordinary user
bindings. This is a binding-resolution policy, **not a sandbox for arbitrary
JavaScript interop**. ECMAScript intrinsic objects remain available. Host paths
are checked by their full `js/` name, but every JavaScript package lookup is
checked as the single name `js/lightCljsPackages`, whatever the package or
export: deny it to refuse all package access, or allow it in an allow list. The
host controls which packages exist by what it puts in the registry.

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
text. Conversion also has finite node/depth bounds. Rust compiler allocation
is not measured by the JS heap limit. Input providers return a nonempty chunk,
EOF or an owned error; canonical reader cursors handle parsing and line state.
No input/output capability is connected implicitly.

Printing uses the configured output callback; `*print-err-fn*` uses the separate
error callback. `set-print-fn!` and `set-print-err-fn!` replace these roots, and
language bindings restore them after exceptions. `with-out-str` captures nested
output with newlines enabled and restores the enclosing sink on success or
failure. `PrintOptions` configures readability, newlines, length, level,
namespace maps, metadata, duplicate printing and newline flushing. Collection
length and level apply to maps, sets, records, sequences and JavaScript
collections. `pr-str-with-opts` and writer helpers preserve explicit options
through nested values and custom printers, including alternate printers and
their fallback. Strings retain their UTF-16 code units inside JavaScript;
configured Rust output callbacks receive UTF-8 text. The Rust `print` display
API escapes isolated surrogate units while preserving valid pairs. `flush` is the upstream
no-op stub; `newline` invokes it when newline flushing is enabled.

## Numbers

All arithmetic uses JavaScript Number: `(/ 1 2)` is `0.5` and
`(+ 9007199254740992 1)` is `9007199254740992`. There are no exact ratios or
arbitrary-precision integers. See the runnable
[embedding example](../examples/embedding.rs) (`cargo run --example embedding`).

## Script and REPL commands

`light-cljs --eval [OPTIONS] [FILE|-]`, `--eval --text SOURCE`, and
`--repl [OPTIONS]` compile each form with Rust and execute emitted JavaScript in
embedded QuickJS. No Node/JVM is needed. `.clj` files receive a migration
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
initializer value.

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
limit below its supported minimum. The obsolete `--fuel`, `--allocation-limit`,
`--call-depth` and `--evaluation-depth` flags are rejected with a pointer to the
replacement control. Engine poll/deadline/memory failures, uncaught stack failures or cancellation
terminate the command nonzero; a fresh invocation allocates a fresh session.
Language errors remain recoverable in the REPL. Source input/host stream failures
are fatal. Read, evaluation and result rendering are separate operations; nested
macros/callbacks share their enclosing budget. Blocking host IO is cooperative,
not preempted by an engine deadline.

The CLI is tested by `tests/eval_cli.rs` (subprocess runs, Rust only) and
`tests/eval_examples.rs`, which runs the scripts in `examples/native`.
