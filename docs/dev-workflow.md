# Development workflow design

Give light-cljs the edit-save-reload loop and the connected REPL that
ClojureScript developers expect from shadow-cljs and figwheel: a watch
command that recompiles a changed namespace in milliseconds and replaces its
definitions in the running browser or Node application without losing state,
and a REPL whose forms evaluate in that same application, from the terminal
and from an editor. No bundler, no JVM, no new crate dependency.

The design builds on two things the project already has. The embedded
session (`--repl`, `--eval`, `EvalSession`) compiles one form at a time
against a retained program and stores every definition in a registry that
later code reads at each use, which is exactly the storage model reloading
needs. And the compiler is fast enough that a watch loop can start by
recompiling whole namespaces rather than caching anything. What is missing
is packaging (compiled applications seal their definitions inside one
closure), a transport (the session evaluates synchronously inside QuickJS),
and the policy layer (what to reload after a change, and when to run
lifecycle hooks).

The plan is incremental: Phase 0 produces a reloadable build that can be
exercised by hand and from Node, Phase 1 separates the incremental compiler
from the QuickJS heap, Phase 2 adds the watch loop, Phase 3 the server,
client and reload policy, and Phase 4 the connected REPL and the editor
protocol. Each phase has a measurable exit criterion.

## Goal

A developer runs

```sh
light-cljs --watch --entry app.main --source-root src --out-dir dev --serve 9630
```

opens `http://localhost:9630/`, edits `src/app/view.cljs`, saves, and sees
the page update with its application state intact. In another terminal,

```sh
light-cljs --repl --connect localhost:9630
app.main=> (swap! state assoc :count 0)
```

evaluates in the page and prints the result. The editor's nREPL client
connects to the same port.

Targets, measured on the example application of Phase 0 and recorded here
as phases land:

| Measurement | Today | Target |
|---|---|---|
| Save to updated page, one changed namespace of the example | no path | under 100 ms on the development machine, including the recompile |
| Application state across a reload | n/a | `defonce` atoms keep their value; `def` values are reinitialized, as upstream |
| Observable behavior of the dev build | n/a | prints what the `none` build prints, for the same program, before any reload |
| Recompile of one changed namespace of the example | whole program in 15-250 ms depending on size | the one namespace, under 20 ms |
| REPL round trip for `(+ 1 2)` in the page | n/a | under 20 ms, dominated by the compile |
| Dev build bytes served after the first load | n/a | the changed namespace's unit only; the runtime is served once and cached |

