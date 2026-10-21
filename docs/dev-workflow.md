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
| Save to updated page, one changed namespace of the example | Phase 3 (light-cljs-286): 29-39 ms from the save to the new render in headless Chrome, 5 saves of `app.view`, debug CLI with `--poll-ms 20` (load 1.9); the client applies the pushed unit in about 1 ms; with the reload policy (light-cljs-287) 26.6-66.3 ms, median 30.8, over 14 saves of `app.view` (steps 8.5-37.2 ms in the debug CLI, load 2.1-3.1 next to a peer's builds), and 18.1-40.5 ms for a save of `app.state` under `--reload-dependents`, which reloads `app.state`, `app.view` and `app.main` | under 100 ms on the development machine, including the recompile |
| Application state across a reload | Phase 0: a replaced unit of the example keeps the `defonce` atom, also when `app.state`'s own unit runs again | `defonce` atoms keep their value; `def` values are reinitialized, as upstream |
| Observable behavior of the dev build | Phase 0: prints what the `none` build prints for the example, examples/features, every phase oracle and every size ratchet fixture | prints what the `none` build prints, for the same program, before any reload |
| Recompile of one changed namespace of the example | whole program in 15-250 ms depending on size; Phase 0's whole dev build of the example 246 ms, as its `none` build (release CLI, median of 15, load 4.3); Phase 2: a saved `app.view` is its correct unit on disk 3.9-4.6 ms after the poll notices it (release CLI, 15 saves, median 4.3, load 3.5-9.7; 8.9-11.1 ms from the debug test binary), and `--watch` starts in 279-290 ms; with light-cljs-301's unit 4.0-4.6 ms (20 saves, median 4.3, load 11.7-12.3; 8.7-9.4 ms debug) | the one namespace, under 20 ms |
| REPL round trip for `(+ 1 2)` in the page | Phase 4 (light-cljs-288): 0.72-1.01 ms from the line typed to the value printed, median 0.80, 50 round trips through headless Chrome running examples/dev, release CLI (load 2.5); 1.1-3.3 ms, median 1.4, over 20 through a Node runtime in tests/dev_repl.rs, debug CLI; over nREPL (light-cljs-289) 0.53-1.64 ms from the message sent to its `done`, median 0.63, 50 round trips through headless Chrome running examples/dev, release CLI (load 3.7), and 0.9-3.7 ms, median 1.3, over 20 through a Node runtime in tests/dev_nrepl.rs, debug CLI (load 4.0) | under 20 ms, dominated by the compile |
| Dev build bytes served after the first load | Phase 0: `runtime.js` 432,093 bytes once; the example's `app.view.js` 3,492 bytes (map 974), its other units 817-1,777; Phase 2's rewritten `app.view.js` 3,565 bytes (map 542), registering its namespace once (light-cljs-301; 9,385 when each form's unit registered every namespace) | the changed namespace's unit only; the runtime is served once and cached |

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
  (`emit_js_mode`'s `mapped`), not for interactive units (since Phase 0,
  for the units of a dev build).
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
  client.js         the dev client (Phase 3; a stub that only defined `define` before)
  app.main.js       one unit per namespace, in dependency order
  app.view.js
  ...
  manifest.json     namespace order, require graph, macro declarers, file per namespace
  index.js          loads runtime.js, client.js and the units in manifest order
```

A unit file is the interactive unit wrapped in a registration call:

```js
globalThis.__lightDev.define("app.view", ["app.macros", "app.state"],
  (__lightInteractiveRuntime) => { globalThis.__lightApplicationRoots ??= Object.create(null); /* forms */ return value; });
//# sourceMappingURL=app.view.js.map
```

The requires are the namespace's source requires; `cljs.core` is the
runtime, which every unit reads. `define` records the unit by namespace and
runs it against `globalThis.__lightRuntime` immediately, keeping its value.
The same text serves the first load (as a script file) and every reload
(evaluated by the client with a `sourceURL`), so there is one unit writer.
`index.js` is a few lines that insert script elements in manifest order and
resolve a promise of the entry namespace's value when the last has run; in
Node it requires the files in that order, and the test driver runs them
itself. The manifest also lists the `^:dev/before-load` and
`^:dev/after-load` functions (and their `-async` variants) in definition
order, which a client needs for the hook protocol. The format is in
[Dev builds](compiler.md#dev-builds).

The dev build is emitted at `none` semantics with the full runtime and with
namespace registration on, like the session: no definition removal, no
specialization, no slicing, no terse names. It never enters
`prepare_application`. Each unit registers its namespace's reader context
and sets the current namespace for the duration of its top-level forms, as
compiled output does; the first unit registers every namespace of the build,
as compiled output does before its first form, so reflection during the
first load matches the `none` build. A session's unit of one form registers
only the namespaces whose reader context or definitions changed since the
session's last unit, every one it knows in its first, so a runtime that ran
the session's units in order sees every namespace as the session knows it;
a unit that a watch step rewrites registers its namespace once, with the
definitions of all its forms, as the build's unit does (light-cljs-301).

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

As built (light-cljs-283; the API is in
[evaluation.md](evaluation.md#actions)), a unit is one top-level form:
namespace, JavaScript, source map (light-cljs-284: the form's map, named as
the namespace's dev build file, `app.view.js`, which the unit's
`//# sourceURL=` names too) and the form's location. A `Load` lists the units of every namespace a
`require` loads, but `Namespace` actions come between them wherever the
session makes a namespace current: when a load enters a namespace and when it
returns to the requiring one. The `Namespace` of a namespace with no forms
besides its `ns` thus still reaches the runtime, which creates the namespace,
and each unit runs after the `Namespace` of its namespace. Reload policy is not
carried by the action: the server's `load` message carries it, the hooks and
the `once` and `always` lists (light-cljs-287). The compiler hands each
action to an interpreter as it produces it, and the `DevSession` methods
collect them; `EvalSession`
interprets them as they come, so a unit that throws while a namespace loads
stops that load before its next form is compiled, as before. A runtime in
another process never makes the compiler wait.

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
- Every namespace marked `^:dev/always`, with its compilation consumers,
  recompiled and reloaded (light-cljs-325; it was reloaded without
  recompiling before, see "Settled questions").
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

As built (light-cljs-287; the load is in [Serve](evaluation.md#serve)): a
`--watch` step computes the set from what it wrote (the changed namespaces,
the requirers of a changed macro namespace, which it recompiled, and the
namespaces a changed `ns` form loaded), in the manifest's load order, which
puts each namespace after those it requires. `^:dev/once` and `^:dev/always`
are read from the metadata of each namespace's last `ns` form in the
session's macro host, and the hooks from the retained program after the
build and after each step that succeeds (`DevSession::hooks`,
`dev_build::program_hooks`), ordered by namespace in load order, then by
definition. A redefinition replaces a global's metadata, so a definition
that loses its flag stops being a hook; one deleted from the source keeps
its last metadata and stays a hook, as it stays defined. A `^:dev/once`
namespace loads once: with the build, or with the step that adds it to the
build; after that a step rewrites its unit (a page loaded later runs it) and
loads nothing of it, and under `--reload-dependents` it passes no change on
downstream (a namespace that depends on the change only through it is not
reloaded), as shadow-cljs's never-load list keeps a namespace out of the
`:full` walk. Unlike shadow-cljs, which leaves a never-load namespace
out even before it has loaded, a new `^:dev/once` namespace loads, since its
requirers need it. The `^:dev/always` namespaces join the load of every step
that wrote anything, a step that wrote only `^:dev/once` namespaces included,
and under `--reload-dependents` the walk downstream starts from them too, as
shadow-cljs's selection includes always-load namespaces; a step that wrote
nothing pushes nothing and runs no hook. `--reload-dependents` requires
`--serve`, since without a server nothing is loaded. A replay applies the
policy in the client, so that a runtime that missed steps loads what they
would have loaded: a `once` unit only where the runtime never ran it to its
end, and when anything else loads, the `always` units and, with the
`dependents` flag, the units downstream along each unit's `requires`.

As built (light-cljs-325): what a step recompiles comes from the build's
dependency manifest ([compiler-cache.md](compiler-cache.md), "Conservative
keys and invalidation"), the record restart validation keys on, and the
policy evaluator the cache uses (`light_cljs::cache_policy`). The startup
build records the manifest; each step revises it
(`DependencyManifest::revise`) with each namespace the session compiled
again: its requires as the session has them, whether namespaces declare
macros now, and what the session observed while it compiled the file (the
namespaces its expansions named, whether it ran a source macro, and what
`light.build` read or declared, all attributed to every namespace the file
wrote, since the session compiles every form as one source). Two rules
keep the revision conservative where the session sees less than a build:
a revised namespace keeps the observed edges, lazy references and coarse
flag of its record (the session analyzes a form as if it required every
namespace it has loaded, so it logs no qualified name of one); what a
deferred unit's initializer read, a value later compiles reuse, goes also
to the macro providers and helpers the namespace depends on, bundled
libraries apart, and from those to their compilation consumers, as the
build's helper records do. A step recompiles, from source and in load
order:

- each changed namespace;
- each namespace that depends on a recompiled one through a macro-provider
  or helper edge, and so on from those: the users of a changed macro
  namespace, and the users of a macro whose namespace requires a changed
  one, which may declare no macros itself (a helper). A namespace that
  begins or stops declaring macros revises the manifest at once, so the
  requirers it gains or loses are recompiled in the same step. A helper
  edge reaches the macro namespace's direct dependencies only, as the
  manifest records helpers; a function those call is looked up when it
  runs, but an expansion that called it is not recompiled;
- the consumers the manifest records of a resource that changed, appeared
  or went: the watch polls every location a `light.build` resource was
  looked for at, present or absent, with the source files. A location read
  differently from how the watch first finds it is reported by the next
  poll;
- on every step a change triggers, and before any retained unit is kept,
  the always-compile set of the policy evaluator: the namespaces marked
  `^:dev/always` and their compilation consumers through every compilation
  edge, runtime requires included. Always compile acts only when a step is
  triggered: a file of no namespace of the build and an unregistered input
  start no step. The session's state is kept, as for every recompile: a
  macro expands again in the same macro heap. A compile that marks a
  namespace `^:dev/always`, or loads a new namespace, evaluates the set
  again, so the consumers it gains compile in the same step.

A disk bypass does not invalidate memory: `^:light-cljs/no-cache` and
`--watch --cache-blocker NS` (repeatable, given to the policy evaluator and
to the startup build's cache) only keep a namespace out of the persistent
cache, so they add nothing to a step. Runtime-only requirers are reloaded, not recompiled, only with
`--reload-dependents`, as before.

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

As built (light-cljs-286; the protocol is in
[Serve](evaluation.md#serve)), what the server keeps for a late tab is the
build as it is now, every namespace's latest unit in load order, rather than
the last `Load` alone, with the last step's diagnostics: a tab that missed two
steps of different namespaces needs both. The client applies of that replay
only the units whose text it does not already run (the source text of the
function `define` recorded is in the unit's), so a page that loaded the
current files does nothing, a tab running older units reloads them between
the hooks, and a client with nothing loaded would load everything. The
client reads the stream with `fetch` rather than `EventSource`, so one
client runs in a page and in Node, where the test driver connects it.

### The connected REPL

`--repl --connect HOST:PORT` is a thin client: it reads forms as today,
posts their text to the server, and prints what comes back. The server
compiles with its `DevSession`, so REPL definitions and watch recompiles
share one program and one current namespace per connection. Printing
happens in the runtime; values that are not finite data come back as
references the server can ask about later. Reader errors are local; analysis
errors come from the server; runtime errors come from the page with a
mapped stack.

As built (light-cljs-288; the client and protocol are in
[Connected REPL](evaluation.md#connected-repl)): the client posts each form
it reads to `POST /repl` of the server, which hands it to the watch's thread
(`dev_repl::ReplRequest`) to compile between polls in the connection's
namespace, then sends the unit as an `invoke` to the session's runtime and
waits on its own thread for the reply. What the form loaded runs before the
unit in that runtime and goes to the others as a `run` message. The runtime
keeps each session's `*1`, `*2`, `*3` and `*e`, and the newest 1,000 values
that were not finite data by reference number, which the `light.dev`
functions the client defines, `ref` and `inspect`, take; `light.dev/runtimes`
and `light.dev/select!` are commands the server answers. The server maps an
error's stack through the REPL units' mappings it keeps and the build's
source maps.

The editor path is nREPL on the same port (`--nrepl PORT` when a separate
port is wanted): bencode framing and the operations `clone`, `close`,
`describe`, `eval`, `load-file`, `interrupt`, `completions` and `lookup`.
Completions and lookup are answered from the retained program's globals and
their metadata (arglists, docstrings), without a runtime round trip.

As built (light-cljs-289; the operations are in [nREPL](evaluation.md#nrepl)):
`dev_nrepl` reads and writes bencode by hand, and the server's port tells an
nREPL client from an HTTP one by its first byte, `d` for a bencoded
dictionary; `--nrepl PORT` adds a listener and writes `.nrepl-port`, removed
when the watch ends (on SIGINT, SIGTERM and SIGHUP too). An nREPL session is
a connected REPL session of the server, so `eval` and `load-file` take the
terminal client's path: the form goes to the watch's thread as a
`ReplRequest`, its unit to the session's runtime, and the value or the error
the runtime gives back becomes `value` and `ns`, or `err` and `ex`, per
form. A session's evaluations run one at a time, in the order they arrive
from any connection, on one queue per session that a worker thread runs
(light-cljs-389); each job carries the connection it answers on.
`completions` and `lookup` are `ReplRequest`s too, answered on the watch's
thread from the retained program: its definitions by namespace, their
metadata (`:arglists`, `:doc`, `:file`, `:line`, `:column`, `:private`), each
namespace's reader context (aliases, refers, excludes, macros), the macro
session's metadata of source macros (arglists, docstring, privacy), and
cljs.core's pinned metadata.
`interrupt` drops the session's pending runtime request, answering the
waiting evaluation as interrupted, and runs no further form; the runtime
finishes the form it was sent. A `load-file` evaluates the file's text as
`eval` does and leaves the session's namespace as it was.

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

As built (light-cljs-286): a module script runs after the page's classic
scripts, so the page above can run a unit before the packages are set. The
documented page ([JavaScript packages in
dev](evaluation.md#javascript-packages-in-dev)) adds `index.js` from the
module once they are, which headless Chrome ran with a stub package read by
a top-level `def`.

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

Status (light-cljs-282, October 15, 2026): done. `--reloadable --out-dir`
writes the directory ([Dev builds](compiler.md#dev-builds));
`Program::namespace_requires` (from code-splitting) is the require graph and
`Program::declares_macros` names the macro declarers. The manifest also
lists the hook functions, and `index.js` resolves with the entry
namespace's value, the single-file output's value, so a host calls `start`
as it calls the value of examples/features. The CLI analyzes a dev build in
warning mode. Besides Node, the example was checked in headless Chrome
(`--dump-dom` of a page that loads `index.js`, calls `start`, then evaluates
an edited `app.view` unit with the hooks): `start`, `clicks: [1]`,
`reloading`, `render: clicks: [2]`; a build whose dependency throws while it
loads rejected `__lightDevLoaded` with that error before the entry ran.

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

Status (light-cljs-283, October 15, 2026): the factoring is done.
`src/dev_session.rs` holds `DevSession`, the actions, form reading,
directives, fragment analysis, unit emission, lazy loads and the
loaded/loading sets; `EvalSession` and its runtime `load-file` callback are
interpreters of the actions, and the two copies of the loading code they
had are one. `tests/evaluation/dev_session.rs` covers a form, a `require`,
a `require :reload` whose source changed, a `load-file` and a failing form
after a successful one by applying the units in a fresh QuickJS heap, a
unit that throws while a namespace loads (in the session and through the
runtime `load-file`), and a standalone `DevSession` idle past its deadline;
and it applies the actions of a `require` of an edited `app.view`, compiled
without a heap and written by `dev_session::script`, to the running Phase 0
build of `examples/dev` through the driver (`render: clicks: [2]`, the
`defonce` state kept, each unit in its namespace).

Status (light-cljs-284, October 15, 2026): the rest of Phase 1 is done. `ns`
goes through the compiler's namespace analysis as `ns*` does (`session_ns`
beside `session_nsstar` in `src/compiler.rs`), so every clause `--compile`
accepts behaves the same in the session, and an `ns` form replaces its
namespace's requires, as upstream's does, so a reloaded file whose `ns`
changed rebinds its aliases; a session name need not be ASCII, as before.
`[lib :reload true]`, `[lib :reload-all true]` and the flags anywhere
reload, and `:reload-all` loads again, once, every namespace the library
reaches. The four SCI reload rows run against a `.cljs` twin of their `.clj`
dependency and match SCI (`adapted`), and the `ns` path made two more rows
match (`portable`). A `DevSession` is in warning mode by default
(`set_warning_mode`); an `EvalSession` takes `EvalOptions::warning_mode` and
hands warnings to `EvalOptions::warnings`, and `--repl --warnings` and
`--eval --warnings` print them to stderr. Every unit carries a source map
(`emit_js_mode`'s mapped path), which the QuickJS session ignores; loading a
500-definition namespace through `--eval` took the same time with and without
the maps (5.5 s and 5.7 s, debug build). `tests/evaluation/dev_session.rs`
adds an `ns` with every clause (its units print what `--compile` output of
the namespace prints), the reload spellings over a source provider whose text
changes between loads, warning mode and the unit maps (read back with Node's
`SourceMap`); `eval_cli` loads and reloads examples/dev through `require` in
`--repl`. The name metadata, docstring and attribute map of an `ns` form
reach macros' `&env` as under `--compile`, and an `ns` form built from data
is read from its data. Checked by hand in headless Chrome: the Phase 0 build
of examples/dev, then the script of a heap-less `require` of an edited
`app.view` (units with their `sourceURL`) between the hooks, printed
`start`, `clicks: [1]`, `reloading`, `render: clicks: [2]`.

As built (light-cljs-348): a form's analysis, unit and registration cost
what the form adds, not what the session retains, so loading a program
form by form grows linearly with it
([benchmarks](../benchmarks/compiler-cache.md#session-load-light-cljs-348-october-18-2026)).
The form is analyzed against the retained program without copying it (its
tables are frozen and shared), and the retained program grows by what the
form's new or changed metadata reaches
(`program_fragment::extend_interactive_snapshot`), compacting once what no
metadata reaches is half of it. `InteractiveCompiler` keeps, beside it, the
global-name index, each namespace's globals in ID order, the session's
namespaces, the declared arities and the namespaces touched since the last
recorded registration, each extended by the form's new globals; syntax
quotes qualify through those indexes, the unit names functions and lists a
registered namespace's definitions from them, and the session registers only
touched namespaces. The session's namespaces stay addressable by name from
every form, for resolution, `undeclared-ns` and `::ns/kw`, through that set
rather than as requires. So `&env`'s `:ns` `:requires`, and the aliases a
macro's own reads resolve, are the namespace's own, as under `--compile`,
where they were every namespace the session had entered: the one change a
macro can see.

As built (light-cljs-364): a loaded file's macros may call any definition
of the file, as upstream's Clojure load of a `.cljc` file gives them before
they expand and as `--compile` allows by completing a namespace's units at
its end (light-cljs-360). Before the first form of a file that names a
macro of its namespace while the file has declared names it has not
defined yet (`(declare f)`, a macro calling `f`, a use, then
`(defn f ...)`, as in cuerdas.core), the session registers ahead the
definitions (`def`, `defn`, `defn-`, `defonce`, `declare`) of the rest of
the file, up to its next directive (`InteractiveCompiler::define_ahead`).
When a form fails anyway because one of its namespace's macros called a
definition the file has not reached, such as one it uses undeclared, the
session registers them then and analyzes the form again. Their top-level
forms are analyzed as one program, which is dropped with its warnings, and
the units they register stay unanalyzed in the compile-time catalog: one is
analyzed only if a macro calls its definition, which expands that
definition's macros a second time, as upstream's Clojure load and
ClojureScript compile do. No
other form is analyzed ahead, so a macro the file redefines later is not
the one an earlier use expands, and each macro use expands once, besides
the failing form's first attempt: what its macros did before the
unavailable call runs again in the retry. The forms read ahead wait in a
queue for their turn (`LoadingFile`), so each data reader runs once per
occurrence, and each is read as after the forms before it: a syntax quote
qualifies what they define as the namespace's own
(`InteractiveCompiler::defined_ahead`). Each form still defines its own
Vars, units and warnings in its turn, so the units run in order. Any other
file is read and analyzed as before.

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

Status (light-cljs-285, October 15, 2026): done. `--watch --entry NS
--source-root DIR --out-dir DIR [--poll-ms N]` ([Watch](evaluation.md#watch))
writes the dev build, then a `DevSession` loads the same program through the
source roots (a `require` of the entry: startup compiles twice, 279-290 ms for
the example with the release CLI), and the poll compares the modification
time and size of every `.cljs` and `.cljc` file under the roots, so a new file
is seen too. A step compiles each changed namespace of the build in load
order with `DevSession::load_namespace_file`, from its source as the provider
finds it then, read from the first form in the namespace as `--compile`
reads it; the units of each namespace whose forms
ran, and of each namespace the step loaded, are put together by
`dev_build::namespace_unit` (the build's `define` call around a function that
registers the namespace, makes it current, then runs each form's statements
in a scope of their own, with one source map made of the forms' maps) and
written by rename; the manifest
(`dev_build::DevManifest`) takes the new namespaces ahead of their requirer
and is put in an order where each namespace follows its requires. A
namespace that declares macros, before or after the change, recompiles its
requirers, each once a step, and a require that closes a cycle through
loaded namespaces fails the step as `--compile` refuses it. A rewritten
namespace whose last form is a directive keeps the value nil. The session
now looks in its source provider before the bundled libraries, as
`--compile` does, so a source root's `clojure/string.cljs` wins in both. A
failing step leaves its namespace, and those after it, to
compile again at the next change of any source file, which also retries a
`require` of a file that did not exist yet. `tests/dev_watch.rs` runs
`--watch` on a copy of the example: the rewritten `app.view` unit, applied by
the driver to the running first build with the hooks, prints `render:
clicks: [2]` with the `defonce` state kept, and the rewritten directory
loads afresh; the unit's map maps its first and last forms to their lines;
a reader error and an analysis error leave the unit and manifest byte for
byte; a warning prints and the unit still runs; a macro change rewrites
`app.macros` and `app.view` (`clicks = [2]`); a new require, first of a
missing file and then of the `.cljc` file once saved, adds `app.extra` ahead
of `app.view` in the manifest and `index.js`; and a require cycle, a
trailing directive, a `::` keyword in an `ns` form after a step in another
namespace, a source root's `clojure.string`, a save over a symbolic link and
a unit that cannot be written (the watch exits) each behave as above, with
the rewritten build checked against a fresh `--reloadable` build of the same
sources where it applies. The test asserts the best of
five step times under 20 ms with the release CLI (`cargo nextest run
--release --test dev_watch`), as the Goal table measures, and under 100 ms
in the debug build CI runs; the times are in the Goal table. A step does not
update the manifest's hook lists (Phase 3 finds hooks in the retained
program). A rewritten unit registers its namespace once, the first in load
order every namespace of the build, and its forms share one prelude
(light-cljs-301; a form whose raw JavaScript renamed its unit's names makes
the file run each form's unit whole): the rewritten `app.view.js` is 3,565
bytes against the build's 3,492, where each form's unit registering every
namespace the session knew made it 9,385. Checked by hand in headless
Chrome: the first build of the example, then the rewritten `app.view.js`
evaluated between the hooks, printed `start`, `clicks: [1]`, `reloading`,
`render: clicks: [2]`.

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

Status (light-cljs-286, October 15, 2026): the server and the client are
done; the policy item (hooks found in the retained program after each step,
`^:dev/once` and `^:dev/always`, the reload set, `--reload-dependents`) is
light-cljs-287's. `--watch --serve PORT [--serve-host HOST]`
([Serve](evaluation.md#serve)) runs `dev_server::DevServer`, an HTTP/1.1
subset on `std::net` with a thread per connection and hand-written JSON
(`dev_server::Json` reads it): `GET` of the build directory and the source
roots with content types and ETags, the built-in page, `GET /events` and
`POST /reply`, loopback by default and answering only loopback `Host` names
there. A watch step publishes its warnings, its error, or the load of every
namespace it wrote (with those of failed steps since the last success), and
the build for runtimes that connect later. `client.js` is src/dev_client.js,
emitted into every dev build: `fetch`-based, it applies loads with the hooks
as the Phase 0 driver does (a throwing hook is logged and skipped), skips
the units of a replay it already runs, evaluates `invoke`s and posts their
printed values (`DevServer::invoke` is the API light-cljs-288's REPL will
call), draws the overlay and reconnects with backoff.
`tests/dev_server.rs` runs `--watch --serve 0` with the driver's
`--connect` mode as the runtime: a saved change reaches a runtime running
the first build, which loads it between the hooks (`render: clicks: [2]`,
the `defonce` state kept); a compile error is pushed located and with no
load, and a warning with its load; a late runtime running the first build
catches up and one running the current files loads nothing; a runtime whose
server restarts reconnects and catches up; a failed step that loaded a new
namespace before its error is not in a late runtime's replay, and the next
step that succeeds loads it; the build's own warnings reach a runtime; the
client runs a load's hooks in order, awaiting `-async` continuations and
skipping one that throws; a replay runs again a unit that threw; `GET`
serves files, sources at their source-map paths, ETags and the built-in page
until the build directory has an `index.html`; and an `invoke` through the
library's server replies with `pr-str` under the print options sent
(`(0 1 2 ...)` for `:length 3`), the application's state and a thrown error,
while a reply naming another runtime is refused. Checked in
headless Chrome, driven over the DevTools protocol, on a copy of
examples/dev served by the debug CLI with `--poll-ms 20`: the built-in page
printed `start` and `clicks: [1]` and connected; five saves of `app.view`
each logged `reloading`, the new render with the click count rising from 2
to 6 and `light-cljs: reloaded app.view in 1 ms`, 29-39 ms from the save to
the new line on the page (load 1.9); a deliberate reader error drew the
overlay with the file, line 23, column 1 and the message, which the fixed
save cleared while the page went on (`fixed: clicks: [7]`); and a warning
drew the overlay while its unit loaded.

Status (light-cljs-287, October 15, 2026): the policy item is done, and with
it Phase 3. A step loads its reload set ("The reload set", as built there):
what it wrote, the `^:dev/always` namespaces with it, never a `^:dev/once`
namespace the runtimes ran, and with `--reload-dependents` (on `--watch
--serve`) every namespace downstream, in load order; it finds the hooks
again in the retained program and rewrites the manifest's hook lists, and
each `load` carries them with the `once`, `always` and `dependents` policy
and each unit's requires, by which the client applies a replay.
`tests/dev_server.rs` adds: a macro change loads `app.macros` and
`app.view` (`clicks = [2]`); a save of `app.state` loads `app.state`, then
with `--reload-dependents` also `app.view` and `app.main` in that order
(their top-level lines print again), and a late runtime's replay loads the
same; a `^:dev/once` `app.view` is rewritten but never loaded, left out of
the downstream walk along with a namespace that reaches the change only
through it, and kept by a late runtime's replay; a `^:dev/always` namespace
joins every load and every replay that loads anything, also a step that
rewrote only a `^:dev/once` namespace, whose load then reaches the always
namespace's dependents under `--reload-dependents`; a step that drops one
hook's flag and adds an `-async` one runs the new list and writes it to the
manifest; and hooks in `app.state` and `app.main` keep their namespace
order across saves of either. The
reload fixtures (tests/support/reload_fixtures.rs: functions, `defonce`,
records, protocols, multimethods, a macro, a new require and changed
refers) print the same through the server's client, through the driver
given the step's rewritten units, and through the driver given a fresh
build's units (tests/compiler/dev_build.rs); they needed no change to the
compiler or the runtime. Checked in headless Chrome over the DevTools
protocol, on copies of examples/dev served by the debug CLI with
`--poll-ms 20` (load 2.1-3.1): 14 saves of `app.view` reached the page in
26.6-66.3 ms (median 30.8; steps 8.5-37.2 ms), each `light-cljs: reloaded
app.view in 0-2 ms`; and with `--reload-dependents`, saves of `app.state`
logged `reloaded app.state, app.view, app.main` and the new render 18.1-40.5
ms after the save.

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

Status (light-cljs-289, October 18, 2026): Phase 4 is done. nREPL
([nREPL](evaluation.md#nrepl)) serves `clone`, `close`, `describe`, `eval`,
`load-file`, `interrupt`, `completions`, `lookup` and `ls-sessions` on the
server's port and on `--nrepl PORT`, with `.nrepl-port`.
`tests/dev_nrepl.rs` drives it with a Node bencode client against
`--watch --serve 0 --nrepl 0` on a copy of examples/dev and a Node runtime:
every operation, a REPL definition read by a save and kept by a reload of
its namespace, an interrupt of a runtime busy with a form, nREPL on the
server's own port, and `.nrepl-port` removed on SIGINT. `(+ 1 2)` takes
0.9-3.7 ms there (median 1.3 over 20, debug CLI). By the user's decision
of October 18, 2026, the test with a Node client replaces the hand checks
against Calva, CIDER and Conjure: no editor was run. Checked in headless
Chrome on a copy of examples/dev served by the release CLI with `--nrepl 0`,
through the Node client: `.nrepl-port` named the port, `(light.dev/runtimes)`
named the HeadlessChrome runtime, `(.-title js/document)` gave
`"light-cljs dev"`, `(app.state/click!)` changed the page's state, a thrown
error came back as `err` and `ex`, `completions` and `lookup` answered for
`app.view`, a `def` made over nREPL in `app.state` was read by a function a
save added to `app.view` and kept by a save of `app.state`, SIGINT removed
`.nrepl-port`, and `(+ 1 2)` took 0.53-1.64 ms (median 0.63 over 50, load
3.7).

Status (light-cljs-288, October 15, 2026): the terminal client and runtime
selection are done; nREPL is light-cljs-289's, which reuses the server's
path from a form to a runtime's reply. `--repl --connect HOST:PORT`
([Connected REPL](evaluation.md#connected-repl)) reads as `--repl` does,
reports reader errors itself, and prints what the server answers: the value
the runtime printed under the client's print options, with `;; ref N` for a
value that is not finite data, and analysis errors, warnings and runtime
errors located in the input, the stacks mapped to the input and the build's
sources. `tests/dev_repl.rs` runs it against `--watch --serve 0` on a copy
of examples/dev with Node runtimes: `(+ 1 2)`, `*1`/`*2`/`*3` and the
application's state round trip; `(atom {:a 1})` comes back as reference 1,
which `@(light.dev/ref 1)` and `(light.dev/inspect 1)` read; a `def` made at
the REPL in `app.state` is read by a function a save adds to `app.view`, and
called from the REPL, and a REPL redefinition replaces it for its callers; a
function a save added that throws comes back with a frame at its line in
`src/app/view.cljs` and one at the REPL form, and is `*e`; two runtimes, the
most recent getting the forms until `light.dev/select!` chooses the other, a
`require` and a save reaching both, a namespace per connection, and a chosen
runtime that left being an error; what a `require` loaded before it failed
reaching the runtimes; a closed session's history leaving the runtime, and a
session of a restarted server getting neither an old client's state nor a
surviving runtime's history for an old session; and the endpoint refusing a
request with an `Origin` or without a JSON content type. Checked in headless Chrome on a
copy of examples/dev served by the release CLI: `(light.dev/runtimes)` named
the HeadlessChrome runtime, `(.-title js/document)` gave `"light-cljs dev"`,
`(app.state/click!)` changed the page's state, a `js/console.log` from the
REPL appeared on the page, a REPL function that threw came back mapped to its
form, `(app.view/explode)` added by a save to `src/app/view.cljs:25:10`, a
`require` of `clojure.string` loaded it, and `(+ 1 2)` took 0.72-1.01 ms
(median 0.80 over 50, load 2.5).

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
- **Multiple watch builds** in one process, and a configuration file: no
  longer deferred. Both are designed in
  [build-configuration.md](build-configuration.md): `light-cljs.edn` with
  named builds, and `light-cljs watch app worker` running them in one
  process behind one server and one nREPL (its Phase 3), queued as the
  pellets group `build-configuration`.

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
  defined until the page is reloaded, as upstream; a removed hook function
  stays a hook too (shadow-cljs, which forgets a namespace's definitions
  before it recompiles it, drops it), while one whose definition loses its
  `^:dev/...` flag stops being one.
- Redefining a record or type creates a new constructor; old instances
  keep their methods and saved factories follow the new Var. Redefining a
  protocol keeps existing extensions usable. `defmulti` keeps its existing
  value. These are the session's semantics today.
- Namespaces that use a changed macro namespace, or a macro whose namespace
  requires a changed namespace, are recompiled and reloaded whether or not
  they used a macro from it, as are the namespaces that read a changed
  `light.build` resource. Nothing else downstream is reloaded by default;
  `--reload-dependents` reloads every downstream namespace in topological
  order. A namespace marked `^:dev/always` is recompiled and reloaded, with
  its compilation consumers, by every step; one marked `^:dev/once` loads
  once and is never
  reloaded, though its unit file follows its source for a page loaded
  later.
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
  so all three paths agree (light-cljs-287: tests/support/reload_fixtures.rs;
  the server test applies the step's rewritten units with the driver too,
  which is the watch path).
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
  build in one process is needed. *Superseded in part on 2026-10-20* by
  [build-configuration.md](build-configuration.md): the configuration file
  `light-cljs.edn` and shadow-cljs's `compile`, `watch`, `release` and
  `cljs-repl` subcommands, which take build ids and read it. These flags
  stay as they are, each one also an override after a subcommand, and a
  command that starts with a flag never reads the file.
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
- **`^:dev/always` means always compile** (superseded on 2026-10-17 by the
  compiler-cache decisions, built in light-cljs-325). It was "always reload,
  without recompiling". It now takes shadow-cljs's meaning: the namespace and
  its compilation consumers are compiled again by every triggered step, then
  reloaded, and never read from or written to the persistent cache. The
  watch chooses what a step recompiles through the dependency manifest and
  the cache's policy evaluator, so the watch and restart validation share
  one record of what depends on what.
- **Recompile, do not cache** (principle 16) is superseded for watch startup
  and session loading only, by Phase 4 of the cache design
  ([compiler-cache.md](compiler-cache.md)). A watch step still recompiles
  against the retained program; the cache does not replace retained
  in-memory state. As built (light-cljs-327): `--watch --cache-dir DIR`
  compiles its startup build once, in the session's engine, with the cache,
  and the session starts from that build (`DevSession::compile_entry`);
  steps neither read nor write the cache. Since light-cljs-328 the cache is
  on by default, in `.light-cljs/cache`, and `--no-cache` gives the uncached
  start this document describes.

## Open questions

- **Runtime-only state on reload.** shadow-cljs calls per-namespace reset
  hooks before reloading a file. Whether light-cljs needs one (for example
  to reset a namespace's registered reader context) is to be seen in
  Phase 3. Phase 3's reload fixtures (light-cljs-287) needed none: a changed
  `ns` form's aliases and refers, records, protocols and multimethods
  behave as upstream's, since a reloaded unit registers its namespace's
  reader context again. Still open for state a program keeps outside its
  definitions.
- **Namespaces first loaded at the REPL.** A `require` at the connected
  REPL (light-cljs-288) loads a namespace into the runtimes connected then,
  but the namespace does not join the build: the watch does not follow its
  file and a runtime that connects later does not have it, so a REPL form
  that names it fails there until a namespace of the build requires it.
  Whether such a namespace should join the manifest, with its unit file and
  its place in replays, is to be seen with nREPL's `load-file` (light-cljs-289).
  As built there, `load-file` evaluates the file's text as `eval` does, so a
  namespace it loads joins the build no more than one a `require` loads; a
  file of the build that an editor loads is compiled again by the watch when
  it is saved. Still open, until an editor workflow needs more.
