# How fast the output runs: light-cljs against shadow-cljs

This answers "output was judged by size only" (shadow-comparison-credibility
group, light-cljs-975). Every earlier comparison measured how big each
compiler's release output is. This one measures how fast that output runs: the
generated projects in Node, and re-frame TodoMVC in headless Chromium.

Both compilers build for release: light-cljs at `:advanced`, its default, and
shadow-cljs with Closure Compiler's advanced optimizations, its default.

## Commands

```sh
# Generated projects: build both outputs (or let compare.bb build them), then run them.
"$LIGHT" release bench                       # in the project directory
clojure -M:shadow release bench
bb benchmarks/generated-project/compare.bb PROJECT --light "$LIGHT" --runs 10 \
  --runtime-only --runtime 1,10,40

# TodoMVC: port, build both and time them in headless Chromium.
bb benchmarks/re-frame.bb setup
bb benchmarks/re-frame.bb port todomvc --porter research/shadow-credibility/shadow-port-tool-983 --light "$LIGHT"
bb benchmarks/re-frame.bb runtime todomvc --runs 10 --light "$LIGHT"
```

- **Generated projects:** `compare.bb --runtime COUNTS` runs `node run-light.mjs 42 N`
  and `node out/shadow.js 42 N` for each iteration count N.
  - The compilers alternate.
  - No run waits for the machine: the 1-minute load average before each run is
    recorded instead.
  - Every run's printed checksum must equal `bb oracle.bb 42 N`, or the run fails.
  - **First output** is the time from starting the process to the first byte it
    prints, which is the checksum. **Wall time** runs to its exit. Both use a
    nanosecond clock.
  - CPU time and peak memory come from `/usr/bin/time -l`.
- **TodoMVC:** `re-frame.bb runtime` runs `benchmarks/re-frame-runtime.mjs` once
  per run under `/usr/bin/time -l`, alternating the compilers.
  - Every run starts a fresh browser, so no compiled-code cache carries over.
  - The page's own DOM events drive the app, as a user's input would. Each
    step's time runs from the first event to the frame after the DOM shows the
    step's result, measured with `performance.now()` in the page.
  - The Chrome DevTools Protocol's Performance domain gives each step's script
    time, and the JavaScript heap after a forced garbage collection.
  - The whole run's CPU time covers Node and the browser processes. Its peak
    memory is the largest single process's.

## 2026-11-14

### Setup

- **light-cljs:** `67b1bbec`, an immutable copy of the release binary at
  `research/shadow-credibility/bin/light-cljs-67b1bbec`. This is the V8 build.
- **shadow-cljs:** 3.5.3 with Closure Compiler v20260907.
  - The generated projects use `:target :node-script`.
  - TodoMVC is the unmodified upstream example, with only the shadow-cljs pin
    raised to 3.5.3.
- **Runtimes:** Node v26.12.1. Headless Chromium 156.0.8078.4 (Playwright
  revision 1248) through playwright-core 1.64.1.
- **Machine:** macOS 15.7.3, Apple M3 Ultra virtual machine with 24 CPUs and
  32 GiB. **Unreserved:** peer workers' builds ran throughout, and the 1-minute
  load average before a run was 4.4 to 17.1. Each table gives its own range.
- **Projects:** private copies of the seed-42 projects in
  `research/shadow-credibility/projects`. Both outputs were rebuilt from scratch
  with the binary above and shadow-cljs 3.5.3.
  - **workload 200k:** light-cljs 3,438,818 bytes, shadow-cljs 4,067,637 bytes.
  - **workload 800k:** light-cljs 13,345,416 bytes, shadow-cljs 15,710,764 bytes.
  - **re-frame-shaped 200k app:** light-cljs 6,215,360 bytes, shadow-cljs
    6,963,402 bytes. This app mounts in jsdom.
- **TodoMVC:** ported with `shadow-port-tool-983`. light-cljs's module is bundled
  with React by esbuild 0.25.10 (minified IIFE). shadow-cljs's `client.js` is
  served as written. Both builds pass all of 973's behaviour checks
  (`bb benchmarks/re-frame.bb browser todomvc`).
- **Repetitions:** 10 runs of every measurement. None was cut.
  - The tables are from the second of two sessions. The first had no
    first-output timing and no whole-run CPU time.
  - The first session's ratios agreed with the second's: steady state 5.92,
    4.36 and 5.30 times; TodoMVC's add step 10.98 times.

### Summary

