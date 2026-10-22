# A shadow-cljs-hosted porter to light-cljs: design and prototype

**Abandoned, 2026-10-21.** The user stopped this project, judging it a
poor fit for what they are trying to do. The note is
kept as a record and is not being worked on. The prototype, the feasibility
note and the ported trees stay in the gitignored `research/` directory.

Research note, 2026-10-20, revised 2026-10-21 with the user's decisions.
Design and prototype only. Nothing in the compiler or the pellets was
changed, nothing was pushed, and no workflow was triggered. `research/penpot`
and `research/penpot-light` hold no edits: the only files the runs wrote
there were toolchain caches, deleted afterwards (§1).

The user decided on 2026-10-20, in chat, that the porter is a Clojure
program hosted by shadow-cljs that works with any shadow-cljs project. It is
not a babashka script with a JVM helper. The macros must run in the
shadow-cljs JVM anyway, so the whole tool runs there and uses shadow's own
compiler state. This note designs that tool. It keeps the findings of
[shadow-port-feasibility.md](../../research/shadow-port-feasibility.md): the ablation of the
hand port's edits, the CSS-prefix divergence, the construct table and the
compiler-fix list. It replaces that note's architecture (bb script plus
`expand.clj`).

The port is a one-way migration. The porter reads the shadow-cljs project
once and writes a light-cljs project, which from then on is the project.
Nothing is kept in sync with the original, and there is no watch mode.
`goog` is not supported: the port contains no `goog` namespace, `goog`
require or Closure Library code.

The prototype's results below predate the one-way and no-goog rules. It
shipped compat shims named `goog.*` and kept `goog` requires. Under the
no-goog rule its todomvc port would fail on `goog.async.nextTick`, and its
Penpot port on `goog.async.Deferred`, `goog.Promise` and a few others, until
those have replacements ("Remaining manual steps"). Its `:keep` count also
predates the host-equality rule (§3).

The prototype is one namespace, `light-cljs.port`, in
`research/shadow-port-tool/`. Run through `npx shadow-cljs run`, it ports
`research/penpot` into a fresh directory, and light-cljs compiles all four
Penpot page entries from it: `app.main`, `app.worker`, `app.render` and
`app.rasterizer`. It also ports re-frame's public `todomvc` example, which
light-cljs then compiles.

## Summary

| | Penpot (`:main` + `:worker`) | re-frame todomvc (`:client`) |
|---|---|---|
| Invocation | `npx shadow-cljs -A:light-cljs/port run light-cljs.port/main :main :worker ../../penpot-port --compile` | same, `:client ../../../todomvc-port` |
| Project setup | deps.edn (`:deps {:aliases [:dev]}`), shadow 3.5.3 | deps.edn (`:deps true`), shadow 3.2.0 |
| Resources in shadow's closure / copied | 1,547 / 924 | 122 / 38 |
| Macros kept / expanded on the JVM / ported | 91 / 15 / 18 | 13 / 1 / 7 |
| Expansions spliced | 6,028, 0 span mismatches | 2 |
| Edited `.cljs`/`.cljc` files with unchanged line counts | 897 of 897 | all |
| light-cljs compile (`--no-cache`) | `app.main` 22–27 s, `app.worker` 3.9 s, `app.render` 4.4 s, `app.rasterizer` 1.2 s: all OK | `todomvc.core` 0.3 s, OK |
| Whole run, cold JVM | 59 s wall, 3.8 GB peak (two shadow compiles 16 s, port 8 s, two light-cljs compiles 28 s) | 19 s |
| Curated catalogue entries used | 7 libraries + 1 project overlay | compat only |

Most of penpot-light's hand port becomes generic. The ablation switched off
42 edits, one per row of `research/shadow-port-prototype/ablation.tsv` (the
feasibility note says 41). Of these, 5 are no longer needed, 24 are now done
by the tool for any project, 1 is split between generic code and the Penpot
overlay, and 12 remain curated: 9 library ports and 3 Penpot compiler-gap
patches (§9). The 204 lines of hand-written `app.*` macro shims and
the generated cursor and font data namespaces are gone. JVM expansion
replaces them, and it is faithful by construction.

## 1. Packaging and invocation

### Where the source lives

- **Now:** `research/shadow-port-tool/`. It is a deps.edn library:
  - `deps.edn` with `:paths ["src" "resources"]` and one dependency,
    `rewrite-clj/rewrite-clj 1.2.50`;
  - `src/light_cljs/port.clj`, the tool;
  - `resources/light_cljs/port/catalogue/`, the curated data;
  - `remap-source-map.bb`;
  - `clj-config/deps.edn`, which stands in for a user alias.
- **Decided:** `tools/port/` in the light-cljs repository, with the same
  layout, usable as a git dependency with `:deps/root "tools/port"`. A Maven
  artifact on Clojars comes later; publishing it is the user's step.
- shadow-cljs is *provided*: the tool runs on the project's own shadow-cljs
  version. The prototype was exercised on 3.5.3 (Penpot) and 3.2.0
  (todomvc).

### How a project runs it

`run` is shadow's public CLI command (`cli_actual.clj` `do-clj-run`, args mode
`:eat-all`). Every argument after the function is passed through, so the
porter's own flags (`--compile`, ...) reach `main`.
Getting the tool onto the classpath depends on how the project declares its
dependencies:

