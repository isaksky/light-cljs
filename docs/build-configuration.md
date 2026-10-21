# Build configuration: light-cljs.edn

Status: settled design, October 20, 2026. The user decided its three open
questions the same day ("Settled questions"). Nothing here is implemented;
the implementation is queued as the pellets group `build-configuration`,
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
| Defaults: `:fs-watch {:loop-wait 500}`, `:compile-threads` the processor count | 100 ms; 1 | Today's `--poll-ms` default; `--jobs` is experimental and did not meet its gate (compiler-concurrency.md) |
| A cache per build and mode, `.shadow-cljs/builds/<id>/<mode>` | One content-addressed store, `<:cache-root>/cache`, shared by every build and mode | Artifacts are keyed by their inputs (compiler-cache.md), so sharing is safe and saves work |

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
  shadow-cljs: `light-cljs release: requires one or more build ids; this
  configuration defines :app, :test, :worker`.
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
                        :macro-timeout-ms 30000        ; light-cljs.edn:11:41
                        :elide-asserts true}           ; light-cljs.edn:16:48 (:release)
     :closure-defines {app.config/DEBUG false          ; light-cljs.edn:14:49 (:release)
                       app.config/VERSION "2.4.0"}     ; light-cljs.edn:15:51 (:release), from APP_VERSION
     :build-options {:cache-level :all}}}}             ; default
  ```
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
| `--no-cache` | `:build-options {:cache-level :off}` | build | both |
| `--cache-blocker NS` | `:cache-blockers #{NS}`, and `:build-options {:cache-blockers #{NS}}` | top level, build | both |
| `--poll-ms N` | `:fs-watch {:loop-wait N}` | top level | `watch` |
| `--serve PORT`, `--serve-host HOST` | `:http {:port PORT :host "HOST"}` | top level | `watch` |
| `--nrepl PORT` | `:nrepl {:port PORT}` | top level | `watch` |
| `--reload-dependents` | `:devtools {:reload-strategy :full}` | build | `watch` |
| `--compile`, `--reloadable`, `--watch` | the command: `compile`, `watch`, `release` | - | - |

Keys light-cljs adds, because shadow-cljs has no word for them:
`:compiler-options` `:cljs-warnings` and the five `:macro-*` limits, and
the build's `:config`. They are listed under "Names for review".

### Top-level keys

| Key | Value | Default |
|---|---|---|
| `:source-paths` | vector of directory strings | required |
| `:builds` | map of build id to build configuration, at least one | required |
| `:cache-root` | directory string | `".light-cljs"`; the store is `.light-cljs/cache`, today's default |
| `:cache-blockers` | set of namespace symbols | `#{}` |
| `:compile-threads` | positive integer (experimental) | 1 |
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
| Scalar flags (`--entry`, `--out-dir`, `--optimize`, `--jobs`, `--poll-ms`, `--serve`, the macro limits, ...) | Replace the value |
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

- **A build from the file and the same flags hit the same artifacts**,
  when the roots are spelled the same. A build from the file spells its
  roots as the project directory joined with the path, lexically
  normalized, so it gets the same keys from any subdirectory; a flag-only
  build spells them as typed. A root spelled two ways gives a "resolution
  changed" miss, never a wrong hit.
- **Editing the file recompiles only what its new values change.** A
  comment, another build's section, `:http`, `:devtools` or `:output-dir`
  changes no front-end key.
- **Builds do not partition the cache.** Two builds with equal front-end
  inputs share artifacts; dev and release of one build differ in warning
  mode, so each namespace has two front-end keys. The store keeps three
  artifacts per namespace (compiler-cache, "Loading, diagnostics, and
  storage"), which a several-build watch makes easier to exceed. Phase 3
  measures whether builds evict each other, and only then adds a retention
  setting ("Settled questions").
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
| A bad `:config` value | `--config`'s own message, at the value: `light-cljs.edn:10:29: :config :app/n of build :app: 1N is not a JavaScript number` |
| `#shadow/env`, `#env`, another tag | `light-cljs.edn:3:15: #shadow/env: write #light-cljs/env, which takes the same forms` |
| A malformed `#light-cljs/env` | `light-cljs.edn:3:15: #light-cljs/env takes "NAME", ["NAME"], ["NAME" DEFAULT] or ["NAME" :as TYPE :default DEFAULT]` |
| An unset variable where a value is required | `light-cljs.edn:15:51: :closure-defines app.config/VERSION of build :app (release) is nil: APP_VERSION is unset and the form gives no default` |
| `:as :int` on a non-integer | `light-cljs.edn:3:15: HTTP_PORT is "web", not a decimal integer (:as :int)` |
| `--config-merge` not a map, `classpath:`, `:dev` inside it | the message, at `--config-merge N` and its line and column |
| Two builds, one directory | `light-cljs.edn:19:16: builds :app and :worker both write out/app in dev mode` |
| An output directory equal to the cache store or a source path | an error naming both |
| A duplicate key, a parse error | the reader's message, at its span |
| An unknown build id | `light-cljs release ap: light-cljs.edn defines no build :ap; did you mean :app? Its builds are :app, :test, :worker` |
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
all hits.

### Phase 2: watch, one build

- `light-cljs watch ID` with `:http`, `:nrepl`, `:fs-watch` and
  `:devtools`, the server always started, the port fallback and `:strict`,
  and `<:cache-root>/http.port` written once the port is bound.
- The configuration file in the watch's poll, with the restarts of
  "Watching the configuration file", tested in `tests/dev_watch.rs`: an
  edit of another build does nothing, an invalid edit keeps the build, a
  changed `:config` restarts it and the next step sees the new value.

Exit criterion: `light-cljs watch features` serves the example, and a
`:config` edit reaches the page without restarting the process.

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
worker; and the retention measurement is recorded.

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
| Switching an nREPL session's build | `(light.dev/repl :worker)` | - |
| Artifacts kept per namespace, only if the Phase 3 measurement shows eviction | top-level `:cache-retention N` | `--cache-retention N` |
| Rust | `light_cljs::config_file`, `FILE_NAME`, `ConfigFile`, `ConfigMerge`, `BuildDefinition`, `BuildSettings`, `BuildMode::{Dev, Release}`, `BuildId`, `Setting`, `SettingOrigin`, `ConfigFileError`, `config_cli`, `BuildWorker`, `BuildConfiguration::set` | - |

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
- **`light-cljs cljs-repl ID`: yes, in Phase 3**, as designed under
  "Commands": the running watch's port from `<:cache-root>/http.port`, then
  today's `--repl --connect` client bound to the build.

## Open questions

None. Further `:modules` wait for code-splitting.md's Phase 4 ("Deferred").