**light-cljs's output is slower everywhere measured.** Every gap is above the
pellet's 10% threshold.

| Workload | light/shadow | light-cljs, shadow-cljs |
| --- | --- | --- |
| workload 200k, first output at 1 iteration | 1.50 | 0.462 s, 0.308 s |
| workload 200k, steady state per iteration | 4.76 | 123 ms, 26 ms |
| workload 800k, first output at 1 iteration | 1.51 | 1.683 s, 1.112 s |
| workload 800k, steady state per iteration | 4.73 | 522 ms, 110 ms |
| re-frame-shaped 200k app, first output (mount and 1 event sequence) | 3.13 | 4.415 s, 1.410 s |
| re-frame-shaped 200k app, steady state per iteration | 5.67 | 1,640 ms, 289 ms |
| TodoMVC, page load to interactive | 1.41 | 48.8 ms, 34.7 ms |
| TodoMVC, add 1,000 todos | 10.92 | 10,328 ms, 946 ms |
| TodoMVC, toggle all | 3.39 | 149.8 ms, 44.2 ms |
| TodoMVC, edit one in a list of 1,000 | 1.37 | 83.9 ms, 61.3 ms |
| TodoMVC, clear completed | 1.42 | 35.9 ms, 25.3 ms |
| TodoMVC, JavaScript heap after load | 1.26 | 3.43 MB, 2.74 MB |
| TodoMVC, JavaScript heap with 1,000 todos | 1.87 | 14.06 MB, 7.52 MB |

- **Startup** (loading and initializing the program) is 1.2 to 2.5 times
  shadow-cljs's.
  - This is estimated from the intercept: wall time at one iteration minus one
    steady-state iteration.
  - workload 200k: 0.35 s against 0.29 s.
  - workload 800k: 1.18 s against 1.01 s.
  - re-frame-shaped app: 2.80 s against 1.14 s.
  - Startup's share of the slowdown is small. Most of first output's 1.5 times
    is the one iteration it includes.
- **Steady-state code** runs about 5 times slower. That is the main finding.
- **TodoMVC** in a browser shows the same picture.
  - The gap is small on a step that does little work, such as an edit or a
    page load: React and the browser dominate both builds there.
  - It reaches 11 times on adding 1,000 todos, where the app's own ClojureScript
    does the work. Each add prints every todo into localStorage, so the step is
    quadratic in that work.
- **Peak memory** is 1.3 to 1.8 times shadow-cljs's in Node. The JavaScript
  heap is 1.3 to 1.9 times in the browser.

### Generated workload, 200k lines

| Iterations | Compiler | First output | Wall time | User CPU | System CPU | Peak memory | 1-minute load before |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | light-cljs | 0.462 s (0.456-0.546) | 0.469 s (0.462-0.552) | 0.58 s (0.57-0.65) | 0.04 s (0.04-0.05) | 0.193 GB (0.176-0.201) | 4.40-4.56 |
| 1 | shadow-cljs | 0.308 s (0.303-0.316) | 0.313 s (0.308-0.321) | 0.36 s (0.35-0.37) | 0.02 s (0.02-0.03) | 0.120 GB (0.119-0.124) | 4.40-4.56 |
| 10 | light-cljs | 1.591 s (1.566-2.221) | 1.601 s (1.575-2.233) | 2.04 s (2.01-2.53) | 0.08 s (0.07-0.44) | 0.269 GB (0.257-0.279) | 4.40-4.61 |
| 10 | shadow-cljs | 0.575 s (0.563-0.686) | 0.582 s (0.570-0.696) | 0.79 s (0.77-0.95) | 0.05 s (0.05-0.08) | 0.205 GB (0.196-0.215) | 4.40-4.61 |
| 40 | light-cljs | 5.239 s (5.160-5.417) | 5.253 s (5.174-5.434) | 6.14 s (6.07-6.33) | 0.16 s (0.15-0.19) | 0.401 GB (0.394-0.410) | 4.70-5.35 |
| 40 | shadow-cljs | 1.308 s (1.282-1.343) | 1.318 s (1.292-1.352) | 1.84 s (1.80-1.88) | 0.10 s (0.09-0.10) | 0.270 GB (0.264-0.274) | 4.75-5.35 |

| Iterations | light/shadow first output | light/shadow wall time | light/shadow CPU (user + system) | light/shadow peak memory |
| --- | --- | --- | --- | --- |
| 1 | 1.50 | 1.50 | 1.63 | 1.60 |
| 10 | 2.77 | 2.75 | 2.52 | 1.32 |
| 40 | 4.01 | 3.99 | 3.26 | 1.48 |