| Project uses | Command | Status |
|---|---|---|
| any (`:dependencies`, `:deps`, `:lein`), after a Clojars release | `npx shadow-cljs -d <group>/light-cljs-port:<version> run light-cljs.port/main :app ../app-light` | From reading the source, not run. shadow's `-d` adds a Maven dependency in all three modes: it joins `:dependencies` for its own resolver, becomes `-Sdeps {:aliases {:shadow-cljs-inject {:extra-deps ...}}}` for deps.edn, and becomes `update-in :dependencies conj` for lein (`npm/cli.cljs` `get-clojure-args`, line 327) |
| deps.edn, before any release | a user-level alias in `~/.clojure/deps.edn`, `:light-cljs/port {:extra-deps {light-cljs/port {:git/url ... :git/sha ... :deps/root "tools/port"}}}`, then `npx shadow-cljs -A:light-cljs/port run light-cljs.port/main :app ../app-light` | Tested with `:local/root`. The prototype points `CLJ_CONFIG` at `clj-config/` so that it never edits the user's file |
| deps.edn, without npx | `clojure -M:dev:light-cljs/port -m shadow.cljs.devtools.cli run light-cljs.port/main :main ../../penpot-port` | The same JVM (Penpot's own `clojure -M:dev:shadow-cljs` style) |
| lein, before any release | a profile in `~/.lein/profiles.clj` with the git checkout as `:source-paths`, plus `:lein {:profiles "+light-cljs-port"}` | Needs an edit to shadow-cljs.edn; `-d` after a release is better |

Arguments: `light-cljs.port/main :build [:build ...] OUT [--compile]
[--light-cljs BIN] [--no-cache] [--entry NS] [--catalogue DIR]
[--allow-version-drift]`. The `:keep` comparison (§3, step 8) always runs and
has no flag; the prototype ran it only with `--verify`. Per-project macro
overrides live in `light-cljs-port.edn` in the project directory:
`{:macros {ns/macro :expand|:keep|:port}}`.

The run must leave the project as it found it. Without `:shadow/requires-server` metadata,
`run` uses `api/with-runtime` and starts no server, so it writes no
`.shadow-cljs/nrepl.port` (verified). The porter sends shadow's per-build
cache to `OUT/.shadow-port/<build>` (`build-api/with-cache-dir`). It runs
light-cljs with OUT as the working directory, so `.light-cljs/cache` lands in
the port. Two writes in the project remain, both made by the toolchain rather
than the porter:

- shadow's runtime adds one `.shadow-cljs/jar-manifest/` entry, for the
  rewrite-clj jar;
- the Clojure CLI adds `.cpcache` files for the new alias combination.

Both were deleted after the runs. Early runs, before the cache redirect, also
rewrote `.shadow-cljs/builds/main/dev/closure-js`. That directory is a
regenerable shadow cache and was deleted.

## 2. Architecture

```
npx shadow-cljs run light-cljs.port/main :main :worker OUT        (one JVM: shadow's)
 1. install a hook on cljs.analyzer/macroexpand-1*
      every non-core expansion -> {macro Var, &form, &env ns, file, positions,
      the RT/nextID range it ran in}; form -> expansion in an IdentityHashMap
 2. per build: api/get-build-config -> util/new-build -> build/configure -> build/compile
      (:cache-level :off so every macro runs; devtools off; nothing flushed)
 3. closure per build from the module :entries, over shadow's :immediate-deps,
      stopping at namespaces light-cljs or the compat layer supply; builds joined
      by resource name (a file belongs to the project, not to a build)
 4. origins: :pom-info / ~/.m2 / ~/.gitlibs path -> {lib version}; catalogue
      entries matched by coordinate and exact version (mismatch: fail)
 5. classify each macro the closure ran (§3): :port | :keep candidate | :expand;
      compare every call site of every candidate in light-cljs and the JVM,
      any difference -> :expand; promote to a fixpoint (an :expand call with no
      position inside a kept macro's output expands the parent too)
 6. rewrite (§4), staged in memory, written only where the file changes:
      splice :expand expansions | unwrap the #?(:clj ..)/(deftime ..) definitions
      kept macros need | .clj macro files -> .cljc or merged into the .cljs |
      ns forms | (new X.) | Closure Library uses -> non-goog code | npm symbol requires ->
      strings | ./x.js -> port paths | missing cljs.core names -> compat |
      Closure JS -> ESM | JS reading cljs.core -> bridge | catalogue patches
 7. shadow-cljs.edn -> light-cljs.edn with a disposition for every key (§6)
 8. optional: --compile (light-cljs on each build's entry)
 9. port-report.edn (classes and reasons, splices, gaps, dispositions,
      verification), port-map.edn (port file -> original URL, origin, SHA-256,
      edits, lines-preserved)
```

```
OUT/
  src/       files from the project's own classpath directories (:local/root included)
  vendor/    files from jars and git libraries; catalogue replacements and :add files
  compat/    the compat layer: Closure Library replacements (no goog.* names), shadow.* APIs, stubs, light.compat
  light-cljs.edn  port-report.edn  port-map.edn
```

### shadow-cljs API the tool relies on

| Namespace / function | Use | Stability |
|---|---|---|
| `shadow.cljs.devtools.api/get-build-config`, `with-runtime` | build config, runtime | public, documented |
| CLI `run` (`cli-actual/do-clj-run`) | entry | public command |
| `shadow.cljs.devtools.server.util/new-build`; `shadow.build/configure`, `compile` | the same pipeline as `api/compile*`, without `flush` | internal; unchanged in shape since shadow 2.x (it is what `compile*` and `release*` call) |
| `shadow.build.api/with-cache-dir` | cache redirect | internal, long-stable |
| `shadow.build.data/get-source-by-id`, `get-source-code` | resources and their text | internal |
| Build state keys: `:build-sources`, `:sources`, `:immediate-deps`, `:sym->id`, `:name->id`, `:str->sym`, `:ns-aliases`, `:compiler-env` (`cljs.analyzer/namespaces` `:defs`) | closure, resolved ns data, string requires shadow resolved (`"./x.js"` → resource), npm symbol requires (`react` → `module$node_modules$react$index` with `:package-name`), each namespace's cljs Vars | internal data; stable in practice |
| Resource keys: `:resource-name :url :file :type (:cljs :goog :js :shadow-js) :ns :provides :deps :ns-info :macro-requires :pom-info :from-jar :fs-root :package-name :virtual` | everything per file | internal data |
| `cljs.analyzer/macroexpand-1*`, `get-expander`, `*cljs-file*` (alter-var-root hook) | the real macro Var, `&env`, file | ClojureScript compiler internals; unchanged for years. One hook, installed once and idempotently |

- **Mitigation for the internal APIs:** resolve every Var above at start-up
  and fail with the shadow version on a mismatch (not in the prototype).
- **CI:** a matrix of shadow 3.2 and 3.5, the versions the prototype ran on.
- **What shadow already resolved, and the porter reuses rather than
  re-parses:**
  - which file provides each namespace, and from which jar version;
  - Closure JS (`:goog`), ES modules (`:js`) and npm packages;
  - `./x.js` resolution;
  - the cljs Vars of every namespace;
  - the Clojure macro namespaces and their Var metadata (`:file :line
    :column`), which locates every macro and helper definition.

## 3. Classifying macros

For each macro Var the compile ran, from files the port copies:

1. **Core** (`cljs.core`, `clojure.core`): never recorded or touched.
2. **`:port`**: its namespace is in the compat layer, in a catalogue
   `:ports-nss`, or the macro is in a catalogue `:ported-macros`. The curated
   definition is used and the call sites stay source.
3. **Project override** from `light-cljs-port.edn`.
4. **`:expand`**, when a `.clj` macro shares its name with a function of the
   same namespace. light-cljs refuses that pair; the JVM expansion is
   faithful. Penpot: beicon `push!`, `end!`, `error!`.
5. **Purity and portability analysis** against light-cljs's capability list.
   A macro that is impure, or that uses behaviour that differs between the
   JVM and JavaScript, is `:expand`. Any other macro is only a `:keep`
   *candidate*. Step 8 decides. The analysis reads the definition from the
   classpath at the Var's own `:line`/`:column` (`:clj` features, syntax quote
   resolved in the macro's namespace). It walks what runs at expansion time:
   everything outside `quote`, minus bound locals. Each symbol resolves with
   `ns-resolve` in the macro's real Clojure namespace:
   - a JVM class or an interop form (`.m`, `C.`, `new`) is impure;
   - `clojure.core` I/O and reflection Vars are impure: `slurp`, `resolve`,
     `ns-interns`, `eval`, `class`, ...;
   - Vars in `cljs.analyzer*`, `cljs.env`, `clojure.java.*`, `clojure.data.json`
     or `shadow.*` are impure;
   - `&env` reads outside light-cljs's keys are impure, as are `:defs` and
     `(meta *ns*)`. light-cljs gives macros `:locals`, `:ns` (`:name
     :requires :require-macros :uses :use-macros :imports :doc`),
     `:context`, `:line` and `:column` (`src/macro_host.rs`). `*ns*` itself
     works (probed: `(str *ns*)` is the caller).
   - A helper Var whose namespace has a ClojureScript definition of the same
     name is pure: light-cljs calls that one, not the JVM one. Other helpers
     are analyzed recursively and become `:needs`, which tells the rewriter
     which `#?(:clj ...)` definitions to unwrap.
   - The JVM branch of the `(if (:ns &env) cljs jvm)` idiom (also `if-not`,
     `when`, `when-not`) is skipped. Its same-namespace Vars get a
     `(declare ...)` so the kept macro compiles. This replaces penpot-light's
     hand `record_declare` patch.
   - Host-dependent behaviour makes a macro `:expand`, because light-cljs
     runs macros as ClojureScript on a JavaScript engine:
     - numbers: `/` (ratio against double), `bit-*`, `int`/`long`
       truncation, `unchecked-*`, and `str` of floats;
     - regexes: `re-pattern`, `re-find`, `re-matches`, `re-seq`, and
       `clojure.string/replace` or `split` with a regex (Java regex against
       JavaScript `RegExp`: flags, possessive quantifiers, replacement
       syntax);
     - hashing and order: `hash`, and iteration over a map or set (`for`,
       `doseq`, `map`, `keys`, `vals`, `seq`, `reduce-kv`, ...). Hash
       values differ, so the order of a map with more than 8 entries, or of
       any set, differs;
     - characters: character literals, `char`, `char?`, `int` of a char
       (Character against a one-character string);
     - printing: `pr-str`, `print-str` and `str` of anything but strings,
       keywords, symbols and integers;
     - nondeterminism: `rand*`, `random-uuid` and time. Gensyms are
       renumbered (§4).

     The list is a filter, not a proof: a macro outside it can still
     differ, which step 8 catches.
6. **Promotion to a fixpoint.** An `:expand` call with no source position
   sits in another macro's output. The recorder maps each recorded form to the
   expansion containing it, by identity. If that parent is `:keep`, light-cljs
   would have to run the impure call itself, so the parent becomes `:expand`.
   Calls under a `:port` parent are ignored: the port emits its own code.
   In Penpot, all 1,089 calls with no position (potok's `deftype` and
   `leaky-exists?`) sit inside the ported `reify`, so they are ignored and
   no macro is promoted.
7. **An `:expand` site is spliced only if its printed expansion is
   light-safe.** Unsafe output is `deftype*`/`defrecord*` or `js*` text naming
   `cljs.core.`/`goog.`. Values with no source form (functions, records,
   objects) also make a site unsafe. Unsafe sites are reported per macro as
   needing a port. Penpot has none.
8. **Equality on both hosts decides `:keep`.** Every recorded call site of
   every `:keep` candidate, not a sample, is expanded with light-cljs's macro
   engine and compared with the JVM's expansion (§7, check 1). A candidate
   is `:keep` only if every site is equal. One difference makes it
   `:expand`, and step 6 runs again. A candidate the comparison cannot run
   is `:expand`. This step is part of every port, not an option.

The rule is: `:keep` only when the macro is pure *and* its expansions are
equal on both hosts. The static analysis is deliberately cautious: any
interop, and any construct in the list above, expands. A macro that
light-cljs could run (`(.toUpperCase s)` works in JavaScript too) is still
expanded. That is faithful, but the migrated source then holds the
expansion where the macro call was. Two trials back up the analysis, and
a capability list keeps it current:

- **Expansion trial** (step 8). It is the arbiter for every candidate. It
  needs `--eval` to accept the `--macro-*` limits (fix 7). Until then,
  `--eval`'s fixed limits block it for half of Penpot's macro namespaces
  (§7), and those candidates fall back to `:expand`. In the prototype,
  `--verify` samples and reports, and it demotes nothing.
- **Compile trial.** `--compile` compiles the port. A light-cljs error at a
  kept macro's definition or call site names the macro to demote. Demotion is
  automatic in the design; in the prototype it is manual, through
  `light-cljs-port.edn`.
- **Capability list.** The `&env` key list and the `*ns*` facts are
  hard-coded from `src/macro_host.rs` in the prototype. light-cljs should
  print them (`--describe-macro-env`, say), so the porter tracks the
  compiler.

Penpot's result is what the feasibility note picked by hand, now derived.
The prototype classified with the purity checks only. Its `:keep` column
predates the host-dependence checks and step 8, so some of those macros may
move to `:expand`:

| Class | Macros |
|---|---|
| `:expand` (15) | `stl/css` 5,131 sites, `stl/css-case` 519 (`(meta *ns*)`, `.getName`); the four `collect-*` (`:defs`); `cursor-ref` 32, `cursor-fn` 12, `preload-gfonts` (`io/resource`, `slurp`); `dm/export` 284 (`cljs.analyzer.api/resolve`); beicon `push!`/`end!`/`error!` (name conflict); potok `deftype`/`leaky-exists?` (only inside ported `reify`) |
| `:keep` (91: 75 `.cljc`, 16 `.clj`) | all of `app.common.data.macros` but `export`, logging (`*ns*` works), `record/defrecord`, `stl/css*` (a separate macro from `stl/css`), `icon-xref`, `perf/with-measure`, `render-wasm.enums/serializers`, `app.util.modules/load`, `load-fn`, cuerdas `ffmt`/`concat`, promesa, garden, instaparse (`gll` diagnostics, `abnf/precompile-cljs-grammar`, `instaparse.macros`), tools.reader `log-source`, malli |
| `:port` (18) | rumext `defc`/`html`/`deps`/`with-memo`/... (catalogue), potok `reify` (catalogue), `cljs.spec.alpha/*`, `clojure.test.check.generators/let` (compat stubs) |

Todomvc: `secretary/defroute` expands (an `IllegalArgumentException.` branch).
re-frame's `trace` macros, reagent's `debug` and `reaction`, and macrovich's
`deftime` are kept.

## 4. Editing: rewrite-clj on the JVM, every line kept

All edits are span edits. rewrite-clj (`{:track-position? true}`) or the
reader positions recorded by shadow locate each span. A replacement must not
add lines: text that is shorter in lines is padded with newlines, and blanked
text becomes spaces, so columns stay too. Edits are applied once per file, last
first. An overlap is dropped and reported. A file's outputs are staged in
memory, merges included, and written once, only if the bytes change.

| Edit | How |
|---|---|
| JVM expansion splice | The call's span becomes the one-line printed expansion plus the newlines the call spanned. Printing keeps only `:tag :const :export :private :dynamic` metadata, stripped recursively (fixes the `^^{:file ..} boolean` bug found in the feasibility M4). `#js` is printed for `JSValue`. Gensyms are renumbered (see below). |
| ns form, in place | `:require-macros`/`:use-macros` and extra `:require` clauses are blanked, including inside `#?(:cljs ...)` and `#?@(:cljs [...])`. Their libspecs join the primary `:require`: merged into an existing libspec of the same namespace and alias (`:refer` union), otherwise appended on its last line (wrapped `#?(:cljs ...)` in `.cljc`). Inside libspecs: `:refer-macros`/`:include-macros` are folded, self-requires dropped, refers of expanded macros dropped, and macro-only namespaces left with nothing are dropped. Requires the splices need (qualified namespaces in the expansions, except under `quote` and `light.lazy/loadable`) are added. `light.compat` and `light.core-bridge` are added when used. |
| Closure Library uses | Every `goog.*` require, `(:import [goog.x Y])`, `goog/fn` and `js/goog.*` reference is rewritten to a compat replacement whose namespace is not under `goog`, or to plain JavaScript interop where that is exact (`goog/global` → `js/globalThis`). A use with no replacement fails the port and is listed in `port-report.edn :missing-goog`. The prototype instead shipped shims named `goog.*` and kept the requires; that is replaced. |
| npm symbol requires | `[react :as r]`, which shadow resolved through `:ns-aliases` to a `:package-name` resource, becomes `["react" :as r]`. |
| `./x.js` requires | Re-pathed to the port's copy, using shadow's `:str->sym` resolution. The path is absolute, so the port is machine-specific until compiler fix 5. |
| `#?(:clj DEF)` unwrap | For each definition a kept macro needs, from the Var's `:line`/`:column`: `#?(:clj ` and the closing paren become spaces. The same is done for macrovich `(deftime ...)` (needed for re-frame). |
| `.clj` macro namespaces | With a same-named `.cljs`: the needed definitions, one line each with comments dropped, are inserted just before the first top-level form that calls them, and their requires are merged. Without one: copied as `.cljc`, with every unneeded top-level form and libspec wrapped in `#?(:clj ...)`, in place. |
| `(new X.)` | the trailing dot becomes a space |
| missing `cljs.core` names | Catalogue `:core-gaps` (`pr-writer` → `light.compat/pr-writer`) renames every code token not defined by the namespace itself. This replaced the tools.reader patch and fixed reagent with no reagent entry. |
| Closure JS, JS reading `cljs.core` | port.bb's conversions, driven by shadow's `:goog` resources instead of a hand list: 21 files in Penpot, 16 of Penpot's own plus promesa and transit-js internals. `goog.provide`/`goog.require`/`goog.scope` become ES module exports and imports, and any other `goog.*` call in the JS is rewritten or fails the port. The `light.core-bridge` namespace is generated from the `cljs.core.X` names the JS reads. |
| catalogue patches | `:replace` text is padded to the original's lines; `:insert-after-ns` and `:replace-form` snippets are put on one line. |

**Gensyms and determinism.** The hook takes `clojure.lang.RT/nextID` before
and after each expansion, realizing lazy output inside that window. Any symbol
whose numeric suffix falls in the window was generated by that expansion and
is renamed `base__L<line>_<k>`. `x__N__auto__` and the reader's `#()`
parameters (`p1__N#` → `%1`) are handled too.

Wrapping `gensym`'s Var did not work, for two reasons found because reruns
rewrote files. Gensyms made lazily inside `(for ...)` ran after the expansion
returned. Even with the output realized inside the hook, calls from shadow's
macro namespaces did not go through the Var. Now two runs on the same input
write the same bytes.

**Source maps.** After the migration the port's files are the sources, so
light-cljs's maps name them directly and need no remapping. Keeping every
line's number still pays off during the migration: a light-cljs error or map
position in the port points at the same line in the original, which makes the
checks against the shadow build (§7) easy to read. The prototype's
`remap-source-map.bb`, which renamed a map's `"sources"` back to the
originals, is not part of the design. It did measure line preservation: on
todomvc, of 1,961 mapped lines into re-frame and todomvc, 1,958 have
identical text in the original and the port, and the other 3 are on edited
lines, which `port-map.edn` lists per file (`:spliced`). Chunked output
(`--out-dir`) still has no source maps.

## 5. One run

The porter runs once per migration. The whole Penpot port, cold, takes 59 s
including the JVM; the port step itself is about 8 s for 924 files. Output is
deterministic, so running it again on the same input writes the same tree.
After the migration the project is developed in light-cljs, and the shadow-cljs
original is no longer an input.

## 6. shadow-cljs.edn to light-cljs.edn

The raw file is read with `#shadow/env` (and `#env`) kept as
`#light-cljs/env` tags, so run-time overrides survive. Every key gets a
disposition in `port-report.edn :config`: kept, translated, dropped with a
reason, needs a feature, or not ported. The names and rules follow
[docs/build-configuration.md](../build-configuration.md). Keys for the
dev server, REPL and reloading (`:http`, `:nrepl`, `:fs-watch`, `:devtools`)
configure light-cljs's own watch for the migrated project; the porter itself
has no watch.

| shadow key | Disposition |
|---|---|
| `:deps`, `:dependencies`, `:lein`, `:repositories`, `:jvm-opts` | translated: the porter vendored the reached sources; `:source-paths ["src" "vendor" "compat"]` |
| `:source-paths` | translated (ported into `src/`) |
| `:http {:port :host}` | kept (the light-cljs watch's dev server) |
| `:nrepl {:port}` | translated; `:host`, `:middleware` and `:init-ns` dropped (sessions are ClojureScript) |
| `:dev-http` | needs feature (a static root at `/`) |
| `:cache-dir` | translated to `:cache-root`, default `.light-cljs` (formats differ) |
| `:fs-watch` | `:loop-wait` kept |
| `:build-defaults` | merged into each build first |
| build `:target :esm` | kept; `:browser`, `:node-script`, ... translated to `:esm` and reported as needs feature |
| build `:output-dir` | translated to `out/<build>/` inside the port, a new folder. Nothing is written into the original project, and there is no mode for that |
| build `:asset-path` | dropped (relative chunk URLs) |
| build `:modules` | One module with one entry: the module named `:main`, or the module nothing depends on, with its first entry or its `:init-fn`'s namespace. Several entries or modules, `:exports`, `:init-fn`, `:prepend-js`/`:prepend`/`:append-js` and `:web-worker` are needs feature (code-splitting Phase 4, named exports, classic worker output). |
| build `:devtools` | `:reload-strategy` kept; `:watch-dir` needs feature; `:before-load`/`:after-load` dropped (use metadata); the rest dropped |
| build `:build-options` | `:cache-level`, `:cache-blockers` kept; `:manifest-name` dropped; `:ns-aliases` needs feature |
| build `:js-options` | dropped: light-cljs resolves no packages, the bundler does |
| `:compiler-options :optimizations` | translated (`:simple`/`:whitespace` → `:prepare`), also inside an env tag's `:default` |
| `:pseudo-names`, `:elide-asserts`, `:checked-arrays` | kept |
| `:closure-defines` | kept for release. In `:dev` dropped and reported: light-cljs refuses dev defines (a permanent deviation; todomvc hit it). Every `goog.*` define dropped (goog is not supported) |
| `:source-map` | needs feature (chunked output has no maps) |
| `:output-feature-set`, `:output-wrapper`, `:rename-prefix-namespace`, `:fn-invoke-direct`, `:anon-fn-naming-policy`, `:cross-chunk-method-motion`, `:source-map-detail-level`, `:warnings`, `:externs`, `:infer-externs` | dropped, each with its reason |
| build `:build-hooks` | dropped (no user code outside macros) |
| `:output-to`, `:main`, `:runtime`, `:ns-regexp`, `:test-dir` | needs feature (no such target) |
| anything else | needs feature |

The porter adds the five `:macro-*` limits to every build, sized for Penpot.

Penpot's file (`research/penpot/frontend/shadow-cljs.edn`, builds `:main` and
`:worker`) converts to this. The prototype wrote it with `:output-dir` rebased onto the original project; the design writes `out/<build>/`:

```clojure
{:source-paths ["src" "vendor" "compat"],
 :http {:port #light-cljs/env ["HTTP_PORT" :as :int :default 3448]},
 :nrepl {:port #light-cljs/env ["NREPL_PORT" :as :int :default 3447]},
 :cache-root #light-cljs/env ["CACHE" :default ".light-cljs"],
 :builds
 {:main
  {:target :esm,
   :output-dir "out/main/",
   :modules {:main {:entries [app.main]}},
   :devtools {:reload-strategy :full},
   :release
   {:closure-defines {app.config/compiled-version-tag #light-cljs/env ["VERSION_TAG" :default "develop"]},
    :compiler-options
    {:optimizations #light-cljs/env ["PENPOT_BUILD_OPTIMIZATIONS" :as :keyword :default :advanced],
     :pseudo-names true, :elide-asserts true}},
   :compiler-options {:macro-expansions 100000000, :macro-nodes 4000000000, :macro-timeout-ms 3600000,
                      :macro-memory-limit 4000000000, :macro-poll-budget 1000000000000}},
  :worker
  {:target :esm,
   :output-dir "out/worker/",
   :modules {:main {:entries [app.worker]}},
   :devtools {:reload-strategy :full},
   :release {:compiler-options {:optimizations #light-cljs/env ["PENPOT_BUILD_OPTIMIZATIONS" :as :keyword :default :advanced],
                                :elide-asserts true}},
   :compiler-options {...the same limits...}}}}
```

The report lists the rest:

- **Needs feature:**
  - `:dev-http`;
  - `:main :modules`: 10 modules, `app.plugins.api` as a second entry,
    `:exports init` on `:main`, `:render` and `:rasterizer`;
  - `:source-map` (both builds);
  - `:watch-dir` (both);
  - `:worker :target :browser`, `:web-worker` and `:prepend-js`.
- **Dropped:**
  - `:asset-path`, `:manifest-name`, `goog.DEBUG` and `goog.debug.LOGGING_ENABLED`;
  - every `:js-options` key and the Closure-only compiler options;
  - `:devtools-url` and `:browser-inject`.
- **Not ported:** `:storybook`, `:test` and `:bench`, which were not named on
  the command line.

The feasibility note's per-key table (`shadow-port-feasibility.md`, "shadow-cljs.edn
→ light-cljs.edn (Penpot)") still holds key for key, except two rows:
`:output-dir` is now `out/<build>/` in the port, not rebased onto the
original, and every `goog.*` define, `goog.DEBUG` included, is now dropped.

## 7. Verifying a port

Compiling is not proof. penpot-light's hand port of `app.main.style`
compiled and produced wrong class names (feasibility M4). The design has
four checks, from cheapest to strongest:

1. **Expansion differential** (run on every port; `--verify` in the
   prototype). For every kept or
   ported macro, the prototype samples up to 20 recorded call forms. The
   design compares every call site of every `:keep` candidate (§3, step 8).
   The JVM's own
   `&form` is printed with the head fully qualified. One `light-cljs --eval`
   process per macro namespace (and per 150 calls) requires that namespace,
   does `(in-ns 'caller)` so `*ns*` and `::kw` match, and prints
   `(macroexpand-1 'form)` for each. The results are compared structurally
   with the JVM expansion, allowing:
   - gensyms that correspond one to one;
   - `clojure.core/` and `cljs.core/` as the same;
   - numbers compared by value (`1.0` equals `1`);
   - `..` with or without a namespace.

   Results:
   - **The CSS divergence is caught.** Verifying penpot-light's hand
     `app.main.style` shim, declared as a port, against the JVM expansions:
     `css` matches at 2,780 of 5,131 sites, and the rest differ, for example
     `"main_ui_releases_v2-8__modal-header"` against the real
     `"main_ui_releases_v2_8__..."`. `css-case` matches at 9 of 519 sites.
     The JVM-expanded port has no such risk, by construction.
   - **Penpot's port: 238 of 1,081 sampled expansions are equal.** Every
     difference among the macro namespaces that ran is a curated edit, which
     is exactly what a reviewer must look at:
     - `defrecord` differs only by the `RecordIter` overlay patch;
     - garden `defpseudoclass` differs by the patch that removes
       `macroexpand`;
     - `cljs.spec` and `test.check` differ because they are compat stubs.
   - **The other 17 of 34 macro namespaces could not be checked.** `--eval`
     takes none of the `--macro-*` flags, so loading `app.common.data` and
     malli exceeds its fixed 1,024-expansion limit.
   - Todomvc: 34 of 83 sampled expansions are equal; the rest are spec stubs
     or namespaces past the eval limit.
2. **Load check.** The same `--eval` processes load the ported macro
   namespaces' closures. Loading found two runtime bugs that compiled cleanly:
   - penpot-light's `goog` shim did `(js/Symbol. ...)`, a TypeError at load
     that would break any page using `goog/getUid`. Fixed in the compat
     layer, whose `getUid` replacement now has a non-goog name.
   - cljs-bean reads `js/cljs.core.PersistentArrayMap.EMPTY`, a
     ReferenceError (light-cljs output has no `cljs` global). It now has a
     catalogue entry.
3. **Both builds' tests.** Compile the project's test build with shadow-cljs
   and with light-cljs, run both, and compare the reports. Like checks 1 and
   4, this runs during the migration, while the shadow-cljs original still
   builds; afterwards the port is the only project. This needs a
   light-cljs `cljs.test`; the port has only a stub, so Penpot's `:test`
   build cannot port yet. It is the strongest check for application code.
4. **Output differential on probes.** For data-producing namespaces (Penpot's
   geometry, transit and schema), a generated probe calls public functions on
   sample values in both builds and compares the printed results. penpot-light's
   `smoke/` harness is a hand-written instance. Not prototyped.

Checks 1 and 2 need two light-cljs fixes to be complete:

- `--eval` must accept the `--macro-*` limits;
- compile-time macro sessions must get the `macroexpand` service, which would
  let a probe macro expand at compile time.

## 8. Libraries in the project's jars

- **Copying.** Every library resource in the closure is copied from shadow's
  resource text (jar or gitlib) into `vendor/<resource-name>`. Macro-only
  `.clj`/`.cljc` files outside the closure are copied when a kept macro needs
  them. `port-map.edn` records each file's URL, `{:lib :version :kind}`,
  source SHA-256 and the edits made.
- **Library ports are versioned against the library version.** Catalogue
  entries are keyed by the deps coordinate and list exact versions: a git sha,
  or a Maven version from shadow's `:pom-info`, `~/.m2` or `~/.gitlibs`.
  - A mismatch fails the port, naming the library and both versions;
    `--allow-version-drift` tries anyway, and `--verify` then shows whether
    the port still matches.
  - Each entry carries a `:why` that names the compiler gap or the reason for
    the port.
  - Short `group/artifact` ids from `:pom-info` are normalized to
    `instaparse/instaparse`. That bug kept the instaparse patch from applying
    and produced a confusing "session invalidated" error.
- **Kinds of entries:**
  - `:ports-nss`/`:ported-macros`, which give the macros the `:port` class;
  - `:replace`, a whole file;
  - `:add`, a file the library lacks: rumext's JVM `compiler.clj` as `.cljc`;
  - `:patches`: `:replace`, `:replace-re`, `:require`, `:insert-after-ns` and
    `:replace-form`, all line-preserving.
- **Project overlays.** Same keys, applied when a build reaches a `:when-ns`
  namespace. Penpot's overlay (`*out*`, `RecordIter`, `es6-iterable`) is all
  compiler gaps.
- **The catalogue used for Penpot:**
  - rumext 27e5a1a (ports `defc`, `html`, ... and the hiccup compiler);
  - potok 0f7e15a (`reify`);
  - transit-cljs 0.8.280 (npm transit-js);
  - instaparse 1.5.0, tools.reader 1.6.0, garden 05590ec and cljs-bean 1.9.0
    (compiler gaps).
- **Long term,** analyzer-level macros (potok, rumext) should be upstreamed as
  `.cljc` macros that light-cljs can run, and compiler-gap patches should
  disappear with the fixes in §11. Separately versioned "port artifacts" for
  popular libraries can wait until a second project needs the same port.

## 9. Mapping the ablation's 42 edits

One row per group of `ablation.tsv` rows; the counts add up to 42.

| Ablation rows | Now |
|---|---|
| No longer needed (5): the config `*assert*`, extends and websocket patches, the cljs-bean `es6-entries-iterator` patch, the malli unwrap | dropped from the catalogue |
| `(new X.)` rewrite (1) | generic |
| 9 `#?(:clj (defmacro ...))` unwraps (cuerdas, object, data macros, record, instaparse gll/abnf, garden color/selectors/units) | generic: Var metadata and the helper call graph pick the definitions; `penpot-light`'s regex list is gone |
| 5 collectors; cursors and fonts patches; `collect-icons` refer patch (`token_status.cljs`); `dm/export` patch (9) | generic: JVM expansion, and refers of expanded macros dropped |
| perf and icons (`icon-xref`) patches, tools.reader `reader_types` patch (`log-source`) (3) | generic `.clj` handling |
| `app.util.modules` override (`shadow.esm/load-by-name`) (1) | generic: kept macro plus a compat macro `shadow.esm/load-by-name` → `light.lazy/loadable` |
| `utils.cljs` `pr-writer` (1) | generic `:core-gaps` rename (one catalogue line, also fixed reagent) |
| `record.cljc` patch (1) | split: `emit-impl-jvm` declare is generic (JVM-branch declares); `RecordIter` stays in the Penpot overlay |
| rumext ×3 (`v2.cljs` patch, `compiler`/`normalize` overrides), potok, transit (5) | curated library ports |
| tools.reader `inspect`, instaparse mix-hash, garden selectors and compiler (4) | curated, compiler gaps |
| `tokens_lib`, `tokens_status` (`*out*`), `objects_map` (`es6-iterable`) (3) | Penpot overlay, compiler gaps |

penpot-light's hand shims outside the ablation (`render_wasm/enums`,
`perf_macros`, `cursors_macros` and the 204 lines of `app.*` macro shims)
are replaced by generic `.clj` handling and JVM expansion.

