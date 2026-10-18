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

Evaluated definitions live in the heap's application registry,
`globalThis.__lightApplicationRoots["ns/name"]`, which later code reads at each
use, so a redefinition is seen by every caller the next time it runs. A dev
build (`--compile --entry NS --source-root DIR --reloadable --out-dir DIR`,
[Dev builds](compiler.md#dev-builds)) emits a program's namespaces as units of
the same shape over the same registry, so reloading a namespace there
redefines as the session does.

## Actions

The session is two parts: a `DevSession` (`light_cljs::dev_session`), the
incremental compiler with its macro host and the namespaces the session has
loaded, and the QuickJS heap, which interprets what the compiler produces
(light-cljs-283; the design is "Actions" in
[dev-workflow.md](dev-workflow.md#actions)). A `DevSession` also works with no
heap at all, for a runtime in another process:

```rust
use light_cljs::dev_session::{Action, DevSession};
use light_cljs::{EvalOptions, source_provider::SourceInput};
let dev = DevSession::new(&EvalOptions::default())?;
let actions = dev.source(&SourceInput::new("repl.cljs", "(def x 41) (inc x)"));
assert!(matches!(&actions[..], [Action::Invoke(_), Action::Invoke(_)]));
# Ok::<(), light_cljs::EvalDiagnostic>(())
```

`DevSession::new` reads the compiler fields of `EvalOptions`: `namespace`,
`aliases`, `compiler`, `engine` (for the macro heap), `data_readers`,
`default_data_reader`, `source_provider`, `max_source_bytes`, `assertions` and
`policy`. `form` and `source` compile as a REPL evaluates: each top-level form
in turn, the next read after the previous one compiled. `load_file` compiles
a file's text as a loaded file, and `load_namespace_file` the file of a given
namespace, read from its first form in that namespace, as `--compile` reads a
namespace source, so a `::` keyword in its `ns` form names it (a watch step).
`namespace` is the current namespace and `read_forms` reads in it; `loaded`,
`requires` and `declares_macros` say whether the session has loaded a
namespace, which namespaces its directives require and whether it declares
macros. Each call runs under a new deadline and poll budget
of `engine`, which the macro heap shares, so a long-lived `DevSession` (a
watch loop) can sit idle between calls; one that runs out, or a cancelled
`DevSession::control`, leaves it terminal. A `DevSession` analyzes in warning
mode, as dev compilation does (light-cljs-284): an undeclared Var (a use of
one defined later, as a file saved before the one defining it has) and
conflicting arities are a `Warnings` action, and the code runs as upstream's
would, reading the Var at each use, where strict analysis rejects the form
and the reload with it. Warning mode also warns of a single-segment `ns`
name and allows `[lib :as js]`, as `--compile --warnings` does.
`DevSession::set_warning_mode(false)` gives the strict analysis an
`EvalSession` has unless `EvalOptions::warning_mode` turns warning mode on.
Each returns the actions in order:

| Action | Carries | Meaning |
|---|---|---|
| `Invoke(Unit)` | one unit | Evaluate this unit for its value: a REPL form. |
| `Load(Vec<Unit>)` | units in order | Run these units: the forms of the namespaces a `require` loads, dependencies first, or of a loaded file. |
| `Namespace(name)` | a namespace name | The current namespace is now `name`. |
| `Warnings(Vec<EvalDiagnostic>)` | located warnings | Analysis warnings, `message [kind]`, before the actions of their form. |
| `Error(EvalDiagnostic)` | a located diagnostic | The form did not compile. Always last: nothing after it was compiled. |

A `Unit` is one top-level form: its `namespace`, its `js` (the function
expression `(__lightInteractiveRuntime) => ...` that a runtime calls with the
runtime, writing definitions to `globalThis.__lightApplicationRoots`, ending
with a `//# sourceURL=` comment), its `source_map` and the `location` of the
form, where an error it throws is located. Before the form runs, the unit
registers with the runtime each namespace whose reader context or
definitions changed since the session's last unit, every namespace the
session knows in its first (light-cljs-301), so a runtime that runs the
units in order sees every namespace for reflection (`find-ns`, `read-string`
with another namespace's aliases) as the session does. The unit of
`EvalSession::analyze` may run late or never: it leaves its registrations to
the next unit too, and once `evaluate` ran it, or after a unit that failed,
the next unit registers every namespace again. The units of
`DevSession::load_namespace_file` register none: a watch step puts them in
the namespace's file, which registers the namespace once. The source map
(light-cljs-284) is
a Source Map v3 of `js`, made as `--source-map` makes one, whose `sources`
name the form's source as the source provider named it; its `file` and the
`sourceURL` are the namespace's file in a [dev build](compiler.md#dev-builds),
`app.view.js`, which a reload of the namespace replaces. A runtime that
evaluates the unit appends a `//# sourceMappingURL=` comment with the map
(as a `data:` URL, say) to map what it runs. A `Namespace` action comes where
the session has always made a namespace current: after each directive (`ns`,
`in-ns`, `require`, `ns*`), and when a load enters a namespace and returns to
the requiring one. So `(require 'b)`, where `b` requires `a`, is
`Namespace(cljs.user)`, `Namespace(b)`, `Namespace(a)`, `Load` of `a`'s forms,
`Namespace(b)`, `Load` of `b`'s forms, `Namespace(cljs.user)`, and a second
`(require 'b)` is `Namespace(cljs.user)` alone: a `DevSession` loads a
namespace once unless the `require` says `:reload` or `:reload-all` (see
[Capabilities and configuration](#capabilities-and-configuration) for the
spellings), which compile the text the source provider returns at that time.
Consecutive
`Load`s are one. The value of a form or file is the value of its last action
when that ran a unit, else nil: a directive's value is nil, and a file whose
last form is a definition has that definition's value.
`dev_session::script` writes actions as a script that applies them to a
running [dev build](compiler.md#dev-builds) as the session's heap does: a
`Namespace` sets the runtime's current namespace, the units of a `Load` are a
`globalThis.__lightDev.define` call per namespace, an `Invoke` runs its unit
and an `Error` throws. So a `DevSession` can replace the namespaces of a
running dev build (`tests/evaluation/dev_session.rs` does so in Node).

`EvalSession` is the interpreter of these actions over its heap: a unit runs
in the heap, and a `Namespace` sets the runtime's `*ns*`. It interprets each
action as the compiler produces it, which is how the session has always
evaluated: a namespace's form runs before the next one is read and compiled,
so a form that throws while a namespace loads stops the load there, a
definition after it is never compiled, and the namespace stays unloaded until
a `require` loads it again. A `DevSession` with no heap cannot know whether
its units ran, and counts a namespace loaded once it compiled it. The
runtime's `load-file`, `eval` and `macroexpand` call the session's
`DevSession` in process while a unit runs. `EvalSession::analyze` returns an
`AnalyzedForm` that wraps the `Invoke` of the form's unit
(`AnalyzedForm::action`), which `evaluate` interprets. The session hands each
`Warnings` action's warnings, located, to `EvalOptions::warnings` when it has
one (an error it returns is the form's), before the form runs, and ignores
source maps.

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

An `ns` form goes through the compiler's namespace analysis, as `ns*` does
and as a source file's header does under `--compile` (light-cljs-284), so
every clause `--compile` accepts behaves the same in the session: name
metadata, a docstring and an attribute map; `:require` with `:as`,
`:as-alias`, `:refer`, `:rename` and package strings; `:use` with `:only` and
`:rename`; `:import`; `:refer-clojure` with `:exclude` and `:rename`;
`:require-global` and `:refer-global`. Each clause kind appears once, reader
conditionals of a `.cljc` source are selected first, and a clause `--compile`
rejects is the same located error. One difference: the name may be any
symbol without a namespace (`js` excepted), as `in-ns` takes it, where a
source file's must be ASCII. An `ns` form declares all of its namespace's
requires, as upstream's does: evaluating it again, as a reloaded file does,
replaces the namespace's aliases, refers and exclusions with its own (the
`EvalOptions::aliases` of the initial namespace included), so an alias may
now name another namespace and one it drops is gone. `ns*` and `require` add
to them. The loader's own rules stay `--compile`'s alone: there a `goog.*`
library with no source is a JavaScript host namespace and a missing
`clojure.x` falls back to `cljs.x`, while the session asks its source
provider for both. A `require` reloads with the flag `:reload` or
`:reload-all` anywhere among its arguments (`(require :reload 'lib)` too),
for every library it names, or with the libspec option `[lib :reload true]`
or `[lib :reload-all true]` for that library. A reload compiles the text the
source provider returns at that time; `:reload-all` also loads again every
namespace the library requires, directly or not, each once.
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
by nested compiler and application work, and by the promise jobs that run
after each top-level form (see [Script and REPL
commands](#script-and-repl-commands)). Nested `eval`, callbacks, macro
expansion and promise jobs cannot replenish it. Cancellation, memory exhaustion, interrupts and
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
Bundled libraries need no root; a root's own file for a bundled namespace
(`clojure/string.cljs`, say) wins over it, as under `--compile`
(light-cljs-285). Missing external sources report the namespace
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

Promise jobs run after each top-level form (light-cljs-295), as an event
loop runs them after a script. Once a form of `--eval`, `--repl`,
`eval_string`, `eval_source` or `eval_form` has run, whether or not it threw,
the session runs the pending jobs of its heap until none is left: `.then`
callbacks, `js/queueMicrotask` callbacks and the rest of an `^:async` function
body after an `await` (see [Async functions and
await](compiler.md#async-functions-and-await)), with the jobs they queue in
turn. A later form sees what they did, so `(def r (atom nil)) (.then
(light.lazy/load l) #(reset! r (%)))` followed by `@r` returns the result of
calling the loaded target, and output a reaction prints comes before the
form's printed result. Jobs never run inside a form: a top-level `do` cannot
see its own reactions. Nor do they run between the forms of a namespace the
session loads or of a `load_file` file, each of which runs as one script, so
its jobs run after the form that loaded it (`load_file`'s after its last
form). **Difference:** compiled output runs a whole file as one script, so
there a form does not see the reactions of an earlier form of the same file;
under `--eval` it does. Jobs run in the operation of the form that queued
them, under its deadline, poll budget and memory limit, and each job costs a
poll: a job that loops, an endless chain of jobs or unbounded allocation
ends the operation like any loop, and the session needs `reset` (the CLI
exits). An embedder that drives `evaluate` or `invoke` itself calls
`EvalSession::run_jobs`. A `.then` reaction's throw rejects its promise (see
unhandled rejections below). An
exception that leaves a job, as a `js/queueMicrotask` callback's does, is
the form's error, with its value in `*e` and its `ex-data` in the
diagnostic, once the other jobs have run (the first, when several throw);
a form that failed keeps its own error and `*e`. The engine has no timers (`js/setTimeout` is absent), so
promise jobs are the only work that runs after a form. `await` outside an
`^:async` function body, including a top-level `(await x)`, is the located
analysis error upstream's assertion gives; upstream has no top-level `await`
either.

A promise still rejected with no handler attached once the jobs after a form
have run out is an unhandled rejection (light-cljs-297), reported once with
its printed reason: `unhandled promise rejection: #error {:message "boom",
:data {}}` (`<unprintable reason>` when printing it throws; printing keeps
`*e`). The session's engine tracks rejections (rquickjs
`set_host_promise_rejection_tracker`; the record lives in the heap, never in
Rust) and forgets one when a handler is attached first, so a rejection that a
`.catch`, an `await` or a job of the same drain handles is not reported, while
a handler that a later form attaches comes too late. Under `--repl` each is a
warning on stderr after the form's printed result or error, `light-cljs:
WARNING: unhandled promise rejection: ...`, leaving `*1` and `*e` as they are,
and the REPL goes on, as Node's REPL prints `Uncaught` and a browser logs
`Uncaught (in promise)`. Under `--eval` the first is the script's error, as
an unhandled rejection ends a Node script: its reason becomes the last
exception, the forms after it do not run, and the CLI exits nonzero.
**Difference:** Node checks for unhandled rejections once the whole script
and its jobs have run, so there a handler that a later form of the script
attaches still counts; under `--eval` the check follows each form's jobs. A
form's own error and an exception that leaves a job come first; under
`--eval` the form's rejections are then dropped. An embedder chooses with
`EvalOptions::unhandled_rejections`: `UnhandledRejections::Report`, the
default, keeps the reports of the latest `eval_string`, `eval_source`,
`eval_form`, `load_file` or `run_jobs` for
`EvalSession::take_unhandled_rejections`, and `UnhandledRejections::Fail`
makes the first one the error of the form, or of `run_jobs`, with its
`ex-data` in the diagnostic.

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
| `--elide-asserts` | off | Compile without `assert` and `fn` `:pre`/`:post` checks |
| `--warnings` | off | Warning mode; warnings go to stderr |

`--warnings` analyzes in warning mode, as `--compile --warnings` does
(light-cljs-284): undeclared Vars and conflicting arities are warnings and
the code runs as upstream's would, so a namespace that uses a Var another
file defines later still loads and reloads. Each warning goes to stderr as
the compiler produces it, before its form runs, one line as `--compile
--warnings` prints it but located in its file: `WARNING: <stdin>.cljc:1:13:
Use of undeclared Var cljs.user/g [undeclared-var] (bytes 12..13)`. Package
requires that bind nothing, single-segment `ns` names and `[lib :as js]`
warn too. Without it the analysis is strict, as before, and nothing is
printed.

Numbers are nonnegative integers; zero is a real bound. Startup rejects a heap
limit below its supported minimum. The obsolete `--fuel`, `--allocation-limit`,
`--call-depth` and `--evaluation-depth` flags are rejected with a pointer to the
replacement control. Engine poll/deadline/memory failures, uncaught stack failures or cancellation
terminate the command nonzero; a fresh invocation allocates a fresh session.
Language errors remain recoverable in the REPL. Source input/host stream failures
are fatal. Read, evaluation and result rendering are separate operations; nested
macros/callbacks share their enclosing budget. Blocking host IO is cooperative,
not preempted by an engine deadline.

The CLI is tested by `tests/evaluation/eval_cli.rs` (subprocess runs, Rust only) and
`tests/evaluation/eval_examples.rs`, which runs the scripts in `examples/native`.

## Watch

`light-cljs --watch --entry NS --source-root DIR [--source-root DIR ...]
--out-dir DIR` (light-cljs-285; the design is "The watch loop" in
[dev-workflow.md](dev-workflow.md#the-watch-loop)) writes the
[dev build](compiler.md#dev-builds) of `--compile --entry NS --reloadable
--out-dir DIR` and keeps it current as the sources change. After the build, a
`DevSession` loads the same program (a `require` of the entry through the
source roots), and the watch polls the `.cljs` and `.cljc` files under the
roots, comparing each one's modification time and size; hidden files are
left out. One poll is one step: every file saved since the last poll is in
it.

```sh
light-cljs --watch --entry app.main --source-root src --out-dir dev
# light-cljs: built app.main (4 namespaces) into dev in 281 ms; watching every 100 ms
# light-cljs: recompiled app.view in 4.1 ms
```

A step reads the source of each namespace of the build whose file changed, as
the source provider finds it now (a file saved over a symbolic link is still
its namespace's), and compiles its forms in order against the session's
retained program, read from the first in the namespace
(`DevSession::load_namespace_file`) and in warning mode, then writes the
namespace's unit file and source map
(`dev_build::namespace_unit`: the `define` call of the build's units, whose
function registers the namespace once, the first in load order every
namespace of the build, makes it current and runs each form's statements
under the one prelude) and the manifest. Files are replaced by a rename, so
a reader sees the old text or the new. Namespaces are compiled in load order, so

- a changed `ns` form loads the namespaces it newly requires through the
  source provider, as a `require` does; their units are written too and go
  ahead of their requirer in `manifest.json` and `index.js`;
- a changed namespace that declares macros (or did) recompiles every
  namespace of the build that requires it from its source, since their units
  hold the expansions;
- a form that fails stops the step: its error goes to stderr with file, line
  and column, the namespace keeps its previous unit, and it is compiled
  again, with whatever the step had left, at the next change of any source
  file, so creating a missing required file retries it. A namespace a
  failing step loaded completely is written all the same, since the session
  will not load it again;
- a require that closes a cycle through namespaces already loaded, which the
  session would not notice, fails the step as `namespace dependency cycle:
  app.macros -> app.view -> app.macros`, as `--compile` refuses it.

A rewritten namespace's value is its last form's, or nil when that is a
directive, as in the first build.

A file outside the build changes nothing until a namespace requires it, and
a deleted file is reported and otherwise ignored: its namespace keeps its
unit. Each step prints one line to stderr, `light-cljs: recompiled
app.macros, app.view in 9.8 ms`, the time from the poll that noticed the
change to the files written; a failed one ends `; app.view failed and keeps
its previous unit`, after the error. Warnings print before it as
`--warnings` prints them (`WARNING: <file>:22:22: Use of undeclared Var
app.view/later [undeclared-var] (bytes 653..658)`). A step changes the
manifest's namespaces and requires, and one that succeeds finds the hooks
again in the program it compiled (light-cljs-287): a definition that gains
`^:dev/after-load` is listed, one that loses its flag is not, and a hook
whose definition was deleted from the source stays listed, as the
definition stays defined. Without `--serve`,
nothing is pushed to a running page, and a client applies the rewritten
units itself, as `tests/support/dev_execution.mjs` does; with it, the
[server](#serve) pushes them.

| Flag | Default | Meaning |
| --- | ---: | --- |
| `--entry NS` | required | The entry namespace, as `--compile --entry` |
| `--source-root DIR` | required | A source root, as `--compile`; repeatable |
| `--out-dir DIR` | required | The dev build directory, created when missing |
| `--poll-ms N` | 100 | Milliseconds between polls, at least 1 |
| `--elide-asserts` | off | Compile without `assert` and `fn` `:pre`/`:post` checks |
| `--serve PORT` | off | Serve the build and push each step to connected pages ([Serve](#serve)); 0 picks a free port |
| `--serve-host HOST` | `127.0.0.1` | The address `--serve` listens on |
| `--reload-dependents` | off | With `--serve`, a load also reloads every namespace downstream of those it loads ([Serve](#serve)) |

The watch runs until it is interrupted, and exits nonzero when the first
build fails or a file of the build cannot be written. Startup compiles the program twice,
once for the build and once in the session, which is what the steps compile
against. A save that keeps both a file's size and its modification time,
possible only on a file system with coarse timestamps, is not seen.
`tests/dev_watch.rs` runs `--watch` on a copy of `examples/dev` and applies
the rewritten units to the running first build through the Node driver.

## Serve

`--watch ... --serve PORT [--serve-host HOST] [--reload-dependents]`
(light-cljs-286 and 287; the design is
"The server and the client" in
[dev-workflow.md](dev-workflow.md#the-server-and-the-client)) also serves the
dev build over HTTP and pushes each step to the pages running it:

```sh
light-cljs --watch --entry app.main --source-root src --out-dir dev --serve 9630
# light-cljs: serving dev at http://127.0.0.1:9630/
# light-cljs: built app.main (4 namespaces) into dev in 281 ms; watching every 100 ms
```

Open `http://localhost:9630/`, save `src/app/view.cljs`, and the page logs
`reloading`, the new render and `light-cljs: reloaded app.view in 1 ms` with
its `defonce` state intact; save a file that does not compile, and an overlay
over the page shows the error with its file, line and column until the next
step that succeeds.

The server (`light_cljs::dev_server::DevServer`) is a subset of HTTP/1.1 on
`std::net`, one thread per connection, with JSON written and read by hand:

- `GET` and `HEAD` serve the files of the build directory, then those of the
  source roots, each at the path its source map URL resolves to from the
  build directory (`../src/app/view.cljs` from `/app.view.js.map` is
  `/src/app/view.cljs`), so the browser's devtools show the sources. Each
  response carries a content type (`.js` is `text/javascript`), an ETag
  (`If-None-Match` gets `304 Not Modified`) and `Cache-Control: no-cache`.
  A path with a `.` or `..` segment or an encoded `/` names nothing.
- `GET /` serves the build directory's `index.html`, or when it has none a
  built-in page that shows console output, loads `index.js`, and calls the
  program's value when it is a function, as a host calls an application's
  start function (examples/dev's `start`).
- `GET /events` is a stream of server-sent events, one JSON object with an
  `op` field per event: `hello` with the runtime ID the server gives the
  connection; then the build as of the last step that succeeded (a `load`
  with `replay`) and the diagnostics of the last step, or before any step
  the build's own warnings; then each step's `warnings` and
  `error` (`{"message", "file", "line", "column"}`) and `load` (`units`,
  each `{"namespace", "file", "js", "requires"}` with the text of the unit
  file and the namespaces of the build it requires; the `before-load` and
  `after-load` hooks of the manifest; and the policy a replay is applied
  with: `once` and `always`, the namespaces marked `^:dev/once` and
  `^:dev/always`, and `dependents`, whether `--reload-dependents` is on).
  A step that fails
  pushes its error and no load, and changes no replay; the units it wrote,
  if any (a namespace a changed `ns` form loaded before the error), go with
  the next step that succeeds. A comment line every 15 s finds closed
  connections.
- `POST /reply` takes the JSON reply of a runtime to an `invoke` (a REPL
  form's unit, `DevServer::invoke`, which the connected REPL sends),
  `{"runtime", "id", "value"}` (with `ref`, a reference number, for a value
  that is not finite data) or `{"runtime", "id", "error": {"message",
  "stack", "data"}}`, and hands it to the requester when the runtime named is
  the one asked; a reply naming another gets `409 Conflict` and the request
  still waits, and one from a page of another origin (its `Origin` header)
  gets `403 Forbidden`. Dropping a `DevServer` ends every stream, and its
  clients connect again.
- `POST /repl` is the [connected REPL](#connected-repl)'s endpoint.

A step's `load` holds the units of its reload set (light-cljs-287; "The
reload set" in [dev-workflow.md](dev-workflow.md#the-reload-set)), in load
order:

- every namespace the step wrote: the changed ones, the users of a
  changed macro namespace or of a macro whose namespace requires a changed
  one, and the readers of a changed `light.build` resource (recompiled,
  since their units hold the expansions), the namespaces a changed `ns` form
  loaded, and every namespace whose `ns` form is marked `^:dev/always` (`(ns
  ^:dev/always app.config)`) with its compilation consumers, which every
  step recompiles (light-cljs-325; [dev-workflow.md](dev-workflow.md#the-reload-set)),
  with those of failed steps since the last that succeeded;
- with `--reload-dependents`, every namespace that requires one of those,
  directly or through others the load reloads, unchanged, so that top-level
  effects that read them run again, as shadow-cljs's `:full` reload strategy
  does (a save of `app.state` in examples/dev loads `app.state`, `app.view`
  and `app.main`; without the flag, `app.state` alone, since a call reads a
  definition when it runs);
- never a namespace marked `^:dev/once` that the pages have run: a step
  recompiles it and rewrites its unit, for a page loaded later, but loads
  nothing of it, and it passes no change on downstream: a namespace that
  depends on the change only through it is not reloaded. One that a step
  adds to the build loads with that step, since no page has it.

A step that wrote nothing pushes no load and runs no hooks; one that wrote
only `^:dev/once` namespaces still loads the `^:dev/always` ones, as
shadow-cljs does after every build. The hooks of a load are those the step
found in the program it compiled, resolved in the page when they run: a new
before-load function is not defined yet when the before-load functions of
its own load run.

`client.js` of every dev build (src/dev_client.js) is the client. In a page
loaded over HTTP it connects to the server it came from once the build has
loaded; elsewhere `globalThis.__lightDev.connect(url, {log, onHandled})`
connects it, which is how Node runs it (the driver's `--connect`). It reads
the stream with `fetch`, so the same code runs in a page and in Node, and

- applies a `load` with the hook protocol: the before-load functions in
  reverse definition order, each unit evaluated with a `sourceURL` naming its
  file (so its `sourceMappingURL` resolves), then the after-load functions;
  an `-async` hook gets a continuation. A hook that throws is logged and
  skipped; a unit that throws stops the load, is logged and shown in the
  overlay, and later units stay unloaded. One console line per load names
  the namespaces;
- applies of a `replay` only the units whose text it does not run already
  (the text of the function `define` recorded is in the unit's, and the unit
  ran to its end), so a page that loaded the current files changes nothing,
  one that missed steps, or reconnects to a server that rebuilt, catches up,
  and a unit that threw runs again; of a `once` namespace only a unit it has
  not run to its end, and, when it applies any unit, those the missed steps
  would have loaded with it: the units of the `always` namespaces and, with
  `dependents`, of the namespaces downstream along the units' `requires`;
- draws the `warnings` and `error` of a step in an overlay with file, line
  and column (and logs them), which the first message of the next step
  clears;
- evaluates an `invoke` in its namespace and posts the value printed by
  `pr-str` under the print options sent (`readably`, `length`, `level`,
  `meta`, `namespace-maps`, `dup`), or the error, with the request ID; one
  from a REPL session first runs its `setup` script (what the form loaded),
  keeps the session's `*1`, `*2`, `*3` and `*e`, and gives a value that is
  not finite data a reference number, and a `run` message runs the script of
  what a REPL form sent to another runtime loaded;
- connects again when the stream ends, after 250 ms doubling up to 30 s, and
  stops when the server has no `/events` (a 404, as from a static server).

The server listens on `127.0.0.1` unless `--serve-host` says otherwise:
`--serve-host 0.0.0.0` opens it to other machines, such as a phone, and since
a connected page runs what the server sends it, only on a trusted network.
While on loopback, it answers only requests whose `Host` is a loopback name
(`localhost`, `127.0.0.1`, `[::1]`), so a page of another site cannot reach
it through a DNS name that resolves to this machine; it sends no CORS
headers, so the page must come from the server itself.

### JavaScript packages in dev

The compiler never resolves packages ([JavaScript
packages](compiler.md#javascript-packages)), and a dev build reads them where
every build does, from `globalThis.lightCljsPackages`, at each use. A dev page
provides them before `index.js` runs, for example from a vendor module the
developer builds once with any tool and keeps in the build directory, which
the server serves:

```html
<!-- dev/index.html, which wins over the built-in page -->
<script type="module">
  import * as react from "./vendor/react.js";
  globalThis.lightCljsPackages = { react };
  const index = Object.assign(document.createElement("script"), { src: "index.js" });
  index.onload = () => globalThis.__lightDevLoaded.then(start => start());
  document.head.append(index);
</script>
```

A module script runs after the classic scripts of the page, so a classic
`<script src="index.js">` next to it could run units before the packages are
set; the page adds `index.js` from the module instead, as a classic script,
once they are. Replacing a package object later is seen by code compiled
earlier. The server serves the directories it is given and does not look in
`node_modules`.

`tests/dev_server.rs` runs `--watch --serve 0` on a copy of `examples/dev`
with Node runtimes: the driver with `--connect`, whose build's `client.js`
reads the stream. It also runs the reload fixtures
(tests/support/reload_fixtures.rs: functions, `defonce`, records, protocols,
multimethods, macros and `ns` changes, each an edit, the namespaces it
reloads and what the program prints), whose output the Phase 0 driver gives
the same from the units the step rewrote and from a fresh build of the
edited sources (tests/compiler/dev_build.rs).

## Connected REPL

`light-cljs --repl --connect HOST:PORT [--print-length N] [--print-level N]`
(light-cljs-288; the design is "The connected REPL" in
[dev-workflow.md](dev-workflow.md#the-connected-repl)) is a REPL whose forms
run in the pages and Node runtimes of a `--watch --serve` dev server:

```sh
light-cljs --watch --entry app.main --source-root src --out-dir dev --serve 9630
# in another terminal, with the page open:
light-cljs --repl --connect localhost:9630
app.main=> (swap! app.state/state update :clicks inc)
{:clicks 2, :card #app.view.Card{:label "clicks", :clicks 1}}
```

The client reads forms from stdin as `--repl` does (prompts and `...=>`
continuation prompts on stderr at a terminal, several forms on a line, EOF
ends it) and posts the text it has read up to the end of its last complete
form, with its line and column, to the server. The server reads the forms
there as the embedded REPL does (a prefix over a reader conditional that
selects nothing takes the next form; discarded forms are checked) and
compiles each with the watch's `DevSession`, so REPL definitions and watch
steps share one program: a definition made at the
REPL is seen by a namespace a save reloads, and a definition a reload makes
is seen at the REPL. Each connection has its own current namespace, the
entry namespace to begin with, which the prompt shows; `in-ns`, `ns` and
`require` work as under `--repl`. The runtime runs the form's unit in that
namespace and prints the value with `pr-str` under the print options the
client sends (`--print-length` 100 and `--print-level` 20 unless given, as
`--repl`), and the client prints that text on stdout. `--repl --connect`
takes no other evaluation option, since nothing runs in the client.

- **History.** The runtime keeps each session's `*1`, `*2`, `*3` and `*e`:
  a value rotates them, a runtime error sets `*e`. **Difference:** a read or
  analysis error leaves `*e` as it is, since it never reaches the runtime.
- **References.** A value that is not finite data (a function, an atom, a
  host object, a sequence the print length cut short; anything but nil,
  booleans, numbers, strings, keywords, symbols, UUIDs, instants, regexes,
  tagged literals and collections of them printed whole) also comes back as
  a reference number, printed on stderr as `;; ref 3`. The runtime keeps the
  newest 1,000: `(light.dev/ref 3)` returns the value there, and
  `(light.dev/inspect 3)` describes it as data, `{:ref 3, :type "Atom",
  :deref 1}`, with a function's `:name` and `:arity`, a counted value's
  `:count` and a plain object's `:keys`. The dev client defines these
  `light.dev` functions in every runtime, and the watch declares them, so
  they need no `require`.
- **Errors.** A form that does not read is reported by the client, which
  discards the rest of the text it has read, as `--repl` does: `light-cljs:
  <stdin>.cljc:4:1: unexpected closing delimiter )`. Analysis errors and
  warnings come from the server, located in the input: `WARNING:
  <stdin>.cljc:5:2: Use of undeclared Var app.main/f [undeclared-var]`,
  since dev compilation is in warning mode. A runtime error comes back from
  the runtime with its stack, whose locations the server maps through the
  source maps to the input (a REPL form, or one that defined a function a
  later form called) and to the sources of the build's units, leaving out
  the frames of the client that ran the form; an `ex-info`'s data follows:

  ```text
  light-cljs: <stdin>.cljc:1:1: Error: kaboom
      at eval (/work/src/app/view.cljs:25:10)
      at eval (<stdin>.cljc:1:1)
  ```
- **Runtimes.** A form goes to the most recently connected runtime.
  `(light.dev/runtimes)` lists the connected ones, `[{:id 1, :agent
  "Mozilla/5.0 ...", :selected true}]`, and `(light.dev/select! 2)` sends
  the session's forms to runtime 2 until `(light.dev/select! nil)`; a chosen
  runtime that disconnects (a reloaded page connects as a new runtime) is an
  error until the session chooses again. Both are commands the server
  answers, as forms of their own; called anywhere else they throw. What a
  form loads (a `require` of a namespace the session has not loaded) goes to
  every runtime: with the form to its runtime, and as a `run` message to the
  others, also when a later part of the form fails, since the session then
  counts it as loaded. A namespace first loaded at the REPL is not part of the build: the
  watch does not follow its file, a page loaded later does not have it, and
  its load runs no reload hooks.
- **Output.** What the form prints (`println`, `js/console.log`) goes to
  the runtime's console, not to the terminal.

The protocol is `POST /repl` with a JSON object, sent as
`application/json`: `{"op": "open"}` answers `{"session", "namespace"}`;
`{"op": "eval", "session", "code", "file", "line", "column", "print"}`
answers `{"namespace", "results"}`, a result per form of `code` (a form that
fails to compile or to run leaves the next to run, as under `--repl`; one
that does not read ends them), each `{"warnings", "value", "ref"}` or `{"warnings",
"error": {"phase", "diagnostic", "stack", "data"}}` with `phase` one of
`compile`, `command` and `runtime`; `{"op": "close", "session"}` forgets the
session, and a `forget` message makes the runtimes drop its history. A
session ID belongs to one server process: a session the server does not know
(after a restart) gets `404`, and the client opens a new one and says so. The server compiles each form on
the watch's thread between polls and never waits there for a runtime; the
connection's thread sends the unit and waits for the reply. Since a
connected runtime runs what the endpoint sends, a request that carries an
`Origin` header (a page's) or another content type than JSON (which a page
cannot send to another origin without the server's consent) is refused, as
well as, on loopback, a request for a non-loopback `Host`.

`(+ 1 2)` takes 0.7-1.0 ms from the line typed to the value printed (median
0.8 ms over 50, release CLI, headless Chrome running examples/dev, load 2.5),
and 1.1-3.3 ms (median 1.4 ms over 20) in `tests/dev_repl.rs`, debug CLI with
a Node runtime. `tests/dev_repl.rs` runs `--watch --serve 0` on a copy of
`examples/dev`, Node runtimes (the driver with `--connect`) and the client:
round trips, history and the application's state; references; a REPL
definition seen by a reloaded namespace and a reloaded definition seen at
the REPL; errors located in the input and in a reloaded namespace's source,
`*e`, analysis errors and warnings; two runtimes, selection, loads reaching
both and a namespace per connection; and the endpoint refusing what a page
could send.