Steady state, from the median wall times at 1 and 40 iterations: light-cljs 122.7 ms per iteration, shadow-cljs 25.8 ms per iteration (light/shadow 4.76).

### Generated workload, 800k lines

| Iterations | Compiler | First output | Wall time | User CPU | System CPU | Peak memory | 1-minute load before |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | light-cljs | 1.683 s (1.649-1.844) | 1.697 s (1.661-1.857) | 2.03 s (1.98-2.25) | 0.10 s (0.10-0.16) | 0.452 GB (0.396-0.482) | 4.81-5.83 |
| 1 | shadow-cljs | 1.112 s (1.083-1.190) | 1.122 s (1.093-1.202) | 1.28 s (1.24-1.36) | 0.07 s (0.07-0.09) | 0.337 GB (0.303-0.344) | 4.81-5.72 |
| 4 | light-cljs | 3.031 s (2.960-3.389) | 3.046 s (2.975-3.407) | 3.74 s (3.67-4.12) | 0.13 s (0.11-0.25) | 0.516 GB (0.460-0.523) | 4.75-6.00 |
| 4 | shadow-cljs | 1.391 s (1.336-1.495) | 1.402 s (1.348-1.506) | 1.73 s (1.68-1.87) | 0.09 s (0.08-0.12) | 0.402 GB (0.351-0.405) | 4.85-6.00 |
| 16 | light-cljs | 9.486 s (8.659-12.939) | 9.521 s (8.685-12.980) | 10.85 s (10.06-12.99) | 0.41 s (0.27-0.67) | 0.802 GB (0.767-0.893) | 8.87-12.92 |
| 16 | shadow-cljs | 2.758 s (2.667-3.603) | 2.777 s (2.686-3.623) | 3.69 s (3.60-4.24) | 0.20 s (0.16-0.41) | 0.562 GB (0.524-0.584) | 9.89-13.52 |

| Iterations | light/shadow first output | light/shadow wall time | light/shadow CPU (user + system) | light/shadow peak memory |
| --- | --- | --- | --- | --- |
| 1 | 1.51 | 1.51 | 1.58 | 1.34 |
| 4 | 2.18 | 2.17 | 2.13 | 1.28 |
| 16 | 3.44 | 3.43 | 2.93 | 1.43 |

Steady state, from the median wall times at 1 and 16 iterations: light-cljs 521.6 ms per iteration, shadow-cljs 110.3 ms per iteration (light/shadow 4.73).

### Generated re-frame-shaped app, 200k lines (jsdom in Node)

One iteration mounts the app and runs the seeded event sequence. The program
prints app-db's checksum and the rendered markup's, and both must match the
oracle.

| Iterations | Compiler | First output | Wall time | User CPU | System CPU | Peak memory | 1-minute load before |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | light-cljs | 4.415 s (4.098-6.309) | 4.437 s (4.177-6.338) | 5.52 s (5.39-6.77) | 0.25 s (0.19-0.50) | 0.689 GB (0.641-0.711) | 10.06-17.11 |
| 1 | shadow-cljs | 1.410 s (1.289-2.102) | 1.428 s (1.304-2.126) | 2.17 s (1.98-2.65) | 0.14 s (0.11-0.24) | 0.464 GB (0.457-0.492) | 10.88-17.11 |
| 4 | light-cljs | 8.997 s (8.862-10.417) | 9.023 s (8.889-10.462) | 10.98 s (10.74-11.69) | 0.30 s (0.26-0.61) | 0.776 GB (0.724-0.972) | 7.92-13.76 |
| 4 | shadow-cljs | 2.346 s (2.275-2.997) | 2.365 s (2.293-3.018) | 3.42 s (3.30-3.66) | 0.19 s (0.17-0.36) | 0.571 GB (0.561-0.600) | 7.92-13.76 |
| 10 | light-cljs | 19.138 s (17.903-20.868) | 19.199 s (17.960-20.938) | 21.03 s (20.39-21.85) | 0.68 s (0.57-1.03) | 1.313 GB (1.278-1.331) | 5.79-10.47 |
| 10 | shadow-cljs | 4.002 s (3.835-4.677) | 4.030 s (3.862-4.702) | 5.44 s (5.22-5.92) | 0.30 s (0.24-0.49) | 0.740 GB (0.729-0.747) | 6.38-10.86 |