What todomvc needed that Penpot did not, all of it generic now except the
compat addition:

- npm symbol requires (`[react ...]`);
- macrovich `deftime`;
- the `pr-writer` core gap;
- replacements for `goog.History` and `goog.history.EventType` in the compat
  layer, added by hand.

## 10. Remaining manual steps

- **Making shadow compile the project.** Its `node_modules` and generated
  files must be present (Penpot: `src/app/render_wasm/api/shared.js`), and
  the env vars its config demands must be set (`SHADOW_SERVER_URL`,
  `HTTP_PORT`, ...). The porter runs shadow's compile.
- **Running the result.** Bundling and running are not done. Penpot needs:
  - several page entries with a shared chunk;
  - named `init` exports;
  - a classic-script worker with `importScripts`;
  - a regenerated importmap;
  - transit-js added to `package.json`.

  todomvc needs an entry that calls `:init-fn` (a generated namespace is the
  stopgap) and a module-script page. These are light-cljs build features, not
  rewrites.
- **Runtime behaviour is unverified** beyond §7. A browser smoke run of
  either port is still to do.
- **Compat additions** when a project uses Closure Library pieces that have
  no replacement yet. The port fails until one is added, and
  `port-report.edn :missing-goog` lists them: todomvc `goog.async.nextTick`;
  Penpot a few (`goog.async.Deferred`, `goog.Promise`, ...).
