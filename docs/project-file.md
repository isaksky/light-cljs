# The project file: light-cljs.edn

Status: proposed design, October 20, 2026. Nothing here is implemented, and
no pellets are queued for it. Every name below (file, keys, flags, Rust
types) is a decision for the user to review, listed in "Naming decisions".

The user asked on 2026-10-20 for "something like shadow-cljs.edn, where you
can define the build", named after the tool. This document designs that
file: `light-cljs.edn` at the project root, holding the project's source
roots, its cache settings and its named builds, each with development and
release settings. A build is run by name:

```sh
light-cljs --watch --build app            # development: the dev build, watched and served
light-cljs --compile --build app          # release: the chunked build
```

It supersedes in part two deferrals: [dev-workflow.md](dev-workflow.md),
"Settled questions", "Flag names" ("no configuration file; flags are the
only configuration until a second build in one process is needed") and
"Phase 5: deferred", "Multiple watch builds ... and a configuration file";
and the sentence of [code-splitting.md](code-splitting.md) that says
light-cljs "has no build configuration file". The file comes first; several
builds in one watch process stay deferred, for the reasons under "Several
builds in one process".

## Goal

- One file says how to build the project, so a command names a build
  instead of repeating `--entry`, `--source-root`, `--out-dir`, `--config`,
  `--serve` and the macro limits.
- Every existing flag keeps working, with and without the file. An
  invocation that names no build never reads the file and behaves exactly as
  it does today.
- A build from the file compiles exactly what the equivalent flags compile:
  the same output bytes and the same cache keys. The cache never sees the
  file, only the compiler inputs the file produced.
- The file is strict: an unknown key, a misplaced key or a bad value is an
  error at its line and column, with a suggestion where one exists.

Non-goals: dependency resolution (no Maven coordinates, no npm installs;
packages stay host- or bundler-supplied, [compiler.md](compiler.md),
"JavaScript packages"), build hooks that run user code, environment-variable
readers, a per-user configuration file, and settings that no flag has today
(each new setting is a flag first, then a key; see "Design principles").

## What shadow-cljs does

Citations are into `research/shadow-cljs/src/main/shadow/`.

- **One file at the project root.** The npm launcher walks up from the
  current directory to the first `shadow-cljs.edn`, so a command run in
  `$PROJECT/src/demo` finds `$PROJECT/shadow-cljs.edn`
  (`cljs/npm/cli.cljs:143-157`), and runs the JVM in that directory, where
  `load-cljs-edn` reads `shadow-cljs.edn` relative to it
  (`cljs/devtools/config.clj:143-156`).
- **Top-level keys** are `:source-paths`, `:dependencies` (Maven
  coordinates), `:deps` (use deps.edn instead), `:builds`, and server
  settings such as `:http`, `:nrepl`, `:dev-http` and `:cache-dir`
  (default `.shadow-cljs`, `:92-95`). Builds are a map of build id keyword
  to build map, and each build gets its id assoc'ed as `:build-id`
  (`:56-90`).
- **A build** needs `:target` (`:browser`, `:esm`, `:node-script`,
  `:npm-module` and more) and has `:output-dir`, `:asset-path`, `:modules`
  (an explicit chunk graph of `:entries` and `:depends-on`), `:devtools`
  (`:http-port`, `:reload-strategy`, hooks), `:compiler-options`,
  `:closure-defines`, `:js-options` and `:build-hooks`
  (`build/config.clj:15-33`).
- **Modes.** `shadow-cljs compile app` and `watch app` are `:dev`;
  `release app` is `:release`. `config-merge` deep-merges the build's `:dev`
  or `:release` map over the build and drops both keys
  (`build.clj:172-178`). The layers are build defaults, target defaults, the
  build, its mode map, then each `--config-merge DATA` from the command
  line (`build.clj:330-334`, `cljs/devtools/cli_opts.cljc:67-73`).
  `deep-merge` merges maps by key and concatenates vectors, keeping distinct
  elements (`build/api.clj:26-40`).
- **Overrides beyond the file.** `~/.shadow-cljs/config.edn` (or under
  `XDG_CONFIG_HOME`) is merged under the project file and the `SHADOW_CLJS`
  environment variable over it (`cljs/devtools/config.clj:111-156`), and
  `#shadow/env` reads environment variables inside the file (`:103-109`).
  Penpot's file uses it for ports, the cache directory, the optimization
  level and a version define (`research/penpot/frontend/shadow-cljs.edn`).
- **Validation is open.** The config spec is `s/keys`, which accepts any
  extra key, and the default target spec is `any?`
  (`cljs/devtools/config.clj:46-54`, `build/config.clj:21-33`): a misspelled
  key is silently ignored.
- **Several builds in one process.** `shadow-cljs watch app worker` runs
  both in one JVM, and the server watches `shadow-cljs.edn` and restarts a
  build whose configuration changed (`cljs/devtools/server/config_watch.clj`).
- **The cache is per build and mode**, `.shadow-cljs/builds/<id>/<mode>`
  (`cljs/devtools/config.clj:183-187`).

### deps.edn aliases and the ClojureScript compiler options

- **deps.edn** has `:paths` and `:deps`, and `:aliases` that compose: `clj
  -M:dev:test` merges `:extra-paths` (concatenated) and `:main-opts` (the
  last wins). Aliases are general: any number of them, in any order, each
  able to change anything.
- **The ClojureScript compiler options** are one flat map per build
  (`:main`, `:output-to`, `:output-dir`, `:optimizations`, `:modules`,
  `:closure-defines`, `:elide-asserts`, `:source-map`, `:asset-path`),
  passed to `cljs.build.api/build` or merged from a file with `cljs.main
  -co opts.edn`. There are no named builds and no modes: a project keeps
  one options file per build.

### What carries over, and what is deliberately different

| shadow-cljs, deps.edn, cljs.main | light-cljs.edn | Why |
|---|---|---|
| `shadow-cljs.edn` at the root, found by walking up | `light-cljs.edn` at the root, found by walking up; `--project-file PATH` names it | Same habit; the explicit flag serves scripts and monorepos |
| `:builds` map of build id to build | `:builds` map of build name to build | Same shape |
| `:dev` / `:release` deep-merged over the build | `:development` / `:release`, merged over the build | Same model, full words |
| `watch app`, `compile app`, `release app` | `--watch --build app`, `--compile --build app --reloadable`, `--compile --build app` | The CLI has mode flags, not subcommands (dev-workflow.md, "Flag names"); the mode follows from them |
| `:source-paths` | `:source-roots` | The CLI's `--source-root`; a build may replace them |
| `:dependencies`, `:deps` | none | The compiler resolves no dependencies; library sources are source roots |
| `:target` | none | Every build writes ES modules (chunks) in release and the dev build in development; one shape runs in browsers, Node and bundlers |
| `:modules` (explicit chunk graph) | none; `:chunks` reserved for shaping | Chunks are inferred from `light.lazy/loadable` (code-splitting.md); Phase 4's overlays get `:chunks` |
| `:output-dir` | `:output-directory` | Required; release and development name different directories |
| `:closure-defines` | `:defines` under `:release` | `--define` |
| `:compiler-options {:optimizations ...}` | `:optimization` under `:release` | `--optimize`; no `:simple` or `:whitespace` |
| `:elide-asserts true` | `:assertions false` | `--elide-asserts`, said positively |
| `:devtools`, `:dev-http`, `:http`, `:nrepl` | `:server` and `:poll-interval-milliseconds` under `:development` | `--serve`, `--serve-host`, `--nrepl`, `--poll-ms` |
| `:cache-dir`, `:cache-blockers` | `:cache {:directory ... :blockers [...]}` | One content-addressed cache for every build and mode |
| deps.edn aliases, composable | one build and one mode | Two layers say what built an output; arbitrary composition does not |
| `--config-merge DATA` | the existing flags | Each flag already overrides its one setting |
| `#shadow/env`, `SHADOW_CLJS`, `~/.shadow-cljs/config.edn` | none | What varies per run is a flag on the command line, which the command shows |
| Open specs: unknown keys ignored | Closed key sets: unknown keys are errors | A misspelled key must not build something else silently |
| `:build-hooks` | none | No user code runs outside macros, whose inputs the cache contract governs |

## Design principles

20. **The file is a stored set of flags.** Every setting the file holds is
    a flag that exists, with the same meaning and the same validation. A
    build from the file is translated into the flags' own structures
    (`CompileFlags` and the watch's `Options` in `src/main.rs` and
    `src/dev_watch.rs`) before anything compiles. A new setting is a flag
    first; the key follows in the same change.
21. **Flags win.** A flag given on the command line overrides the file's
    value for its setting. The one exception, cache blockers, accumulate,
    because a blocker is a correctness declaration (see "Precedence").
22. **No file unless asked.** Only `--build NAME`, `--project-file PATH` and
    `--describe-build` read a project file. Today's commands never look for
    one.
23. **Strict and located.** The key sets are closed. Every error names the
    file, line and column, the build and the mode, and what would be valid.
    The whole file is validated each time it is read, not only the build
    that runs.
24. **Full words.** Keys, flags, modes and types are spelled out; a
    quantity spells its unit as a suffix (`:timeout-milliseconds`,
    `:memory-bytes`). Existing abbreviated flags stay as they are (see the
    open question on first-class spellings).

Principles 1-19 are those of advanced-optimization.md, code-splitting.md
and dev-workflow.md; the numbering continues theirs.

## The file and where it is found

The file is named `light-cljs.edn`, after the tool, as `shadow-cljs.edn`
is. Its directory is the **project directory**: every relative path in the
file resolves against it, never against the current directory.

- **Discovery.** With `--build NAME` and no `--project-file`, light-cljs
  looks for `light-cljs.edn` in the current directory, then in each parent
  directory up to the file system root, and uses the first one found. No
  boundary (such as a `.git` directory) stops the walk, as neither Cargo's
  nor shadow-cljs's does. The file used is named in the first status line
  of `--watch`, in `--describe-build`, and in every error about it.
- **Explicit path.** `--project-file PATH` reads that file and walks
  nothing; PATH is relative to the current directory. It requires `--build
  NAME` (or `--describe-build`); alone it is an error, since it would
  configure nothing.
- **Not found.** `--build app` with no file found is an error naming the
  directories searched: `--build app: no light-cljs.edn in /work/site/src
  or any parent directory; create one (docs/project-file.md) or name it
  with --project-file PATH`.
- **The cache directory** of a build from the file defaults to
  `.light-cljs/cache` in the project directory, not in the current
  directory, so the build hits the same artifacts from any subdirectory.
  Flags-only builds keep today's default under the current directory.

## Schema

The file is one EDN map, read with the compiler's syntax reader
(`parser::parse`), which gives every form a byte span for messages. It is
plain EDN: no tagged literals, no reader conditionals, no discards that
hide a duplicate key. Symbols appear only where a key asks for a namespace
or a Var.

### Top-level keys

| Key | Value | Required | Flag |
|---|---|---|---|
| `:source-roots` | vector of directory strings | no (default: none, so each build must give its own) | `--source-root DIR` |
| `:cache` | `{:directory "DIR" :blockers [ns ...]}` or `false` | no (default `{:directory ".light-cljs/cache"}`) | `--cache-dir DIR`, `--cache-blocker NS`, `--no-cache` |
| `:defaults` | a build map with no `:entry`, `:output-directory` or `:source-roots`, in its mode maps either | no | - |
| `:builds` | map of build name to build map, at least one | yes | - |

- `:source-roots` is the project's search path and the default of every
  build. It is not a key of `:defaults`: a setting has one place at each
  level.
- `:cache` is project-wide. Artifacts are content-addressed by their keys
  ([compiler-cache.md](compiler-cache.md)), so every build and both modes
  share one directory safely: two builds with the same front-end inputs
  share artifacts, which shadow-cljs's per-build directories cannot. The
  store is safe for concurrent processes (its advisory lock, compiler-cache
  "Loading, diagnostics, and storage"). `:blockers` lists namespace symbols,
  as `--cache-blocker` does. `false` is `--no-cache`. Its keys are
  `:directory` and `:blockers` only.
- `:defaults` applies under every build (see "Precedence"). It may hold
  `:development` and `:release` maps of its own.

### Build names

A build name is an unqualified keyword (`:app`, `:worker`, `:test`): a
letter followed by letters, digits and hyphens. On the command line it is
written without its colon, as a `--config` key is: `--build app`. A
leading colon (`--build :app`) is an error that says so. Builds get dense
integer IDs, `BuildId`, in ascending name order (principle 1 of
dos-and-donts.md), which the several-build watch indexes its per-build
state by; names appear only in the file, flags and messages.

### Build keys

Keys that both modes read are written in the build map, and either mode map
may override them:

| Key | Value | Required | Flag |
|---|---|---|---|
| `:entry` | namespace symbol | yes | `--entry NAMESPACE` |
| `:source-roots` | vector of directory strings; replaces the project's | no | `--source-root DIR` |
| `:output-directory` | directory string | yes, after merging, for each mode the build runs in | `--out-dir DIR` |
| `:configuration` | map of qualified or simple keyword to EDN value | no | `--config KEY=EDN` |
| `:assertions` | boolean, default `true` | no | `--elide-asserts` (as `false`) |
| `:macro-limits` | map, below | no | `--macro-*` |
| `:development` | map of development keys and shared keys | no | `--watch`, `--reloadable` |
| `:release` | map of release keys and shared keys | no | `--compile` |

`:macro-limits` holds the five macro session limits, each a positive
integer, each optional, defaults as today:

| Key | Flag | Default |
|---|---|---|
| `:expansions` | `--macro-expansions N` | 1024 |
| `:generated-nodes` | `--macro-nodes N` | 100000 |
| `:timeout-milliseconds` | `--macro-timeout-ms N` | 10000 |
| `:memory-bytes` | `--macro-memory-limit N` | 67108864 |
| `:poll-budget` | `--macro-poll-budget N` | 1000000 |

`:memory-bytes` keeps the flag's meaning under `--jobs`: one budget for all
isolates together (compiler-concurrency.md, "Memory is one budget").

`:configuration` values follow `--config`'s rules exactly: keywords,
decimal integers and floats, strings, booleans, nil and lists, vectors,
maps and sets of them, with no symbols, tagged literals, `1N`, `1M`, radix
or ratio numbers, and no key or set element repeated by value. A value's
source text is handed to the same reader (`BuildConfiguration::insert`), so
`:configuration {:app/flavor :dev}` and `--config app/flavor=:dev` give the
same canonical text and the same fingerprint.

### Release keys

Written under `:release` (of a build or of `:defaults`) only. `--watch`
takes none of these flags and `--reloadable` refuses the first three; the
analysis switches and `:jobs` stay in release too, so that a build's
development mode means the same under `--watch` and `--compile
--reloadable`. Their flags still work on a `--compile --reloadable`
command line, as today:

| Key | Value | Flag |
|---|---|---|
| `:optimization` | `:none`, `:prepare` (default) or `:advanced` | `--optimize LEVEL` |
| `:defines` | map of qualified symbol to boolean, number or string | `--define NS/NAME=VALUE` |
| `:pseudo-names` | boolean | `--pseudo-names` |
| `:warning-mode` | boolean | `--warnings` |
| `:checked-arrays` | `:warn` or `:error` | `--checked-arrays-warn`, `--checked-arrays-error` |
| `:jobs` | positive integer, default 1 (experimental) | `--jobs N` |

A release build always writes `:output-directory` as `--out-dir` does: one
ES module per chunk and `chunks.json`, or one module named after the entry
when the program has no loadables. The default optimization stays
`:prepare`, as for `--compile`, so a build from the file and the same flags
agree; a project that wants `:advanced` says so. `:defines` is read
against the program once it is analyzed, as `--define` is, so a define that
names no `goog-define` fails the build with the flag's message.

### Development keys

Written under `:development` only, since only `--watch` reads them:

| Key | Value | Flag |
|---|---|---|
| `:poll-interval-milliseconds` | positive integer, default 100 | `--poll-ms N` |
| `:server` | `{:port N :host "HOST" :nrepl-port N}` | `--serve PORT`, `--serve-host HOST`, `--nrepl PORT` |
| `:reload-dependents` | boolean | `--reload-dependents` |

`:server` starts the server as `--serve` does; `:port` is required in it
and is 0 to 65535 (0 picks a free port), `:host` defaults to loopback, and
`:nrepl-port` adds the separate nREPL listener and `.nrepl-port`.
`:reload-dependents` requires `:server`, as the flag requires `--serve`.

`--compile --build NAME --reloadable` writes the development build once
and reads only the shared keys; it does not reject the development keys,
which belong to the same mode and are simply not used by a one-shot build.

### Reserved: `:chunks`

Chunk shaping, deferred to code-splitting.md "Phase 4: shaping and
tooling", gets one key under `:release` when it is built:
`:chunks {:groups {"editing" [app.editor app.toolbar]} :merge-below-bytes
4096 :host-entries [app.embed]}`, for `--chunk NAME:ns,ns`,
`--chunk-min-bytes N` and `--chunk-entry NS`. Until then `:chunks` is an
error that says it is reserved and names that phase.

### Not in the file, and why

| Flag | Reason |
|---|---|
| `--stats`, `--stats-file`, `--size-report` | They report on one run and change no output |
| `--source-map`, `--source-map-inline`, `--source-map-include-sources` | Builds from the file write directories: `--out-dir` refuses source maps and dev builds always have them. A key arrives with chunk source maps |
| `--esm` | Implied by an output directory |
| `--reloadable` | It is the development mode, chosen on the command line |
| `--parse`, `--analyze`, `--text`, FILE | Single-source tools, not builds |
| `--eval`, `--repl` and their limits | Not builds; see the open question on `--repl --build` |

### A full illustration

A project with a page and a web worker:

```clojure
{:source-roots ["src"]
 :cache {:blockers [app.build-info]}
 :defaults {:macro-limits {:timeout-milliseconds 30000}
            :release {:optimization :advanced}}
 :builds
 {:app {:entry app.main
        :configuration {:app/api-origin "https://api.example.org"}
        :development {:output-directory "out/app/development"
                      :configuration {:app/api-origin "http://127.0.0.1:8080"}
                      :server {:port 9630 :nrepl-port 0}
                      :reload-dependents true}
        :release {:output-directory "out/app/release"
                  :defines {app.config/DEBUG false}
                  :assertions false}}
  :worker {:entry app.worker
           :development {:output-directory "out/app/development/worker"}
           :release {:output-directory "out/app/release/worker"}}
  :test {:entry app.test-runner
         :source-roots ["src" "test"]
         :configuration {:app/api-origin "http://127.0.0.1:8080"}
         :release {:output-directory "out/test"
                   :optimization :none}}}}
```

`light-cljs --compile --build app` compiles `app.main` from `src` at
`advanced` with `DEBUG` false and no assertions into `out/app/release`;
`light-cljs --watch --build app` writes `out/app/development` and serves it
on port 9630. The worker's development output sits inside the app's, so
the app's server serves it from the same origin, as a `Worker` requires,
until one process can watch both (see "Several builds in one process").

## Modes

A build has two modes, **development** and **release**, and the command
line chooses one through the flags it already has:

| Command | Mode | Writes |
|---|---|---|
| `light-cljs --watch --build NAME` | development | the dev build, then watches |
| `light-cljs --compile --build NAME --reloadable` | development | the dev build once |
| `light-cljs --compile --build NAME` | release | the chunked build |

There is no `--release` or `--development` flag: `--reloadable` already
means the dev build, and `--compile` without it the release build, so a
second spelling of the mode could only disagree with the first. A watch
of the release settings is not offered: the watch writes the dev build,
whose contract (dev-workflow.md, "Dev-mode contract") is development's.

The mode map of the chosen mode is merged over the build and the other is
ignored for this run, but both are validated whenever the file is read. A
build may leave a mode out: one with no `:output-directory` for
development (the `:test` build of the illustration) runs only in release,
and naming it with `--watch` is an error that says so.

## Selecting a build on the command line

- `--build NAME` goes where `--entry` goes, after `--compile` or `--watch`.
  With it, `--entry`, `--source-root` and `--out-dir` are optional; given,
  they override the build's values.
- `--build` is given once. With `--watch`, a second `--build` is an error
  until several builds in one process exist: `--watch takes one --build
  for now; run one watch per build, in separate terminals
  (docs/project-file.md, "Several builds in one process")`.
- `--describe-build` with `--build NAME` and a mode flag (`--compile`,
  `--compile --reloadable` or `--watch`) prints every setting of that build
  and mode, its value and where it came from (the file's line and column,
  `:defaults`, the built-in default, or the flag), then exits without
  compiling. It is how a user checks precedence. For the illustration
  above, `light-cljs --compile --build app --optimize none --describe-build`
  prints

  ```text
  light-cljs: build :app, release, from /work/site/light-cljs.edn
  entry             app.main                         light-cljs.edn:6:16
  source-roots      ["src"]                          light-cljs.edn:1:16
  output-directory  "out/app/release"                light-cljs.edn:12:37
  optimization      :none                            --optimize none, over light-cljs.edn:4:37 (:defaults)
  assertions        false                            light-cljs.edn:14:31
  defines           {app.config/DEBUG false}         light-cljs.edn:13:28
  configuration     {:app/api-origin "https://api.example.org"}  light-cljs.edn:7:24
  macro-limits      {:timeout-milliseconds 30000}    light-cljs.edn:3:50 (:defaults)
  cache             {:directory ".light-cljs/cache" :blockers [app.build-info]}  default, light-cljs.edn:2:9
  jobs              1                                default
  ```

- Without `--build`, every command parses exactly as today, and
  `--project-file` or `--describe-build` without `--build` is an error.

## Precedence

From lowest to highest, each layer overriding the ones before it:

1. The built-in defaults, which are today's flag defaults.
2. The project's top-level `:source-roots` and `:cache`.
3. `:defaults`.
4. `:defaults`'s map for the chosen mode.
5. The build.
6. The build's map for the chosen mode.
7. Flags on the command line.

Everything a build says beats everything `:defaults` says, and a mode map
beats the map it sits in.

How a layer overrides depends on the value:

| Setting | Override |
|---|---|
| Scalars (`:entry`, `:output-directory`, `:optimization`, `:assertions`, `:jobs`, ...) | Replaced |
| `:source-roots` | Replaced as a whole; a build that adds `test` lists `["src" "test"]`. A list of `--source-root` flags replaces the file's list |
| `:configuration`, `:defines` | Merged by key; the higher layer wins per key. `--config K=V` sets K over the file's value; `--config` naming one key twice is still an error |
| `:macro-limits`, `:server` | Merged by key |
| `:cache` (top level only) | `--cache-dir DIR` replaces `:directory`, and turns the cache on in DIR over `:cache false`; `--no-cache` turns it off |
| `:cache :blockers` | Union of the file's and every `--cache-blocker` |

**The exception.** Cache blockers accumulate: no layer and no flag removes
one. A blocker declares that a namespace's macros are impure, and dropping
it would let a stale artifact be reused; adding one is always safe. Every
other flag wins outright.

**Booleans without a negative flag.** A flag wins only where one exists.
The file can set `:assertions false`, `:pseudo-names true`, `:warning-mode
true`, `:reload-dependents true` and `:server`, and no flag turns them back
(`--elide-asserts` has no opposite). See the open question on negative
flags.

**Exclusions are checked after merging**, with both origins in the
message: `--compile --build app --reloadable --optimize advanced` fails as
`--reloadable --optimize` does today, and `:reload-dependents` without
`:server` fails at its line. Two flags that exclude each other
(`--no-cache --cache-dir`) still do; a flag and a file value never
conflict, since the flag wins.

**Paths.** File paths resolve against the project directory and are
spelled, before they reach the compiler, as that directory joined with the
path, lexically normalized; absolute paths are kept. Flag paths are
relative to the current directory, as today.

## Cache keys and fingerprints

The project file does not enter any cache key, and it cannot weaken the
cache contract, because the cache never sees it. The file is translated
into the flags' structures before analysis starts; the keys
([compiler-cache.md](compiler-cache.md), "Conservative keys and
invalidation") are computed from those structures as for any flags-only
build:

| File setting | Where it enters | Key component |
|---|---|---|
| `:configuration` | `BuildConfiguration`, `MacroHost::set_build_configuration` | Config: the SHA-256 of the whole map's canonical text, in every namespace's front-end key and the `:config` broad view |
| `:assertions`, `:warning-mode`, `:checked-arrays`, the development mode's warning mode | `AnalysisOptions`, `MacroHost::set_assertions` | Options |
| `:macro-limits` `:expansions`, `:generated-nodes`, `:memory-bytes` | `MacroLimits`, `EvaluatorOptions` | Options (the logical limits and the memory limit) |
| `:macro-limits` `:timeout-milliseconds`, `:poll-budget` | `EvaluatorOptions` | None, as for the flags: they stop work, which is never stored |
| `:source-roots` | `FileSourceProvider` roots | Resolution: every candidate path and the file found |
| `:optimization`, `:defines`, `:pseudo-names`, output layout | `ApplicationOptions`, `OutputLayout` | Emission key only (recorded as zeros today; compiler-cache "As built (light-cljs-324)") |
| `:cache :blockers`, `:cache false` | `CacheOptions`, the policy evaluator | Policy: what may read or write, never a key |
| `:jobs` | `scheduler::Slots` | None: output does not depend on it |
| Build name, mode, file path, file text | nowhere | None |

Consequences, each of which keeps the contract as written:

- **A build from the file and the same flags hit the same artifacts**, when
  the roots are spelled the same. A build from the file spells its roots
  from the project directory, so it gets the same keys from any current
  directory; a flags-only build spells them as typed. A root spelled two
  ways gives a "resolution changed" miss, never a wrong hit.
- **Editing the file recompiles only what its new values change.** A
  comment, another build's section, `:server` or `:output-directory`
  changes no front-end key. A `:configuration` change misses every
  namespace with "config changed", as a new `--config` value does.
- **Build names do not partition the cache.** Two builds with equal
  front-end inputs share artifacts; development and release of one build
  differ in warning mode, so each namespace has two front-end keys. The
  store keeps three artifacts per namespace (compiler-cache, "Loading,
  diagnostics, and storage"), so a project alternating among more than
  three distinct front-end configurations evicts its own artifacts; see the
  open questions.
- **The compiler key already covers the reader.** The file's reader and
  translation live under `src/`, which `build.rs` hashes, so a change to
  how a key translates is another compiler.

The test that holds this: for every key, compile the build from the file
and then the equivalent flags with the same absolute roots and the same
cache directory; the outputs are byte-identical and the second run reports
every namespace a hit (`--stats`, `cache.*` lines).

## Errors

Every error is `PATH:LINE:COLUMN: message`, with the line and column from
the reader's spans (`LineIndex`), names the build and the mode where they
apply, and fails before anything compiles or writes. The whole file is
validated each time it is read; filesystem checks (a source root that is
not a directory) and whether a mode has an `:output-directory` apply to the
build and mode that run, since another build's roots may be generated later
and a build may have one mode only.

| Case | Message |
|---|---|
| Unknown key, close to a valid one | `light-cljs.edn:9:9: unknown key :output-dir in build :app; did you mean :output-directory?` |
| A shadow-cljs key | `light-cljs.edn:1:2: unknown top-level key :source-paths; shadow-cljs's :source-paths is :source-roots here` |
| `:dependencies`, `:deps` | `... light-cljs resolves no dependencies: put library sources in :source-roots, and let the host or a bundler supply JavaScript packages` |
| `:target` | `... builds have no :target: a release build writes ES modules into :output-directory and a development build writes the dev build (docs/project-file.md, "Schema")` |
| `:modules` | `... chunks are inferred from light.lazy/loadable sites (docs/code-splitting.md); there is no explicit module graph` |
| `:dev` | `... unknown key :dev in build :app; the mode is spelled :development` |
| `:compiler-options`, `:closure-defines` | the key it is here (`:optimization` under `:release`, `:defines`, `:assertions`) |
| A key in the wrong place | `light-cljs.edn:12:5: :server of build :app is a development setting; write it under :development` |
| `:entry` or `:output-directory` in `:defaults` | `... each build names its own :entry` / `... two builds would write one directory` |
| A bad value | `light-cljs.edn:4:37: :optimization of :defaults (release) must be :none, :prepare or :advanced, got :simple` |
| A bad configuration value | `--config`'s own message, at the value: `light-cljs.edn:8:40: :configuration :app/n of build :app: 1N is not a JavaScript number` |
| A missing `:entry` | `light-cljs.edn:15:3: build :worker has no :entry` |
| A mode without an output directory, when run | `--watch --build test: build :test (light-cljs.edn:18:3) gives no :output-directory for development; it runs only in release, with --compile` |
| One directory for both modes | `light-cljs.edn:6:3: build :app writes its development and release output to out/app; give each mode its own :output-directory` |
| Two builds, one directory | `... builds :app and :worker both write out/app in release` |
| An output directory equal to the cache directory, or to a source root | an error naming both |
| A tagged literal | `light-cljs.edn:2:9: #shadow/env: the project file is plain EDN; pass values that vary per run as flags (--config, --define, --serve)` |
| A duplicate key, a parse error | the reader's message, at its span |
| An unknown build | `--build ap: light-cljs.edn defines no build :ap; did you mean :app? Its builds are :app, :test, :worker` |
| `--build :app` | `--build takes the build's name without its colon: --build app` |

"Did you mean" uses the edit distance against the closed key set of the
map the key sits in, and the shadow-cljs table above is checked first. A
key is matched by name only: `:source_roots` and `:sourceRoots` get the
suggestion too.

## Several builds in one process

Deferred, with the CLI spelling reserved: `--watch --build app --build
worker`. Until it is built a second `--build` is the error above, and two
watches run as two processes, which works today: the cache is shared
safely between processes, and each watch has its own session, server and
macro engine.

Why it waits:

- **Little is shared.** Each build's watch keeps a `DevSession`, a macro
  engine and a retained program of its own; on Penpot one program is about
  1 GB. One process saves a terminal, one directory scan per poll and, with
  one server, one port. It saves no compilation and no memory.
- **The watch is one thread.** `dev_watch::run` polls, compiles and serves
  REPL requests between polls on one thread, and a V8 isolate is bound to
  the thread that made it. Two builds in one process need a thread per
  build to keep one build's step from delaying the other's; on one thread
  a save of a shared namespace recompiles in both, one after the other.
- **The server and REPL would need routing.** One port for two builds
  means a path prefix per build for units, events and the built-in page,
  and nREPL and `--repl --connect` sessions choosing a build as they choose
  a runtime today (`light.dev/select!`).
- **The same-origin case has an interim answer.** A web worker's script
  must come from the page's origin. A worker build whose
  `:output-directory` lies inside the app's is served by the app's server
  (the illustration above); it reloads only on a page reload until both run
  in one process.

When a project needs both builds live-reloaded from one origin, the design
is: one thread per build, owning its session and engine; one poller that
scans the union of the builds' roots once per interval, with roots
interned as dense `RootId`s and each root's builds as a `BuildId` list, and
sends each build the changed files under its roots; one server per
distinct `:server :port`, mounting each build at `/<name>/` when several
share a port; each nREPL session bound to one build, `:app` by default
when it is the first named. `.nrepl-port` names the first build's port.

## Watching the project file

Phase 2 adds the project file to the watch's poll. When its text changes,
the watch reads and validates it again and compares the selected build's
merged settings with the running ones:

- **Invalid**: the error goes to stderr and the overlay, and the build
  keeps running with its old settings.
- **Unchanged** (a comment, another build): nothing happens.
- **Changed**: the watch restarts the build in the same process, ending the
  session and starting from the cache as at startup; the server stays up
  when `:server` is unchanged, so connected pages reconnect to the new
  session, and is rebound otherwise. This is shadow-cljs's
  `config_watch` behavior. A watch that kept running on stale settings
  would show output the file no longer describes.

Until Phase 2, a watch started with `--build` names the file it read in its
first line, and a changed file is reported once per change: `light-cljs:
light-cljs.edn changed; restart the watch to apply it`.

## Migration

- **Every existing flag works without a file**, unchanged: an invocation
  without `--build`, `--project-file` or `--describe-build` never reads
  one, even when a `light-cljs.edn` is present, and its defaults (the cache
  under the current directory) are today's. The suites that run the binary
  pass unchanged.
- **Every existing flag works with a file**, as an override of its one
  setting (see "Precedence").
- **Moving a command into the file** is mechanical. The README's watch
  example

  ```sh
  light-cljs --watch --entry app.main --source-root examples/dev \
    --out-dir /tmp/light-dev --serve 9630 --nrepl 0
  ```

  becomes, in `examples/dev/light-cljs.edn`,

  ```clojure
  {:source-roots ["."]
   :builds {:dev-workflow {:entry app.main
                           :development {:output-directory "out/development"
                                         :server {:port 9630 :nrepl-port 0}}
                           :release {:output-directory "out/release"}}}}
  ```

  and `light-cljs --watch --build dev-workflow` from that directory.
- **From shadow-cljs.edn**, the table under "What carries over" maps each
  key; the errors name the light-cljs key for the common shadow-cljs ones.
  `bb` scripts that translate a shadow-cljs build (such as
  `research/penpot-light/port.bb`, which could write the port's
  `light-cljs.edn` from Penpot's `:main` build) follow AGENTS.md: babashka,
  not Python.

## Example: examples/features

`examples/features/light-cljs.edn`, the code-splitting application
(README, "Examples"), whose source root is its own directory:

```clojure
;; The code-splitting example's build (docs/project-file.md).
;; From this directory:
;;   light-cljs --compile --build features    release: chunks in out/release
;;   light-cljs --watch --build features      development: the dev build in
;;                                            out/development, served on 9630
{:source-roots ["."]
 :builds
 {:features
  {:entry app.main
   :development {:output-directory "out/development"
                 :server {:port 9630}}
   :release {:output-directory "out/release"
             :optimization :advanced}}}}
```

From the repository root the same build is
`light-cljs --compile --project-file examples/features/light-cljs.edn
--build features`, and `--optimize none` on that command compiles it at
`none` into the same directory. The release output is what
`--compile --optimize advanced --entry app.main --source-root
examples/features --out-dir examples/features/out/release` writes today,
byte for byte; `examples/features/out/` joins `.gitignore` with the file.
`index.html` stays where it is; as its own comment says, it is served
beside the chunks, now those in `out/release`.

## Implementation sketch

Names are decisions for review (see "Naming decisions"); the structure is
a sketch.

- **`src/project_file.rs`** (`light_cljs::project_file`):
  - `FILE_NAME: &str = "light-cljs.edn"`.
  - `ProjectFile::discover(start: &Path) -> Result<Option<PathBuf>, ...>`
    and `ProjectFile::read(path: &Path) -> Result<ProjectFile,
    ProjectFileError>`, which parses with `parser::parse`, validates the
    whole file and keeps each build as a `BuildDefinition` in a
    `Vec<BuildDefinition>` indexed by `BuildId(u32)`, ascending by name.
  - `ProjectFile::settings(&self, build: BuildId, mode: BuildMode) ->
    BuildSettings`, the merged settings of layers 1-6, each with its
    `SettingOrigin` (a span in the file, `:defaults`, or the built-in
    default). Settings are a closed enum, `Setting`, with dense
    discriminants; `BuildSettings` holds one slot per `Setting` in an array,
    and key lookup is a binary search over a static sorted table of key
    names, so validation, merging and `--describe-build` walk arrays, not
    maps.
  - `BuildMode::{Development, Release}`.
  - `ProjectFileError` carries the path, span, build and mode, and renders
    the messages above.
- **CLI.** `--build`, `--project-file` and `--describe-build` parse in
  `run_namespace` and `dev_watch::options` beside `--entry`. When given,
  the CLI reads the file, computes `BuildSettings`, translates them into
  `CompileFlags` or the watch's `Options` (layer 7 is then today's flag
  parsing, writing over them), and continues on today's path. One new
  library call, `BuildConfiguration::set`, replaces a key's value so that a
  flag can override the file; `insert` keeps refusing a duplicate.
- **Embedding.** `BuildSettings` is public, so an embedder that keeps its
  own builds in a `light-cljs.edn` gets the same validation; nothing in the
  compiler's API depends on it.

## Phases

### Phase 1: the file and release builds

- `src/project_file.rs`, the schema, every error above, `--build`,
  `--project-file` and `--describe-build` for `--compile` (release, and
  development with `--reloadable`).
- `examples/features/light-cljs.edn` and its `.gitignore` entry; README
  "Command-line usage" and the documentation list; this document's "As
  built" notes; dev-workflow.md and code-splitting.md point here.
- Tests in `tests/cli.rs` (or a `tests/project_file.rs` binary if the
  table grows): a table of bad files and their exact messages, one row per
  case and per key; the precedence layers, each overriding the one below;
  the byte-identical and all-hits comparison for every key ("Cache keys and
  fingerprints"); and `--build` from a subdirectory finding the file and
  hitting the cache populated from the project directory.

Exit criterion: `light-cljs --compile --build features` in
examples/features writes the same files as the equivalent flags, and the
second of the two runs is all hits.

### Phase 2: the watch

- `--watch --build NAME`, with the development keys.
- The project file in the watch's poll, with the restart described under
  "Watching the project file", tested in `tests/dev_watch.rs`: an edit of
  another build does nothing, an invalid edit keeps the build, a changed
  `:configuration` restarts it and the next step sees the new value.

Exit criterion: `light-cljs --watch --build features` serves the example
and a `:configuration` edit reaches the page without restarting the
process.

### Phase 3: deferred

- Several builds in one watch process, as designed above, when a project
  needs two live builds from one origin.
- `:chunks`, with code-splitting.md Phase 4.

## Naming decisions

For the user's review. Each is spelled out in full; none is a placeholder.

| Thing | Name | Rejected, and why |
|---|---|---|
| The file | `light-cljs.edn` | `light.edn` (the user's objection: not the tool's name) |
| The concept | the **project file**; its directory the **project directory** | "build configuration file": `BuildConfiguration` already names the `--config` map |
| This document | `docs/project-file.md` | `docs/build-config.md`: an abbreviation, and the same collision |
| Selecting a build | `--build NAME` | `--build-id`; a positional build name (no subcommands in this CLI) |
| Naming the file | `--project-file PATH` | `--config-file` (collides with `--config`), `--manifest-path` (Cargo's word) |
| Showing merged settings | `--describe-build` | `--print-config`, `--show` |
| A build's identifier | **build name**, a keyword (`:app`); internally `BuildId(u32)` | "build id" for the keyword, since the dense integer is the ID |
| Modes | `:development`, `:release`; `BuildMode::{Development, Release}` | `:dev` (abbreviation), `:production` (the README calls advanced "the release mode") |
| Top-level keys | `:source-roots`, `:cache`, `:defaults`, `:builds` | `:source-paths` (shadow), `:paths` (deps.edn): the CLI says `--source-root` |
| Cache keys | `:cache {:directory ... :blockers [...]}`, `:cache false` | `:cache-dir`, `:cache-root` |
| Shared build keys | `:entry`, `:source-roots`, `:output-directory`, `:configuration`, `:assertions`, `:macro-limits` | `:main`, `:init-fn`, `:output-dir`, `:config`, `:elide-asserts` |
| Macro limits | `:expansions`, `:generated-nodes`, `:timeout-milliseconds`, `:memory-bytes`, `:poll-budget` | `:nodes`, `:timeout-ms`, `:memory-limit` (unit unstated) |
| Release keys | `:optimization`, `:defines`, `:pseudo-names`, `:warning-mode`, `:checked-arrays`, `:jobs` | `:optimizations` (upstream's plural for one level), `:optimize`, `:closure-defines` (no Closure here), `:warnings` (reads as "show warnings") |
| Development keys | `:poll-interval-milliseconds`, `:server {:port :host :nrepl-port}`, `:reload-dependents` | `:poll-ms`, `:devtools`, `:http-port`, `:reload-strategy :full` |
| Reserved | `:chunks {:groups :merge-below-bytes :host-entries}` | `:modules` (an explicit graph this compiler does not have), `:minimum-bytes` |
| Rust | `light_cljs::project_file`, `FILE_NAME`, `ProjectFile`, `BuildDefinition`, `BuildSettings`, `BuildMode`, `BuildId`, `Setting`, `SettingOrigin`, `ProjectFileError`, `BuildConfiguration::set` | `Config`, `BuildConfig` |
| Units in key names | spelled in full as a suffix: `-milliseconds`, `-bytes` | `-ms`, bare numbers |

## Open questions

Each has a recommendation; none blocks Phase 1 except the first three,
whose recommendations Phase 1 would build.

- **The mode follows existing flags.** `--compile` is release,
  `--reloadable` and `--watch` development, with no `--release` or
  `--development` flag. *Recommendation:* as designed; a mode flag could
  only restate or contradict `--reloadable`.
- **Discovery walks up** to the file system root, stopping at nothing.
  *Recommendation:* as designed (shadow-cljs and Cargo do the same), with
  the file named in every status line and error. The alternative, the
  current directory only, breaks running from `src/`.
- **The file is read only with `--build`.** A `light-cljs.edn` in the
  current directory does not change `--compile --entry ...` or `--repl`.
  *Recommendation:* as designed, so that today's commands, tests and
  scripts cannot change meaning when a file appears.
- **First-class spellings of today's abbreviated flags.** `--out-dir`,
  `--cache-dir`, `--poll-ms`, `--macro-timeout-ms`, `--macro-nodes`,
  `--macro-memory-limit`, `--esm` and `--elide-asserts` abbreviate or
  differ from their keys. *Recommendation:* in a follow-up change, add
  `--output-directory`, `--cache-directory`,
  `--poll-interval-milliseconds`, `--macro-timeout-milliseconds`,
  `--macro-generated-nodes` and `--macro-memory-bytes` as the documented
  spellings, and keep the old ones working permanently as aliases; leave
  `--esm` (an established format name) and `--elide-asserts` (upstream's
  name) as they are. The keys do not wait for it.
- **`light.build/config`** abbreviates "configuration", while the file's
  key is `:configuration`. It was settled on 2026-10-17 and is in use.
  *Recommendation:* keep `b/config`; renaming a shipped compile-time API
  for spelling would break every macro that reads it.
- **Several builds in one watch.** *Recommendation:* defer, as above, until
  a project needs two live builds from one origin; the interim is one
  process per build with nested output directories.
- **Environment values in the file** (`#shadow/env`). *Recommendation:*
  no. Values that vary per run go on the command line (`--config`,
  `--define`, `--serve`), where the command shows them; Penpot's uses of
  `#shadow/env` (ports, cache directory, optimization, a version define)
  are each one flag.
- **Negative flags** for the booleans the file can set and no flag can
  unset (`:assertions false`, `:pseudo-names`, `:warning-mode`,
  `:reload-dependents`, `:server`). *Recommendation:* none until asked; a
  second build or the other mode covers the cases seen so far.
- **`--repl --build NAME` and `--eval --build NAME`**, taking the build's
  source roots, `:configuration` and `:assertions` in development mode.
  *Recommendation:* yes, after Phase 2; the REPL needs a `--config`
  equivalent first, since `EvalOptions::configuration` exists but no flag
  sets it.
- **Cache retention across many builds.** Three artifacts per namespace
  hold development and release of one build plus one more configuration.
  *Recommendation:* measure on a project with three or more builds before
  adding a setting; if they evict each other, make the retention count a
  `:cache` key (and a flag) rather than per-build directories, which would
  lose the sharing.
- **`:target`.** Absent: one output shape serves browsers, Node and
  bundlers. *Recommendation:* add it only with a second shape, such as the
  script-format chunks of code-splitting.md "Phase 5: deferred"; until
  then a `:target` key is an error that explains why.