| Iterations | light/shadow first output | light/shadow wall time | light/shadow CPU (user + system) | light/shadow peak memory |
| --- | --- | --- | --- | --- |
| 1 | 3.13 | 3.11 | 2.50 | 1.48 |
| 4 | 3.83 | 3.82 | 3.13 | 1.36 |
| 10 | 4.78 | 4.76 | 3.78 | 1.78 |

Steady state, from the median wall times at 1 and 10 iterations: light-cljs 1640.2 ms per iteration, shadow-cljs 289.2 ms per iteration (light/shadow 5.67).

### re-frame TodoMVC in headless Chromium


| Step | shadow-cljs ms | light-cljs ms | light/shadow | shadow-cljs script ms | light-cljs script ms |
|---|---:|---:|---:|---:|---:|
| page load to interactive | 34.65 (31.20-47.40) | 48.75 (47.10-62.80) | 1.41 | 14.40 (13.49-15.47) | 28.25 (26.94-30.69) |
| add 1,000 todos | 946.05 (926.00-1033.70) | 10327.75 (10131.30-10870.40) | 10.92 | 905.63 (885.65-985.30) | 10287.08 (10092.28-10828.03) |
| toggle all | 44.15 (41.80-55.10) | 149.80 (143.70-175.60) | 3.39 | 21.95 (21.16-24.72) | 127.42 (122.67-144.89) |
| edit one in a list of 1,000 | 61.30 (57.90-74.20) | 83.85 (78.40-100.10) | 1.37 | 8.14 (7.42-9.05) | 47.38 (45.60-54.35) |
| clear completed | 25.25 (19.10-28.70) | 35.85 (34.70-41.90) | 1.42 | 7.05 (6.40-7.81) | 30.06 (29.18-36.39) |
| JavaScript heap after load, MB | 2.74 (2.73-2.80) | 3.43 (3.43-3.44) | 1.26 | | |
| JavaScript heap with 1,000 todos, MB | 7.52 (7.52-7.53) | 14.06 (14.05-18.31) | 1.87 | | |
| whole run (fresh browser, load, every step, close), wall s | 1.54 (1.46-1.85) | 11.09 (10.86-11.62) | 7.23 | | |
| whole run, user CPU s (Node and the browser processes) | 1.78 (1.71-1.98) | 11.83 (11.59-12.53) | 6.66 | | |
| whole run, system CPU s | 0.41 (0.30-0.80) | 0.56 (0.47-0.70) | 1.35 | | |
| whole run, peak memory of the largest process, MB | 258.70 (255.95-266.73) | 360.86 (344.80-367.90) | 1.39 | | |
| 1-minute load average before the run | 10.94 (9.62-12.84) | 11.06 (9.62-12.84) | | | |

What each step does:
- **Page load to interactive:** navigation start to the moment `#new-todo` is
  in the DOM.
- **Add 1,000 todos:** sets the input's value and sends a keydown Enter, 1,000
  times. It ends at the frame after the 1,000th item renders.
- **Toggle all:** clicks `#toggle-all`. It ends when every item is completed.
- **Edit one in a list of 1,000:** double-clicks the 500th label, waits for the
  edit input, then types and sends Enter. It ends when the label shows the new
  text.
- **Clear completed:** clicks `#clear-completed`. It ends when the list is
  empty.
- **Script time:** the Performance domain's ScriptDuration during the step.

The edit step includes at least one frame of waiting for reagent to show the
edit input. That waiting is why its wall time is well above its script time on
both builds.

### Where light-cljs's output spends its time

Every scenario that is more than 10% slower was profiled with V8's sampling
profiler:
- **Node:** `node --cpu-prof` on the generated programs: workload 200k at 30
  iterations, workload 800k at 8, the re-frame-shaped app at 4.
- **Chromium:** the Chrome DevTools Protocol Profiler (100 µs sampling) around
  TodoMVC's page load and each of its four steps.

The light-cljs builds for profiling were the same release builds with
`--pseudo-names`, which keeps the runtime's function names. TodoMVC's was
bundled by esbuild without minifying.

The table gives self time by cause, as a share of all samples. "Brand stores"
is the stores' own functions plus the anonymous `storesPredicate` closure,
which the profile lists apart (the second figure).