- **Catalogue updates** when a ported library changes version; the port
  fails loudly until then.
- **Test builds** stay unported while `cljs.test` is a stub.
- **Overrides.** `light-cljs-port.edn` must be edited by hand when a kept
  macro fails in light-cljs. Automatic demotion is design only.

## 11. Compiler fixes that would remove the most porter code

Ranked by porter code they delete. Line counts are of the prototype.

light-cljs will not accept shadow's ns syntax (`:require-macros`,
`:refer-macros`, `:include-macros`, `:use-macros`, several `:require`
clauses), not even behind a flag. The porter rewrites it once and the
migrated code never needs it again (decided 2026-10-21).

1. **Expose the macro caller's file and the namespace's defs:** `&env :defs`
   and `(meta *ns*) :file`. `stl/css`, `css-case` and `collect-*` (5,655 of
   the 6,028 Penpot splices) then run in light-cljs and stay macro calls in
   the migrated source.
2. **Core gaps:** `pr-writer`, `es6-iterable`, `es6-entries-iterator`,
   `RecordIter`, `*out*`, `mix-collection-hash`, the seq and tree type names,
   the macroexpand service in compile-time sessions, and a `cljs.core` global
   for `js/cljs.core...` reads. This removes the Penpot overlay and the
   instaparse, tools.reader, garden and cljs-bean entries, plus `:core-gaps`
   and `light.compat`.
