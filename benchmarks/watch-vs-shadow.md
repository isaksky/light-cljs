# The edit-to-reload loop: `light-cljs watch` against `shadow-cljs watch`

This answers "developers use `shadow-cljs watch`, not cold builds" (shadow-comparison-credibility group, light-cljs-972). Both tools run their watch with their own dev server and their own reload client, one tool at a time, and the same edits are saved into the same sources.

Everything here is rerun with `benchmarks/watch-vs-shadow.bb` (and its browser side, `benchmarks/watch-vs-shadow.mjs`):

```sh
bb benchmarks/re-frame.bb setup                     # TodoMVC: npm install, shadow-cljs 3.5.3, esbuild, playwright-core
bb benchmarks/watch-vs-shadow.bb prepare todomvc     # light-cljs's dev page (React bundle + index.js)
bb benchmarks/watch-vs-shadow.bb prepare re-frame-200k   # copy of the generated app, a shadow :browser build, both dev pages
bb benchmarks/watch-vs-shadow.bb run todomvc --tools shadow,light --raw A.edn --light BIN
bb benchmarks/watch-vs-shadow.bb run todomvc --tools light,shadow --raw B.edn --light BIN
bb benchmarks/watch-vs-shadow.bb report A.edn B.edn
```

## 2026-11-14: TodoMVC and the generated 200k re-frame app

### Setup

- **light-cljs:** an immutable copy of the release binary at master `e5b7832e`, `research/shadow-credibility/bin/light-cljs-e5b7832e`. V8 build. `light-cljs watch BUILD --out-dir DIR --serve 0 --no-nrepl`, default polling (every 100 ms).
- **shadow-cljs:** 3.5.3, `npx shadow-cljs watch client` for TodoMVC and `clojure -M:shadow watch watch` for the generated app (the project's own alias, `-Xmx8g`). Default file watcher: on macOS that is the JVM's watch service, read every 500 ms (`:fs-watch` unset). OpenJDK 25.0.2.
- **Other tools:** headless Chromium 1248 through playwright-core 1.64.1, Node 26.12.1, React 18.2.0 (development builds on both sides), esbuild 0.25.10 for light-cljs's React bundle.
- **Machine:** macOS, 24 cores, 32 GB, **unreserved**. Two other workers ran builds and timings throughout. The one-minute load average before every edit is in the tables: 1.5 to 3.6 for TodoMVC, 6.3 to 13.0 for light-cljs and 8.2 to 17.7 for shadow-cljs on the generated app (the shadow-cljs sessions happened to coincide with a peer's heavier work).
- **Runs:** two watch sessions per tool and app, in the orders shadow, light then light, shadow (TodoMVC) and light, shadow then shadow, light (generated app). Each session starts with nothing cached (`.shadow-cljs/builds/ID` and `.light-cljs` removed), opens the page, and runs every scenario: one warm-up edit, then five timed edits, then the file restored. So each cell below pools 10 timed edits.

**Apps.**
- **TodoMVC:** shadow-cljs builds the unmodified upstream example in `research/re-frame/examples/todomvc` with its own `:client` build, whose `^:dev/after-load` hook re-renders. light-cljs builds the porter's port `research/shadow-credibility/todomvc-port-980` (porter `shadow-port-tool-980`), the same port benchmarks/re-frame-vs-shadow.md uses, with its `light-cljs.edn` and an out dir of its own.
- **Generated app:** `research/shadow-credibility/projects/re-frame-200k-seed42` (202,053 nonblank lines, 2,800 feature namespaces of reagent views and re-frame events and subscriptions, 180 utility namespaces, 23 project macro namespaces), copied so the edits never touch the shared project. Its shadow-cljs build is a `:node-script`, so `prepare` adds a `:browser` build of the same sources and entry (`:init-fn workload.main/main`); light-cljs uses the project's own `:bench` build. `workload.main/main` mounts the app and dispatches 2,100 events in the page.

**What is timed.** Each edit replaces one string (or, for the macro, one number) with a new token, the same text for both tools. Time zero is the edited file's modification time. Then:

1. **Output written:** the newest modification time in the tool's output directory, read once the page runs the new code.
2. **Reload message:** the push reaching a listener on the same machine: shadow-cljs's `cljs-build-complete` websocket frame to the page, light-cljs's `load` server-sent event on a second `/events` connection.
3. **New code running:** a probe in the page turns true. For TodoMVC's view it is the rendered page text, after each tool's reload client ran the app's after-load hook. On the generated app the probe calls the function the edit changes, as the page holds it (the namespace object for shadow-cljs, `app.baga.views.root`; the definition registry for light-cljs, `__lightDev.roots["app.baga.views/root"]` (the build's `__lightApplicationRoots$KEY` since light-cljs-1045)), and checks the result: the token in `root`'s hiccup and in `geinklarvou`'s string, and for the macro the exact number `dokgem`, a function of a namespace that uses the macro, returns with the new constant, so it turns true only once the expansion was recompiled, reloaded and runs. For TodoMVC's `re-frame.loggers` it is the source text of `set-loggers!` (its message shows only when its assertion fails), so there it shows the new definition installed rather than called.

The probe is read every 2 ms. "Step time" is each tool's own figure from its log: shadow-cljs's `Build completed. (... s)` and light-cljs's `recompiled ... in N ms`. CPU time is the watch's whole process tree, user plus system, from before the save to the probe. Peak memory is the process tree's resident memory, sampled every 250 ms: during startup (until the first build) and while editing (from the page's first render to the end).