| Cause (runtime functions) | 200k | 800k | re-frame app | TodoMVC add | toggle all | edit | clear completed |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Protocol checks before core operations (`publicMethod`, `publicDeclared`, `satisfies`, `implementsProtocol`, `coreProtocolImplementation`, `builtinCoreImplementation`, `keyMethod`, `externalMethod`) | 15.1% | 14.7% | 14.9% | 21.0% | 14.4% | 11.7% | 5.1% |
| Brand stores: value-kind tests (`has`/`get`, `canHoldField`; `storesPredicate` closure) | 8.7% + 5.1% | 6.5% + 4.4% | 6.4% + 6.1% | 12.2% + 5.2% | 7.4% + 2.9% | 6.9% + 4.1% | 2.5% + 0% |
| Array maps and map entries (`putEntry`, `copyEntries`, `entryIndex`, `mapEntry`, `makeMap`) | 8.4% | 6.5% | 9.8% | 17.6% | 21.6% | 3.3% | 28.8% |
| Protocol-function dispatchers and variadic calls (`dispatch`, `callable`, `fn`, `apply`) | 4.1% | 3.3% | 1.1% | 2.3% | 0.9% | 0.5% | 0% |
| Loading the module (parse and compile) | 4.3% | 15.6% | 5.3% | - | - | - | - |
| Engine or browser native code, and idle | 1.3% | 1.6% | 2.2% | 2.6% | 15.1% | 45.3% | 34.8% |

Notes on the table:
- The edit and clear-completed steps took 125 and 63 samples, so their
  percentages are coarse.
- At 800k, the module-load share is inflated by the larger pseudo-names output.

**TodoMVC's page load** has no JavaScript hot spot. Of its samples:
- 70% are the browser's own work or idle;
- the light-cljs runtime's top-level setup is 12% inclusive, and React's first
  render about 10%.

The 14 ms of extra script time on load is compiling and initializing a runtime
that sets up its stores and protocol tables eagerly. It is spread over many
small functions.

What the profiles show:

1. **A protocol check runs in front of every core sequence operation.**
   - `seq`, `first`, `next` and the other core operations are wrapped by
     `behindProtocol`. Once a program contains any protocol code, which every
     real program does, each call first asks `protocolHooks.publicMethod`
     whether the value implements the core protocol.
   - That lookup goes through `satisfies` and the brand stores before the
     runtime's own branch for its vector, map or seq runs.
   - In TodoMVC's add step, `publicMethod` and its callees are 33.6% of
     inclusive time. In workload 200k they are 19.2%.
   - Closure-compiled ClojureScript makes the same decision with one property
     or bitmask read on the value.
2. **Value kinds are tested through private-field brand stores.**
   - A test such as "is this seqable" (`storesPredicate`) probes up to eight
     stores in turn, each with a `#s in value` check.
   - These tests are a large part of the protocol checks above, and they also
     run on their own.
3. **Small maps are expensive to build and to search.**
   - Each array-map `assoc` or `dissoc` copies the entries (`copyEntries`).
     Clear completed is 28.8% this.
   - Each new entry is a frozen object with a hash getter closure.
   - `entryIndex` searches with `findIndex` and a closure, and compares keyword
     keys with `equalPair` rather than identity first.
   - Every `seq` over a map builds map entries with `Object.setPrototypeOf` on
     a fresh vector plus a `WeakSet` registration. `mapEntry` alone is 12.8%
     of TodoMVC's add step.
   - The add step is dominated by `(str todos)` of a sorted map, which
     TodoMVC writes to localStorage after every event: 31% of inclusive time.
     That is a map walk.
4. **Smaller costs:**
   - protocol-function dispatchers that collect rest arguments (`...args`,
     `args.slice`) on every call;
   - `ex-info` stack capture in the workload;
   - loading the module, a little more than shadow-cljs's at the same size.
5. **Not a cause: Node's module-type warning.**
   - Node warns that `out/light/workload.main.js` has no `"type": "module"`
     and is parsed twice.
   - Adding that marker changed workload 200k's one-iteration time by 1 ms
     (482 against 481 ms, 9 runs each), so it is not a cause.

**Peak memory** follows from the extra objects above: frozen entry objects,
map-entry vectors, and brand-store payloads.

The three largest causes are filed as pellets in this group:
- light-cljs-988: protocol checks in front of core operations;
- light-cljs-989: small maps and map entries;
- light-cljs-990: the brand stores, a design question for the user first.

The smaller costs (dispatchers' rest arguments, runtime setup at load) are
recorded here and not filed.

The group's rule applies: where the remedy is a design change, such as
replacing the private-field brand stores or how core operations find protocol
implementations, it goes to the user before anyone designs it. No remedy is
designed here.
