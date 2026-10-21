# Build configuration: light-cljs.edn

Status: settled design, October 20, 2026. The user decided its three open
questions the same day ("Settled questions"). The reader and the schema
("As built (light-cljs-398)"), the merging of a build's layers ("As
built (light-cljs-399)"), the `compile` and `release` commands ("As
built (light-cljs-400)"), `--print-config` ("As built
(light-cljs-401)"), `watch` with one build ("As built
(light-cljs-403)"), rereading the file in the watch ("As built
(light-cljs-404)"), the dev server's mounts (light-cljs-405, under "Several
builds in one process"), several builds in one watch ("As built
(light-cljs-406)"), `light.dev/repl` with the Phase 3 exit criterion's
server and nREPL parts ("As built (light-cljs-407)") and `cljs-repl` ("As
built (light-cljs-408)") are implemented.
The implementation is queued as the pellets group `build-configuration`,
one or more pellets per phase ("Phases"). Names that shadow-cljs has no
word for are listed under "Names for review"; the pellets use them as
written unless the user renames one.

The user asked on 2026-10-20 for "something like shadow-cljs.edn, where you
can define the build", named after the tool, and then decided, the same
day: follow shadow-cljs everywhere, and deviate only where light-cljs
genuinely has no equivalent, saying why. This document designs that file:
`light-cljs.edn`, found by walking up from the current directory, holding
the project's source paths, cache settings, dev server and named builds,
each with `:dev` and `:release` overrides. Builds are run as shadow-cljs
runs them:

```sh
light-cljs compile app          # dev mode: the dev build, once
light-cljs watch app worker     # dev mode: both builds, watched, one dev server, one nREPL
light-cljs release app          # release mode: optimized chunks
```

It revises e05a164 (`docs/project-file.md`, "The project file"). What
changed, by the user's decisions of 2026-10-20:

- **Names are shadow-cljs's.** `:source-paths`, `:builds`, `:target`,
  `:output-dir`, `:modules`, `:dev`/`:release`, `:compiler-options`
  (`:optimizations`, `:closure-defines`, `:pseudo-names`, ...), `:devtools`,
  `:http`, `:nrepl` and the rest are used verbatim, each mapped to the flag
  that exists. e05a164's spelled-out names (`:source-roots`,
  `:output-directory`, `:development`, ...) and its "full words" principle
  are withdrawn: they rested on a misreading of the user's objection to
  `light.edn`, which was about naming the file after the tool. The document
  takes shadow-cljs's vocabulary too: **the configuration file** for
  `light-cljs.edn` and a **build configuration** for one build's map, as the
  shadow-cljs User's Guide says.
- **Commands are shadow-cljs's**: `compile`, `watch` and `release`, each
  taking build ids, replace e05a164's `--build NAME`.
- **Several builds in one process** are in scope (Phase 3), no longer
  deferred.
- **`#light-cljs/env` and `--config-merge`** are in, as shadow-cljs's
  `#shadow/env` and `--config-merge`.
- **No `:defaults` and no `--project-file`**, which shadow-cljs does not
  have.

Kept from e05a164: the cache contract (the file never enters a cache key;
"Cache keys and fingerprints"), strict validation located at line and
column, the translation into today's flag structures, the command that
prints the merged configuration (`--describe-build`, renamed `--print-config` after the `--config` family, since shadow-cljs has no such command), and the example
for examples/features.

It supersedes in part [dev-workflow.md](dev-workflow.md), "Settled
questions", "Flag names" ("No `dev` subcommand and no configuration file;
flags are the only configuration until a second build in one process is
needed") and its "Phase 5: deferred" item on several watch builds and a
configuration file; and the sentence of [code-splitting.md](code-splitting.md)
that says light-cljs "has no build configuration file".

## Deviations from shadow-cljs

Only where light-cljs has no equivalent. Everything else is shadow-cljs's
key, value and behavior.

| shadow-cljs | light-cljs | Why |
|---|---|---|
| `:target` `:browser`, `:esm`, `:node-script`, `:npm-module`, ... | `:esm` only | The compiler writes one output shape, ES modules, which runs in browsers, Node and bundlers; another target is a new output format, not a key |
| `:modules`, an explicit chunk graph | Exactly one module with one `:entries` namespace; its chunk is named after the namespace (`app.main.js`), not the module id; no `:init-fn`, `:depends-on`, `:exports` | Chunks are inferred from `light.lazy/loadable` sites (code-splitting.md) and named by the compiler; the entry's value is the startup chunk's default export. More modules arrive with code-splitting's Phase 4 shaping |
| `:optimizations` `:advanced`, `:simple`, `:whitespace` | `:none`, `:prepare`, `:advanced` | light-cljs's optimizer levels (`--optimize`) are not Closure's |
| `:dependencies`, `:deps`, `:js-options`, `classpath:` | none | The compiler resolves no dependencies and has no classpath: library sources are source paths, and JavaScript packages are host- or bundler-supplied (compiler.md, "JavaScript packages") |
| `:dev-http`, `:asset-path` | none: the `:http` dev server serves each build's `:output-dir` | One server serves builds, events and REPL on one port (dev-workflow.md, "The server and the client"); chunks import one another by relative URL |
| `:build-hooks`, `:devtools` `:before-load`/`:after-load` symbols | none; `^:dev/before-load` and `^:dev/after-load` metadata, which shadow-cljs also reads | No user code runs outside macros, whose inputs the cache contract governs |
| `:closure-defines` and `:checked-arrays` apply in dev | an error when they reach dev mode, permanently | The dev build takes no `--define` (`--reloadable` refuses it) and the watch no checked-arrays flag; an error, not a dev build that silently differs. The user decided on 2026-10-20 that dev builds do not gain either, so this is a lasting deviation, not a gap to close ("Settled questions") |
| nREPL on its own port, starting in a Clojure REPL, `.shadow-cljs/nrepl.port` | nREPL always on the `:http` port, sessions in ClojureScript from the start; `:nrepl {:port N}` adds a listener and writes `.nrepl-port` | There is no JVM or Clojure runtime to start in (dev-workflow.md, "The connected REPL") |
| `server`, `start`, `stop`, commands that connect to a running server; about twenty actions | `compile`, `watch`, `release` and `cljs-repl` | No JVM startup to amortize, so no server mode; `--repl` and `--eval` are flags already, and `classpath`, `pom`, `npm-deps` and `clj-*` have no counterpart |
| `cljs-repl ID` starts a server and the build when none is running | `cljs-repl ID` connects to a running `watch` and is an error without one | There is no server mode to start in the background; the watch is the server |
| `~/.shadow-cljs/config.edn`; `#env` beside `#shadow/env` | none; `#light-cljs/env` only | The user file exists for per-user dependencies and nREPL middleware, which light-cljs has none of; EDN reserves unqualified tags such as `#env` |
| `release --source-maps`, `--debug` | refused | Chunked output has no source maps yet (`--out-dir` refuses the source map flags) |
| Open specs: an unknown key is ignored | Closed key sets: an unknown key is an error at its line and column | A misspelled key must not silently build something else (kept from e05a164) |
| `deep-merge` throws "failed to merge config value" for a list, or a map, vector or set over a value of another kind | the upper value replaces the lower one | "Precedence": anything else is replaced, so `LIGHT_CLJS` can turn `:nrepl false` into `{:port 0}` and a `:config` list can be changed; the merged value is validated as any other |
| Defaults: `:fs-watch {:loop-wait 500}`, `:compile-threads` the processor count | 100 ms; 1 | Today's `--poll-ms` default; `--jobs` is experimental and did not meet its gate (compiler-concurrency.md) |
| The config watch compares a build's old and new configuration with `=` | Slot by slot by EDN text: a list for a vector, `1.0` for `1`, `-0.0` for `0` or a reordered map restarts the build | Each is visible to a macro through `light.build/config`, which shadow-cljs has no counterpart of, so treating them as equal would keep a stale compiler input ("As built (light-cljs-404)") |
| A cache per build and mode, `.shadow-cljs/builds/<id>/<mode>` | One content-addressed store, `<:cache-root>/cache`, shared by every build and mode | Artifacts are keyed by their inputs (compiler-cache.md), so sharing is safe and saves work |
| No retention count: each build's cache holds its one configuration | Top-level `:cache-retention N`, as `--cache-retention N`, three by default | The shared store keeps N artifacts per namespace; the Phase 3 test project's five builds give its shared namespace five keys and evict each other at three ("As built (light-cljs-409)") |

## Goal

- One file says how to build the project, so a command names builds
  instead of repeating `--entry`, `--source-root`, `--out-dir`, `--config`,
  `--serve` and the macro limits.
- A shadow-cljs user reads the file and the commands without a glossary.
- Every existing flag-only command keeps working unchanged. A command that
  starts with a flag never reads the file.
- A build from the file compiles exactly what the equivalent flags
  compile: the same output bytes and the same cache keys. The cache never
  sees the file, only the compiler inputs the file produced.
- The file is strict: an unknown key, a misplaced key or a bad value is an
  error at its line and column, with a suggestion where one exists.

Non-goals: dependency resolution, build hooks, a per-user configuration
file, and settings that no flag has today (each new setting is a flag
first, then a key; see "Design principles").

## What shadow-cljs does

Citations are into `research/shadow-cljs/src/main/shadow/`.

- **One file, found by walking up.** The npm launcher walks up from the
  current directory to the first `shadow-cljs.edn` (`cljs/npm/cli.cljs:143-157`)
  and runs the JVM in that directory, where `load-cljs-edn` reads it
  (`cljs/devtools/config.clj:143-156`), so every relative path, in the file
  and on the command line, is relative to the project directory.
- **Top-level keys**: `:source-paths`, `:dependencies`, `:deps`, `:builds`,
  and process settings: `:cache-root` (default `.shadow-cljs`, `:92-95`),
  `:cache-blockers` (a set, unioned into every build's,
  `cljs/devtools/server/util.clj:80-104`), `:compile-threads` (the build
  executor's size, `cljs/devtools/server/common.clj:76-83`), `:http {:port
  :host :strict}` (the server, port 9630, trying the next port when one is
  taken unless `:strict`, `cljs/devtools/server.clj:158-195`), `:nrepl
  {:port :write-port-file}` or `false` (`:265-375`), `:dev-http` and
  `:fs-watch {:impl :loop-wait}` (`fswatch.clj`, `fswatch/common.clj:33`).
- **A build** has its id as key and gets it assoc'ed as `:build-id`
  (`cljs/devtools/config.clj:56-90`); it needs `:target` and has
  `:output-dir`, `:asset-path`, `:modules` (`:entries`, `:init-fn`,
  `:depends-on`), `:compiler-options` (`:optimizations`, `:pseudo-names`,
  `:elide-asserts`, `:checked-arrays`, `:warnings`, ...), `:closure-defines`
  (merged into the compiler options' own, `build.clj:405-412`),
  `:build-options` (`:cache-level :all|:jars|:off`, `:cache-blockers`,
  `build/api.clj:100-116`), `:devtools` (`:reload-strategy`, `:after-load`,
  ..., `build/targets/shared.clj:20-60`), `:js-options` and `:build-hooks`.
- **Modes and commands.** `shadow-cljs compile ID...` and `watch ID...` are
  `:dev`, `release ID...` is `:release`; ids are written with or without
  their colon (`cljs/devtools/cli_opts.cljc:155-162`). The layers are build
  defaults, target defaults, the build, its mode map, then each
  `--config-merge DATA` (an EDN map, a file, or `classpath:`)
  (`build.clj:172-178`, `:329-334`, `cli_opts.cljc:21-73`). `deep-merge`
  merges maps by key, concatenates vectors keeping distinct elements and
  unions sets (`build/api.clj:26-40`).
- **Environment values.** `#shadow/env "NAME"`, `["NAME"]`, `["NAME"
  "default"]` and `["NAME" :as :int|:bool|:symbol|:keyword :default V]`
  read environment variables, an unset one being nil
  (`cljs/config_env.cljc`); the `SHADOW_CLJS` environment variable holds
  EDN deep-merged over the whole file (`cljs/devtools/config.clj:103-156`).
  Penpot's file uses `#shadow/env` for ports, the cache directory, the
  optimization level and a version define
  (`research/penpot/frontend/shadow-cljs.edn`).
- **Validation is open**: `s/keys` accepts any extra key and the default
  target spec is `any?` (`cljs/devtools/config.clj:46-54`,
  `build/config.clj:21-33`).
- **Several builds in one process.** `watch app worker` runs both workers
  in one JVM behind one server and one nREPL, and the server watches
  `shadow-cljs.edn` and restarts a build whose configuration changed
  (`cljs/devtools/server/config_watch.clj`). An nREPL session picks a build
  with `(shadow.cljs.devtools.api/repl :app)` (`cljs/devtools/api.clj:451`).

## Design principles

20. **The file is a stored set of flags.** Every setting the file holds is
    a flag that exists, with the same meaning and validation. A build from
    the file is translated into the flags' own structures (`CompileFlags`
    and the watch's `Options` in `src/main.rs` and `src/dev_watch.rs`)
    before anything compiles. A new setting is a flag first; its key
    follows in the same change.
21. **Shadow-cljs's word wherever it has one.** A key, value, command or
    behavior shadow-cljs has is used as shadow-cljs has it. A light-cljs
    concept shadow-cljs lacks gets a short kebab-case name after its flag,
    under the map shadow-cljs would put it in.
22. **Flags win.** A flag on the command line overrides every layer for its
    setting. Cache blockers accumulate instead (see "Precedence").
23. **A subcommand reads the file; a flag-only command never does.**
    `light-cljs --compile --entry ...` with a `light-cljs.edn` present
    behaves exactly as today.
24. **Strict and located.** The key sets are closed. Every error names the
    file, line and column, the build and the mode, and what would be valid.
    The whole file is validated each time it is read.

Principles 1-19 are those of advanced-optimization.md, code-splitting.md
and dev-workflow.md; the numbering continues theirs.

## The file and where it is found

The file is `light-cljs.edn`, after the tool, as `shadow-cljs.edn` is. Its
directory is the **project directory**.

- **Discovery.** A subcommand looks for `light-cljs.edn` in the current
  directory, then in each parent up to the file system root, and uses the
  first one found, as shadow-cljs's launcher does. No boundary (such as a
  `.git` directory) stops the walk. The file used is named in the first
  status line and in every error about it.
- **The project directory is the working directory.** As shadow-cljs runs
  its JVM there, every relative path resolves against it: paths in the
  file, and paths in flags after a subcommand (`--out-dir`,
  `--source-root`, `--cache-dir`, `--config-merge FILE`). `.nrepl-port` is
  written there.
- **Not found** is an error naming where it looked: `light-cljs release
  app: no light-cljs.edn in /work/site/src or any parent directory; create
  one (docs/build-configuration.md)`.
- There is no flag that names the file: like shadow-cljs, change to the
  project's directory. Tests and scripts set the child's working directory.

## Commands

Shadow-cljs's shape, subcommands with build ids:

| Command | Mode | Today's equivalent, per build |
|---|---|---|
| `light-cljs compile ID [ID ...] [FLAGS]` | dev | `--compile --reloadable --entry NS --source-root ... --out-dir DIR` |
| `light-cljs watch ID [ID ...] [FLAGS]` | dev | `--watch --entry NS --source-root ... --out-dir DIR --serve PORT` |
| `light-cljs release ID [ID ...] [FLAGS]` | release | `--compile --optimize advanced --entry NS --source-root ... --out-dir DIR` |
| `light-cljs cljs-repl ID [FLAGS]` (Phase 3) | - | `--repl --connect 127.0.0.1:PORT`, PORT the running watch's, then `(light.dev/repl :ID)` |