3. **A macro and a function of the same name,** as upstream allows. This
   removes the conflict rule; beicon's three macros become `:keep`
   candidates.
4. **`.clj` macro companions read as macro sources** when no `.cljc` exists
   (or `#?(:clj (defmacro ...))` honoured in macro namespaces). This removes
   `clj-macro-file`, `unwrap-edits`, the needed-definition tracking and the
   `declare` insertion (about 200 lines).
5. **Relative JS requires resolved against the source file.** This removes
   the string-require re-pathing and the machine-specific absolute paths.
6. **`(new X.)`.** Trivial.
7. **`--eval` accepting `--macro-*` limits,** plus fix 2's macroexpand
   service. The `:keep` comparison (§3, step 8) then covers every macro
   namespace.
8. **Source maps for chunked output.**
9. **Build features:** named exports, several entries and named chunk
   groups, a classic worker target, `:prepend-js`, `:init-fn` and a node
   target. Each one moves a key out of "needs feature".
10. **A machine-readable macro capability list.** This keeps the purity
    analysis in sync with the compiler.

With 1, 3, 4 and 5, the porter reduces to:

- the ns-form rewriting;
- the expansion recorder for a few impure macros;
- the catalogue of analyzer-level library ports;
- the config converter;
- verification.

## Rough size