**The edits.**

| App | Scenario | File | Edit | Probe |
|---|---|---|---|---|
| TodoMVC | view (leaf) | `src/todomvc/views.cljs` | `[:h1 "todos"]` gets a token | page text |
| TodoMVC | widely required | `re_frame/loggers.cljc` (upstream re-frame source for shadow-cljs, the port's identical vendored copy for light-cljs), required by 14 re-frame namespaces | the `set-loggers!` assertion message gets a token | source of `re-frame.loggers/set-loggers!` |
| generated | view (leaf) | `src/app/baga/views.cljs` | `[:h2 "Baga"]` gets a token | `(app.baga.views/root)` |
| generated | widely required | `src/workload/common.cljs`, required by 217 namespaces | a string in `geinklarvou` gets a token | `(workload.common/geinklarvou 1)` |
| generated | macro | `macros/workload/sobis.cljc`, used by 8 namespaces | the constant 11 in macro `hainfoulvei` becomes a new number | `(workload.breisrei/dokgem 3)`, which `hainfoulvei` defines |

TodoMVC has no project macro namespace, so the macro scenario runs on the generated app only. TodoMVC's own namespaces are each required by at most one other, so its "widely required" scenario edits re-frame's `re-frame.loggers`, which is a source directory on both classpaths and which both watches pick up.

### TodoMVC

Milliseconds from the save, median (range) of 10 edits.

| Scenario | Tool | Output written | Reload message | New code running | Tool's own step time | Namespaces or files compiled | CPU user + system per edit, s | Load average |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| view (leaf): todomvc.views | shadow-cljs | 967 (940-1007) | 973 (946-1014) | 979 (951-1020) | 40 (40-80) | 2 (2-2) | 0.16 (0.11-0.26) | 3.1 (2.7-3.4) |
| view (leaf): todomvc.views | light-cljs | 57 (42-84) | 58 (44-85) | 64 (51-89) | 4 (4-8) | 1 (1-1) | 0.01 (0.00-0.02) | 2.4 (2.3-2.5) |
| widely required: re-frame.loggers | shadow-cljs | 945 (913-963) | 950 (919-969) | 988 (957-1006) | 150 (140-160) | 15 (15-15) | 0.87 (0.51-1.05) | 3.0 (2.5-3.6) |
| widely required: re-frame.loggers | light-cljs | 169 (139-194) | 171 (141-196) | 173 (143-198) | 144 (134-150) | 10 (10-10) | 0.15 (0.13-0.16) | 2.5 (2.4-2.7) |

Where the time to new code running goes (medians, ms): noticing the save and anything outside the step (output written minus step time), the step, output to message, and message to running.

| Scenario | Tool | Noticing and other | Step | Output to message | Message to running |
|---|---|---:|---:|---:|---:|
| view (leaf) | shadow-cljs | 919 | 40 | 6 | 6 |
| view (leaf) | light-cljs | 52 | 4 | 1 | 5 |
| widely required | shadow-cljs | 795 | 150 | 6 | 37 |
| widely required | light-cljs | 25 | 144 | 2 | 2 |

| Session | Tool | Startup to first build, s | Page load in Chromium, s | Peak memory during startup, MB | Peak memory while editing, MB | Load average before |
|---|---|---:|---:|---:|---:|---:|
| 1 | shadow-cljs | 10.4 | 0.3 | 2,233 | 2,250 | 1.5 |
| 1 | light-cljs | 0.5 | 0.2 | 46 | 90 | 2.5 |
| 2 | light-cljs | 0.5 | 0.2 | 47 | 94 | 2.4 |
| 2 | shadow-cljs | 10.3 | 0.3 | 2,228 | 2,258 | 2.6 |

- light-cljs gets the new code running in 64 ms for a view and 173 ms for re-frame's logger namespace; shadow-cljs in 979 and 988 ms.
- Most of shadow-cljs's time is noticing the save: 800 to 920 ms of its default macOS watcher, against light-cljs's 100 ms poll. Its compile step alone is 40 ms for the view and 150 ms for the logger namespace (15 files). light-cljs's steps are 4 ms and 144 ms (10 namespaces: the logger namespace and 9 re-frame namespaces the watch recompiles with it). On compile step alone, the widely-required case is a tie.
- TodoMVC is far too small to say anything about compile speed or memory beyond this: shadow-cljs's startup and peak memory are the JVM's start and default heap (the group's rule; see generated-shadow-vs-light.md for scale).

### Generated re-frame app, 200k lines

Milliseconds from the save, median (range) of 10 edits.

| Scenario | Tool | Output written | Reload message | New code running | Tool's own step time | Namespaces or files compiled | CPU user + system per edit, s | Load average |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| view (leaf): app.baga.views | light-cljs | 122 (70-223) | 208 (150-300) | 209 (151-304) | 128 (119-161) | 1 (1-1) | 0.19 (0.17-0.23) | 8.7 (7.2-11.0) |
| view (leaf): app.baga.views | shadow-cljs | 2891 (2751-4914) | 2948 (2811-4986) | 2961 (2823-4997) | 800 (770-2890) | 2 (2-2) | 1.21 (1.06-3.13) | 14.1 (12.2-16.8) |
| widely required: workload.common | light-cljs | 114 (90-143) | 192 (162-219) | 193 (164-220) | 132 (118-151) | 1 (1-1) | 0.19 (0.15-0.21) | 8.6 (7.1-10.2) |
| widely required: workload.common | shadow-cljs | 5430 (3706-6328) | 5492 (3767-6393) | 6603 (4874-7600) | 3640 (3270-4280) | 206 (206-206) | 52.48 (49.00-59.64) | 13.9 (10.9-17.7) |
| macro: workload.sobis/hainfoulvei | light-cljs | 1340 (1028-1445) | 1417 (1103-1525) | **1425 (1109-1533)** | **1306 (1073-1442)** | 9 (9-9) | 1.66 (1.32-1.80) | 7.6 (6.3-9.4) |
| macro: workload.sobis/hainfoulvei | shadow-cljs | 856 (832-892) | 918 (887-949) | 962 (929-993) | 295 (270-320) | 9 (9-9) | 1.30 (1.23-1.43) | 14.3 (13.8-15.3) |

| Scenario | Tool | Noticing and other | Step | Output to message | Message to running |
|---|---|---:|---:|---:|---:|
| view (leaf) | light-cljs | -16 | 128 | 77 | 1 |
| view (leaf) | shadow-cljs | 2083 | 800 | 59 | 12 |
| widely required | light-cljs | -23 | 132 | 76 | 1 |
| widely required | shadow-cljs | 1671 | 3640 | 62 | 1137 |
| macro | light-cljs | 17 | 1306 | 77 | 7 |
| macro | shadow-cljs | 562 | 295 | 58 | 44 |

(light-cljs's "noticing and other" is near zero or negative because its logged step ends after work it does once the files are written; that work is the 77 ms "output to message", see below.)

| Session | Tool | Startup to first build, s | Page load in Chromium, s | Peak memory during startup, MB | Peak memory while editing, MB | Load average before |
|---|---|---:|---:|---:|---:|---:|
| 1 | light-cljs | **174.6** | **9.2** | 1,310 | 1,386 | 10.0 |
| 1 | shadow-cljs | 40.3 | 2.3 | 2,669 | 4,532 | 8.2 |
| 2 | shadow-cljs | 38.4 | 2.1 | 2,633 | 3,852 | 12.4 |
| 2 | light-cljs | **168.7** | **8.1** | 1,306 | 1,403 | 13.0 |

An earlier pair of sessions per tool, identical except that the probe read the changed definition's source text instead of calling it (the review of this pellet asked for a call), gave the same picture: new code running in 200, 204 and 1,411 ms for light-cljs and 2,992, 6,711 and 957 ms for shadow-cljs; startup 155 to 168 s against 36.5 to 38 s; page load 8.1 to 9.6 s against 2.0 s.

- **View and widely required:** light-cljs runs the new code in 209 and 193 ms. shadow-cljs takes 3.0 s for the view and 6.6 s for `workload.common`: about 2 s passes outside its own step (mostly noticing the save, which its watcher does more slowly in this larger tree), it recompiles all 206 namespaces that require `workload.common` (3.6 s, 52 s of CPU), and the page then loads them (1.1 s). light-cljs recompiles the one namespace, since its callers look definitions up when they run.
- **Peak memory while editing:** 1.4 GB for light-cljs against 3.9 to 4.5 GB for shadow-cljs (`-Xmx8g`, so this reflects the JVM heap it was allowed, as the group context notes).
- No console errors or uncaught exceptions on either side.

### Where light-cljs is slower

Three results are worse for light-cljs, recorded as found. Remedies are not designed here; per the group's rules they wait for the user. Filed as light-cljs-991 (watch steps), light-cljs-992 (startup) and light-cljs-993 (page load).

**1. The macro edit: 1.43 s against 0.96 s to new code running.** (Fixed by light-cljs-991: 0.43 s against 1.00 s, see "watch steps after light-cljs-991" below.) Both tools recompile the same 9 namespaces (the macro namespace and its 8 users). shadow-cljs's step takes 295 ms; light-cljs's takes 1,306 ms, about 145 ms per namespace, while the same per-namespace step on TodoMVC takes 4 ms. shadow-cljs still loses about 560 ms to noticing the save, so light-cljs is slower end to end despite noticing at once. A `sample` profile of six macro saves in a running watch (macOS `sample`, 16 s): 95% of the step thread's busy samples are in `Watch::compile` → `DevSession::load_namespace_file` → `InteractiveCompiler::analyze_forms` → `analyzer::analyze_input`, and 72% of all of them in `RecordLayouts::extend` (src/analyzer.rs:3246, called from `Analyzer::body` at :12405 for every top-level form). A session analysis starts from empty record layouts, so each top-level form of each recompiled namespace scans every expression the 200k program holds. The same scan is in the one-namespace steps (about a quarter of their 128 ms); the rest of those is `Watch::refresh_hooks` (about a third), the dependency manifest's `Watch::revise` (a fifth) and `DevSession::collect_garbage` (a tenth), each of which walks the whole program on every step. They run after the files are written, which fits the 77 ms between the files written and the push.

**2. Startup: 169 to 175 s against 38 to 40 s.** (Fixed by light-cljs-992: 18.7 to 21.9 s against 38.3 to 102.3 s, see "watch startup after light-cljs-992" below.) `light-cljs watch --stats` on the same build: wall 130.1 s, of which emission 77.5 s, the session's load of the program 46.2 s and analysis 5.5 s (a release build of the same project took 8.6 s uncached with an earlier binary, the project's compare-result.edn). A `sample` of the first 60 s of the startup: 97% of the build thread is in `write_dev_build` → `dev_build::emit_dev_build`, which calls `emitter::emit_js_mode` once per namespace (3,073), and more than half of all its samples are `Arena<GlobalBinding>` lookups inside it: each namespace's emission walks the whole program's globals.

**3. Page load: 8.1 to 9.2 s against 2.1 to 2.3 s,** from navigation to the first render. (Narrowed by light-cljs-993: 5.0 and 5.2 s against 2.0 s, see "dev page load after light-cljs-993" below.) A separate measurement of the same output served statically: light-cljs's units finish loading 5.0 to 8.8 s after navigation and the app's first run takes another 3.2 s; shadow-cljs's page has loaded and rendered at 1.5 s. Bytes are not the cause: light-cljs's dev output is 40 MB in 3,077 files, shadow-cljs's 95 MB in 3,203. light-cljs's `index.js` (`dev_build::index_text`) appends one script element at a time and waits for its load event before the next, so its 3,077 requests and evaluations run strictly one after another. How much of the 5 to 9 s that serial loading costs, against the evaluation of the units themselves, was not separated.

docs/dev-workflow.md's target, under 100 ms from save to updated page, holds on TodoMVC (64 ms) but not on the 200k app (193 to 209 ms), since the one-namespace step grows with the program for the reasons in 1. It is still 14 to 34 times faster than shadow-cljs's there.

## 2026-11-14, later: watch steps after light-cljs-991

light-cljs-991 fixed the main cause of item 1 above. Each top-level form a watch step analyzes now starts from the record layouts the session keeps for its retained program, instead of scanning every expression of the 200k program. A step also finds the dev hooks in one pass over each definition's metadata, instead of a linear search of the 3,073 namespaces per definition. Output and watch messages are unchanged.

**Setup.** Same kit, app and edits as above. light-cljs is `research/shadow-credibility/bin/light-cljs-8ec924e1`, the fix's commit on master `71da573e`. Two sessions per tool in the orders shadow, light then light, shadow: 10 timed edits per cell. For a same-day baseline, one more light-cljs session used master's own binary `light-cljs-71da573e`, giving 5 timed edits per cell. **Unreserved:** peer workers were building and testing throughout. The one-minute load average before each edit is in the table: 5.8 to 10.1 for the fixed light-cljs, 12.0 to 28.3 for shadow-cljs, and 18.7 to 24.3 for the baseline session.

Milliseconds from the save, median (range). Only the rows this fix moves are shown; shadow-cljs's rows come from the same sessions.

| Scenario | Tool | Output written | Reload message | New code running | Tool's own step time | CPU user + system per edit, s | Load average |
|---|---|---:|---:|---:|---:|---:|---:|
| view (leaf): app.baga.views | light-cljs, master `71da573e` | 107 (73-209) | 182 (166-285) | 183 (168-286) | 130 (120-141) | 0.18 (0.16-0.22) | 24.3 (22.6-24.3) |
| view (leaf): app.baga.views | light-cljs, `8ec924e1` | 134 (73-174) | 179 (119-217) | **180 (122-218)** | **66 (63-69)** | 0.10 (0.09-0.12) | 8.4 (6.8-10.1) |
| view (leaf): app.baga.views | shadow-cljs | 2860 (2349-3390) | 2927 (2404-3481) | 2940 (2416-3496) | 820 (740-1590) | 1.18 (1.05-2.48) | 16.2 (12.0-18.8) |
| widely required: workload.common | light-cljs, master `71da573e` | 105 (91-192) | 181 (167-267) | 182 (169-269) | 132 (129-135) | 0.19 (0.18-0.20) | 20.9 (20.9-22.6) |
| widely required: workload.common | light-cljs, `8ec924e1` | 120 (102-170) | 164 (146-213) | **167 (147-214)** | **59 (57-62)** | 0.09 (0.08-0.13) | 7.6 (6.3-9.0) |
| widely required: workload.common | shadow-cljs | 7954 (5071-15149) | 8046 (5133-15238) | 9463 (6251-16486) | 6845 (4030-14350) | 43.39 (38.15-64.84) | 16.9 (11.4-25.3) |
| macro: workload.sobis/hainfoulvei | light-cljs, master `71da573e` | 1542 (1331-1689) | 1622 (1407-1788) | 1630 (1414-1797) | 1510 (1340-1630) | 1.99 (1.72-2.13) | 18.8 (18.7-19.4) |
| macro: workload.sobis/hainfoulvei | light-cljs, `8ec924e1` | 367 (264-423) | 421 (310-473) | **428 (317-481)** | **277 (260-325)** | 0.39 (0.34-0.45) | 6.8 (5.8-8.0) |
| macro: workload.sobis/hainfoulvei | shadow-cljs | 874 (836-2911) | 953 (895-2985) | 1002 (938-3034) | 385 (310-660) | 1.64 (1.39-2.15) | 22.8 (17.5-28.3) |

- **Macro edit:** light-cljs's step for the 9 namespaces fell from 1,510 to 277 ms, and new code runs after 428 ms instead of 1,630. shadow-cljs took 385 ms for its step and 1,002 ms to new code running in the same sessions, so item 1 above no longer holds. shadow-cljs ran under a heavier load (17.5 to 28.3) than light-cljs (5.8 to 8.0), so its numbers here may be inflated; 972's sessions, at a load of 13.8 to 15.3, gave it 295 ms and 962 ms.
- **One-namespace edits:** the step halved, from 130 to 66 ms for the view and from 132 to 59 ms for `workload.common`, and CPU time per edit halved too. Time to new code running did not drop with it: 180 against 183 ms for the view and 167 against 182 ms for `workload.common`. The rest is noticing the save (the 100 ms poll and its scan of the tree), work before the files are written, and 44 ms from the files to the push, down from 76. docs/dev-workflow.md's under-100 ms target still does not hold on the 200k app. No regression.
- **Startup and page load** (items 2 and 3, light-cljs-992 and light-cljs-993) were not changed by this fix. In these sessions light-cljs started in 111 and 119 s and its page loaded in 7.2 to 7.7 s. shadow-cljs started in 39 and 83 s and its page loaded in 2.0 to 3.6 s. The baseline session took 272 s to start, under a load of 16.9.
- **Still per-step whole-program work,** from a `sample` of the fixed watch: the dependency manifest's `Watch::revise` (about 25 ms per step), the hook listing (about 19 ms in a profile taken before its one-pass change, not profiled since), and, on the macro edit, merging each deferred unit's reader contexts (about 35 ms per step, possibly light-cljs-996's cause). Emitting each namespace also walks all of the program's globals, which is light-cljs-992's cause.

## 2026-11-14, later: watch startup after light-cljs-992

light-cljs-992 fixed item 2 above. A dev build's units now share what one build needs once: the source map names of the program's functions and its definitions grouped by namespace (`emitter::NamespaceUnits`). Before, each of the 3,073 units scanned every expression of the program for function names and walked every global to list its own namespace's definitions. The dev build is byte-identical to master's (all 6,150 files of the 200k app compared), and a unit test requires the same of examples/dev and examples/features.

**Setup.** Same kit, app and edits as above. light-cljs is `research/shadow-credibility/bin/light-cljs-f9aec825`, the fix's commit on master `34bd7c1d`. Four sessions per tool, in two pairs of invocations, each pair in the orders light, shadow then shadow, light. The kit now also reads the watch process tree's user and system CPU time when the first build is logged (`benchmarks/watch-vs-shadow.bb`); the first pair ran before that was added. Two more light-cljs sessions used master's own binary `light-cljs-34bd7c1d` as a same-day baseline. **Unreserved:** peer workers were building and running timings throughout, and the second pair ran under a load average of 13 to 41. The one-minute load average before each session is in the table.

| Session | Tool | Startup to first build, s | CPU user + system during startup, s | Page load in Chromium, s | Peak memory during startup, MB | Peak memory while editing, MB | Load average before |
|---|---|---:|---:|---:|---:|---:|---:|
| baseline 1 | light-cljs, master `34bd7c1d` | 99.3 | - | 8.2 | 1,299 | 1,387 | 14.5 |
| baseline 2 | light-cljs, master `34bd7c1d` | 135.4 | 117.2 + 11.2 | 7.6 | 1,319 | 1,395 | 17.8 |
| 1 | light-cljs, `f9aec825` | **18.7** | - | 7.2 | 1,259 | 1,372 | 5.3 |
| 1 | shadow-cljs | 38.3 | - | 2.0 | 3,021 | 3,667 | 4.1 |
| 2 | shadow-cljs | 45.7 | - | 2.3 | 3,343 | 3,564 | 21.3 |
| 2 | light-cljs, `f9aec825` | **21.9** | - | 9.0 | 1,272 | 1,370 | 18.7 |
| 3 | light-cljs, `f9aec825` | **20.8** | 19.2 + 1.8 | 10.5 | 1,277 | 1,389 | 17.7 |
| 3 | shadow-cljs | 39.0 | 105.2 + 441.4 | 2.2 | 2,324 | 5,142 | 13.2 |
| 4 | shadow-cljs | 102.3 | 103.5 + 332.5 | 3.9 | 3,344 | 4,378 | 14.3 |
| 4 | light-cljs, `f9aec825` | **21.6** | 19.4 + 2.3 | 8.5 | 1,288 | 1,385 | 35.0 |

`light-cljs watch bench --stats`, one startup each, load average 5.2 and 7.1: on master, wall 106.0 s, of which emission 88.9 s, the session's load of the program 11.1 s and analysis 5.2 s; with the fix, wall 21.7 s, of which emission 1.8 s, session load 12.6 s and analysis 5.9 s.

- **Startup:** median 21.2 s (18.7 to 21.9) over four sessions, against shadow-cljs's 42.4 s (38.3 to 102.3) in the same invocations; master took 99.3 and 135.4 s. CPU time to the first build: 21.0 and 21.7 s user plus system, against master's 128.4 s and shadow-cljs's 546.6 and 436.0 s. Most of shadow-cljs's is system time (441 and 333 s), which these loaded sessions may inflate; it is recorded as found. Item 2 no longer holds.
- **What is left of startup** is mostly the session's own load of the program (11 to 13 s; light-cljs-991 had already cut it from 46 s). The startup build analyzes the program with V8, and then the watch's session analyzes it again in its own macro host, since this project turns the cache off (`:cache-level :off`). With a cache directory the session starts from the build instead (light-cljs-327). A startup with an empty `--cache-dir` took 20.0 s wall (analysis 12.1 s in the session's host, session load 0.2 s), against 21.7 s without one. Starting every watch that way would save about 2 s here, so it was not done.
- **Edits, no regression** (milliseconds to new code running, median and range of the first pair's 10 edits per cell, against the first baseline session's 5): view 190 (159-231) against 185 (121-1565); `workload.common` 198 (101-225) against 206 (117-225); macro 408 (305-452) against 419 (385-1075). Steps: 67, 60 and 288 ms against 74, 62 and 305 ms. shadow-cljs in the same sessions: 2,938, 6,969 and 974 ms. The second pair agrees: 177, 197 and 362 ms (steps 67, 60 and 276 ms), the second baseline session 183, 178 and 366 ms (steps 67, 58 and 284 ms).
- **Page load** (item 3, light-cljs-993) is unchanged: 7.2 to 10.5 s against 2.0 to 3.9 s.
- The per-step whole-program work listed in the light-cljs-991 note (manifest revision, hook listing, garbage collection, polling) is light-cljs-1001's and was not touched.

## 2026-11-14, later: dev page load after light-cljs-993

light-cljs-993 addressed item 3 above. `index.js` used to append one script element and wait for its load event before appending the next, so the 3,077 files were requested and run strictly one after another. It now appends them all at once with `async = false`: the browser fetches them in parallel and still runs them in manifest order, as shadow-cljs's dev loader does. Namespace order, `^:dev/once` and the reload protocol are unchanged. After a file throws or cannot load, the client's `define` skips every later unit, so as before no unit after the failure runs; the promise then rejects.

**Where the time went (separated first).** The same dev output (master `9d06189f`), served statically and opened in headless Chromium, three runs at a load average of 3.7 to 4.4. Before the change, the units finished loading 4.3 to 5.0 s after navigation, and the app had rendered by 7.0 to 7.7 s. A Chromium trace of one load puts 1.35 s of the main thread in evaluating the 3,080 scripts. So about 3 s of the load was waiting on one request at a time. With the files requested at once (the change's output, served statically and through the watch's dev server, four runs at a load average of 4.4 to 4.7), the units finished loading at 1.4 to 1.6 s (1.2 s of it evaluating scripts) and the app rendered at 4.2 to 4.4 s.

The 2.7 s left after loading is the app's first run (`workload.main/main` mounts the app and dispatches 2,100 events). A CPU profile of it puts 2.5 s in the runtime's own functions (map entries, protocol dispatch, keywords). That is the runtime-speed gap benchmarks/runtime-vs-shadow.md measures on release output, where this app's first output takes 4.4 s against 1.4 s. The same profile has 0.7 s inside `define`, the main thread compiling the unit functions as they run. Wrapping each unit function in parentheses, which V8 takes as a hint to compile it eagerly, cut the load to 1.1 to 1.2 s in two of three runs of a hand-edited copy, against 1.4 s without it. That was not done here.

**Setup.** Same kit, app and edits as above. light-cljs is `research/shadow-credibility/bin/light-cljs-951119a3`, the change's commit on master `9d06189f`. There were two sessions per tool on the generated app, in the orders light, shadow then shadow, light, and the same on TodoMVC in the orders shadow, light then light, shadow. A same-day baseline used master's own binary, `light-cljs-9d06189f`: two light-cljs sessions on the generated app and one on TodoMVC, run between the others. **Unreserved:** peer workers were building and testing. The one-minute load average before each session is in the table.

| App | Session | Tool | Page load in Chromium, s | Startup to first build, s | CPU user + system during startup, s | Peak memory during startup, MB | Peak memory while editing, MB | Load average before |
|---|---|---|---:|---:|---:|---:|---:|---:|
| generated | 1 | light-cljs, `951119a3` | **5.0** | 18.8 | 18.0 + 1.4 | 1,267 | 1,385 | 15.6 |
| generated | 1 | shadow-cljs | 2.0 | 37.5 | 99.7 + 458.0 | 2,640 | 4,961 | 8.7 |
| generated | baseline 1 | light-cljs, master `9d06189f` | 8.2 | 21.6 | 19.8 + 2.0 | 1,285 | 1,390 | 9.0 |
| generated | 2 | shadow-cljs | 2.0 | 37.0 | 99.9 + 445.2 | 2,440 | 4,911 | 6.7 |
| generated | 2 | light-cljs, `951119a3` | **5.2** | 19.7 | 18.4 + 1.5 | 1,258 | 1,383 | 9.5 |
| generated | baseline 2 | light-cljs, master `9d06189f` | 7.8 | 19.7 | 18.8 + 1.5 | 1,268 | 1,386 | 6.2 |
| TodoMVC | 1 | shadow-cljs | 0.31 | 13.7 | 56.6 + 20.2 | 2,256 | 2,268 | 5.1 |
| TodoMVC | 1 | light-cljs, `951119a3` | 0.17 | 0.5 | 0.5 + 0.1 | 45 | 87 | 4.5 |
| TodoMVC | baseline | light-cljs, master `9d06189f` | 0.22 | 0.5 | 0.5 + 0.0 | 45 | 88 | 4.3 |
| TodoMVC | 2 | light-cljs, `951119a3` | 0.17 | 0.6 | 0.5 + 0.1 | 66 | 88 | 4.0 |
| TodoMVC | 2 | shadow-cljs | 0.29 | 11.7 | 56.8 + 19.3 | 2,168 | 1,745 | 4.1 |

The kit measures CPU time and peak memory for the watch's process tree, not for the browser, so page load has wall time only; the main thread's share of it is in the static measurement above.

- **Page load of the generated app:** 5.0 and 5.2 s, against master's 7.8 and 8.2 s and shadow-cljs's 2.0 s in the same invocations. light-cljs is still 2.5 times slower here, and what is left is mostly the first run, which is the runtime's speed. Item 3 holds in a smaller form. The kit's interval starts before the browser context opens and ends when its 20 ms poll sees the header. It reads about 0.8 s above the 4.2 and 4.3 s the static probe gave through the same watch's dev server; that difference was not separated.
- **TodoMVC page load** did not get worse: 0.17 s in both sessions, against 0.22 s for the baseline and 0.29 to 0.31 s for shadow-cljs.
- **Edits, no regression** (median and range of 10 edits per cell, against the baseline's 10). Milliseconds to new code running: view 183 (157-235) against 190 (118-227); `workload.common` 188 (151-220) against 191 (101-230); macro 363 (334-477) against 406 (325-477). Steps: 70, 61 and 292 ms against 68, 62 and 293 ms. CPU user + system per edit: 0.10, 0.10 and 0.39 s against 0.10, 0.10 and 0.39 s. On TodoMVC (5 baseline edits): 63 and 170 ms against 57 and 163 ms to new code running, and 0.00 and 0.14 s of CPU per edit against 0.01 and 0.15 s. Startup, CPU time and peak memory are in the table and unchanged within noise.
- No console errors or uncaught exceptions on either side.

## 2026-11-14, later: watch steps after light-cljs-1001

light-cljs-1001 made the rest of a step's whole-program work (the light-cljs-991 note's last item) follow what the step changed. A poll tells directories apart by device and inode instead of resolving every path, and lists a directory again only when its stamp changed. A step whose namespace keeps its requires, observations and macros keeps the dependency manifest instead of rebuilding all of it. The session keeps each definition's hook flags and looks only at the definitions a form changed. A rewritten unit's registration reads the session's per-namespace index. A deferred unit's merge visits only the reader contexts that changed since its snapshot. The watch output of this app (all five snapshots of a view, `workload.common`, macro and second view edit) and its release output are byte-identical to master's.

**Setup.** Same kit, app and edits as above. light-cljs is `research/shadow-credibility/bin/light-cljs-fa9856b4`, the fix's commit on master `1579f4fc`. The same-day baseline is master's own binary, `light-cljs-1579f4fc`. **Unreserved:** peer workers were building and testing. The one-minute load average before each edit is in the tables.

**The kit's pause, and a new option.** After each edit the kit waits until the page runs the new code, then pauses one second. With a fixed pause every save lands at about the same point of the watch's poll period, so "noticing" measured nearly a full period (about 100 ms) on almost every edit, or nearly none, depending on how long the step before took. `--pause-jitter-ms N` (new in `benchmarks/watch-vs-shadow.bb`) adds a random 0 to N ms to each pause, so that saves land at random points of either tool's period. Both methods were run:

- **Fixed pause, as in the notes above:** four invocations for the fix, in the orders light, shadow then shadow, light, twice (5 timed edits per session, 20 per cell); two baseline sessions (10 per cell).
- **Random pause, `--pause-jitter-ms 1000 --edits 10`:** two invocations for the fix in the two orders (20 timed edits per cell) and one baseline session (10 per cell).

Milliseconds from the save, median (range). Only the rows this fix moves are shown; shadow-cljs's rows come from the same invocations.

| Scenario | Pause | Tool | Output written | Reload message | New code running | Tool's own step time | CPU user + system per edit, s | Load average |
|---|---|---|---:|---:|---:|---:|---:|---:|
| view (leaf): app.baga.views | fixed | light-cljs, master `1579f4fc` | 139 (109-168) | 185 (156-210) | 186 (165-217) | 67 (63-69) | 0.10 (0.09-0.12) | 9.2 (8.4-10.2) |
| view (leaf): app.baga.views | fixed | light-cljs, `fa9856b4` | 113 (21-136) | 117 (25-140) | 118 (26-142) | **13 (12-16)** | 0.03 (0.02-0.04) | 6.2 (4.4-8.2) |
| view (leaf): app.baga.views | fixed | shadow-cljs | 2900 (2424-3197) | 2955 (2477-3257) | 2968 (2488-3270) | 770 (740-1040) | 1.25 (0.99-1.82) | 12.5 (10.8-14.7) |
| view (leaf): app.baga.views | random | light-cljs, master `1579f4fc` | 109 (46-175) | 156 (92-222) | 158 (94-224) | 70 (68-76) | 0.11 (0.10-0.13) | 5.4 (5.1-5.6) |
| view (leaf): app.baga.views | random | light-cljs, `fa9856b4` | 76 (40-125) | 81 (46-130) | **83 (47-139)** | **13 (12-16)** | 0.03 (0.01-0.04) | 5.9 (4.8-7.2) |
| view (leaf): app.baga.views | random | shadow-cljs | 2314 (1954-2797) | 2376 (2011-2852) | 2388 (2024-2863) | 770 (760-790) | 1.24 (1.02-1.66) | 9.4 (7.0-12.3) |
| widely required: workload.common | fixed | light-cljs, master `1579f4fc` | 152 (134-176) | 196 (180-219) | 199 (180-222) | 59 (57-65) | 0.11 (0.10-0.12) | 8.2 (7.5-9.1) |
| widely required: workload.common | fixed | light-cljs, `fa9856b4` | 112 (21-130) | 116 (30-134) | 117 (32-135) | **6 (5-6)** | 0.02 (0.01-0.03) | 5.9 (4.2-7.3) |
| widely required: workload.common | fixed | shadow-cljs | 5081 (3603-5933) | 5142 (3662-6006) | 6211 (4750-7114) | 3475 (3170-4130) | 51.95 (48.22-59.94) | 11.8 (10.4-13.8) |
| widely required: workload.common | random | light-cljs, master `1579f4fc` | 121 (76-168) | 167 (123-212) | 168 (125-215) | 61 (59-62) | 0.10 (0.08-0.12) | 4.9 (4.6-4.9) |
| widely required: workload.common | random | light-cljs, `fa9856b4` | 72 (23-111) | 76 (27-116) | **77 (27-117)** | **6 (5-10)** | 0.02 (0.00-0.03) | 5.6 (4.3-9.5) |
| widely required: workload.common | random | shadow-cljs | 5279 (3776-6769) | 5339 (3843-6833) | 6463 (4922-7991) | 3830 (3500-5100) | 55.28 (49.75-65.56) | 9.3 (6.1-16.3) |
| macro: workload.sobis/hainfoulvei | fixed | light-cljs, master `1579f4fc` | 276 (261-404) | 320 (307-449) | 327 (313-455) | 278 (269-296) | 0.38 (0.35-0.40) | 7.5 (6.6-8.7) |
| macro: workload.sobis/hainfoulvei | fixed | light-cljs, `fa9856b4` | 228 (186-305) | 235 (197-310) | 241 (203-317) | **165 (157-181)** | 0.18 (0.18-0.20) | 5.4 (4.1-6.9) |
| macro: workload.sobis/hainfoulvei | fixed | shadow-cljs | 856 (360-902) | 913 (414-965) | 957 (459-1009) | 280 (250-390) | 1.38 (1.12-2.24) | 11.8 (10.1-15.2) |
| macro: workload.sobis/hainfoulvei | random | light-cljs, master `1579f4fc` | 357 (281-412) | 403 (327-458) | 411 (334-466) | 302 (292-319) | 0.39 (0.38-0.43) | 5.3 (4.4-5.4) |
| macro: workload.sobis/hainfoulvei | random | light-cljs, `fa9856b4` | 242 (183-289) | 247 (189-294) | **255 (196-302)** | **174 (159-186)** | 0.20 (0.18-0.21) | 6.8 (3.7-10.3) |
| macro: workload.sobis/hainfoulvei | random | shadow-cljs | 2179 (519-2638) | 2236 (579-2696) | 2281 (625-2743) | 300 (270-370) | 1.40 (1.22-2.05) | 10.0 (7.6-13.6) |

Where the time to new code running goes, medians in milliseconds, light-cljs only (shadow-cljs's split is as in the notes above):

| Scenario | Pause | Binary | Noticing and other | Step | Output to message | Message to running |
|---|---|---|---:|---:|---:|---:|
| view (leaf) | random | master `1579f4fc` | 39 | 70 | 47 | 2 |
| view (leaf) | random | `fa9856b4` | 62 | 13 | 4 | 2 |
| widely required | random | master `1579f4fc` | 59 | 61 | 45 | 1 |
| widely required | random | `fa9856b4` | 67 | 6 | 4 | 1 |
| macro | random | master `1579f4fc` | 55 | 302 | 47 | 7 |
| macro | random | `fa9856b4` | 71 | 174 | 5 | 7 |

- **Steps:** 67-70 to 13 ms for the view and 59-61 to 6 ms for `workload.common`, and the 45 ms between the files written and the push is down to 4 or 5. CPU time per edit fell from 0.10 to 0.02 or 0.03 s. The macro edit's step for 9 namespaces fell from 278-302 to 165-174 ms; what is left is analyzing and expanding those namespaces.
- **Save to new code running, random pause:** 83 ms for the view and 77 ms for `workload.common`, against master's 158 and 168 ms. docs/dev-workflow.md's target, under 100 ms from save to updated page, now holds on the 200k app at the median; the slowest edits (139 and 117 ms) still miss it. With the fixed pause the medians are 118 and 117 ms, since nearly every save there waited a full poll period. What is left is mostly noticing the save: half the 100 ms poll on average, plus the poll's own scan. A `sample` of a running watch puts that scan at about 11 ms (it was about 39 ms on master), nearly all of it the `stat` of each of the 3,100 files and 750 directories.
- **shadow-cljs and the pause:** with the random pause, shadow-cljs's macro edit took 2,281 ms to new code running, against 957 ms with the fixed pause. Its noticing went from 566 to 1,879 ms, so the fixed pause had happened to favour its watcher on that scenario. Recorded as found; its other scenarios moved less (view 2,968 to 2,388 ms, `workload.common` 6,211 to 6,463 ms).
- **Startup and page load** are unchanged: 18.6 to 20.0 s to the first build and 4.9 to 5.3 s page load, against the baseline's 19.5 to 20.1 s and 4.9 to 5.1 s.
- **Still whole-program per step, all small:** the poll's `stat` of every file (polling is the design, docs/dev-workflow.md item 18; an operating-system file watcher would be a new subsystem); writing `manifest.json` and `index.js` and the server's copy of the build (about 1 ms each); and the first macro edit of a session, which registers every namespace with the macro engine once (about 90 ms, that edit only).
- No console errors or uncaught exceptions on either side. The raw runs are in `research/shadow-credibility/watch-1001-fa9856b4/`.