Non-goals of this design: a bundler, npm resolution (packages stay
host-supplied, see [JavaScript packages](compiler.md#javascript-packages)),
a cache of compiled namespaces on disk, hot reloading of CSS and other
assets, a browser-resident compiler, and reloading the runtime itself.
"Phase 5: deferred" lists what is left open on purpose.

## What shadow-cljs does

Citations are into `research/shadow-cljs/src/main/shadow/`.

- **Dev output is the reloadable format.** A dev build is `:optimizations
  :none` with source maps (`build.clj:392-400`): one JavaScript file per
  namespace, each definition assigned to a namespace object under the global
  `$CLJS`, and a loader that records which files are loaded. Release builds
  are a different artifact. `defonce` compiles to an `exists?` check in dev
  and to a plain `def` in release (`build/cljs_hacks.cljc:752-760`).
- **The compiler never evaluates.** The REPL compiles a form against the
  watch build's analyzer state and returns an action, `:repl/invoke` with the
  JavaScript and a source map (`cljs/repl.clj:508-575`). A REPL `require`
  resolves the namespace into the build, compiles and flushes it, resets it
  first on `:reload`, and returns a `:repl/require` action naming the sources
  to load (`:242-335`). The runtime interprets the actions: it asks the
  worker to compile, evaluates the JavaScript with `goog.globalEval`, prints
  locally and sends back object references for the results
  (`cljs/devtools/client/shared.cljs:273-300`,
  `remote/runtime/eval_support.cljs:15-65`).
- **Invalidation reaches dependents.** A changed source resets itself and
  every resource affected through requires or used Vars; a changed macro
  namespace resets every source that requires it (`build/api.clj:374-447`,
  `cljs/devtools/server/worker/impl.clj:350-400`). Macro tracking is
  deliberately coarse: "prefer to do too much over too little"
  (`build/macros.clj:175-260`). The analysis cache is keyed on dependency
  keys, compiler options and macro file timestamps
  (`build/compiler.clj:944-1000`).
- **Reload policy is metadata.** `^:dev/after-load`, `^:dev/before-load`,
  their `-async` variants, `^:dev/once` (never reload) and `^:dev/always`
  (always reload) are read from analyzer metadata at build time
  (`cljs/devtools/server/worker/impl.clj:233-300`). The client runs
  before-load hooks in reverse order, loads the code, then after-load hooks
  in order (`cljs/devtools/client/env.cljs:222-252`). The default strategy
  reloads the recompiled sources; `:reload-strategy :full` also reloads
  everything downstream (`:325-360`). Each loaded file gets a `sourceURL`
  comment and marks itself loaded (`cljs/devtools/client/browser.cljs:21-37`).
- **Warnings, not errors.** Undeclared Vars and arity conflicts are
  warnings shown in a heads-up display; a build with warnings still reloads
  unless configured otherwise (`cljs/devtools/client/browser.cljs:69-95`).

## Where light-cljs stands

- **Two front doors to the compiler.** `analyze_namespace_entry_with_provider`
  (`src/compiler.rs:1290`) walks the entry's requires depth first
  (`Loader::visit`, `src/compiler.rs:949-1134`), analyzes the sources into
  one `Program` and discards the require graph it built
  (`NamespaceSource`, `:117`). `InteractiveCompiler`
  (`src/eval_compiler.rs:12-27`) analyzes one form at a time against a
  retained program, keeping aliases, refers and resolution per namespace, and
  `program_fragment::append` (`src/program_fragment.rs:92`) appends each
  fragment with stable global IDs, so a redefinition reuses the `GlobalId`
  of the first definition.
- **Two storage models for definitions.** `emit_js_mode`
  (`src/emitter.rs:160`) writes a compiled application as one closure with
  `let g<n>` for every global (`:249-274`); nothing outside the closure can
  read or assign them, at every level. `emit_interactive_unit`
  (`src/emitter.rs:5010`) writes a form as a function of the runtime whose
  globals are `globalThis.__lightApplicationRoots["ns/name"]`, read at every
  use and call (`:1939-1947`). Below `advanced` every call of a program
  function is a late lookup followed by `.call`, so a redefinition is seen by
  every caller the next time it runs.
- **Reloading already exists in the session.** `require` with `:reload`
  and `:reload-all` (`src/source_provider.rs:5`, `src/eval_session.rs:363`,
  `:1006`) re-reads a namespace through the source provider and re-runs
  every form; `load_file` (`src/eval_session.rs:292`) does the same for a
  path. `defonce` keeps existing roots including nil and false. Record,
  type, protocol and multimethod redefinition behave as upstream's and are
  covered by the SCI reference rows (`tests/fixtures/sci-outcomes.tsv:2030`,
  `:2428-2429`, `:2460`). No Rust test exercises `:reload` itself; the
  fixture rows that did are retired or marked as gaps (`:1463-1487`).
- **The session is bound to the QuickJS heap.** `EvalSession`
  (`src/eval_session.rs:197`) evaluates each unit synchronously
  (`evaluate`, `:450`), keeps the current namespace in sync through a JS
  function (`namespace_update`, `:198`), returns value handles, and serves
  runtime `require`, `load-file` and `eval` by calling back into the
  compiler during execution (`callback_source`, `:964`). Nothing in the
  crate speaks to another process: the only dependency is `rquickjs`.
- **The session's `ns` is narrower than the compiler's.** `directive_inner`
  (`src/eval_compiler.rs:584`) accepts `:require` with `:as` and `:refer`
  and `:refer-clojure :exclude`; other clauses are rejected (`:665`, `:716`).
  `ns*` already goes through the compiler's full analysis
  (`session_nsstar`, `:551`). The session uses strict analysis;
  `AnalysisOptions::warning_mode` (`src/analyzer.rs:2624`) is not exposed to
  `--repl`.
- **What the IR already records.** Every global carries its namespace, name
  and definition metadata (`GlobalBinding`, `src/analyzer.rs:2241-2256`),
  and the macro host records per calling namespace which namespaces it
  imports and which names it refers (`MacroImport`, `src/macro_host.rs:10`;
  `set_interactive_imports`, `:407`). Namespaces register their reader
  context when units run, so `find-ns`, `ns-publics` and `resolve` work in
  the session.
- **Source maps** exist for compiled output and `--esm` at every level
  (`emit_js_mode`'s `mapped`), not for interactive units.
- **The compiler runs in wasm** in the browser playground
  (`playground/crate/src/lib.rs`) with an in-memory source provider.

## Design principles

Continuing the principles of advanced-optimization.md and
code-splitting.md:

14. **The session's storage model is the dev build's.** A dev build is the
    program emitted as interactive units: the same registry, the same unit
    shape, the same runtime instance. The REPL and the running application
    therefore share definitions by construction, and the session's tested
    redefinition semantics are the dev build's semantics.
15. **The compiler produces actions; runtimes interpret them.** Compiling a
    form or a file yields a list of actions (invoke this unit, load these
    units, the current namespace is now X, these warnings, this error).
    QuickJS, a browser tab and a Node process are interchangeable
    interpreters. The compiler never waits on a runtime.
16. **Recompile, do not cache.** A watch step recompiles the changed
    namespace's forms against the retained program. There is no on-disk
    cache and no incremental analysis inside a namespace; the compile is
    fast enough, and the retained program is the only state.
17. **Reload policy is explicit and small.** What gets reloaded after a
    change is a function of the require graph, the macro imports and
    metadata on namespaces and definitions. The defaults match shadow-cljs so
    existing habits transfer.
18. **No dependencies for the plumbing.** File watching is polling, the
    server is a hand-written HTTP/1.1 subset over `std::net`, pushes are
    server-sent events, and replies are plain POSTs. A WebSocket or a
    watcher crate is a decision to take later with a reason, not a default.
19. **Warn in dev, refuse in builds.** Dev compilation uses warning mode so
    the order in which files are saved never blocks a reload; `--compile`
    keeps its strict contract.

## The model

### The dev build

`--compile --entry NS --source-root DIR --out-dir DIR --reloadable` writes a
directory instead of one expression:

```text
dev/
  runtime.js        the whole runtime, once: globalThis.__lightRuntime = (factory)();
  client.js         the dev client (Phase 3); a stub before that
  app.main.js       one unit per namespace, in dependency order
  app.view.js
  ...
  manifest.json     namespace order, require graph, macro declarers, file per namespace
  index.js          loads runtime.js, client.js and the units in manifest order
```

A unit file is the interactive unit wrapped in a registration call:

```js
globalThis.__lightDev.define("app.view", ["cljs.core", "app.state"],
  (__lightInteractiveRuntime) => { globalThis.__lightApplicationRoots ??= Object.create(null); /* forms */ return value; });
//# sourceMappingURL=app.view.js.map
```

`define` records the unit by namespace and runs it against
`globalThis.__lightRuntime` immediately. The same text serves the first load
(as a script file) and every reload (evaluated by the client with a
`sourceURL`), so there is one unit writer. `index.js` is a few lines that
insert script elements in manifest order and resolve a promise when the last
has run; in Node, the driver requires the files in that order.

The dev build is emitted at `none` semantics with the full runtime and with
namespace registration on, like the session: no definition removal, no
specialization, no slicing, no terse names. It never enters
`prepare_application`. Each unit registers its namespace's reader context
and sets the current namespace for the duration of its top-level forms, as
compiled output does.

Definitions live in `globalThis.__lightApplicationRoots` keyed by
`"ns/name"`. This is the session's registry today; keeping it means the
session's tests cover the dev build's storage. Nested namespace objects
were considered and rejected (see "Settled questions").

### Actions

A `DevSession` (the name is provisional) owns an `InteractiveCompiler` and
its macro host and no application heap. Its methods take a form or a source
and return actions:

| Action | Carries | Meaning |
|---|---|---|
| `Invoke` | namespace, JavaScript, source map | Evaluate this unit for its value: a REPL form. |
| `Load` | units in dependency order, each with namespace, JavaScript, source map, reload policy | Define and run these namespace units: the result of `require`, `load-file`, a watch step. |
| `Namespace` | name | The current namespace changed. |
| `Warnings` | located warnings | Analysis warnings for the input. |
| `Error` | located diagnostic | The input did not compile; nothing was emitted for it. |

`EvalSession` becomes one interpreter: it evaluates `Invoke` and `Load`
units in its QuickJS heap, applies `Namespace` to `*ns*`, and keeps its
public API, value handles and `*1`/`*2`/`*3`/`*e` exactly as today. The
runtime callbacks for `require` and `load-file` inside QuickJS keep working
through the same `DevSession` because the heap is in-process; a browser
runtime does not get runtime `eval` or `load-file` (see the contract).

### The watch loop

`--watch` holds the `DevSession` after the initial dev build and polls the
source roots' files (the manifest's file-per-namespace map plus a directory
scan for new files) for modification time changes. Debounced by one poll
interval, a change to `src/app/view.cljs`:

1. Reads the file, reads its forms, and compiles them in order against the
   retained program as fragments, in namespace `app.view`, with warning
   mode on. A new definition gets a new global ID; a redefinition reuses
   its ID; a form that fails stops the step with an `Error` action and no
   unit, leaving the previous unit and the page as they were.
2. Writes `dev/app.view.js` and its map, and updates the manifest.
3. Computes the reload set (below) and pushes a `Load` action to connected
   runtimes, or just writes the files when there is no server.

A change to the `ns` form (a new require) is a change like any other: the
directive loads the new dependency through the source provider, which
appends its unit to the manifest ahead of the requiring namespace. A new
file is picked up when a namespace requires it, as in the session.

### The reload set

Given the changed namespace N:

- N itself, always, with its new unit.
- Every namespace whose require set contains a namespace that declares
  macros and changed, recompiled from source and reloaded, in topological
  order. Expanded code is baked into units, so a macro change must reach
  its users. The macro host knows which namespaces each namespace imports
  (`MacroImport`), and the analyzer knows which namespaces declare macros;
  the over-approximation (every requirer of a macro-declaring namespace,
  whether or not it used a macro) matches shadow-cljs.
- Every namespace marked `^:dev/always`, reloaded without recompiling.
- Never a namespace marked `^:dev/once`, and never `runtime.js`.
- Downstream namespaces that only *read* N's definitions are not reloaded
  by default: their calls look the definition up at call time. With
  `--reload-dependents`, every namespace downstream of N in the require
  graph is reloaded too, in topological order and without recompiling, as
  shadow-cljs's `:full` strategy does, for programs whose dependents have
  top-level effects that must run again.

The client applies a `Load` by running every `^:dev/before-load` function
in the build in reverse definition order, defining the units in order, then
running every `^:dev/after-load` function in order; `-async` variants take
a continuation. Hook functions are found by reading definition metadata from
the retained program when the build is made and after each step, and
resolved at run time through the registry. A failing unit stops the load,
reports the error with its location, and leaves later units unloaded.

### The server and the client

`--serve PORT` starts an HTTP/1.1 server on the loopback interface
(`--serve-host 0.0.0.0` opens it to the network, for phones and other
machines; the server can evaluate code in the page, so loopback is the
default) in the watch process: it serves `dev/` and the source roots (for
source maps), answers `GET /events` with a server-sent-events stream, and
accepts `POST /reply` with JSON bodies. Messages are JSON objects with an
`op` field. The server keeps the last `Load` so a late-connecting tab can
catch up, and assigns each connection a runtime ID. When the out dir has no
`index.html`, `GET /` serves a built-in page that loads `index.js` and
shows console output and the overlay, so a first run needs no files besides
the sources; an `index.html` in the out dir wins over it.

`client.js` is a runtime unit kept in `src/` and emitted into dev builds.
It opens the event stream, applies `Load` actions, evaluates `Invoke`
actions and posts their printed result (`pr-str` with the session's print
options) or error back with the request ID, and draws an overlay for
compile errors and warnings with file, line and column, cleared on the next
successful step. It keeps `define` and the hooks runner; nothing else in
the dev build depends on the server.

### The connected REPL

`--repl --connect HOST:PORT` is a thin client: it reads forms as today,
posts their text to the server, and prints what comes back. The server
compiles with its `DevSession`, so REPL definitions and watch recompiles
share one program and one current namespace per connection. Printing
happens in the runtime; values that are not finite data come back as
references the server can ask about later. Reader errors are local; analysis
errors come from the server; runtime errors come from the page with a
mapped stack.

The editor path is nREPL on the same port (`--nrepl PORT` when a separate
port is wanted): bencode framing and the operations `clone`, `close`,
`describe`, `eval`, `load-file`, `interrupt`, `completions` and `lookup`.
Completions and lookup are answered from the retained program's globals and
their metadata (arglists, docstrings), without a runtime round trip.

### JavaScript packages in dev

The compiler never resolves packages. A dev page provides them before
`runtime.js` runs, for example from a vendor module the developer builds
once with any tool:

```html
<script type="module">
  import * as react from "./vendor/react.js";
  globalThis.lightCljsPackages = { react };
</script>
<script src="dev/index.js"></script>
```

Package references read the registry at each use, so replacing a package
object is seen by code compiled earlier. The dev server serves whatever
directories it is given; it does not look for `node_modules`.

## Phases

### Phase 0: reloadable build, example and driver

Scope: the dev build directory of "The dev build", exercised from Node and
by hand, with no watch loop and no server.

- **Emitter.** A `reloadable` emission that writes the runtime factory
  once as `globalThis.__lightRuntime`, then one unit per namespace by
  wrapping `emit_interactive_unit` of the namespace's forms in the `define`
  call, each with a source map (enable the `mapped` path for units), and
  the manifest and `index.js`. Units are emitted from the whole-program
  `Program` by namespace entry (`namespace_entries`), not by re-analyzing
  forms one at a time, so the initial build is one compilation.
- **Loader.** Return the require graph from `analyze_namespace_entry_with_provider`
  (`namespace_requires` by source index, which code-splitting Phase 1 also
  wants) and which namespaces declare macros.
- **CLI.** `--reloadable --out-dir DIR` after `--compile --entry`.
- **Example.** `examples/dev`: `app.state` (a `defonce` atom), `app.macros`
  (a macro and a helper), `app.view` (a record, a protocol extended to it,
  a function using the macro, a function that renders state to a string
  through `console.log`), `app.main` (`^:dev/after-load` and
  `^:dev/before-load` functions, a `start` function that logs and
  schedules nothing). Each file under 40 lines. Works in a page and in Node
  without a DOM.
- **Driver.** `tests/support/dev_execution.mjs`: given the directory,
  loads the files in manifest order, runs `start`, then applies the units
  passed on the command line (new unit texts) with the hook protocol, and
  prints the shared outcome protocol (`tests/support/execution.js`). The
  same driver later tests `Load` actions.
- **Tests.** `tests/compiler/dev_build.rs`: the dev build of each phase
  oracle and ratchet fixture prints what the `none` build prints; the
  example prints its expected lines; applying a hand-edited unit of
  `app.view` (a changed function) changes the next render while the
  `defonce` counter keeps its value; applying a unit with a changed record
  definition keeps old instances usable.

Done when: the example runs as a dev build in Node with the same output as
`--optimize none`, and a replaced unit changes behavior with state intact.

### Phase 1: the compiler as an action producer

Scope: `DevSession` with the actions of "Actions", `EvalSession` and the
CLI's `--repl` and `--eval` rebuilt on it, and the session gaps closed.

- **Factoring.** Move form reading, directives, fragment analysis, unit
  emission and the loaded/loading namespace sets out of `EvalSession` into
  `DevSession`. `EvalSession` keeps the heap, value handles, printing,
  bindings, the input provider and the runtime callbacks, and interprets
  actions. `AnalyzedForm` becomes an `Invoke` action with its program.
- **Full `ns`.** Route `ns` through the compiler's namespace analysis the
  way `ns*` is routed, so `:import`, `:use`, `:rename`, `:require-global`,
  `:as-alias` and string libspecs behave as in `--compile`. Accept
  `[lib :reload true]` and the flag-first spelling. Reinstate the retired
  SCI reload rows against `.cljs` sources.
- **Warning mode.** `DevSession` runs with `warning_mode` on by default;
  `--repl --warnings` and `--eval --warnings` expose it; warnings are an
  action, printed to stderr by the CLI.
- **Source maps.** Every `Invoke` and `Load` unit carries a source map;
  the QuickJS session ignores it.
- **Tests.** `tests/evaluation/dev_session.rs`: action sequences for a
  form, a `require`, a `require :reload` whose source changed between
  loads, a `load-file`, an `ns` with every clause, and a failing form after
  a successful one. The existing `eval_sessions`, `eval_cli`,
  `eval_nsstar` and SCI reference suites pass unchanged.

Done when: `--repl` and `--eval` behave as before on the full suite, and a
`DevSession` without a heap produces the units the Phase 0 driver can apply.

### Phase 2: watch

Scope: `--watch` of "The watch loop", writing files, no server.

- **Polling.** A loop over the manifest's files and the source roots'
  directories at a fixed interval (default 100 ms), comparing modification
  times and sizes, debounced to one step per interval. Deleted files are
  reported and otherwise ignored.
- **Recompile.** The step of "The watch loop": forms compiled as fragments
  against the retained program in the file's namespace; a changed `ns` form
  loads new dependencies and reorders the manifest; a changed
  macro-declaring namespace recompiles its requirers from source.
- **Reporting.** One line per step on stderr with the namespaces
  recompiled and the time taken; errors with file, line and column.
- **Tests.** `tests/dev_watch.rs` (a new test binary, subprocess based like
  `eval_cli`): start `--watch` on a copy of the example, edit a file,
  observe the rewritten unit and manifest, feed them to the Phase 0 driver;
  an edit that fails to compile leaves the previous unit; a macro change
  rewrites the user's unit; a new require adds a unit.

Done when: a saved change to `app.view` in the example produces a correct
unit on disk in under 20 ms after the poll detects it, measured in the test
and recorded in the Goal table.

### Phase 3: server, client and reload policy

Scope: `--serve`, `client.js`, the overlay, and the reload set and hooks of
"The reload set".

- **Server.** HTTP/1.1 GET for files with content types and ETags, the
  event stream, the reply endpoint, JSON encoding and decoding without a
  crate (the source map writer already emits JSON), the built-in index page
  when the out dir has none. Loopback by default, `--serve-host` to open.
- **Client.** The event stream, `Load` application with hooks, `Invoke`
  evaluation and replies, the overlay, reconnection with backoff, and a
  console line per reload naming the namespaces.
- **Policy.** Hook discovery from definition metadata in the retained
  program; `^:dev/once` and `^:dev/always` on namespaces; the reload set
  computation; topological order from the require graph;
  `--reload-dependents` for the downstream strategy.
- **Tests.** `tests/dev_server.rs`: a Node client (a small script using
  `fetch` to read the event stream) connects, receives the initial `Load`,
  the test edits a file, the client receives the next `Load`, applies it
  with the driver's hook protocol and reports the outcome; a compile error
  produces an `Error` action and no `Load`; hooks run in the documented
  order; a macro change loads the user's unit.

Done when: the Goal row "save to updated page" is measured under 100 ms on
the example in a real browser, by hand, and the Node test passes.

### Phase 4: connected REPL and nREPL

Scope: "The connected REPL".

- **Terminal client.** `--repl --connect HOST:PORT`: reading as today,
  compile and evaluation remote, results printed by the runtime, `*1`,
  `*2`, `*3` and `*e` kept in the runtime.
- **Runtime selection.** The server picks the most recently connected
  runtime for `Invoke`; `(light.dev/runtimes)` and
  `(light.dev/select! id)` from the REPL choose another. `Load` goes to
  every runtime.
- **nREPL.** Bencode, sessions, the operations listed in "The connected
  REPL", `completions` and `lookup` from the retained program, `interrupt`
  as cancellation of the pending runtime reply. Verified against Calva,
  CIDER and Conjure by hand; a Node bencode client in the test.
- **Tests.** `tests/dev_repl.rs`: round trips through a Node runtime,
  including a definition at the REPL being seen by a reloaded namespace and
  the reverse; nREPL operation coverage with the Node client.

Done when: an editor connected over nREPL evaluates in the page and a watch
reload keeps the REPL-defined Vars.

### Phase 5: deferred

- **A Node runtime client**, the client without the overlay, for server
  applications; the driver of Phase 0 is most of it.
- **The compiler in the page** through the wasm build, for a serverless
  playground-style REPL; the action model makes it a second interpreter.
- **A WebSocket transport** if server-sent events plus POST turn out to be
  limiting (binary payloads, proxies).
- **Asset reload** (CSS) and a HUD with build progress.
- **Reloading a changed `js_runtime.js`** during runtime development: a
  page reload is fine for that.
- **Multiple watch builds** in one process, and a configuration file: the
  CLI flags stay the only configuration until a second build is needed.

## Dev-mode contract

The deliberate differences between a dev build (and the session) and a
`--compile` build of the same program:

- Definitions are registry properties read at each use, so a redefinition
  is seen by every caller the next time it runs; functions saved in data
  structures keep their old code, as upstream. `set!`, `binding` and
  `with-redefs` are unchanged.
- Reloading a namespace re-runs all of its top-level forms. `def` values
  are reinitialized, `defonce` values are kept (nil and false included),
  top-level effects run again. A definition removed from the source stays
  defined until the page is reloaded, as upstream.
- Redefining a record or type creates a new constructor; old instances
  keep their methods and saved factories follow the new Var. Redefining a
  protocol keeps existing extensions usable. `defmulti` keeps its existing
  value. These are the session's semantics today.
- Namespaces that use a changed macro namespace are recompiled and reloaded
  whether or not they used a macro from it. Nothing else downstream is
  reloaded by default; `--reload-dependents` reloads every downstream
  namespace in topological order.
- Analysis uses warning mode: undeclared Vars and arity conflicts are
  warnings, and the code runs as upstream's would.
- Reflection is always available: namespaces register themselves as below
  `advanced`.
- Runtime `eval` and `load-file` work only where the compiler is in
  process (QuickJS); in a browser or Node runtime they throw with a message
  naming the REPL as the alternative. `read-string` works everywhere.
- Output is never terse or sliced; dev builds are larger and slower than
  `prepare`, and the runtime is served as one file.
- `js*` raw text cannot name registry entries, as it cannot name compiler
  names today.
- Everything else prints what the `none` build prints.

## Verification

- Differential execution: dev builds of the phase oracles, the ratchet
  fixtures and the example against `--optimize none`, through the Phase 0
  Node driver.
- Reload semantics as fixtures: a table of (initial source, edited source,
  expected output after reload) rows for functions, `defonce`, records,
  protocols, multimethods, macros and `ns` changes, run through the driver
  in `tests/compiler/dev_build.rs` and through the watch and server tests
  so all three paths agree.
- Subprocess tests for `--watch`, `--serve` and `--repl --connect` with
  Node runtimes and clients, no browser; browser checks by hand on the
  example before closing a phase, recorded in the pellet.
- Timing rows of the Goal table measured in the subprocess tests and
  recorded here and in `benchmarks/compiler.md` when a phase changes them.
- The existing evaluation suites (`eval_sessions`, `eval_cli`,
  `eval_nsstar`, `sci_reference`, `eval_examples`) pass unchanged through
  every phase.

## Settled questions

Decided on 2026-10-15, before any phase started. A pellet does not reopen
these; a finding that contradicts one goes under "Open questions".

- **Flag names.** Mode flags consistent with the existing CLI:
  `--reloadable` and `--out-dir` on `--compile --entry`, `--watch`,
  `--serve PORT` with `--serve-host`, `--reload-dependents`, `--repl
  --connect HOST:PORT` and `--nrepl PORT`. No `dev` subcommand and no
  configuration file; flags are the only configuration until a second
  build in one process is needed.
- **Registry shape.** The flat `"ns/name"` registry the session uses. It
  is tested and needs no migration. Nested namespace objects
  (`$light.app.view.render`) would read better in devtools but would be a
  second storage model or a migration of the session's; rejected.
- **Transport.** Server-sent events for pushes and POST for replies: no
  framing or handshake code, works through proxies, testable with `fetch`
  from Node. A WebSocket server is a Phase 5 option if a reason appears.
- **Where printing happens.** In the runtime, as shadow-cljs does: the
  page runs `pr-str` with the print options the server sends, and values
  that are not finite data come back as references. The server holds no
  value handles.
- **Editor protocol.** nREPL first, since Calva, CIDER and Conjure speak
  it out of the box. A prepl or plain socket REPL is not planned.
- **Dev page.** `--serve` answers `GET /` with a built-in page when the
  out dir has no `index.html`; a page in the out dir wins.
- **Binding.** Loopback by default; `--serve-host` opens the server to the
  network.
- **Downstream reloads.** The documented reload set by default, and
  `--reload-dependents` in Phase 3 for shadow-cljs's `:full` behavior.

## Open questions

- **Runtime-only state on reload.** shadow-cljs calls per-namespace reset
  hooks before reloading a file. Whether light-cljs needs one (for example
  to reset a namespace's registered reader context) is to be seen in
  Phase 3.