| Part | Prototype |
|---|---|
| `light-cljs.port` (one namespace) | 1,896 lines. Recorder and closure about 200; purity and classification about 300; printing, splices, gensyms and compat rewrites about 250; ns and file rewriting about 550; JS conversion about 100; config conversion about 230; verification about 150; orchestration and reports about 100 |
| `remap-source-map.bb` | 38 lines |
| Catalogue | `catalogue.edn` 111 lines; compat layer 591 lines (Closure Library subset named `goog.*`, which the design renames, `shadow.*`, stubs, `light.compat`, `goog.History`); library and project ports 1,739 lines (rumext 1,232, transit 435) |

A production version with tests, automatic demotion, goog-free Closure Library
rewriting and an API version check would be about 2,500 lines plus the
catalogue. Fixes 1–5 would shrink it to about 1,350.

## Decisions

Made by the user on 2026-10-21. No open questions remain.

1. **Where the tool lives.** In `tools/port/` in the light-cljs repository,
   usable as a deps.edn git dependency at once. A Clojars release later
   makes `-d` work for `:dependencies` and lein projects.
2. **Who owns the catalogue.** The light-cljs repository, versioned with
   light-cljs. Each entry names the compiler gap it covers.
3. **What makes a macro `:keep`.** It must be pure and expand equally on
   the JVM and in light-cljs. The static analysis expands anything impure
   or host-dependent (§3, step 5). Every call site of every remaining
   candidate is then compared on both hosts (step 8), and any difference,
   or a comparison that cannot run, means `:expand`.