**Why subcommands, not flags.** e05a164 chose `--watch --build app` to fit
the flag CLI. Shadow-cljs's shape wins here because nothing compelling
argues against it: several build ids read naturally as positional
arguments (`watch app worker`); the mode is the command, so `release` can
default to `:advanced` as shadow-cljs's does without changing what
`--compile` means; and principle 23 gets a visible boundary, a subcommand
reads the file and a leading flag never does. The flag CLI stays exactly as
it is, as the scripting and test interface.

The one spelling that changes: today a first argument that is not a flag
is a FILE to tokenize, so `light-cljs compile` lexes a file named
`compile`. The words `compile`, `watch`, `release` and `cljs-repl` become
commands in that position; such a file is still read as `./compile`. No test or script
in the repository does this.

Parsing:

- Build ids follow the command, up to the first argument that starts with
  `-`. An id is written without its colon, and a leading colon is dropped,
  as shadow-cljs drops it (`release :app`). No id is an error, as in
  shadow-cljs: `light-cljs release: requires one or more build ids;
  /work/site/light-cljs.edn defines :app, :test, :worker`.
- **Flags after the ids** are today's flags for that mode, and override the
  file for every build named: `compile` takes what `--compile --reloadable`
  takes, `watch` what `--watch` takes, `release` what `--compile` takes,
  plus `--warnings` and `--checked-arrays-*` (today's prefix flags) where
  they apply. `--entry` and `--out-dir` are refused with more than one id,
  since two builds cannot share them. `--source-map*` and `--esm` are
  refused, as `--out-dir` refuses them today.
- **New flags**, valid after any command: `--config-merge DATA`
  (shadow-cljs's, see "Environment values and --config-merge") and
  `--print-config`. `release` also takes `--pseudo-names`, which exists.
- **`--print-config`** prints, for each build named, the configuration the
  command would run with, as EDN a `light-cljs.edn` could hold: the
  top-level settings the mode reads and the one build, merged, with each
  value's origin in a comment, then exits without compiling. Shadow-cljs
  has no such command; it is how a user checks precedence. For the
  illustration below, `APP_VERSION=2.4.0 light-cljs release app --optimize
  none --print-config` prints

  ```clojure
  ;; build :app, release mode, from /work/site/light-cljs.edn
  {:source-paths ["src" "test"]                        ; light-cljs.edn:1:16
   :cache-root ".light-cljs"                           ; default
   :cache-blockers #{app.build-info}                   ; light-cljs.edn:2:18
   :compile-threads 1                                  ; default
   :builds
   {:app
    {:target :esm                                      ; light-cljs.edn:7:12
     :output-dir "out/app"                             ; light-cljs.edn:8:16
     :modules {:main {:entries [app.main]}}            ; light-cljs.edn:9:13
     :config {:app/api-origin "https://api.example.org"} ; light-cljs.edn:10:29
     :compiler-options {:optimizations :none           ; --optimize none, over the release default :advanced
                        :elide-asserts true            ; light-cljs.edn:16:48 (:release)
                        :macro-timeout-ms 30000}       ; light-cljs.edn:11:41
     :build-options {:cache-level :all}                ; default
     :release {:closure-defines {app.config/DEBUG false ; light-cljs.edn:14:49 (:release)
                                 app.config/VERSION "2.4.0"}}}}} ; light-cljs.edn:15:51 (:release), from APP_VERSION
  ```

  The keys that are errors in dev mode, `:closure-defines` and
  `:checked-arrays`, print under `:release`, so the printed EDN reads back
  as a valid file ("As built (light-cljs-401)").
- **`light-cljs cljs-repl ID`** (Phase 3) is shadow-cljs's command for a
  REPL into a running watch's build. It reads the file for `:cache-root`
  and the build ids, reads the port the running watch wrote to
  `<:cache-root>/http.port` (see `:http`), and runs today's `--repl
  --connect 127.0.0.1:PORT` client with its session bound to build ID, as
  if its first form were `(light.dev/repl :ID)`. It takes the `--repl`
  client's flags (`--print-length`, `--print-level`). Errors: an id the
  file does not define (the unknown-id message of "Errors"); no port file
  or a refused connection, `light-cljs cljs-repl app: no watch is running
  for /work/site/light-cljs.edn; start one with light-cljs watch app`; and
  an id the running watch does not run, naming the ids it does. Unlike
  shadow-cljs, it never starts a watch itself (see "Deviations").

## Schema

The file is one EDN map, read with the compiler's syntax reader
(`parser::parse`), which gives every form a byte span for messages. Its
one tagged literal is `#light-cljs/env`; no other tag, no reader
conditional, and no discard that hides a duplicate key.

### Flags and keys

| Flag | Key | Where | Modes |
|---|---|---|---|
| `--source-root DIR` | `:source-paths ["DIR" ...]` | top level | both |
| `--entry NS` | `:modules {:main {:entries [NS]}}` | build | both |
| `--out-dir DIR` | `:output-dir "DIR"` | build | both |
| `--optimize LEVEL` | `:compiler-options {:optimizations :none\|:prepare\|:advanced}` | build | release (ignored in dev, as shadow-cljs ignores it) |
| `--define NS/NAME=VALUE` | `:closure-defines {NS/NAME VALUE}`, or under `:compiler-options` | build | release |
| `--pseudo-names` | `:compiler-options {:pseudo-names true}` | build | release (ignored in dev) |
| `--elide-asserts` | `:compiler-options {:elide-asserts true}` | build | both |
| `--checked-arrays-warn`, `--checked-arrays-error` | `:compiler-options {:checked-arrays :warn\|:error}` | build | release |
| `--warnings` | `:compiler-options {:cljs-warnings true}` | build | release (dev implies it) |
| `--macro-expansions N`, `--macro-nodes N`, `--macro-timeout-ms N`, `--macro-memory-limit N`, `--macro-poll-budget N` | `:compiler-options {:macro-expansions N ...}`, the flag's name as key | build | both |
| `--config KEY=EDN` | `:config {KEY EDN}` | build | both |
| `--jobs N` | `:compile-threads N` | top level | both (the watch runs one slot) |
| `--cache-dir DIR` | `:cache-root "ROOT"`, the store in `ROOT/cache` | top level | both |
| `--cache-retention N` | `:cache-retention N` | top level | both |
| `--no-cache` | `:build-options {:cache-level :off}` | build | both |
| `--cache-blocker NS` | `:cache-blockers #{NS}`, and `:build-options {:cache-blockers #{NS}}` | top level, build | both |
| `--poll-ms N` | `:fs-watch {:loop-wait N}` | top level | `watch` |
| `--serve PORT`, `--serve-host HOST` | `:http {:port PORT :host "HOST"}` | top level | `watch` |
| `--nrepl PORT` | `:nrepl {:port PORT}` | top level | `watch` |
| `--no-nrepl` | `:nrepl false` | top level | `watch` |
| `--reload-dependents` | `:devtools {:reload-strategy :full}` | build | `watch` |
| `--compile`, `--reloadable`, `--watch` | the command: `compile`, `watch`, `release` | - | - |

Keys light-cljs adds, because shadow-cljs has no word for them:
`:compiler-options` `:cljs-warnings` and the five `:macro-*` limits, the
build's `:config`, and the top-level `:cache-retention` (light-cljs-409).
They are listed under "Names for review".

### Top-level keys

| Key | Value | Default |
|---|---|---|
| `:source-paths` | vector of directory strings | required |
| `:builds` | map of build id to build configuration, at least one | required |
| `:cache-root` | directory string | `".light-cljs"`; the store is `.light-cljs/cache`, today's default |
| `:cache-blockers` | set of namespace symbols | `#{}` |
| `:compile-threads` | positive integer (experimental) | 1 |
| `:cache-retention` | positive integer: the artifacts the store keeps per namespace | 3 |
| `:http` | `{:port N :host "HOST" :strict BOOL}` | `{:port 9630 :host "127.0.0.1"}` |
| `:nrepl` | `{:port N}` or `false` | nREPL on the `:http` port only |
| `:fs-watch` | `{:loop-wait N}`, milliseconds, positive; `:impl :polling` is accepted, the only one | `{:loop-wait 100}` |

- `:source-paths` is the search path of every build, as in shadow-cljs: a
  test build's namespaces sit in a path every build can resolve, and only
  what an entry requires is read. `--source-root` replaces the list.
- `:cache-root` is light-cljs's state directory, as `.shadow-cljs` is
  shadow-cljs's. The store under it is content-addressed by its keys
  ([compiler-cache.md](compiler-cache.md)), so every build and mode share
  it safely and two builds with the same front-end inputs share artifacts.
  It is safe for concurrent processes (compiler-cache, "Loading,
  diagnostics, and storage"). `--cache-dir DIR` names the store itself.
- `:cache-blockers` is unioned into every build's, as shadow-cljs unions it.
- `:cache-retention N` is how many artifacts the shared store keeps per
  namespace after each build, `--cache-retention N`'s value, three by
  default. Builds of one file with different front-end inputs (a `:config`
  per build, dev and release, another entry requiring a shared namespace)
  each need one, and a store holding fewer evicts what the next build of
  another needs ("As built (light-cljs-409)"). It is a top-level key, not a
  build key, because the store is one: the last build to trim it decides.
- `:http` is the dev server of `watch`, which always starts it, as
  shadow-cljs's server always runs: port 9630 by default and, when the port
  is taken, the next free one upward unless `:strict true`, as shadow-cljs
  does. Port 0 picks a free port. `--serve PORT` given on the command line
  binds that port or fails, as today. The server serves each build's
  `:output-dir` and the source paths (for source maps), events, REPL
  requests and nREPL on the same port (see "Several builds in one
  process"). Once bound, `watch` writes the port to
  `<:cache-root>/http.port` and removes the file when it exits, as
  shadow-cljs writes `.shadow-cljs/http.port` (`cljs/devtools/server.clj:148`);
  `cljs-repl` reads it. A flag-only `--watch` writes no such file.
- `:nrepl {:port N}` adds the separate nREPL listener of `--nrepl` and
  writes `.nrepl-port`; `false` turns nREPL off on the `:http` port too.

### Build ids

A build id is a simple keyword: a letter followed by letters, digits and
hyphens (`:app`, `:worker`, `:test`). Builds get dense integer ids,
`BuildId`, in ascending id order (principle 1 of dos-and-donts.md), which
the several-build watch indexes its per-build state by; keyword ids appear
only in the file, the commands and messages.

### Build keys

| Key | Value | Required |
|---|---|---|
| `:target` | `:esm` | yes |
| `:output-dir` | directory string | yes |
| `:modules` | `{ID {:entries [NS]}}`: one module, conventionally `:main`, with one namespace | yes |
| `:config` | map of keyword to EDN value | no |
| `:compiler-options` | map, below | no |
| `:closure-defines` | map of qualified symbol to boolean, number or string | no; release only |
| `:build-options` | `{:cache-level :all\|:off :cache-blockers #{NS ...}}` | no |
| `:devtools` | `{:reload-strategy :full}` | no; `watch` only |
| `:dev`, `:release` | a map of any of the keys above but `:dev` and `:release` | no |

- **`:modules`** names the build's entry, the one shadow-cljs concept for
  it. Its one module's `:entries` holds the namespace `--entry` takes. The
  release output is what `--out-dir` writes: the startup chunk
  `NAMESPACE.js`, whose default export is the entry value, one chunk per
  loadable group and `chunks.json`; the module id appears in messages only.
  A second module, a second entry, `:init-fn`, `:depends-on`, `:exports`
  and `:prepend` are errors that point to code-splitting.md "Phase 4:
  shaping and tooling", where named chunk groups (`--chunk NAME:ns,ns`)
  become further `:modules` with `:entries` and `:depends-on`.
- **`:output-dir`** is shared by both modes unless a mode map overrides
  it, as in shadow-cljs. Each mode writes its own files and leaves others
  alone (the dev build's `manifest.json` and units beside release's
  `chunks.json`); a project that wants them apart writes `:dev
  {:output-dir "out/dev"}`.
- **`:config`** follows `--config`'s rules exactly: keywords, decimal
  integers and floats, strings, booleans, nil and lists, vectors, maps and
  sets of them, with no symbols, tagged literals other than
  `#light-cljs/env`, `1N`, `1M`, radix or ratio numbers, and no key or set
  element repeated by value. Each value is printed in canonical EDN and
  handed to the same reader (`BuildConfiguration::insert`), so `:config
  {:app/flavor :dev}` and `--config app/flavor=:dev` give the same
  canonical text and the same fingerprint.
- **`:closure-defines`** may also sit under `:compiler-options`, as in
  shadow-cljs; the build's own map is merged over that one. Each entry is
  translated as `--define NS/NAME=TEXT` with TEXT the value's characters, so
  the goog-define's own type decides, and a define naming no `goog-define`
  fails the build with the flag's message.
- **`:build-options`**: `:cache-level :off` is `--no-cache`; shadow-cljs's
  `:jars` is an error, since there are no jars.

### Compiler options

| Key | Value | Default |
|---|---|---|
| `:optimizations` | `:none`, `:prepare` or `:advanced` | `:advanced` in release, as shadow-cljs's; dev builds run at none semantics |
| `:pseudo-names` | boolean | `false` |
| `:elide-asserts` | boolean | `false` |
| `:checked-arrays` | `:warn` or `:error`; implies `:cljs-warnings`, as the flags do | unset |
| `:cljs-warnings` | boolean: undeclared Vars and conflicting arities are warnings, not errors | `false` in release, implied in dev |
| `:closure-defines` | as the build key | `{}` |
| `:macro-expansions` | positive integer | 1024 |
| `:macro-nodes` | positive integer | 100000 |
| `:macro-timeout-ms` | positive integer | 10000 |
| `:macro-memory-limit` | positive integer, bytes; one budget for all isolates under `:compile-threads` (compiler-concurrency.md, "Memory is one budget") | 67108864 |
| `:macro-poll-budget` | positive integer | 1000000 |

Shadow-cljs's other compiler options (`:infer-externs`, `:externs`,
`:output-feature-set`, `:output-wrapper`, `:source-map`, `:warnings`,
`:warnings-as-errors`, `:fn-invoke-direct`, ...) are errors that say there
is no equivalent and, where one is close, name it (`:warnings` names
`:cljs-warnings`; `:source-map` explains that chunked output has none
yet).

### Modes

`:dev` and `:release` are deep-merged over the build for their mode and
dropped, as shadow-cljs's `config-merge` does; both are validated whenever
the file is read. A key applies in the modes the table gives. A key that
reaches a mode that does not use it is ignored where shadow-cljs ignores it
(`:optimizations` and `:pseudo-names` in dev, `:devtools` in `compile` and
`release`) and an error where shadow-cljs would apply it but light-cljs
cannot (`:closure-defines` and `:checked-arrays` in dev, a permanent
deviation, see "Deviations").
Dev mode means the same for `compile` and `watch`: the settings both
commands' flags accept.

### Not in the file

| Flag | Reason |
|---|---|
| `--stats`, `--stats-file`, `--size-report` | They report on one run and change no output |
| `--source-map`, `--source-map-inline`, `--source-map-include-sources`, `--esm` | Builds from the file write directories, which refuse source maps and imply ES modules; a key arrives with chunk source maps |
| `--parse`, `--analyze`, `--text`, FILE, `--eval`, `--repl` | Not builds |

### A full illustration

A project with a page, a web worker and a test runner:

```clojure
{:source-paths ["src" "test"]
 :cache-blockers #{app.build-info}
 :http {:port #light-cljs/env ["HTTP_PORT" :as :int :default 9630]}
 :nrepl {:port 0}
 :builds
 {:app
  {:target :esm
   :output-dir "out/app"
   :modules {:main {:entries [app.main]}}
   :config {:app/api-origin "https://api.example.org"}
   :compiler-options {:macro-timeout-ms 30000}
   :devtools {:reload-strategy :full}
   :dev {:config {:app/api-origin "http://127.0.0.1:8080"}}
   :release {:closure-defines {app.config/DEBUG false
                               app.config/VERSION #light-cljs/env ["APP_VERSION" "dev"]}
             :compiler-options {:elide-asserts true}}}
  :worker
  {:target :esm
   :output-dir "out/worker"
   :modules {:main {:entries [app.worker]}}}
  :test
  {:target :esm
   :output-dir "out/test"
   :modules {:main {:entries [app.test-runner]}}
   :compiler-options {:optimizations :none}}}}
```

`light-cljs release app` compiles `app.main` at `:advanced` with `DEBUG`
false, `VERSION` from `APP_VERSION` (or `"dev"`) and no assertions into
`out/app`. `light-cljs watch app worker` writes both dev builds and serves
them on `HTTP_PORT` (or 9630): the app at `/` and `/app/`, the worker at
`/worker/`, so the page's `new Worker("/worker/index.js", {type:
"module"})` is same-origin, as a `Worker` requires; both reload on save,
and one nREPL listener serves both.

## Environment values and --config-merge

### #light-cljs/env

Shadow-cljs's `#shadow/env`, named after the tool, with its forms:

| Form | Value |
|---|---|
| `#light-cljs/env "NAME"`, `#light-cljs/env ["NAME"]` | the variable's string, or nil when unset |
| `#light-cljs/env ["NAME" DEFAULT]` | the variable's string, or DEFAULT, any EDN value, when unset |
| `#light-cljs/env ["NAME" :as TYPE :default DEFAULT]` | the variable converted by TYPE: `:int` (a decimal integer, else an error at the tag), `:bool` (`"true"` is true, anything else false, as shadow-cljs), `:symbol`, `:keyword`; DEFAULT when unset; `:as` and `:default` are each optional |

- The tag may stand for any value in the file. It is resolved when the
  file is read, against the process's environment, and the value is
  indistinguishable from a literal written there: validation and
  translation see only the value, and keep the tag's span and the
  variable's name for messages and `--print-config`.
- Every tag in the file is resolved, but a nil from an unset variable
  without a default is checked only in the build and mode that run, so a
  release-only variable need not be set for `watch`.
- The environment is read once per process. A watch re-reads the file when
  it changes, against the same environment; a changed variable needs a new
  process, as in shadow-cljs.

### LIGHT_CLJS and --config-merge

- **`LIGHT_CLJS`**, as shadow-cljs's `SHADOW_CLJS`: when set and non-empty,
  its EDN text, a map of the file's own shape, is deep-merged over the whole
  file before a build is selected. For containers and CI.
- **`--config-merge DATA`**, as shadow-cljs's: DATA starting with `{` is
  EDN text, anything else a file path relative to the project directory
  (`classpath:` is an error: there is no classpath). It must be a map of
  build keys, deep-merged over each build named after its mode map, in the
  order given; `:dev` or `:release` inside it is an error, since the mode
  has already been chosen. `--config-merge '{:compiler-options
  {:elide-asserts false}}'` is how a run turns off a boolean that no flag
  negates.

Both may hold `#light-cljs/env`, and both are validated as strictly as the
file, with their own origin in messages (`LIGHT_CLJS:1:14`,
`--config-merge 2:1:30`).

## Precedence

From lowest to highest, each layer deep-merged over the ones before:

1. The built-in defaults: today's flag defaults, with release's
   `:optimizations :advanced` and watch's `:http {:port 9630}`.
2. The file, then `LIGHT_CLJS` over it.
3. The build.
4. The build's `:dev` or `:release` map.
5. Each `--config-merge`, in order.
6. Flags on the command line.

Layers 2-5 merge with shadow-cljs's `deep-merge`: maps by key, vectors
concatenated keeping distinct elements, sets unioned, anything else
replaced. The result is then validated, so a merge that adds a second
`:entries` namespace is an error like any other. Flags then set their one
setting each:

| Flag | Effect |
|---|---|
| Scalar flags (`--entry`, `--out-dir`, `--optimize`, `--jobs`, `--cache-retention`, `--poll-ms`, `--serve`, the macro limits, ...) | Replace the value |
| `--source-root` (repeated) | The list replaces `:source-paths` |
| `--config K=V`, `--define NS/NAME=V` | Set that key; the flag naming one key twice is still an error |
| `--cache-dir DIR` | Names the store, and turns the cache on over `:cache-level :off`; `--no-cache` turns it off |
| `--cache-blocker NS` | Unioned with the file's blockers |

**Cache blockers accumulate**: no layer and no flag removes one, since a
blocker declares impure macros and dropping it could reuse a stale
artifact. Shadow-cljs's set union already behaves so.

**Exclusions are checked after merging**, with both origins in the
message: `release app --serve 0` fails as `--compile --serve` does today,
and `:devtools` outside a build fails at its line. Two flags that exclude
each other (`--no-cache --cache-dir`) still do; a flag and a file value
never conflict, since the flag wins.

## Cache keys and fingerprints

The configuration file does not enter any cache key, and it cannot weaken
the cache contract, because the cache never sees it. The file, `LIGHT_CLJS`
and `--config-merge` are merged and translated into the flags' structures
before analysis starts; the keys ([compiler-cache.md](compiler-cache.md),
"Conservative keys and invalidation") are computed from those structures as
for any flag-only build:

| Setting | Where it enters | Key component |
|---|---|---|
| `:config` | `BuildConfiguration`, `MacroHost::set_build_configuration` | Config: the SHA-256 of the whole map's canonical text, in every namespace's front-end key and the `:config` broad view |
| `:elide-asserts`, `:cljs-warnings`, `:checked-arrays`, dev mode's warning mode | `AnalysisOptions`, `MacroHost::set_assertions` | Options |
| `:macro-expansions`, `:macro-nodes`, `:macro-memory-limit` | `MacroLimits`, `EvaluatorOptions` | Options (the logical limits and the memory limit) |
| `:macro-timeout-ms`, `:macro-poll-budget` | `EvaluatorOptions` | None, as for the flags: they stop work, which is never stored |
| `:source-paths` | `FileSourceProvider` roots | Resolution: every candidate path and the file found |
| `:optimizations`, `:closure-defines`, `:pseudo-names`, `:output-dir`, `:modules` | `ApplicationOptions`, `OutputLayout`, the entry | Emission key only (recorded as zeros today; compiler-cache "As built (light-cljs-324)"); the entry is the build's root, not a key |
| `:cache-root`, `:cache-blockers`, `:cache-level` | `CacheOptions`, the policy evaluator | Policy: what may read or write, never a key |
| `:cache-retention` | `CacheOptions::retention`, `Store::trim` | None: what the store keeps after a build, never a key |
| `:compile-threads` | `scheduler::Slots` | None: output does not depend on it |
| `:http`, `:nrepl`, `:fs-watch`, `:devtools` | the watch's `Options` | None |
| Build id, mode, file path, file text, `LIGHT_CLJS` text | nowhere | None |

### Environment values in the key

An environment value is a compiler input exactly when the setting it lands
in is, and it enters the key exactly as a literal in its place would:

- **Through its value, after conversion.** The key component of the
  setting it lands in (the table above) is computed from the resolved
  value. With `APP_FLAVOR=dev`, `:config {:app/flavor #light-cljs/env
  "APP_FLAVOR"}` contributes the canonical text `"dev"` (a string) to the
  Config fingerprint, and `#light-cljs/env ["APP_FLAVOR" :as :keyword]`
  contributes `:dev`. The second gives the same fingerprint as `:config
  {:app/flavor :dev}` and as `--config app/flavor=:dev`, and shares their
  artifacts; the first does not, because the value differs.
- **Through nothing else.** The variable's name, whether the default was
  used, `LIGHT_CLJS`'s text and every other variable enter no key. Setting
  or changing a variable the file does not read misses nothing; changing
  one it reads changes exactly the component of its setting: for `:config`,
  every namespace's front-end key misses with "config changed", as a new
  `--config` value does; for `:closure-defines`, only the emission key; for
  `:http :port`, nothing.
- **Values `--config` refuses are refused.** A `:config` value from
  `:as :symbol`, or a number outside JavaScript's range, is an error at the
  tag, before any key is computed.
- **Macros still cannot read the environment.** The cache contract forbids
  ambient inputs (compiler-cache.md, "Cache contract"); `#light-cljs/env`
  into `:config` is the sanctioned path, read by `light.build/config`, and
  keyed.

`--config-merge` values enter the same way: a merged value is a value in
its place.

### Consequences

- **A build from the file and the same flags hit the same artifacts.** A
  build from the file spells its roots as the project directory joined with
  the path, lexically normalized, so it gets the same keys from any
  subdirectory; a flag-only build spells them as typed. Either way the
  compiler canonicalizes each root before resolving (`analyze_from_roots`),
  so a root spelled relative or absolute gets the same resolution key
  (light-cljs-402 found this; the design said a root spelled two ways would
  miss). A root that names a different directory gives a "resolution
  changed" miss, never a wrong hit.
- **Editing the file recompiles only what its new values change.** A
  comment, another build's section, `:http`, `:devtools` or `:output-dir`
  changes no front-end key.
- **Builds do not partition the cache.** Two builds with equal front-end
  inputs share artifacts; dev and release of one build differ in warning
  mode, so each namespace has two front-end keys, and three once the
  build is also watched (light-cljs-409 found that the watch's startup
  build runs its macros in the session's engine, which the compiler
  component covers, so `watch` and `compile` of one build do not share).
  The store keeps three
  artifacts per namespace (compiler-cache, "Loading, diagnostics, and
  storage"), which a several-build watch makes easier to exceed. Phase 3
  measured that the builds of the test project do evict each other, and
  added `:cache-retention N` ("As built (light-cljs-409)").
- **The compiler key already covers the reader.** The file's reader and
  translation live under `src/`, which `build.rs` hashes.

The tests that hold this: for every key, compile the build from the file
and then the equivalent flags with the same absolute roots and cache
directory; the outputs are byte-identical and the second run reports every
namespace a hit (`--stats`, `cache.*` lines). For environment values, a
`:config` value from `#light-cljs/env` with `:as :keyword` and the same
literal hit each other; changing the variable misses with "config
changed"; changing an unread variable misses nothing.

## Errors

Every error is `PATH:LINE:COLUMN: message`, with the line and column from
the reader's spans (`LineIndex`), names the build and the mode where they
apply, and fails before anything compiles or writes. The whole file is
validated each time it is read; filesystem checks (a source path that is
not a directory) and unset environment values apply to the builds and mode
that run.

| Case | Message |
|---|---|
| Unknown key, close to a valid one | `light-cljs.edn:8:4: unknown key :output-path in build :app; did you mean :output-dir?` |
| A shadow-cljs key with no equivalent | `light-cljs.edn:1:2: :dependencies: light-cljs resolves no dependencies; put library sources in :source-paths, and let the host or a bundler supply JavaScript packages` |
| `:dev-http`, `:asset-path`, `:js-options`, `:build-hooks` | the reason from "Deviations", and what to use instead |
| `:target :browser` | `light-cljs.edn:7:12: :target of build :app must be :esm: light-cljs writes ES modules, which run in browsers, Node and bundlers` |
| A second module or entry, `:init-fn`, `:depends-on` | `light-cljs.edn:9:13: build :app has two modules; chunks are inferred from light.lazy/loadable sites (docs/code-splitting.md), and named chunks arrive with its Phase 4` |
| A key in the wrong place | `light-cljs.edn:12:4: :http in build :app is a top-level key, as in shadow-cljs` |
| A dev-only or release-only key reaching the other mode, where it is an error | `light-cljs.edn:13:10: :closure-defines of build :app applies in dev mode, whose dev build takes no defines; write it under :release` |
| A bad value | `light-cljs.edn:11:41: :optimizations of build :app must be :none, :prepare or :advanced, got :simple` |
| A bad `:config` value | `--config`'s own message, at the value: `light-cljs.edn:10:29: :config :app/n of build :app: "1N" is not a configuration number: write a decimal integer or floating-point number, such as 3, -2.5 or 1e3` |
| `#shadow/env`, `#env`, another tag | `light-cljs.edn:3:15: #shadow/env: write #light-cljs/env, which takes the same forms` |
| A malformed `#light-cljs/env` | `light-cljs.edn:3:15: #light-cljs/env takes "NAME", ["NAME"], ["NAME" DEFAULT] or ["NAME" :as TYPE :default DEFAULT]` |
| An unset variable where a value is required | `light-cljs.edn:15:51: :closure-defines app.config/VERSION of build :app (release) is nil: APP_VERSION is unset and the form gives no default` |
| `:as :int` on a non-integer | `light-cljs.edn:3:15: HTTP_PORT is "web", not a decimal integer (:as :int)` |
| `--config-merge` not a map, `classpath:`, `:dev` inside it | the message, at `--config-merge N` and its line and column: `--config-merge 1:1:1: --config-merge must be a map of build keys, such as {:output-dir "out/x"}, got [1 2]`; `--config-merge 1: classpath:demo.edn: light-cljs has no classpath; give EDN text starting with { or a file path relative to the project directory`; `--config-merge 1:1:2: :dev in --config-merge: the command chooses the mode (compile and watch run :dev, release runs :release), so --config-merge holds the build keys themselves` |
| `--config-merge` adding a key that is an error in dev mode | `--config-merge 2:1:2: :closure-defines of build :app applies in dev mode, whose dev build takes no defines; merge it into release builds only` |
| Two builds, one directory | `light-cljs.edn:19:16: builds :app and :worker both write out/app in dev mode` |
| An output directory equal to the cache store or a source path | `light-cljs.edn:8:16: build :app writes out/cache in release mode, the cache store of :cache-root "out"; give it a directory of its own`, and `..., the source path "src"; give it a directory of its own` |
| A duplicate key, a parse error | the reader's message, at its span |
| An unknown build id | `light-cljs release ap: /work/site/light-cljs.edn defines no build :ap; did you mean :app? Its builds are :app, :test, :worker` |
| `--entry` with two ids | `light-cljs watch app worker --entry x: --entry and --out-dir name one build's entry and directory; give one build id` |

"Did you mean" uses the edit distance against the closed key set of the
map the key sits in, after a table of shadow-cljs keys with no equivalent;
`:output_dir` and `:outputDir` get the suggestion too.

## Several builds in one process

`light-cljs watch app worker` runs both builds in one process, sharing one
dev server and one nREPL listener, as shadow-cljs does. `compile` and
`release` with several ids build them one after another in the order
named, stopping at the first failure with a nonzero status.

- **A thread per build.** Each build owns its `DevSession`, macro engine
  and retained program on a thread of its own: a V8 isolate is bound to
  the thread that made it, and one build's step must not wait for
  another's. Startup builds run one after another in the order named, so
  peak memory is one analysis beside the finished programs; then every
  build steps independently. Memory is the sum of the builds' programs
  (about 1 GB each on Penpot), and `:macro-memory-limit` is per build.
- **One poller.** One thread scans `:source-paths` (shared, so scanned
  once) and `light-cljs.edn` every `:loop-wait`, and sends each build the
  changed files; each build's dependency manifest decides whether a file
  is its own. A save of a shared namespace recompiles in each build that
  has it, concurrently.
- **One server.** `:http` serves each build's `:output-dir` under
  `/<id>/`, and the first build named also at `/`, so a single-build watch
  keeps today's URLs. The events, reply and REPL endpoints are per build
  under the same prefix; `client.js` resolves them relative to its own URL,
  so a dev build's bytes do not depend on where it is mounted. Status lines,
  stderr and the overlay name the build (`[:worker] compiled 3 namespaces
  in 41 ms`), as shadow-cljs's `[:app] Build completed.` does.
- **One nREPL.** Sessions on the `:http` port and on `:nrepl :port` start
  bound to the first build named. `(light.dev/repl :worker)` rebinds a
  session, as shadow-cljs's `(shadow.cljs.devtools.api/repl :worker)` does,
  and `light.dev/runtimes` lists each runtime with its build. A REPL
  request goes to its session's build's thread. `--repl --connect` sessions
  behave the same.
- **Failures stay in their build.** A compile error in one build is that
  build's overlay and stderr; the other keeps stepping.

As built (light-cljs-405; the server only, tested in `tests/dev_server.rs`):
`DevServer::start_mounts(host, port, Vec<Mount>, roots)` serves a mount
table indexed by `MountId(u32)`, dense and in the order given (light-cljs-406
gives the builds' `BuildId`s the same order). `DevServer::mounts()` and
`mount(id)` hand out a `DevMount` per build, which a build's thread keeps to
`publish`, `invoke` and `serve_repl` on its own mount; `DevServer`'s methods of
the same names act on the first mount. `DevServer::start`, which the
flag-only `--watch` still calls, is one unnamed mount at `/` alone, so its
responses are unchanged. Choices the design left open:

- A mount's name is one URL path segment (the build id without its colon);
  an empty, `.`, `..`, separator-holding or duplicate name is refused.
- `GET /NAME` answers `301 Moved Permanently` to `/NAME/`, so the built-in
  page's relative `index.js` resolves inside the mount.
- A path whose first segment names a mount is that mount's; any other path
  is the first mount's, and so are `/events`, `/reply` and `/repl` even when
  a mount has that name (it is served under `/NAME/` alone). Source roots are served once, from the server's
  root, at the prefix each mount's build directory gives them: a source map
  under `/NAME/` names its sources with at least one `..`, which resolves
  past the mount's segment to the same path as from `/`.
- Runtime IDs are the server's, not the mount's, so an ID names one runtime
  (for light-cljs-407's `light.dev/runtimes`); each mount counts its own
  steps, and each has its own REPL sessions, which another mount's
  `repl` endpoint does not know. nREPL connections, on the `:http` port or
  `serve_nrepl`'s, have the first mount's sessions until light-cljs-407
  binds a session to a build.

## Watching the configuration file

Phase 2 adds `light-cljs.edn` to the watch's poll, as shadow-cljs's
`config_watch` does. When its text changes, the watch reads and validates
it again, against the same environment, and compares each running build's
merged settings with the new ones:

- **Invalid**: the error goes to stderr and every overlay, and the builds
  keep running with their old settings.
- **Unchanged** for a build (a comment, another build's section): nothing
  happens to it.
- **Changed** for a build: that build restarts in the same process, ending
  its session and starting from the cache as at startup; connected pages
  reconnect to the new session.
- **Top-level `:source-paths`, `:cache-root`, `:cache-blockers` or
  `:compile-threads` changed**: every build restarts. **`:http`, `:nrepl`
  or `:fs-watch` changed**: reported once, `light-cljs: :http changed in
  light-cljs.edn; restart the watch to apply it`, since the running server
  holds connections, as shadow-cljs's server does.

## Migration

- **Every flag-only command works unchanged**, with or without a
  `light-cljs.edn` present: it never reads one, and its defaults are
  today's. The suites that run the binary pass unchanged. The one changed
  spelling is a bare FILE argument named `compile`, `watch` or `release`
  (see "Commands").
- **Every existing flag works after a subcommand**, as an override of its
  one setting.
- **Moving a command into the file** is mechanical. The README's watch
  example

  ```sh
  light-cljs --watch --entry app.main --source-root examples/dev \
    --out-dir /tmp/light-dev --serve 9630 --nrepl 0
  ```

  becomes, in `examples/dev/light-cljs.edn`,

  ```clojure
  {:source-paths ["."]
   :nrepl {:port 0}
   :builds {:dev-workflow {:target :esm
                           :output-dir "out"
                           :modules {:main {:entries [app.main]}}}}}
  ```

  and `light-cljs watch dev-workflow` from that directory or below it.
- **From shadow-cljs.edn**, most keys carry over as written; the
  deviations table lists the rest, and the errors name what to do for each.
  A `bb` script can port a shadow-cljs build (such as
  `research/penpot-light/port.bb`, which could write the port's
  `light-cljs.edn` from Penpot's `:main` build, keeping its `#shadow/env`
  forms as `#light-cljs/env`), following AGENTS.md: babashka, not Python.

## Example: examples/features

`examples/features/light-cljs.edn`, the code-splitting application
(README, "Examples"), whose source path is its own directory:

```clojure
;; The code-splitting example's build configuration
;; (docs/build-configuration.md). From this directory or below it:
;;   light-cljs release features   optimized chunks in out/release
;;   light-cljs compile features   the dev build in out/dev, once
;;   light-cljs watch features     the dev build, rebuilt on save and
;;                                 served on http://127.0.0.1:9630/
{:source-paths ["."]
 :builds
 {:features
  {:target :esm
   :output-dir "out/release"
   :modules {:main {:entries [app.main]}}
   :dev {:output-dir "out/dev"}}}}
```

`light-cljs release features --optimize none` compiles it at `none` into
the same directory. The release output is what `--compile --optimize
advanced --entry app.main --source-root examples/features --out-dir
examples/features/out/release` writes today, byte for byte;
`examples/features/out/` joins `.gitignore` with the file. `index.html`
stays where it is; as its own comment says, it is served beside the
chunks, now those in `out/release`.

## Implementation sketch

Names are decisions for review; the structure is a sketch.

- **`src/config_file.rs`** (`light_cljs::config_file`):
  - `FILE_NAME: &str = "light-cljs.edn"`.
  - `ConfigFile::discover(start: &Path) -> Option<PathBuf>` and
    `ConfigFile::read(path: &Path, environment: &dyn Fn(&str) ->
    Option<String>) -> Result<ConfigFile, ConfigFileError>`, which parses
    with `parser::parse`, resolves `#light-cljs/env` against the given
    lookup (tests pass a map, the CLI `std::env::var`), merges `LIGHT_CLJS`,
    validates the whole file and keeps each build as a `BuildDefinition` in
    a `Vec<BuildDefinition>` indexed by `BuildId(u32)`, ascending by id.
  - `ConfigFile::settings(&self, build: BuildId, mode: BuildMode, merges:
    &[ConfigMerge]) -> Result<BuildSettings, ConfigFileError>`, the merged
    settings of layers 1-5, each with its `SettingOrigin` (a span in the
    file, an environment variable at a span, `LIGHT_CLJS`, a
    `--config-merge`, or the default). Settings are a closed enum,
    `Setting`, with dense discriminants; `BuildSettings` holds one slot per
    `Setting` in an array, and key lookup is a binary search over a static
    sorted table of key names, so validation, merging and `--print-config`
    walk arrays, not maps.
  - `BuildMode::{Dev, Release}`, shadow-cljs's mode names.
  - `ConfigFileError` carries the origin, span, build and mode, and renders
    the messages above.
- **CLI.** `run` dispatches the first argument `compile`, `watch` or
  `release` to `config_cli::run(Command, args)`, which discovers and reads
  the file, parses the ids and flags, computes each build's
  `BuildSettings`, translates them into `CompileFlags` or the watch's
  `Options` (layer 6 is then today's flag parsing, writing over them), and
  continues on today's paths. One new library call,
  `BuildConfiguration::set`, replaces a key's value so that a flag can
  override the file; `insert` keeps refusing a duplicate.
- **Several builds.** `dev_watch::run` becomes `dev_watch::run_builds(&[Options])`:
  one `BuildWorker` thread per build, one poller, one server with a
  `BuildId`-indexed mount table, and a session-to-`BuildId` binding in the
  REPL and nREPL layers.
- **Embedding.** `BuildSettings` is public, so an embedder that keeps its
  builds in a `light-cljs.edn` gets the same validation; nothing in the
  compiler's API depends on it.

### As built (light-cljs-398): the reader and the schema

- `src/config_file.rs` (`light_cljs::config_file`) has `FILE_NAME`,
  `ConfigFile::discover`, `ConfigFile::read` and `ConfigFile::from_source`
  (the same for text in hand, which tests and, later, `LIGHT_CLJS` use),
  `BuildDefinition`, `BuildId`, `BuildMode`, `Setting` and
  `ConfigFileError`. No command calls it yet; light-cljs-400 adds them.
- **Reading.** One `parser::Parser` pass with discards retained, then a
  `Value` tree: each form's kind, its span, and, for a `#light-cljs/env`
  value, the variable and whether it was set. The value's kind is exactly
  what a literal in its place would have; its span is the whole tag's.
  Duplicate map keys and set elements are found on the tree (the syntax
  parser does not check them), with the first one's line and column in
  the message; values are equal as `BuildConfiguration` finds them (`1`
  and `1.0`, a list and a vector, maps and sets in any order). A `#_`
  directly in a map that discards a value equal to one of its keys is an
  error: `#_#_ :output-dir "old"` beside a live `:output-dir`. A string
  holding an unpaired surrogate escape keeps its UTF-16 code units, so
  `:config` takes it as `--config` does; no path or host does.
  Auto-resolved keywords, namespaced maps, metadata, quoting forms and
  `#()` are errors, since EDN has none of them. `ConfigFile::document`
  keeps the whole resolved tree, so a map-valued key given by an unset
  variable keeps its origin for light-cljs-399.
- **`#light-cljs/env`** follows `shadow.cljs.config-env/read-env`: a vector
  of two is `["NAME" DEFAULT]`, an odd length is `:as`/`:default` pairs, and
  the default is not converted by `:as`. Unlike shadow-cljs, an unknown
  `:as` type or option is an error whether or not the variable is set.
  `:as :int` takes an optional sign and decimal digits within 64 bits,
  `:as :symbol` and `:as :keyword` a string that reads as exactly one
  symbol or keyword name.
- **Layers.** Each map's settings go into an array with one slot per
  `Setting` (29 settings: every key position of the schema's tables;
  `:closure-defines` at build level and under `:compiler-options`, and
  `:cache-blockers` at the top level and under `:build-options`, are
  separate settings, merged by light-cljs-399). A build has three layers:
  its own map, `:dev` and `:release`. Each closed map is a static table
  sorted by key, searched by binary search. Builds are sorted by id and
  `find_build` is a binary search.
- **Unknown keys** are looked up, in order, in the table of shadow-cljs keys
  with no equivalent (in any map), then in the other maps' tables (a
  misplaced key says where it belongs: "is a top-level key", "is a build
  key", "belongs under :compiler-options", "belongs in a module under
  :modules"), then by edit distance against the enclosing map's keys, after
  spelling the key in kebab case (`outputDir` and `output_dir` are
  `output-dir`), suggesting the closest key when no more than half of the
  longer one differs. Without a suggestion the message lists the map's keys.
- **Static checks at read**, besides the tables' types and ranges: the
  required build keys `:target`, `:output-dir` and `:modules` must be in the
  build or in its mode map, for each mode; the mode rule, where the build's
  own map and its `:dev` map reach dev mode, even with an empty
  `:closure-defines {}`. A literal nil, or a nil default, is a bad value
  like any other; the nil of an unset variable without a default is kept,
  also inside a collection (a source path, a cache blocker, a module's
  entry), for light-cljs-399 to check in the build and mode that run. Integers are decimal digits, as the flags read them; ports are 0 to
  65535 and counts positive.
- **Messages** name the map as "at the top level", "in :http", "in build
  :app", "in build :app (dev)" for a mode map, "in :compiler-options of
  build :app" and "in module :main of build :app", and values as ":KEY of
  build :app must be ..., got VALUE". The error table above gives the
  shapes; `tests/config_file/reader.rs` asserts every message exactly.

### As built (light-cljs-399): layers, LIGHT_CLJS and --config-merge

- `ConfigFile::settings(build, mode, merges) -> Result<BuildSettings,
  ConfigFileError>` merges layers 1-5 of "Precedence" slot by slot: a
  setting is one key path and every map on the way is validated as a map,
  so merging the slots is `deep-merge` of the maps that hold them.
  `BuildSettings` has a `SettingValue` (value and `SettingOrigin`) per
  `Setting`, empty where no layer gives one and there is no default.
  Flags, layer 6, are light-cljs-400's.
- **Values know their text.** `Value::source` is a `ConfigSource`
  (`Default`, `File`, `LightCljs`, `ConfigMerge(N)`), so a merged map's
  entries and a concatenated vector's elements each keep where they were
  written; `ConfigFile::origin(value, merges)` locates any of them (for
  light-cljs-401's per-entry comments). A slot's `SettingOrigin` is its
  value's: the source, span, line and column, the mode map that gave it,
  and the `#light-cljs/env` variable. It prints as `light-cljs.edn:14:49
  (:release), from APP_VERSION`, `LIGHT_CLJS:1:14`, `--config-merge
  2:1:30` or `default`; an unset variable whose default was used prints
  `, APP_VERSION unset`. A merged collection's origin is its highest
  layer's.
- **`deep_merge`** follows `shadow/build/api.clj` but for the deviation
  above: a nil lower value gives the upper one and a nil upper value keeps
  the lower one, so an unset variable without a default in a higher layer
  leaves the value below it, as in shadow-cljs. Keys and elements are equal
  as `BuildConfiguration` finds them (`1` and `1.0`), not by Clojure's `=`,
  so a merged `:config` is still a valid one.
- **Defaults**, layer 1: `:cache-root ".light-cljs"`, `:cache-blockers
  #{}` at both levels, `:compile-threads 1`, `:http {:port 9630 :host
  "127.0.0.1"}`, `:fs-watch {:loop-wait 100}`, `:optimizations :advanced`
  in release and `:none` in dev (which runs at none semantics),
  `:pseudo-names` and `:elide-asserts` false, `:cljs-warnings` false in
  release and true in dev (which implies it), the five macro limits from
  `MacroLimits::default()` and `EvaluatorOptions::default()`, and
  `:cache-level :all`. `:http :strict`, `:nrepl`, `:fs-watch :impl`,
  `:config`, `:closure-defines`, `:checked-arrays` and `:devtools` have
  none.
- **Derived slots.** `:closure-defines` under `:compiler-options` is merged
  under the build's own into `Setting::ClosureDefines`, after every layer,
  and `Setting::CompilerClosureDefines` is left empty; as in shadow-cljs's
  `build.clj`, a build-level define in the file wins over a merge's
  `:compiler-options` one. The top-level `:cache-blockers` are unioned
  into `Setting::BuildCacheBlockers`; `Setting::CacheBlockers` keeps the
  top level's own.
- **`LIGHT_CLJS`** is read in `ConfigFile::read` and `from_source` through
  the same lookup; empty, or holding no form (only a comment), it is not
  there. It is validated as strictly as the file but may leave anything
  out, as may the file under it; after the merge the whole is checked
  (`:source-paths`, a build, each build's `:target`, `:output-dir` and
  `:modules` per mode). `builds()`, `top()` and `build(id)` are the merged
  ones; `document()` stays the file's, and `light_cljs()` gives the text. A
  build only `LIGHT_CLJS` defines is added in id order, and its errors are
  located in `LIGHT_CLJS`.
- **`ConfigMerge::parse(index, data, project_dir, environment)`**, with
  the merge's position on the command line (from 1, for `--config-merge
  N` in messages) and the environment lookup that its `#light-cljs/env`
  needs, beside the sketch's two arguments. Text starting with `{` is EDN,
  anything else a path joined to the project directory; a file with no
  form is not a map. Inside it, a top-level key says it is one ("is a
  top-level key"), and messages name "the merged build".
- **Checks after merging at read**: each build's own map with each mode
  map over it, after `LIGHT_CLJS`, must hold one module of one `:entries`
  namespace, whichever build and mode a command runs (an unset
  variable's nil is not counted; `settings` reports it in the mode that
  reads it).
- **Checks after merging**, in `settings`: in dev mode, a merge's
  `:closure-defines` (at either level) or `:checked-arrays`, at its key; a
  second module or `:entries` namespace, at the second; a nil from an
  unset variable without a default where a value is required, at the tag:
  `:source-paths`, `:target`, `:output-dir` or `:modules` themselves, a
  source path, a cache blocker, a module, its `:entries` or its entry, a
  define's value and `:nrepl :port`, each only in a mode that reads the
  setting (`:http`, `:nrepl`, `:fs-watch` and `:devtools` in dev, for
  `watch`). A nil in an optional setting leaves it empty, and `:config`
  takes nil as `--config` does. Output directories are compared as paths
  joined to the project directory, made absolute when the file is read
  (so an absolute and a relative spelling of one directory are equal), with
  `.` and `..` resolved by name:
  `settings` compares the build's with each other build's as the file
  defines it in that mode (merges apply only to the builds a command
  names), with `<:cache-root>/cache` and with each source path;
  `ConfigFile::check_outputs(settings, merges)` compares the builds of one
  command with each other, for light-cljs-400 when a merge applies to
  several. A directory inside a source path, as examples/features' is, is
  not an error.
- Tests: `tests/config_file/merge.rs`.

### As built (light-cljs-400): the compile and release commands

- `run` in `src/main.rs` hands a first argument `compile` or `release` to
  `config_cli::run(Command, args)` (`src/config_cli.rs`, a module of the
  binary). `watch` and `cljs-repl` are still read as FILE until light-cljs-403
  and light-cljs-408. Only the very first argument is a command:
  `--warnings release app` still reads a FILE named `release`.
- **Order.** The ids are taken first, then the file is found from the current
  directory and read (the whole file validated), the ids are checked, the
  flags after them are read, each `--config-merge` is read, every named
  build's `ConfigFile::settings` is computed, `ConfigFile::check_outputs`
  compares them, and every build is translated, all before anything compiles.
  When a flag replaces an output directory, the source roots or the cache
  store (`--out-dir`, `--source-root`, `--cache-dir`, `--no-cache`), the
  builds' settings come from `ConfigFile::settings_before_flags`, which skips
  the output comparison, and the command compares the directories each build
  will use instead: its output directory against the other builds' (as the
  file defines them in the mode), the cache store when the cache is on, and
  the source roots, naming both origins (`light-cljs release other: builds
  :app and :other both write out/app in release mode (:other's from
  --out-dir, :app's from light-cljs.edn:7:21)`; `build :app writes out/app in
  release mode (light-cljs.edn:7:21), the source path "out/app"
  (--source-root); give it a directory of its own`). So a flag can move a
  build out of a conflict a merge made, and cannot move it into one.
  Without such a flag, `settings` checks as light-cljs-399 built it.
  The builds then run in the order named; a repeated id is built once. A
  failure stops the command with status 1, so a later build's directory is
  never created.
- **Translation** (layer 6) builds a `NamespaceFlags` and a `CompileFlags`
  per build, the structures `--compile --entry` fills (its argument loop is
  now `NamespaceFlags::accept` and `CompileFlags::accept`, then
  `build_namespace`, so a flag-only build runs the same code). The flags after
  the ids are read once with those two parsers into a separate pair, and each
  build's values are taken from them where given, else from the settings:
  - Paths go through `ConfigFile::resolve`, the project directory joined with
    the path and normalized by name: source paths, `--source-root`,
    `:output-dir`, `--out-dir`, `--cache-dir`, `--stats-file` and the store
    `<:cache-root>/cache`. So a command from a subdirectory gets the same
    roots and keys; a flag-only build still spells them as typed.
  - `release` sets `--optimize` from `:optimizations` unless `--optimize` is
    given; `:pseudo-names`, `:elide-asserts` and `--pseudo-names`,
    `--elide-asserts` are or-ed, since the flags can only turn them on (a
    merge turns one off). Each `:closure-defines` entry becomes `--define
    NS/NAME=TEXT`, a string by its characters; a `--define` of the same name
    replaces it, and two `--define`s of one name are an error (`--define
    app.other/label=b: the define app.other/label is given twice`; a
    flag-only `--compile` still lets the last one win). `:cljs-warnings` and `:checked-arrays` set the analysis
    options as `--warnings` and `--checked-arrays-*` do, and those flags after
    the ids win.
  - `:config` goes through `BuildConfiguration::insert`, then each `--config`
    through the new `BuildConfiguration::set`, which replaces a key; the flags
    are read into their own map first, so one key named twice by flags is
    still `--config`'s error.
  - The macro limits and `:compile-threads` are taken only when a layer wrote
    them (a default is what the flag structures already hold); `--jobs` wins.
  - Cache: `--no-cache` turns it off, else `--cache-dir` names the store (and
    turns it on over `:cache-level :off`), else `:cache-level :off` turns it
    off, else the store is `<:cache-root>/cache`. Blockers are the build's
    (the top level's already unioned in) followed by each `--cache-blocker`
    not among them.
- **Flags after the ids.** `compile` takes what `--compile --reloadable` takes
  plus `--warnings` (which dev mode implies); `release` takes what
  `--compile` takes plus `--warnings`, `--checked-arrays-warn` and
  `--checked-arrays-error`; both take `--config-merge DATA`. Refused, each
  with the flag (and its value) in the message's prefix:
  `--esm` and the source map flags (`light-cljs release app --esm: release
  writes a chunked build into :output-dir, as --out-dir does: ES modules
  without source maps; it takes no --esm`; `compile` says it writes the dev
  build, as `--reloadable` does, with a source map per namespace);
  `--watch`'s flags (`light-cljs release app --serve 0: --serve is a flag of
  light-cljs watch, which serves and watches the build; release builds
  once`, where `--compile --serve` prints the usage text); `--reloadable`
  (the command chooses the build); in `compile`, `--optimize`,
  `--size-report`, `--pseudo-names`, `--define` and the checked-arrays flags
  (`--optimize applies to release builds; compile writes the dev build at
  none semantics, as --compile --reloadable does`), since dev builds take no
  checked arrays (the user's decision); `--entry` and `--out-dir` with more
  than one id; and anything else (`--frobnicate is not a flag of release;
  see light-cljs --help`). `--no-cache --cache-dir` is refused as before.
- **Messages.** Every error of a command is printed as
  `light-cljs COMMAND IDS: message`, the ids as typed, not after the
  `light-cljs: ` prefix of other errors. File errors keep their
  `PATH:LINE:COLUMN:` after it, PATH the absolute path found. The no-ids and
  unknown-id messages name the file by that path where the table says
  "this configuration" and "light-cljs.edn", so the file is named in every
  error. A build that fails is prefixed with its own id alone: `light-cljs
  release broken: PATH:3:2: unresolved symbol: ...`.
- **Status lines**, on stderr, after shadow-cljs's `shadow-cljs - config:
  PATH` and `[:app] Build completed.`: `light-cljs - config: PATH` once,
  then `[:ID] Compiling ...` and `[:ID] Build completed in N ms: DIR` per
  build, DIR the absolute output directory. `--stats`, `--size-report` and
  warnings print between them as for `--compile`.
- **Flag-only commands** run no new code before their own: the dispatch
  looks at the first argument only. Their output, messages, status, written
  files and cache keys are unchanged, but for `USAGE`, which now lists the
  two commands, so `--help` and a bad argument's usage text show them.
- Tests: `tests/config_file/commands.rs`, on copies of examples/features and
  of `tests/fixtures/build-configuration/commands`; the ground-rule test is
  `flag_only_commands_never_read_the_configuration_file` in `tests/cli.rs`.

### As built (light-cljs-402): file builds are flag builds, and the Phase 1 exit criterion

- **Every row.** `tests/config_file/equivalence.rs` writes a `light-cljs.edn`
  over a copy of `tests/fixtures/build-configuration/commands` per case and
  runs `ID --stats` and the equivalent flags, `--compile --reloadable` for
  `compile` and `--compile --optimize advanced` for `release`, with the
  project directory joined to every path (`--source-root`, `--out-dir`,
  `--cache-dir <project>/.light-cljs/cache`). Each pair runs in both orders,
  each order from an empty store. All four runs write the same bytes into
  `out/app`, and the second of each order has `cache.misses 0` and
  `cache.hits` equal to the first's `cache.published`. The rows both
  commands read: the entry, the output directory and the default store
  (every case), `:source-paths` of one root and of two, `:elide-asserts`,
  each of the five macro limits, `:config` (the flags naming its keys in the
  other order, since the map's canonical text sorts them), `:compile-threads`
  as `--jobs 2`, `:cache-root` as `--cache-dir <root>/cache`, `:cache-level
  :off` as `--no-cache` (no `cache.*` lines and no store, the same bytes),
  and top-level and build `:cache-blockers` as two `--cache-blocker`s (the
  same bypass rows; every other namespace hits). The rows only `release`
  reads: `:optimizations` `:none`, `:prepare` and `:advanced`;
  `:closure-defines` under `:release` and under its `:compiler-options`, as
  `--define`; `:pseudo-names`; `:checked-arrays` `:warn` and `:error`; and
  `:cljs-warnings`, as `--warnings` on a namespace that fails without it.
  `--warnings` and the checked-arrays flags go before `--compile`, where the
  flag CLI takes them.
- **Two roots that both hold a required namespace** are `ambiguous
  namespace app.main: ROOT/alt/app/main.cljs, ROOT/src/app/main.cljs`, from
  the file as from the flags; the order of `:source-paths` does not pick
  one. The two-root case therefore builds a namespace only one root holds.
- **Environment values**, asserted on the `[cache-misses-and-bypasses]`
  rows: with `APP_FLAVOR=dev`, `#light-cljs/env ["APP_FLAVOR" :as
  :keyword]`, the literal `:dev` and `--config app/flavor=:dev` hit each
  other with the same bytes; `#light-cljs/env "APP_FLAVOR"` misses every
  namespace (`light.build`, `app.macros`, `app.util`, `app.main`) with
  `miss config: config changed`, as does `APP_FLAVOR=prod`; setting
  `APP_UNREAD`, which the file does not read, misses nothing, and going back
  to `APP_FLAVOR=dev` hits the first artifacts. A `:release
  {:closure-defines {app.other/label #light-cljs/env "APP_LABEL"}}` changes
  the output's label with the variable and every namespace hits.
- **From a subdirectory**, `release app` in `src` and in `src/app` hits every
  artifact the same command stored from the project directory, writes the
  same bytes into the project's `out/app` and creates no `.light-cljs` or
  `out` below it.
- **Edits that change no compiler input** hit every namespace with the same
  bytes: a comment, another build's `:config` and `:compiler-options`, the
  top-level `:http`, `:nrepl` and `:fs-watch` and the build's `:devtools`,
  which `release` does not read, and the build's `:output-dir` (the same
  files in the new directory).
- **The exit criterion is met.** In a copy of examples/features, `light-cljs
  release features --stats` and `light-cljs --compile --optimize advanced
  --entry app.main --source-root . --out-dir out/release --stats`, run in
  the same directory with the default `.light-cljs/cache`, write the same
  five files byte for byte (`app.editor+app.reports.js`, `app.editor.js`,
  `app.main.js`, `app.reports.js`, `chunks.json`) in either order; the first
  publishes all 6 namespaces and the second hits all 6
  (`release_features_equals_its_flags`). The flags' relative `.` hits the
  artifacts keyed by the file's absolute root, which corrected
  "Consequences" above.
- README "Examples" names `light-cljs.edn` and the two commands for
  examples/features, the documentation list names this document, and
  `examples/features/index.html` says to serve it beside what `light-cljs
  release features` writes into `out/release`.

### As built (light-cljs-401): --print-config

- `compile` and `release` take `--print-config` after the ids. Every check
  of light-cljs-400 runs first (the file, the ids, the flags, each
  `--config-merge`, the settings, the output directories and the
  translation), then each build's `BuildSettings` gets the flags (layer 6)
  and `ConfigFile::print_config` prints it on stdout, a block per id in the
  order named, separated by a blank line, and the command exits 0 with
  nothing on stderr. Nothing compiles, no directory is created and the cache
  is not opened.
- **Layer 6 in `BuildSettings`.** `ConfigSource::Flag(N)` is a flag's value;
  `BuildSettings::set_by_flag` replaces a setting, `set_entry_by_flag` one
  entry of `:config` or `:closure-defines`, `add_by_flag` adds cache
  blockers. `SettingOrigin::flag` holds the flag's text and what it
  replaced: `--optimize none, over the release default :advanced` (a
  default that depends on the mode names it, as `:optimizations` and
  `:cljs-warnings` do), `--jobs 2, over the default 1`, `--out-dir out/x,
  over "out/app" at light-cljs.edn:7:21`, `--cache-blocker app.util, added
  to #{app.macros} at light-cljs.edn:4:18`, or the flag alone over nothing.
  `config_cli::apply_flags` maps the flags as `translate` applies them;
  `translate` is unchanged. The flags: `--source-root` (the list), `--entry`
  (the file's module id kept), `--out-dir`, `--optimize`, `--pseudo-names`,
  `--define` (a boolean or decimal number as itself, anything else a string,
  each giving `--define` its text back), `--warnings`, the checked-arrays
  flags, `--elide-asserts`, the macro limits, `--jobs`, `--config`,
  `--no-cache` (`:cache-level :off`), `--cache-dir DIR` (`:cache-root` DIR's
  parent when DIR is named `cache`, and over `:cache-level :off` it is
  `:all`; any other store no `:cache-root` names, so `--print-config` refuses
  it: `light-cljs release other: --cache-dir store: a light-cljs.edn names
  its store as :cache-root's cache directory, which store is not, so
  --print-config cannot print it; give --cache-dir a directory named cache`) and
  `--cache-blocker`. `--stats`, `--stats-file` and `--size-report` are not in
  the file and do not print.
- **What prints.** The top-level `:source-paths`, `:cache-root`,
  `:cache-blockers` and `:compile-threads`, then `:builds` with the one
  build: `:target`, `:output-dir`, `:modules`, `:config`,
  `:compiler-options`, `:build-options`, in the schema tables' order. A
  setting with a default prints at its default (`; default`), but for the
  compiler options, which print when a layer or a flag gives them, and
  `:optimizations`, which release always prints. Dev mode prints no setting
  it ignores (`:optimizations`, `:pseudo-names`, `:cljs-warnings`). The
  build's `:cache-blockers` prints only those the top level's, unioned into
  it, do not hold. `ConfigFile::print_config` takes a `watch` argument for
  light-cljs-403, which adds `:http`, `:nrepl`, `:fs-watch` and `:devtools`;
  no command passes it yet.
- **Release-only keys print under `:release`.** `:closure-defines` and
  `:compiler-options {:checked-arrays ...}` reaching dev mode are errors, so
  printed at the build level the EDN would not read back as a file. They
  print in the build's `:release` map, last, as the Build keys table orders
  `:release`. The example under "Commands" was written with them at the
  build level and is corrected, as is its `:compiler-options` order, which
  was the file's rather than the table's (`:elide-asserts` before
  `:macro-timeout-ms`). Its line and column numbers were right.
- **Canonical EDN.** `:config` prints one entry per line in key order, each
  value in the canonical text `BuildConfiguration` fingerprints;
  `:closure-defines` one entry per line in name order; sets in sorted order;
  everything else as `Value::to_edn` writes it. A map's entries align after
  its `{`. Each entry of `:config` and `:closure-defines` has its own origin
  (`ConfigFile::value_origin`, which finds whether the build's mode map
  holds the value, for `(:release)`). Comments start after the code padded
  to 52 columns, or one space after a longer line. A line break in a
  comment or in the header's path, which a flag's value or a directory name
  can hold, prints as `\n` (and `\r`), so the comment stays one line.
- Tests: `tests/config_file/print_config.rs`: the document's illustration
  and example, read from this document; `release features` and `compile
  features` from a copy of examples/features and from its `app/`
  subdirectory; two ids; every flag's origin; the round trip (each printed
  block read back with `ConfigFile::read` gives a build whose settings equal
  the original's, or, with flags, prints the same EDN); the build from a
  printed configuration writing the same files as the flags it printed; the
  refused store and a line break in a flag's value; each entry of a map
  kept from the file, a mode map, `LIGHT_CLJS`, two merges and a flag, with
  its own origin; and the library's watch rendering. Every process test asserts that the
  project holds the same paths afterwards, with no `out` or `.light-cljs`.

### As built (light-cljs-403): watch ID

- `run` hands a first argument `watch` to `config_cli::run` as `compile` and
  `release` are handed. The order of light-cljs-400 holds (ids, the file, the
  ids checked, the flags, each `--config-merge`, the settings in dev mode, the
  output directories, the translation), with one more check after the ids:
  more than one distinct id is `light-cljs watch app worker: watch runs one
  build for now; several builds in one process, with one server and one
  nREPL, arrive with Phase 3 (docs/build-configuration.md, "Several builds in
  one process"); start a watch per build`, also with `--print-config`
  (light-cljs-406 lifted it). A repeated id is one build.
- **Flags after the id** are `--watch`'s: those `compile` takes but
  `--jobs` and `--stats-file` (`--jobs is not a flag of watch; see
  light-cljs --help`), plus `--serve`, `--serve-host`, `--nrepl`,
  `--no-nrepl`, `--poll-ms` and `--reload-dependents`, read with
  `--watch`'s own messages (`dev_watch::port_value`, `poll_value`). The
  release-only flags are refused as `compile` refuses them (`--optimize
  applies to release builds; watch writes the dev build at none semantics, as
  --watch does`), and so are `--esm`, the source map flags and
  `--reloadable`. `compile` and `release` refuse `--no-nrepl` with the other
  watch flags.
- **Translation.** `config_cli::watch_options` builds the watch's `Options`
  (now `pub(crate)` in `src/dev_watch.rs`, whose `run` parses the flags and
  calls `dev_watch::watch(options)`) from what `translate` makes of the build
  for `--compile --reloadable` (entry, roots, output directory, `:config`,
  `:elide-asserts`, macro limits, cache store and blockers, with the flags
  already over them) and the watch's settings: `:http :port` (default 9630)
  as `--serve`, `:http :host` as `--serve-host`, `:fs-watch :loop-wait` as
  `--poll-ms`, `:devtools {:reload-strategy :full}` as `--reload-dependents`,
  `:nrepl {:port N}` as `--nrepl N` and `:nrepl false` as `--no-nrepl`; each
  flag after the id wins over its setting, so `--nrepl 0` turns nREPL on over
  `:nrepl false`, and `--no-nrepl` turns off the listener of `:nrepl {:port
  N}` with the server port's nREPL. `:compile-threads` prints but the watch runs one slot, as
  "Flags and keys" says.
- **The server always starts.** The watch serves the build as one mount named
  after the build id (`DevServer::start_mounts_with`), so it is served under
  `/ID/` and at `/` (light-cljs-405); the flag-only `--watch --serve` still
  serves its one unnamed mount at `/` alone (`DevServer::start_with`, which
  `DevServer::start` now calls). `dev_server::Listen { host, port,
  next_free, nrepl }` says how the server binds:
  - **The port.** A port from the file gives way, when it is in use
    (`AddrInUse` only, as shadow-cljs retries on a `BindException`), to the
    next one upward, each one passed over reported as shadow-cljs's
    `TCP Port N in use.` is: `light-cljs: TCP port 9630 in use, trying
    9631`; port 0 never retries, and 65535 in use fails. With `:http {:strict
    true}` the bind's error ends the watch: `light-cljs watch app: cannot
    serve on 127.0.0.1:9630: Address already in use (os error 48); :http
    {:strict true} tries no other port`. `--serve PORT` binds that port or
    fails as `--watch --serve` does (`light-cljs watch app: cannot serve on
    127.0.0.1:9630: ...`), so `--print-config` prints it as `:strict true` too,
    with `--serve PORT` as its origin. The bound address is in the
    `light-cljs: serving DIR at http://ADDRESS/` line, after the
    `light-cljs - config: PATH` line; the watch's other status lines are
    `--watch`'s.
  - **nREPL.** `:nrepl false` needed a flag first (principle 20), so
    `--watch` gained `--no-nrepl` (it requires `--serve`, and excludes
    `--nrepl`): the server's port then closes an nREPL client's connection,
    one whose first byte opens a bencoded dictionary, without answering,
    and serves HTTP as before. `:nrepl {:port N}` writes `.nrepl-port` in the
    project directory (`Options::nrepl_port_dir`; the flag-only watch keeps
    the working directory).
- **`<:cache-root>/http.port`.** Once the server is bound, and before the
  `serving` line, the watch writes the port into `http.port` under the
  file's `:cache-root` (layers 1-5, resolved against the project directory),
  creating the directory as shadow-cljs's `make-parents` does. `--cache-dir`
  and `--no-cache` do not move it, since `cljs-repl` finds the file from
  `light-cljs.edn` alone. `dev_nrepl::PortFile::write` writes it, so it goes
  on the exit paths `.nrepl-port` goes on: when the watch's state is dropped
  (a step that cannot write the build), and on SIGINT, SIGTERM and SIGHUP,
  whose handler now removes every port file of a fixed table of four slots,
  each only if it still holds this watch's port. The flag-only `--watch`
  writes none.
- **`--print-config`** after `watch` prints the top-level `:http`, `:nrepl`
  (when a layer or flag gives it) and `:fs-watch`, and the build's
  `:devtools`, with `ConfigFile::print_config(.., true)`. The watch's flags
  print as origins: `--serve 0, over the default 9630`, `--serve-host`,
  `--nrepl 0` as `{:port 0}`, `--no-nrepl` as `false`, `--poll-ms 50, over
  the default 100` and `--reload-dependents` as `:reload-strategy :full`.
  Since `http.port` stays in the file's `:cache-root`, a `--cache-dir` whose
  parent is another directory cannot print as `:cache-root`: `light-cljs watch
  features: --cache-dir state/cache: watch keeps http.port in :cache-root
  ".light-cljs", which --cache-dir does not move, and a light-cljs.edn names
  its store as :cache-root's cache directory, so --print-config cannot print
  both; give --cache-dir .light-cljs/cache or no --cache-dir`.
- Tests: `tests/dev_watch.rs` (`watch features` from the example's `app/`
  directory, serving `/index.js` and a unit at `/` and `/features/`, stepping
  on a save, `.light-cljs/http.port` present while it runs and gone after
  SIGINT; the watch's flags over the file's `:http`, `:fs-watch` and
  `:devtools`; `watch app` of the commands fixture, with `:config`,
  `:elide-asserts`, a macro limit, blockers and a `--config` over the file,
  writing the same files at startup and after a save as the equivalent
  `--watch` flags, which then miss no artifact), `tests/dev_server.rs`
  (`Listen`'s next port, its bind error and
  its nREPL refusal; a held port giving way, `:strict true` and `--serve PORT`
  failing with exact messages beside `--watch --serve`'s; a flag-only
  `--watch --serve` beside an invalid `light-cljs.edn` writing no
  `http.port`), `tests/dev_nrepl.rs` (`:nrepl {:port 0}`, `:nrepl false` and
  `--nrepl 0` over it; a held `:nrepl` port failing the watch after
  `http.port` is written, which the error removes, and `--no-nrepl` over
  it), `tests/config_file/commands.rs` (`watch`'s exact
  errors) and `tests/config_file/print_config.rs`, all on temporary copies
  of examples/features through `tests/support/file_watch.rs`.

### As built (light-cljs-404): rereading the file in the watch

- **The poll.** `config_cli` hands the watch a `dev_watch::ConfigWatch`: the
  file's path, and a reread that owns what startup used (the build's name,
  the parsed `--config-merge`s, the flags after the id) and the settings the
  build runs with. Each poll stats `light-cljs.edn` before it scans the
  source roots; when the stamp changed, the reread reads the text, and only
  when the text differs from the text read last (valid or not) reads and
  validates it again with `ConfigFile::from_source`, against the process's
  environment, then runs startup's checks in startup's order (the build by
  name, its dev settings, `check_outputs`, `check_directories` when a flag
  replaces a directory, `translate`, `watch_options`). The first poll reads
  the file once, in case it was saved after startup read it.
- **The comparison** is `BuildSettings::changed`: slot by slot over the
  settings array, by value alone (origins, spans, layout, comments and
  `#light-cljs/env` provenance left out), each value by its EDN text
  (`Value::to_edn`). That is stricter than shadow-cljs's `=`, on purpose: a
  list and a vector, `1` and `1.0`, `0` and `-0.0`, and a map's entry order
  (which an array map's keys follow) are each visible to a macro through
  `light.build/config`, so each restarts the build. Both sides have the
  flags after the id written over them (`apply_flags`, layer 6, with a
  `--cache-dir` the file cannot name written as `:cache-root`'s text), so a
  setting a flag gives does not change when the file's does, and a restart
  translates the new file under the same flags: flags still win.
- **Outcomes.**
  - Invalid (a validation error, a build the file no longer defines, a
    failed check): `light-cljs: PATH:LINE:COLUMN: message` on stderr, and a
    step with that error, located when the error is, for the overlay; the
    build keeps running with its settings, which the next reread compares
    with.
  - Only `:http`, `:nrepl` or `:fs-watch` slots changed: one line per key,
    `light-cljs: :http changed in light-cljs.edn; restart the watch to apply
    it`, and nothing else: the server, its port, the port files and the poll
    interval stay those the watch started with, also across restarts.
  - Any other slot changed (top-level `:source-paths`, `:cache-root`,
    `:cache-blockers` and `:compile-threads` among them): `light-cljs:
    light-cljs.edn changed the settings of :ID; restarting the build`. The
    session ends, then the build starts as at startup (`dev_watch::start`,
    from the cache), and `light-cljs: built ...` is printed again.
    `DevServer::restart` then points the mount at the new `:output-dir` and
    source roots, makes the new build the one a runtime catches up with, its
    startup warnings the diagnostics it receives, and ends every runtime's
    event stream: the client reconnects, as after a server restart, gets
    `hello` and the replay of the new build, and loads the units whose text
    changed. REPL sessions stay; pending invokes end as a disconnect ends
    them.
  - A comment, layout or another build's section: nothing.
- **A restart whose build fails** (a missing entry namespace, say) keeps the
  process and the server: `light-cljs: ERROR; the build starts again at the
  next change` and an error step for the overlay; the server keeps serving
  the last build, the REPL is closed, and the next change of a source or of
  the file starts the build again.
- Tests: `tests/dev_watch.rs` on temporary copies of the commands fixture
  (another build's section and a comment edit doing nothing; an invalid edit
  on stderr and as an `error` event, the next save stepping with the old
  `:config`; a `:config` edit restarting the build, its startup build and
  the next step reading the new value, the event stream ending and a new one
  replaying the new build on the same port; an `:http` edit reported once
  and the old port serving; `--cache-dir`, `--config` and `--serve` over a
  reread; a moved `:output-dir` and `:source-paths`, served from the new
  directory and roots; `:cache-blockers`, `:compile-threads` and
  `:cache-root` restarting; `:http`, `:nrepl` and `:fs-watch` edits each
  reported once and none applied by a later restart; a restart that fails
  and one that recovers) and of examples/features (the exit criterion,
  below); `tests/config_file/merge.rs` (`changed`, a table of edits).

Phase 2's exit criterion is met: in a temporary copy of examples/features
with `:config {:app/greeting "hello"}` and `app.greeting`, whose macro reads
it, `light-cljs watch features` serves the example to the Node dev driver
(tests/support/dev_execution.mjs `--connect`), and an edit to `"bonjour"`
restarts the build; the driver's stream ends, it reconnects as runtime 2 and
reloads `app.main`, logging `greeting: bonjour`, from the same process on the
same port (`watch_features_carries_a_config_edit_to_a_connected_runtime`).
Checked by a Node driver, not in a browser.

### As built (light-cljs-406): several builds in one watch

- **`watch ID ID ...`.** `config_cli::run` no longer refuses a second id:
  each build named (a repeated id is one build) is translated into the
  watch's `Options` as light-cljs-403 translates one, and
  `dev_watch::run_builds(Vec<Options>, Option<ConfigWatch>)` runs them, in
  the order named; the flag-only `--watch` is `run_builds` of its one build,
  without a file. `--entry` and `--out-dir` are refused with two ids as
  `compile` refuses them, and `--print-config` prints one block per id. An
  error that ends the watch names its build (`light-cljs watch worker:
  cannot write ...`), or every build for the server's (`light-cljs watch app
  worker: cannot serve on ...`).
- **A thread per build** (`BuildThread`, the sketch's `BuildWorker`, with an
  8 MiB stack, as deep as the main thread `--compile` analyzes on) makes the
  startup build, owns its `Watch` (session, macro engine, retained program
  and the files of its `light.build` resources, which it stamps every
  `:loop-wait` itself) and steps it. The poller, the thread that started the
  watch, spawns the next build's thread only once the one before has made
  its startup build, so the peak is one analysis beside the programs made.
  A restart, too, waits its turn in a queue (`Starts`), which the poller
  fills in the order named before it asks the builds to restart, so a
  top-level change restarts every build one after another, `:app` first.
  REPL forms reach a build's thread through its work channel, tagged with
  the start of the session they were sent to; a form for a session that
  ended is dropped, as the session's receiver was dropped before.
- **One poller** scans the shared source roots every `:loop-wait` of the
  first build (every build has the same, from the top level or the flags)
  and `light-cljs.edn`, and sends every build the files that changed; each
  build's manifest decides which are its own (`Watch::namespace_of`), so a
  save of `app.shared` steps the app and the worker concurrently and a save
  of `app.jobs` steps the worker alone. A build that a failed step left with
  a namespace to compile again compiles it at any change, another build's
  file included, as one build always has: the change may be the file the
  failed namespace was waiting for.
- **One server.** The builds are the mounts of
  `DevServer::start_mounts_with`, `MountId` in the order named, each under
  `/ID/` and the first also at `/`. A `light-cljs: serving DIR at
  http://ADDRESS/` line is printed per build, the others' URLs ending in
  `/ID/`. Each build's thread publishes on its own `DevMount`, serves its
  REPL there, and restarts through `DevServer::restart` with its mount's id;
  the nREPL listener and the port files are the poller's, after every
  build's REPL, and every build's `built` line comes after them, in the
  order named. nREPL sessions are the first build's until light-cljs-407.
- **Names.** A build of the file begins its lines on stderr with `[:ID] `
  where `--watch` writes `light-cljs: ` (`[:worker] built app.worker (4
  namespaces) into ... in 41 ms; watching every 100 ms`, `[:worker]
  recompiled app.jobs in 3.1 ms`, the step's errors, `[:worker] WARNING:
  ...`, the restart's failure), as shadow-cljs's `[:app] Build completed.`
  does, also when one build is watched; the overlay messages of its steps,
  warnings and errors begin with `[:ID] ` too. The process's own lines (the
  file's, `serving`, `nREPL at`, a reread's) stay `light-cljs: ...`, and a
  flag-only `--watch` prints exactly what it printed before.
- **Rereads per build.** The reread reads and validates the file once for
  every build: an invalid file, or one that no longer defines one of the
  builds, is reported once, on stderr and to every mount's runtimes, and
  every build keeps its settings; `check_outputs` runs over all the builds'
  settings, as at startup. Each build's settings are compared with those it
  runs with: a build whose settings changed restarts on its thread, its
  runtimes' streams ending while the others' go on, and a top-level change
  is a change of every build's settings, so every build restarts. `:http`,
  `:nrepl` and `:fs-watch` changes are reported once, whichever builds they
  reach.
- Tests: `tests/dev_watch.rs` on temporary copies of the Phase 3 test
  project, `tests/fixtures/build-configuration/multi/` (the illustration's
  file, with `app.main`, `app.worker` and its own `app.jobs`,
  `app.test-runner` and `app.shared`, which all three require; light-cljs-407
  and 409 reuse it): both builds starting in order (every file of the app's
  build written before the worker's first), served at `/`, `/app/` and
  `/worker/`; a save of `app.shared` stepping both, with a Node runtime on
  each build's event stream receiving its step; a save of `app.jobs`
  stepping the worker alone; the worker's compile error on `[:worker]` lines
  and in its runtime's error event while a save of `app.main` steps the app;
  an edit of `:worker`'s `:config` restarting the worker alone, the app's
  stream going on, and a top-level `:cache-blockers` edit restarting both in
  order; a worker step held three seconds by a spinning macro while a save of
  `app.main` steps the app first and a second save of `app.jobs`, queued
  meanwhile, is stepped after it; a worker whose restart failed using no CPU
  while it waits (it blocks on its channel) and starting again at the next
  change. `tests/dev_server.rs`: `watch features :features-b` on a copy of
  examples/features with a second build of the same entry, serving each
  under its prefix and `features` at `/`. `tests/config_file/print_config.rs`:
  `watch app :worker --print-config` printing both blocks. The existing
  file-watch tests expect `[:features]` and `[:app]` lines; the flag-only
  ones are unchanged. Checked with Node runtimes, not in a browser.

### As built (light-cljs-407): one REPL for every build, light.dev/repl

- **Sessions are the server's.** The REPL sessions moved from each mount's
  state to the server's (`dev_server::Site`, one table with one
  incarnation), each with the mount whose endpoint opened it (its *home*:
  the first build's for nREPL on either port and for `/repl`, which
  `--repl --connect` uses; the build's for `/ID/repl`) and the build it is
  bound to, its home's to begin with. An endpoint knows only the sessions it
  opened, as light-cljs-405 had it, while their forms go wherever they are
  bound. The session table's lock is taken before a mount's, never after.
- **A REPL request goes to its session's build's thread.** Each form of an
  evaluation, and each `completions` and `lookup`, is a `ReplRequest` for
  the compiler of the build the session is bound to, read again before
  every form; its unit goes to that build's runtimes, its interrupt to that
  build's pending request, and its error stack is mapped through that
  build's units and source maps. `close` makes every build's runtimes
  forget the session.
- **`(light.dev/repl :ID)`** is a fifth `light.dev` function, a command the
  server answers, declared in every dev session and defined by the client
  to throw when called inside another form, as `runtimes` and `select!`
  are. Its argument is one unqualified keyword (`light.dev/repl takes one
  build id, a keyword such as :app` otherwise). It binds the session to the
  build and, as shadow-cljs's `nrepl-select` starts the build's REPL
  afresh, moves it to the namespace that build's sessions start in (its
  entry, where shadow-cljs has `:repl-init-ns` or `cljs.user`) and to its
  most recently connected runtime; the value is shadow-cljs's,
  `[:selected :worker]`. Rebinding to the bound build starts it afresh in
  the same way. shadow-cljs's "To quit, type: :cljs/quit" has no
  counterpart: there is no Clojure REPL to return to (the nREPL row of
  "Deviations from shadow-cljs"). The errors, exact:
  - an id the watch does not run: `the watch runs no build :nope; its
    builds are :app, :worker`, the ids in the order named;
  - a flag-only `--watch`, whose one build is unnamed (`DevServer::start`):
    `the watch runs no named builds: light.dev/repl switches between the
    builds of light-cljs watch ID ...`.

  A build that cannot compile, because its restart failed or its thread
  ended, would otherwise trap its sessions: when the bound build's
  compiler does not answer, the server reads the form itself (with the
  compiler's reader, `compiler_reader::read_form`, in no namespace's
  context) and runs it if it is `(light.dev/repl :ID)`; any other form is
  the compiler's error as before (`the compiler stopped`).
- **`light.dev/runtimes`** lists every build's runtimes, by ID (the order
  they connected in, since IDs are the server's), each as `{:id 2, :build
  :worker, :agent "node", :selected false}`; `:selected` marks the runtime
  the session's forms go to. A flag-only watch prints no `:build`, so its
  output is unchanged. **`light.dev/select! ID`** chooses among the bound
  build's runtimes; another build's is `runtime 1 runs :app, and the
  session is bound to :worker; (light.dev/repl :app) binds it to :app`.
- `src/dev_watch.rs` needed no change: each build's thread already served
  its mount's REPL (light-cljs-406), and the connect client needed none
  either, since it shows the namespace each answer gives.
- Tests: `tests/dev_nrepl.rs`, the Phase 3 exit criterion on a copy of the
  multi fixture: `watch app worker` serves `/app/index.js` and
  `/worker/index.js` from one host and port, and the page's unit names the
  worker by the absolute path `/worker/index.js`, which resolves on the
  page's origin; a save of `app.shared` reloads a Node runtime of each
  build; one nREPL connection evaluates in the app (`app.build-info`, which
  only the app has, and the runtime's process ID), then after
  `(light.dev/repl :worker)` in the worker (`app.jobs`, the worker's alone),
  with completions from the bound build, `light.dev/runtimes` listing both
  runtimes, `select!` of the other build's runtime refused, an unknown id's
  exact error, an interrupt of the worker's form, a second session still
  the app's, and `(light.dev/repl :app)` followed by a form in the same
  evaluation running in the app; a session cloned from one bound to the
  worker bound there too, with its runtime choice, and the close of a
  session that ran in both builds reaching both runtimes as `forget`; and
  a session of an app whose restart failed, and one opened after it,
  switching to the worker. `tests/dev_repl.rs`: `--repl --connect` on
  `watch app worker` switching to the worker and back, a form after the
  command in the same input running in the app; `watch features` on a
  copy of examples/features with `--repl --connect`, where
  `(light.dev/repl :features)` succeeds, `:nope` and four malformed calls
  are exact errors, and the runtime is listed with `:build :features`; and
  a flag-only `--watch`, where the call is the no-named-builds error, the
  runtimes carry no `:build`, and the call inside a form throws in the
  runtime. The existing REPL, nREPL and server tests pass unchanged.
  Checked with Node runtimes and the Node nREPL client, not in a browser
  or an editor.

### As built (light-cljs-408): cljs-repl ID

- **Dispatch.** `run` hands a first argument `cljs-repl` to
  `config_cli::cljs_repl`, which finds and validates the file as the other
  commands do (`config_cli::discover`), so a FILE named `cljs-repl` is read
  as `./cljs-repl` like the others. It takes exactly one id, with or
  without its colon: none is `light-cljs cljs-repl: requires one build id;
  /work/site/light-cljs.edn defines :app, :test, :worker`, two are
  `light-cljs cljs-repl app worker: takes one build id, the build the
  session is bound to; (light.dev/repl :ID) binds it to another`, and an id
  the file does not define is the unknown-id message of "Errors". After the
  id it takes `--print-length N` and `--print-level N`, read by the
  `--repl` client's own parser (`eval_cli::print_flag`, with its messages
  and defaults); any other flag, `--config-merge` and `--print-config`
  included, is `light-cljs cljs-repl app: --engine is not a flag of
  cljs-repl, which connects to a running watch; it takes --print-length and
  --print-level`. "Commands" lists those two as valid after any command;
  for `cljs-repl` they would change nothing, since it builds nothing and
  `:cache-root` is a top-level key no merge reaches, so they are refused
  rather than ignored.
- **Finding the watch.** The port file is `http.port` under the build's
  dev-mode `:cache-root` (layers 1-5, with `LIGHT_CLJS`, resolved against the
  project directory), the one `watch` writes. Its output directories are not
  compared (`ConfigFile::settings_before_flags`), since `cljs-repl` writes
  nothing. No file, a file that does not hold a port number, and a port
  whose connection is refused are all `light-cljs cljs-repl app: no watch
  is running for /work/site/light-cljs.edn; start one with light-cljs watch
  app`, the id without its colon. Any other failure to open the session is
  `--repl --connect`'s message under the command's words. The host is
  127.0.0.1, as designed, whatever `:http :host` says: a watch bound to
  another interface alone is reachable with `--repl --connect HOST:PORT`.
- **Binding.** The client opens a session on `/repl` (the first build's
  endpoint, as `--repl --connect` does; the session is the server's and
  goes to the build it is bound to, light-cljs-407) and posts
  `(light.dev/repl :ID)` before reading stdin, printing nothing for it; its
  error is the command's, the server's own text: `light-cljs cljs-repl test:
  the watch runs no build :test; its builds are :app, :worker` (or, for a
  port file a flag-only `--watch` could not have written but naming one,
  the no-named-builds error). The session then starts in the build's entry
  namespace on its most recently connected runtime, and the input's lines
  count from 1. A session the server forgot, after the watch restarted, is
  opened again bound to the same build (`repl_connect::Connection::bind`),
  which `--repl --connect` sessions, bound to no build by the client, do
  not need; if the restarted watch no longer runs the build, the client
  ends with that error.
- Tests: `tests/dev_repl.rs`, on temporary copies: `cljs-repl worker`
  against `watch app worker` of the multi fixture, starting in
  `app.worker` and running `jobs/run`, which only the worker's build has,
  in the worker's Node runtime (by its process ID), then switching to the
  app with `(light.dev/repl :app)`; `cljs-repl :worker` from `src/`;
  `cljs-repl test`, which the file defines and the watch does not run, as an
  exact error; `cljs-repl features` from the `app/` subdirectory of
  examples/features with `watch features` running, printing `3` for `(+ 1
  2)` and nothing else; a `:cache-root "state/light"` holding the port file,
  with `--print-length 3 --print-level 2` shortening the values printed; a
  session the server forgot, the watch started again on its port, opened
  again in the worker, the next form running in the new worker's runtime;
  and a table of exact errors with no watch running:
  an unknown id with and without a suggestion, no `http.port` (for `features`
  and `:features`), an `http.port` naming a closed port, one holding no
  port, no id, two ids, a foreign flag and a bad `--print-level`, none
  writing a file. `tests/cli.rs`: the flag-only test also runs
  `--repl --connect` and `./cljs-repl` beside an invalid `light-cljs.edn`.
  Checked with Node runtimes, not in a browser or a terminal by hand.

### As built (light-cljs-409): cache retention, measured

The driver is `bb benchmarks/build-configuration-retention.bb [FLAG ...]`
(each FLAG is added to every run; `LIGHT_CLJS_BINARY` picks the binary,
`target/release/light-cljs` by default). On a temporary copy of
`tests/fixtures/build-configuration/multi/`, with its one store
`.light-cljs/cache`, it runs each with `--stats`: `watch app worker` until
both builds print their built line, then stops it; `release app`; `release
worker`; `compile test`; `watch app worker` again. Then, as the single-build
baseline, `watch features`, `release features` and `watch features` on a
copy of examples/features. It prints each build's hits, misses, bypasses
and misses with their reasons, the store's artifacts per namespace (read
from its index files) and whether the last run missed; it exits 1 if it
did. No source changes between runs, so a miss in the last run is an
artifact `Store::trim` removed.

Measured on 2026-10-21 with the release binary, at the default three
(`app.build-info` and `app.main` bypass in every app build, blocked by the
file's `:cache-blockers`):

| Run | Build | Hits | Misses | Misses' reasons |
|---|---|---|---|---|
| 1 `watch app worker` | `:app` | 0 | 4 | `light.build`, `app.api`, `app.config`, `app.shared`: no artifact |
| 1 | `:worker` | 0 | 3 | `app.jobs`, `app.worker`: no artifact; `app.shared`: config changed |
| 2 `release app` | `:app` | 0 | 4 | compiler changed (the macro engine, below) |
| 3 `release worker` | `:worker` | 0 | 3 | `app.jobs`, `app.worker`: compiler changed; `app.shared`: analysis options changed |
| 4 `compile test` | `:test` | 0 | 2 | `app.shared`: analysis options changed; `app.test-runner`: no artifact |
| 5 `watch app worker` | `:app` | 3 | **1** | **`app.shared`: compiler changed** |
| 5 | `:worker` | 2 | **1** | **`app.shared`: config changed** |

Store afterwards: `app.shared` 3 artifacts, `app.api`, `app.config`,
`app.jobs`, `app.worker` and `light.build` 2 each, `app.test-runner` 1.
`cljs.core` and the runtime are not stored (the runtime is not a
namespace artifact), so `app.shared` is the one shared namespace;
`light.build` is the app's alone (`app.api`'s macro reads it).

**The builds evict each other.** `app.shared` has five front-end keys in
the sequence, one per build and command: the app's and the worker's
watches (they differ in `:config`, the app's `:dev :config`), the two
release builds, and the test build. A watch's startup build runs its
macros in the engine of the watch's session, and `compile` and `release`
in the native one; the macro engine is part of the compiler component
(`cache_key::compiler_identity`), so a watch never shares artifacts with
`compile` or `release`, even of the same build in the same mode (`compile
worker` then `watch worker` misses all three namespaces, "compiler
changed"). The release builds differ from `compile test` in analysis
options (release's warning mode; the app's also in `:elide-asserts` and
`:config`), and `compile test` from a `compile worker` in its resolution
component: a namespace's lookup records the namespace that required it
(`cache_key::lookup`, `app.test-runner` rather than `app.worker`). The miss
reason names the first component that differs from the newest candidate
kept, so the last run reports "compiler changed" and "config changed" for
artifacts that were evicted, not for changed inputs. Trimming keeps the
newest three by publish time, the build's own first: `release worker`
evicts the app watch's artifact and `compile test` the worker watch's,
which the last watch then misses.

The single-build baseline does not evict: its watch and release give each
namespace two keys, and every namespace keeps 2 (`app.editor`, `app.main`,
`app.reports`, `app.shared`, `clojure.string`, `light.lazy`). Adding
`compile features` to it would make three, the default's limit.

| Run | Build | Hits | Misses | Misses' reasons |
|---|---|---|---|---|
| 1 `watch features` | `:features` | 0 | 6 | no artifact |
| 2 `release features` | `:features` | 0 | 6 | compiler changed (the macro engine) |
| 3 `watch features` | `:features` | **6** | **0** | - |

Again with a larger retention, the driver's FLAG (the same with
`:cache-retention N` in the file). Runs 1 to 4 and the baseline are the
same as at three, hits, misses and reasons, since every artifact they could
use is still kept; the last run and the store differ:

| `--cache-retention` | Run 5 `:app` hits/misses (bypasses 2) | Run 5 `:worker` hits/misses | `app.shared` artifacts kept | Eviction |
|---|---|---|---|---|
| 3 (default) | 3 / 1 | 2 / 1 | 3 | yes |
| 4 | 3 / 1 | 2 / 1 | 4 | yes: `compile test` evicts the app watch's |
| 5 | **4 / 0** | **3 / 0** | 5 | no |

Five is the smallest value that removes the misses here, one per build and
command that requires the namespace; the baseline at five is the table
above. The default stays three: the flag-only commands are unchanged, and
a single build's watch, `compile` and `release` need three. The driver
runs every command without `LIGHT_CLJS`, so an ambient
`LIGHT_CLJS='{:cache-retention 5}'` does not change what it measures (run
so, it still reports the eviction at three). The numbers were the same
before and after rebasing onto light-cljs-408.

What was added, a flag first (principle 20):

- **`--cache-retention N`** on `--compile` (with `--entry`) and `--watch`, a
  positive integer, read by `count_flag` as `--jobs` is: `--cache-retention
  requires a number`, `--cache-retention requires a positive integer, got
  0`; without `--entry`, `--cache-retention bounds the cache of namespaces;
  it requires --entry NAMESPACE`, checked after the existing cache-flag
  message so its text is unchanged. It sets `Retention::per_namespace` in
  `CacheOptions::retention`, which `CompileCache::finish` passes to
  `Store::trim` in place of `Retention::default()`; the byte cap and the
  temporary files' grace are unchanged. With `--no-cache` it does nothing.
  The watch's startup build trims with it; steps never touch the cache.
- **`:cache-retention N`**, top level (`Setting::CacheRetention`), validated
  as `:compile-threads` is (`:cache-retention must be a positive integer,
  got 0`, at the value), with a built-in default of 3. `compile`, `release`
  and `watch` translate it into `--cache-retention`; the flag after the
  command wins (`--cache-retention 2, over 1 at light-cljs.edn:8:19`).
  `--print-config` prints it only when a layer or a flag gives it, as the
  compiler options print, so a file without it prints exactly as before. It
  enters no cache key ("Cache keys and fingerprints"). A watch whose reread
  finds it changed restarts every build, as for any top-level key; the
  restart's startup build trims with the new value.
- In a build map, the key gets the misplaced-key message of every top-level
  key (`:cache-retention in build :app is a top-level key, as in
  shadow-cljs`), whose "as in shadow-cljs" is about where top-level keys
  go; shadow-cljs has no such key (the "Deviations from shadow-cljs" row).

Tests: `tests/cli.rs` (`--cache-retention` parsing for `--compile` and
`--watch`, exact; five configurations leaving three candidates per namespace
without the flag, so the first misses again, and five with
`--cache-retention 5`, so it hits, with the same output; a flag-only
`--compile --cache-retention 1` from a directory holding an invalid
`light-cljs.edn`); `tests/config_file/reader.rs` (zero, a string, a negative
and a float); `tests/config_file/retention.rs` on copies of
examples/features (`:cache-retention 1` keeping one candidate, so `compile`
after `release` misses all six; `--cache-retention 2` over it keeping both,
so the next `compile` and `release` hit all six; the plain file keeping
three after four configurations; `--print-config` with no key, a flag, the
key, and the flag over the key; a bad key located, with nothing written);
`tests/dev_watch.rs` (`watch features` over a store seeded with four
`compile` artifacts per namespace: `:cache-retention 9` keeps all five at
startup, an edit to 2 restarts the build and trims to two, and
`--cache-retention 4` over `:cache-retention 1` keeps four at startup and
after a reread that changes the key and restarts the build).

## Phases

Queued on 2026-10-20 as the pellets group `build-configuration`, in this
order (the group's context names which can run in parallel):

| Phase | Pellet | Scope |
|---|---|---|
| 1 | light-cljs-398 | `src/config_file.rs`: discovery, the closed schema, `#light-cljs/env`, located errors; `examples/features/light-cljs.edn` |
| 1 | light-cljs-399 | Layers: `:dev`/`:release`, `LIGHT_CLJS`, `--config-merge`, origins |
| 1 | light-cljs-400 | The `compile` and `release` commands, flags over the file |
| 1 | light-cljs-401 | `--print-config` |
| 1 | light-cljs-402 | File builds equal flag builds, bytes and cache hits; the exit criterion |
| 2 | light-cljs-403 | `watch ID`: `:http`, `:nrepl`, `:fs-watch`, `:devtools`, `http.port` |
| 2 | light-cljs-404 | Rereading the file in the watch; the exit criterion |
| 3 | light-cljs-405 | Dev-server mounts per build (server only) |
| 3 | light-cljs-406 | Several builds in one watch: a thread each, one poller |
| 3 | light-cljs-407 | `light.dev/repl`, one nREPL for every build; the exit criterion |
| 3 | light-cljs-408 | `cljs-repl ID` |
| 3 | light-cljs-409 | The cache-retention measurement, and `:cache-retention` only if builds evict each other |

### Phase 1: the file, compile and release

- `src/config_file.rs`, the schema, `#light-cljs/env`, `LIGHT_CLJS`,
  `--config-merge`, every error above, discovery, and the `compile` and
  `release` commands, one or several ids, with `--print-config`.
- `examples/features/light-cljs.edn` and its `.gitignore` entry; README
  "Command-line usage" and the documentation list; this document's "As
  built" notes. dev-workflow.md and code-splitting.md already point here.
- Tests in a `tests/config_file.rs` binary: a table of bad files and their
  exact messages, one row per case and per key; the precedence layers,
  each overriding the one below; the byte-identical and all-hits comparison
  for every key; the environment-value key tests of "Cache keys and
  fingerprints"; and a command from a subdirectory finding the file and
  hitting the cache populated from the project directory.

Exit criterion: `light-cljs release features` in examples/features writes
the same files as the equivalent flags, and the second of the two runs is
all hits. Met by light-cljs-402: the same five files byte for byte, and 6
of 6 namespaces hit on the second run, in either order ("As built
(light-cljs-402)").

### Phase 2: watch, one build

- `light-cljs watch ID` with `:http`, `:nrepl`, `:fs-watch` and
  `:devtools`, the server always started, the port fallback and `:strict`,
  and `<:cache-root>/http.port` written once the port is bound.
- The configuration file in the watch's poll, with the restarts of
  "Watching the configuration file", tested in `tests/dev_watch.rs`: an
  edit of another build does nothing, an invalid edit keeps the build, a
  changed `:config` restarts it and the next step sees the new value.

Exit criterion: `light-cljs watch features` serves the example, and a
`:config` edit reaches the page without restarting the process. Met by
light-cljs-404: the edit reaches a connected Node runtime from the same
process on the same port ("As built (light-cljs-404)").

### Phase 3: several builds in one process

- `watch ID ID ...` as designed under "Several builds in one process":
  threads per build, one poller, one server with per-build mounts, one
  nREPL listener, `light.dev/repl`, per-build restarts.
- Tests in `tests/dev_server.rs` and `tests/dev_repl.rs`: two builds of a
  page and a worker, both reloading on a save of a shared namespace; one
  build's compile error leaving the other stepping; an nREPL session
  switching builds with `light.dev/repl`.
- `light-cljs cljs-repl ID` (see "Commands"), tested against a running
  two-build watch: a session bound to the named build, and the three
  errors.
- **Cache retention, measured.** On the Phase 3 test project (the
  illustration's `:app`, `:worker` and `:test` with a shared namespace),
  run with one cache directory: `watch app worker` until both are built,
  stop it, `release app`, `release worker`, `compile test`, then `watch app
  worker` again, each run with `--stats`. Builds evict each other when the
  last run reports a miss for a namespace whose inputs did not change,
  because `Store::trim` (`src/artifact_store.rs`, `Retention::per_namespace`,
  three) removed its artifact. Record the per-run hit and miss counts and
  the store's artifacts per shared namespace in "As built". **Only if builds
  evict each other**, add a flag, `--cache-retention N` (artifacts kept per
  namespace, positive, default 3), then its top-level key,
  `:cache-retention N`, under principle 20, and repeat the measurement with
  a value that removes the misses; one store stays shared, never a store
  per build. If they do not, record the numbers and add neither.

Exit criterion: the illustration's `watch app worker` serves the page and
its worker from one origin, a save reloads both, and one nREPL connection
evaluates in either; `light-cljs cljs-repl worker` evaluates in the
worker; and the retention measurement is recorded. The server and nREPL
parts are met by light-cljs-407 ("As built (light-cljs-407)"), and
`cljs-repl worker` by light-cljs-408, tested against that watch ("As built
(light-cljs-408)"); the measurement is recorded by light-cljs-409 ("As
built (light-cljs-409)"): the builds evict each other at three, and
`:cache-retention 5` removes the misses.

### Deferred

- Further `:modules`, with code-splitting.md Phase 4.

## Names for review

Names shadow-cljs does not supply, each after the flag it stores, in
shadow-cljs's style:

| Thing | Name | Flag |
|---|---|---|
| The file | `light-cljs.edn` | - |
| The concept and this document | **configuration file**, **build configuration**; `docs/build-configuration.md` | - |
| Build-time values for `light.build/config` | build key `:config` | `--config KEY=EDN` |
| ClojureScript warning compatibility | `:compiler-options {:cljs-warnings true}` | `--warnings` |
| Macro limits | `:compiler-options` `:macro-expansions`, `:macro-nodes`, `:macro-timeout-ms`, `:macro-memory-limit`, `:macro-poll-budget` | the same names |
| Environment values | `#light-cljs/env`, and the variable `LIGHT_CLJS` | - |
| Printing the merged configuration | `--print-config` | - |
| No nREPL on the dev server's port | `:nrepl false` (shadow-cljs's) | `--no-nrepl` |
| Switching an nREPL session's build | `(light.dev/repl :worker)` | - |
| Artifacts kept per namespace, added because the Phase 3 measurement showed eviction (light-cljs-409) | top-level `:cache-retention N` | `--cache-retention N` |
| Rust | `light_cljs::config_file`, `FILE_NAME`, `ConfigFile`, `ConfigMerge`, `BuildDefinition`, `BuildSettings`, `BuildMode::{Dev, Release}`, `BuildId`, `Setting`, `SettingOrigin`, `SettingValue`, `ConfigSource`, `deep_merge`, `ConfigFileError`, `config_cli`, `BuildWorker`, `BuildConfiguration::set` | - |

Shadow-cljs's own names this design maps a light-cljs flag to, for
checking: `:source-paths`, `:modules` `:entries`, `:output-dir`,
`:optimizations`, `:closure-defines`, `:pseudo-names`, `:elide-asserts`,
`:checked-arrays`, `:compile-threads`, `:cache-root`, `:cache-blockers`,
`:build-options` `:cache-level`, `:fs-watch` `:loop-wait`, `:http` `:port`
`:host` `:strict`, `:nrepl` `:port`, `:devtools` `:reload-strategy :full`,
`:dev`, `:release`, `--config-merge`, `--pseudo-names`.

## Settled questions

Decided by the user on 2026-10-20. A pellet does not reopen these; a
finding that contradicts one is recorded under a new "Open questions"
section, and the pellet still closes.

- **Follow shadow-cljs everywhere**, with its names verbatim (keys, values,
  commands, `--config-merge`, `:dev`/`:release`), and deviate only for a
  real reason, listed under "Deviations from shadow-cljs".
- **Subcommands**: `compile`, `watch`, `release` (and `cljs-repl`), taking
  build ids. A command that starts with a flag never reads the file and
  behaves exactly as today.
- **Several builds in one process** (Phase 3).
- **`#light-cljs/env` and `--config-merge`**, as shadow-cljs's
  `#shadow/env` and `--config-merge`, with `LIGHT_CLJS` as `SHADOW_CLJS`.
- **Discovery** walks up parent directories to the first `light-cljs.edn`;
  no flag names the file.
- **Flags always win** over every layer of the file (cache blockers
  accumulate, "Precedence").
- **Defines and checked arrays in dev mode: no.** Dev builds do not gain
  `:closure-defines` or `:checked-arrays` support. Either key reaching dev
  mode stays a located error, a permanent deviation from shadow-cljs, not
  a gap a later pellet closes. Shadow-cljs files that put
  `:closure-defines` at the build level move them under `:release`.
- **Cache retention across several builds: measure first.** Three
  artifacts per namespace hold dev and release of one build plus one more
  configuration; a watch of two builds with different `:config` already
  uses two. Phase 3 measures on its test project whether builds evict each
  other ("Phase 3"); a retention flag and its top-level key,
  `--cache-retention N` and `:cache-retention N`, are added only if they
  do. Per-build stores are not an option: they would lose the sharing.
  **Measured (light-cljs-409): they do.** The test project's sequence gives
  `app.shared` five front-end keys, one per build and command, so at three the
  last `watch app worker` misses it in both builds with unchanged sources;
  `--cache-retention 5` (or `:cache-retention 5`) removes the misses, and 4
  does not. The flag and key were added; the default stays three ("As
  built (light-cljs-409)").
- **`light-cljs cljs-repl ID`: yes, in Phase 3**, as designed under
  "Commands": the running watch's port from `<:cache-root>/http.port`, then
  today's `--repl --connect` client bound to the build.

## Open questions

None. Further `:modules` wait for code-splitting.md's Phase 4 ("Deferred").