4. **Shadow ns syntax.** light-cljs does not accept it, not even behind a
   flag. The porter rewrites it once (§4, §11).
5. **Where output goes.** The port is written to a new folder, and each
   build's `:output-dir` is `out/<build>/` inside it (§6). The porter never
   writes into the original project, and there is no mode for overwriting
   one.

## Reproduction

Run from `research/penpot/frontend`:

```sh
export PATH=/opt/homebrew/opt/node/bin:$PATH
export CLJ_CONFIG=../../shadow-port-tool/clj-config
export SHADOW_SERVER_URL=http://localhost:3999 HTTP_PORT=3999 NREPL_PORT=3998 DEV_PORT=3997
npx shadow-cljs -A:light-cljs/port run light-cljs.port/main :main :worker ../../penpot-port \
  --compile --no-cache --verify --light-cljs ../../../target/release/light-cljs
```

- **todomvc:** run from `research/re-frame/examples/todomvc` after
  `npm install`, with `:client ../../../todomvc-port`.
- **Other entries:** `app.render` and `app.rasterizer` were compiled by hand
  with the same flags.
- **Line counts:** compare against `port-map.edn` (897 of 897 files equal).
- **Source maps:**
  1. compile with `--source-map build/x.js.map`;
  2. run `bb ../shadow-port-tool/remap-source-map.bb build/x.js.map .`.
- **The CSS demonstration:**
  1. copy `penpot-light/shims/app/main/style.cljs` into `OUT/verify/app/main/`;
  2. in the same JVM, call `verify-expansions` with `app.main.style/css` and
     `css-case` reclassified as `:port`.
- **Results:**
  - `research/penpot-port/` and `research/todomvc-port/` hold the trees,
    `light-cljs.edn`, `port-report.edn` and `port-map.edn`;
  - compiler outputs and caches were deleted;
  - `node_modules`, `.cpcache` and `.shadow-cljs` were removed from todomvc.
