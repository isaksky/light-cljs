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

## 2026-11-14: protocol lookup on the runtime's own values (light-cljs-988)

**Change.** The protocol lookup in front of the core operations
(`publicMethod`) now answers in one step for one of the runtime's
collections or seqs that declares no core protocol itself:
- It reads the protocol's mask bit and marker first, as before.
- Brand checks then name the payload store that holds the value. They read
  no property of it, so a host value's getters and Proxy traps never run.
- The native test's answer for that store is cached for `ISeqable`, `ISeq`,
  `INext`, `IMeta` and `ILookup`.

Before, the value was tested against every brand store up to three times
(the native test, then `satisfies` or `externalMethod`, then
`publicDeclared`). Nothing else changed:
- an implementation from `extend-type`, `extend-protocol` or `specify!` is
  still found;
- so is a method JavaScript writes on a value or a prototype, with or
  without the protocol's marker;
- `extend-type object` and `default` still give the runtime's collections
  nothing, and still reach host values.

`builtin_types::sequence_protocol_extensions_to_runtime_types_still_win`
checks this against the outputs from before the change.

**Setup.** The kit and machine of the 2026-11-14 section above, with two
differences:
- before is `light-cljs-67b1bbec` (975's binary, whose runtime equals
  master's before this change) and after is `light-cljs-32cb0373`;
- before and after ran back to back on each workload, each alternating with
  shadow-cljs.

10 runs of each. **Unreserved:** peer workers ran throughout. The 1-minute
load average before a run was 2.2 to 4.7. Raw results are in
`research/shadow-credibility/runtime-988`.

Only the rows that moved are shown. Steady state is computed as in the
tables above. Ranges are in brackets.

| Workload | light-cljs before | light-cljs after | change | light/shadow before | light/shadow after |
| --- | ---: | ---: | ---: | ---: | ---: |
| workload 200k, steady state per iteration | 119.7 ms | 95.7 ms | -20% | 5.02 | 4.15 |
| workload 200k, wall time at 40 iterations | 5.155 s (4.882-5.355) | 4.163 s (4.137-4.186) | -19% | 4.12 | 3.47 |
| workload 800k, steady state per iteration | 468.9 ms | 392.7 ms | -16% | 4.35 | 3.79 |
| workload 800k, wall time at 16 iterations | 8.554 s (8.369-8.854) | 7.314 s (7.197-7.522) | -14% | 3.27 | 2.86 |
| re-frame-shaped 200k app, steady state per iteration | 1,347 ms | 1,162 ms | -14% | 5.42 | 4.77 |
| re-frame-shaped 200k app, wall time at 10 iterations | 16.245 s (16.063-16.734) | 14.076 s (13.908-14.324) | -13% | 4.58 | 4.01 |
| re-frame-shaped 200k app, first output | 4.050 s (4.011-4.180) | 3.535 s (3.503-3.657) | -13% | 3.12 | 2.72 |
| TodoMVC, add 1,000 todos | 9,768 ms (9,625-10,434) | 8,645 ms (8,589-9,360) | -11% | 10.67 | 9.58 |
| TodoMVC, toggle all | 138.1 ms (137.3-144.1) | 127.6 ms (123.8-138.5) | -8% | 2.77 | 2.68 |
| TodoMVC, edit one in a list of 1,000 | 85.2 ms (75.4-93.0) | 75.0 ms (71.4-80.9) | -12% | 1.39 | 1.21 |
| TodoMVC, whole run wall time | 10.43 s (10.29-11.09) | 9.29 s (9.22-10.04) | -11% | 7.25 | 6.52 |
| TodoMVC, whole run user CPU | 11.09 s (10.94-11.82) | 9.94 s (9.87-10.88) | -10% | 6.68 | 6.02 |

- **Unchanged:** peak memory, the JavaScript heap, startup, TodoMVC's page
  load and its clear-completed step all stayed within their ranges.
- **Output size:** each output grows by 339 bytes before gzip.
- **Where the gain comes from.** In TodoMVC's add step, most of the lookups
  are on the runtime's own values:
  - `nth` of map entries;
  - lookups and `contains?` on array maps;
  - `first` and `next` of indexed seqs and of the seqs of sorted maps.
  All of these now take the one step.
- **What still takes the full test:** keywords and numbers being printed,
  and the empty list. These are not in a payload store.
- **What is left of the protocol checks.** A sampling profile of workload
  200k at 30 iterations still puts 10% of inclusive time in `publicMethod`
  (5% self, and 3% in the store test): the mask, marker and slot reads that
  observe extensions. A further 5% inclusive is in `implementsProtocol`, which the `satisfies` and
  `implements` hooks call; this change leaves those hooks alone.
- **Not changed here:** the brand stores themselves (light-cljs-990) and
  small maps (light-cljs-989). Closures are a large share of what remains:
  the profile's anonymous functions and the operations' named wrappers
  (`Object.defineProperty.value`) are a quarter of workload 200k's self
  time.

## 2026-11-14: small maps and map entries (light-cljs-989)

**Change.** Three of the small-map costs named in "Where light-cljs's output
spends its time" go:
- An array map's entry is an instance of one class, `ArrayMapEntry`, whose
  `hash` getter keeps the key's hash in a field. Before, each entry was a
  frozen object with a getter closure of its own. Map literal shapes build
  the same class, so all array-map entries have one shape.
- `entryIndex` loops over the entries instead of calling `findIndex` with a
  closure. A keyword key still compares with each entry as `equalPair`
  does: identity, then a read of its own `IEquiv` slot. Only the lookup in
  `extend-type`'s tables, which reads nothing of the key, is made once per
  search, and again after any `-equiv` runs. Before, that lookup ran for
  every entry the key did not match.
- A map entry is made with `Object.create` of its type's prototype instead
  of `Object.setPrototypeOf` on a fresh vector.

Nothing observable changes. Two tests in `builtin_types`, whose expected
output is what master's runtime gives, check:
- key comparison, including equal keywords that are not identical;
- the order of a key's `-equiv` and `-hash` calls, and reads of a keyword
  key's `IEquiv` slot through a getter;
- that a search covers the entries there were when it started, though a
  key's `-equiv` adds one to a transient;
- that `assoc` keeps the existing key, and the order of keys;
- `map-entry?`, metadata and identity of the entries seqs give.

**Setup.** The kit and machine of the 2026-11-14 section above, with these
differences:
- before is `light-cljs-32cb0373` (988's binary, whose runtime equals
  master's `1aee1a66`) and after is `light-cljs-c755984d`;
- each output was built with its binary just before its runs;
- before and after ran back to back on each workload, each alternating with
  shadow-cljs.

10 runs of each. **Unreserved:** peer workers ran builds and test suites
throughout. The 1-minute load average before a run was 3.8 to 16.5, and it
differed between before and after (workload 200k: 7.5 to 16.5 before, 3.8
to 7.3 after). The light/shadow ratios, whose two sides ran alternately, are
the fairer comparison. Raw results are in
`research/shadow-credibility/runtime-989/c755984d`. An earlier pass, with a
build that also made the keyword's slot read once per search
(`light-cljs-5467ea43`, in `runtime-989`), gave changes within a few
percent of these.

Only the rows that moved are shown. Ranges are in brackets.

| Workload | light-cljs before | light-cljs after | change | light/shadow before | light/shadow after |
| --- | ---: | ---: | ---: | ---: | ---: |
| workload 200k, steady state per iteration | 101.1 ms | 91.4 ms | -10% | 4.00 | 3.74 |
| workload 200k, wall time at 40 iterations | 4.407 s (4.259-4.867) | 4.012 s (3.910-4.102) | -9% | 3.38 | 3.14 |
| workload 200k, peak memory at 40 iterations | 0.417 GB (0.406-0.432) | 0.322 GB (0.317-0.327) | -23% | 1.55 | 1.20 |
| workload 800k, steady state per iteration | 428.2 ms | 392.6 ms | -8% | 3.72 | 3.28 |
| workload 800k, wall time at 16 iterations | 7.952 s (7.697-12.132) | 7.359 s (7.259-7.499) | -7% | 2.86 | 2.59 |
| workload 800k, peak memory at 16 iterations | 0.794 GB (0.762-0.882) | 0.662 GB (0.614-0.664) | -17% | 1.45 | 1.17 |
| re-frame-shaped 200k app, steady state per iteration | 1,419 ms | 1,012 ms | -29% | 4.40 | 3.76 |
| re-frame-shaped 200k app, wall time at 10 iterations | 16.697 s (14.988-19.380) | 12.713 s (12.220-13.107) | -24% | 3.88 | 3.22 |
| re-frame-shaped 200k app, peak memory at 10 iterations | 1.280 GB (1.127-1.349) | 0.767 GB (0.718-0.796) | -40% | 1.75 | 1.05 |
| TodoMVC, add 1,000 todos | 9,255 ms (8,974-9,932) | 8,551 ms (8,469-9,116) | -8% | 9.82 | 8.98 |
| TodoMVC, JavaScript heap with 1,000 todos | 14.06 MB (14.06-18.26) | 13.56 MB (13.56-13.56) | -4% | 1.87 | 1.80 |
| TodoMVC, whole run wall time | 9.97 s (9.67-10.63) | 9.33 s (9.15-10.17) | -6% | 6.60 | 6.09 |
| TodoMVC, whole run user CPU | 10.66 s (10.32-11.39) | 10.00 s (9.83-10.41) | -6% | 6.14 | 5.65 |

- **Unchanged within their ranges:** one iteration of each generated
  program, including the re-frame-shaped app's first output; TodoMVC's page
  load, toggle all, edit and clear completed; and its peak memory. (The
  earlier pass had toggle all and clear completed 15% faster, under a
  1-minute load above 20.)
- **Peak memory** falls most where many small maps live at once: an entry no
  longer holds a closure and its context.
- **Output size:** a program building array maps keeps the entry class and
  the longer search, about 310 bytes at advanced (`map-only` 8,066 to 8,378
  bytes); the size ratchet notes it in tests/support/ratchet_fixtures.rs.

**What is left of the small-map costs.** A sampling profile of TodoMVC's add
step (`--pseudo-names`, as above, with the earlier pass's build) puts `entryIndex`
at 1.8% inclusive, from 5.8%. `mapEntry` is still 12% self:
- The WeakSet registration that brands each map entry (`mapEntries`) is most
  of it: in a micro-benchmark, building an entry costs about 36 ns without
  it and 220 to 350 ns with it. It is a value-kind brand, left to
  light-cljs-990.
- The add step builds so many entries because the map's `seq` builds all of
  them at once. TodoMVC checks its app database with cljs.spec after every
  event, and spec's map conform seqs the sorted map of todos and every todo
  in it. Upstream's array-map and hash-map seqs build an entry only when it
  is read, and a sorted map's tree nodes are its entries. Building entries
  lazily would change what a map's seq holds, a design question filed as
  light-cljs-1002 for the user.

## 2026-11-14: one kind brand for the runtime's stores (light-cljs-990)

**Change.** The user approved testing value kinds with a cheaper tag. The
runtime's private-field stores now share one brand class (`payloads`):
- A value holds its payload, its kind (the number of the store holding it)
  and, for a map or set, its map kind, in three private fields that one
  construction adds.
- A kind test reads the kind once, where it probed each store in turn: up to
  eight stores for "is this seqable", seventeen for printing's "is this a
  runtime value", seven for the protocol lookup in front of the core
  operations. Only the `WeakMap` stores (keywords, symbols, vars, atoms and
  the like) and the tests that hash tries and records widen are still probed
  one by one.
- A map entry is a vector of its own kind. The `WeakSet` registration that
  light-cljs-989 measured at 220 to 350 ns per entry is gone.

**Unforgeability is kept everywhere.** The tag is itself a private field, so
no host object can have, inherit or copy it; a value forged with
`Object.create` of a runtime prototype, or by copying a value's own
properties, is still no language value. Nothing that a host value could
forge decides a kind. `builtin_types::value_kind_predicates_answer_as_the_brand_stores_did`
pins 34 predicates over 54 runtime, host and forged values to master's
answers (docs/compiler.md, "Cons cells, views ...").

**Setup.** The kit and machine of the 2026-11-14 section above, with these
differences:
- before is `light-cljs-f0f5e81b` (master, whose runtime equals master's
  `21e1a111`) and after is `light-cljs-181dc839`;
- after the runs, the Sol review found that an inherited `kindBits` on
  `Object.prototype` could reach the fast path; the landed commit reads only
  a store's own metadata, once when the runtime loads, so the per-call code
  measured here is unchanged;
- each output was built with its binary just before its runs;
- before and after ran back to back on each workload, each alternating with
  shadow-cljs.

10 runs of each. **Unreserved:** peer workers ran builds and test suites
throughout. The 1-minute load average before a run was 3.7 to 16.3, and it
differed between before and after (workload 200k at 40 iterations: 4.1 to
11.6 before, 10.5 to 15.4 after). The light/shadow ratios, whose two sides
ran alternately, are the fairer comparison. Raw results are in
`research/shadow-credibility/runtime-990/181dc839`.

Only TodoMVC's rows moved beyond their ranges. Ranges are in brackets.

| Workload | light-cljs before | light-cljs after | change | light/shadow before | light/shadow after |
| --- | ---: | ---: | ---: | ---: | ---: |
| TodoMVC, add 1,000 todos | 8,813 ms (8,651-9,109) | 6,774 ms (6,639-6,897) | -23% | 8.97 | 7.04 |
| TodoMVC, JavaScript heap with 1,000 todos | 13.45 MB (13.45-13.45) | 9.29 MB (9.29-9.30) | -31% | 1.79 | 1.24 |
| TodoMVC, whole run wall time | 9.61 s (9.34-10.05) | 7.53 s (7.31-8.04) | -22% | 5.63 | 4.95 |
| TodoMVC, whole run user CPU | 10.24 s (10.05-10.59) | 8.21 s (8.01-8.52) | -20% | 5.52 | 4.56 |
| TodoMVC, whole run peak memory | 360 MB (351-362) | 341 MB (340-343) | -5% | 1.39 | 1.32 |

- **The generated programs did not gain**, and two look slightly worse. Their
  medians moved within the ranges, under a higher load for the after runs:
  - workload 200k: 93.3 to 97.0 ms per iteration in steady state
    (light/shadow 3.72 to 3.96); wall time at 40 iterations 4.072 s
    (3.897-5.945) to 4.229 s (3.957-5.045);
  - workload 800k: 381.0 to 409.9 ms per iteration (3.42 to 3.50); wall time
    at 16 iterations 7.327 s (6.657-8.796) to 7.653 s (7.411-8.648);
  - re-frame-shaped 200k app: 1,075 to 1,028 ms per iteration (3.61 to
    3.69); wall time at 10 iterations 12.995 s (12.434-13.683) to 12.788 s
    (12.441-15.259).
  A sampling profile of workload 200k at 30 iterations shows why there is
  little to gain there: after light-cljs-988 its kind tests were spread
  thin, and each store's own `has` and `get` now also compare the kind,
  one more private-field read than before.
- **Unchanged within their ranges:** peak memory and first output of the
  generated programs; TodoMVC's page load, toggle all, edit and clear
  completed.
- **Where TodoMVC's gain comes from.** In a sampling profile of its add step
  (`--pseudo-names`), `mapEntry` falls from 11.2% to 0.3% of the samples,
  the anonymous `storesPredicate` closure from 6.6% to 1.1% inclusive, and
  `printableValue`, the printer's runtime-value test, from 4.7% to 1.2%
  inclusive (the profiled after build was `181dc839`'s runtime before its
  comments were trimmed). The heap falls because the entries no longer sit in a `WeakSet`.
- **Output size:** every program keeping a store keeps the brand class, about
  220 to 390 bytes at advanced (`map-only` 8,378 to 8,689 bytes); the size
  ratchet notes it in tests/support/ratchet_fixtures.rs.

## 2026-11-14: the protocol hooks on the runtime's own values (light-cljs-998)

**Change.** Once a program keeps `extend-type` or `specify!`, the generic
predicates and operations (`vector?`, `seq?`, `map?`, `map-entry?`, `cons`,
`count`, `nth` and others) ask the `satisfies` and `implements` hooks. These
ran the full protocol test on every call. Now a value whose kind tag names
one of the runtime's collection or seq stores takes one step, as
light-cljs-988 did for `publicMethod`:
- the protocol's mask bit, then its marker, are read as before, with the
  same declaration demands;
- otherwise the native test decides, cached per store for `ISeqable`,
  `ISeq`, `INext`, `IMeta` and `ILookup`. The full test gives the same
  answer: the value is the runtime's own, so `extend-type` of a host type,
  `object` or `default` cannot reach it.

Only the kind tag is read, not the stores that are not exact. Probing the
maps' store asks whether a value is a record, which at advanced can walk a
host value's prototype chain and so run a Proxy's `getPrototypeOf` trap.
Hash maps and sets, records, keywords, numbers and host values still take the
full test. `builtin_types::protocol_hooks_answer_runtime_values_as_the_full_lookup`
pins 21 predicates plus `count`, `nth` and `cons` over 47 values to master
`e379d6c5`'s answers. Each of its three programs extends runtime types, host
types, `object` and `default`, uses `specify!`, or writes marked and unmarked
slots and a mask bit from JavaScript. The test also checks how many times a
Proxy is asked anything, at every level.

**Setup.** The kit and machine of the 2026-11-14 section above, with these
differences:
- before is `light-cljs-e379d6c5` (master) and after is `light-cljs-cdf39aae`,
  this change before its commits were squashed. The Sol review then made
  the kind table's entries all its own (an index a host gives
  `Array.prototype` had reached it); the per-call code measured here is
  otherwise the landed one;
- each output was built with its binary just before its runs;
- before and after ran back to back on each workload, each alternating with
  shadow-cljs.

10 runs of each. **Unreserved:** peer workers were active. The 1-minute load
average before a run was 4.0 to 5.5 for workload 200k; for the re-frame-shaped
app it was 3.7 to 10.7 before and 5.6 to 12.6 after; for TodoMVC 5.1 to 11.7
before and 4.0 to 5.9 after. Raw results are in
`research/shadow-credibility/runtime-998/cdf39aae`.

Only the rows that moved are shown. Ranges are in brackets.

| Workload | light-cljs before | light-cljs after | change | light/shadow before | light/shadow after |
| --- | ---: | ---: | ---: | ---: | ---: |
| workload 200k, steady state per iteration | 98.8 ms | 95.2 ms | -4% | 3.94 | 3.75 |
| workload 200k, wall time at 10 iterations | 1.430 s (1.415-1.441) | 1.373 s (1.357-1.398) | -4% | 2.34 | 2.30 |
| workload 200k, wall time at 40 iterations | 4.312 s (4.070-4.374) | 4.167 s (4.100-4.240) | -3% | 3.31 | 3.17 |
| TodoMVC, add 1,000 todos | 6,583 ms (6,497-6,719) | 6,473 ms (6,421-6,515) | -2% | 7.07 | 6.95 |
| TodoMVC, whole run wall time | 7.25 s (7.15-7.41) | 7.12 s (7.06-7.16) | -2% | 4.80 | 4.84 |

- **Small gains.** Only workload 200k at 10 iterations moved clear of its
  range. The other rows' ranges overlap. TodoMVC's whole-run ratio got worse
  because shadow-cljs's runs were faster too, under a lower load.
- **The re-frame-shaped 200k app did not move beyond its ranges.** Its wall
  time at 10 iterations went from 13.101 s (12.577-13.413) to 12.428 s
  (11.420-15.191), and steady state from 1,087 to 998 ms per iteration.
  The after runs had the higher load. shadow-cljs's steady state also fell,
  from 314.5 to 270.6 ms, so light/shadow went from 3.46 to 3.69.
- **Unchanged within their ranges:** first output, peak memory, the
  JavaScript heap, and TodoMVC's other steps.
- **Where the time went.** In a sampling profile of workload 200k at 30
  iterations (`--pseudo-names`), `implementsProtocol` falls from 4.6% to 0.6%
  of inclusive time and `satisfies` from 1.7% to 0.1%. The hooks themselves,
  with the new one-step answer, take 2.7% inclusive. The step reads the mask
  bit and marker that observe extensions.
- **Output size:** advanced output does not change for programs without
  `extend-type` or `specify!`. The readable runtime grows about 1,550 bytes,
  as noted in tests/support/ratchet_fixtures.rs.

## 2026-11-14: the per-operation gate (light-cljs-1007)

**What.** `benchmarks/core-ops/` checks in the output-speed investigation's
micro-benchmark (`research/output-speed/investigation-6ceeb23b`, which is
gitignored) as the gate of the output-speed-parity group. One source times
64 core operations. Both compilers build it for release, and both outputs run
in Node. `benchmarks/core-ops/README.md` has the commands. A later pellet names
the operations it touches and gets their before and after:

```sh
OUT=benchmarks/core-ops/out
bb benchmarks/core-ops/run.bb --light "$BEFORE" --ops kw-small,vec-nth --out $OUT/before
bb benchmarks/core-ops/run.bb --light "$AFTER" --ops kw-small,vec-nth --out $OUT/after \
  --baseline $OUT/before.edn
bb benchmarks/core-ops/run.bb --light "$AFTER" --pseudo-names kw-small   # where the time goes
```

The source keeps the investigation's 62 operations, with two differences:
- **`map-literal-3`** now passes its literal through a JavaScript array before
  counting it. light-cljs had folded the investigation's version to a constant:
  its loop added 3 and never built the map, so the old ratio of 0.06 measured
  nothing. Built for real, a three-key map literal costs light-cljs 10.5 times
  what it costs shadow-cljs.
- **`bit-and` and `aget-bit-and`**, the investigation's later check of the loop
  itself (`results/bitand.txt`), run last, so they do not change the type
  feedback the other 62 see.

**Setup.**
- **light-cljs:** `6ceeb23b` (master), an immutable copy of the release binary
  at `research/shadow-credibility/bin/light-cljs-6ceeb23b`. This is the V8
  build. The investigation ran `light-cljs-c5dad571`, whose runtime and emitter
  equal master's.
- **shadow-cljs:** 3.5.3 with `:target :node-script`.
- **Runtime and machine:** Node v26.12.1, and the machine of the 2026-11-14
  section above.
- **Repetitions:** 5 Node processes per compiler, alternated. Each process
  prints the median of 5 timed rounds per operation, after one warm-up at a
  tenth of its iterations. The table gives medians over the processes, with
  ranges in brackets.
- **Checksums:** every operation's checksum was the same in all ten processes.
- **Unreserved:** a peer worker was active. The 1-minute load average before a
  run was 4.58 to 5.28. The investigation's was 1.8 to 2.4.
- **Raw results:** `research/output-speed/core-ops-1007/master-6ceeb23b.edn`
  and `.md`.

| op | light-cljs ns/op | shadow-cljs ns/op | light/shadow | investigation's light/shadow | change of ratio |
| --- | ---: | ---: | ---: | ---: | ---: |
| kw-small | 121.0 (117.5-121.6) | 9.83 (9.78-9.89) | 12.31 | 11.84 | +4% |
| get-small | 162.5 (159.2-163.3) | 14.9 (14.8-14.9) | 10.92 | 10.91 | +0% |
| get-small-miss | 203.8 (203.2-206.0) | 19.2 (19.2-19.2) | 10.62 | 10.61 | +0% |
| contains-small | 162.1 (159.8-163.7) | 12.0 (12.0-12.1) | 13.47 | 13.12 | +3% |
| count-small | 30.9 (30.2-31.5) | 2.12 (2.10-2.13) | 14.58 | 14.82 | -2% |
| assoc-small-existing | 643.9 (636.3-655.5) | 30.6 (30.5-30.8) | 21.06 | 20.24 | +4% |
| assoc-small-new | 759.8 (741.3-774.1) | 39.3 (39.2-39.5) | 19.35 | 18.72 | +3% |
| dissoc-small | 1,008 (988.4-1,031) | 57.5 (57.1-58.0) | 17.54 | 16.96 | +3% |
| update-small | 721.7 (705.6-736.0) | 43.9 (43.3-44.1) | 16.45 | 15.17 | +8% |
| map-literal-3 | 105.3 (103.7-109.6) | 10.1 (9.67-10.2) | 10.47 | 0.06, folded | - |
| get-hash100 | 77.4 (76.7-79.2) | 12.2 (11.9-12.6) | 6.34 | 5.82 | +9% |
| kw-hash100 | 94.5 (93.5-96.3) | 13.5 (13.3-13.8) | 7.01 | 6.49 | +8% |
| assoc-hash100 | 392.7 (391.2-409.4) | 77.9 (77.9-78.9) | 5.04 | 4.83 | +4% |
| dissoc-hash100 | 357.4 (355.1-362.8) | 76.1 (75.3-76.5) | 4.70 | 4.44 | +6% |
| update-hash100 | 483.0 (476.5-494.3) | 95.8 (94.3-96.7) | 5.04 | 4.86 | +4% |
| get-in-3 | 766.9 (764.0-775.7) | 57.7 (56.9-58.1) | 13.28 | 12.68 | +5% |
| assoc-in-3 | 2,435 (2,415-2,470) | 133.9 (128.6-135.3) | 18.19 | 16.78 | +8% |
| update-in-3 | 2,560 (2,528-2,599) | 139.2 (138.4-140.0) | 18.39 | 16.61 | +11% |
| vec-conj-64 | 6,308 (6,163-6,344) | 1,298 (1,291-1,302) | 4.86 | 4.15 | +17% |
| vec-literal-4 | 40.1 (39.5-40.2) | 6.86 (6.64-7.05) | 5.84 | 5.76 | +1% |
| vec-nth-1024 | 28.3 (27.6-29.4) | 5.22 (5.13-5.63) | 5.42 | 5.07 | +7% |
| vec-invoke-1024 | 6.71 (6.58-7.09) | 4.82 (4.77-4.90) | 1.39 | 1.65 | -16% |
| vec-peek | 15.6 (15.4-15.6) | 3.43 (3.40-3.47) | 4.54 | 4.20 | +8% |
| vec-assoc-1024 | 158.4 (156.3-159.9) | 73.1 (72.7-73.3) | 2.17 | 2.02 | +7% |
| destructure-vec-3 | 88.6 (84.7-88.9) | 9.78 (9.73-9.82) | 9.06 | 8.59 | +5% |
| destructure-map-3 | 461.9 (459.8-469.3) | 49.3 (49.2-50.7) | 9.37 | 8.76 | +7% |
| reduce-vec-1000 | 5,291 (5,274-5,410) | 2,508 (2,487-2,515) | 2.11 | 2.09 | +1% |
| reduce-map-inc-1000 | 603,669 (600,988-606,010) | 39,981 (39,667-41,594) | 15.10 | 13.61 | +11% |
| filter-count-1000 | 320,109 (316,395-320,405) | 28,526 (28,003-28,838) | 11.22 | 10.72 | +5% |
| into-xform-1000 | 101,019 (99,037-101,486) | 18,406 (18,217-18,428) | 5.49 | 5.36 | +2% |
| mapv-1000 | 82,765 (81,806-83,433) | 14,297 (14,207-14,533) | 5.79 | 5.61 | +3% |
| lazy-walk-1000 | 598,912 (591,333-602,470) | 39,415 (38,752-39,677) | 15.19 | 14.67 | +4% |
| seq-walk-vec-1000 | 545,517 (519,519-564,829) | 9,999 (9,939-10,125) | 54.56 | 49.45 | +10% |
| reduce-range-1000 | 4,879 (4,721-5,034) | 5,442 (5,384-5,516) | 0.90 | 0.88 | +2% |
| reduce-kv-small | 152.1 (145.7-152.6) | 37.8 (37.5-37.9) | 4.02 | 3.89 | +3% |
| reduce-entries-small | 1,557 (1,477-1,572) | 77.8 (76.9-78.5) | 20.01 | 19.10 | +5% |
| str-nested-map | 8,160 (7,778-8,236) | 2,248 (2,233-2,260) | 3.63 | 3.44 | +6% |
| str-concat | 145.9 (145.0-148.6) | 14.5 (14.5-14.5) | 10.05 | 9.27 | +8% |
| eq-map-7 | 1,762 (1,707-1,774) | 152.0 (149.8-153.3) | 11.59 | 11.15 | +4% |
| eq-vec-10 | 3,950 (3,925-3,968) | 81.4 (81.2-82.2) | 48.50 | 46.21 | +5% |
| hash-fresh-vec-4 | 1,302 (1,298-1,315) | 70.9 (70.2-71.4) | 18.38 | 17.92 | +3% |
| hash-fresh-map-7 | 1,503 (1,490-1,527) | 548.9 (539.7-553.6) | 2.74 | 2.73 | +0% |
| protocol-on-record | 3.42 (3.35-3.43) | 2.37 (2.34-2.39) | 1.44 | 1.45 | +0% |
| record-kw-field | 9.95 (9.93-10.1) | 4.19 (4.16-4.21) | 2.37 | 2.44 | -3% |
| record-dot-field | 1.96 (1.90-1.98) | 0.87 (0.86-0.89) | 2.25 | 2.25 | +0% |
| record-assoc | 72.5 (71.9-73.6) | 13.8 (13.5-14.1) | 5.27 | 5.41 | -3% |
| multimethod | 525.0 (523.6-537.2) | 47.2 (45.3-47.6) | 11.12 | 10.73 | +4% |
| multi-arity-call | 2.02 (2.00-2.03) | 2.01 (1.99-2.03) | 1.00 | 1.09 | -8% |
| variadic-call | 326.6 (325.2-328.1) | 14.2 (14.0-14.3) | 22.95 | 23.12 | -1% |
| apply-vec | 1,178 (1,168-1,190) | 93.1 (91.0-94.7) | 12.65 | 12.75 | -1% |
| unknown-fn-call | 5.36 (5.31-5.59) | 6.50 (6.48-6.57) | 0.82 | 0.85 | -3% |
| closure-alloc-call | 9.16 (9.10-9.21) | 8.47 (8.37-8.68) | 1.08 | 1.28 | -16% |
| swap-inc | 12.6 (11.7-13.0) | 5.63 (5.61-5.79) | 2.23 | 2.32 | -4% |
| swap-assoc | 1,156 (1,147-1,169) | 37.6 (37.5-38.0) | 30.74 | 30.30 | +1% |
| merge-small | 7,868 (7,799-7,911) | 894.5 (891.4-900.0) | 8.80 | 8.48 | +4% |
| select-keys-3 | 2,896 (2,879-2,936) | 249.8 (245.8-255.2) | 11.59 | 11.42 | +2% |
| group-by-100 | 84,834 (84,623-86,359) | 5,097 (5,072-5,117) | 16.65 | 15.26 | +9% |
| sort-by-100 | 60,948 (59,741-63,781) | 4,724 (4,648-4,791) | 12.90 | 11.27 | +14% |
| transient-vec-100 | 8,494 (8,363-8,577) | 1,154 (1,143-1,176) | 7.36 | 7.10 | +4% |
| transient-map-100 | 21,873 (21,652-21,942) | 4,210 (4,129-4,292) | 5.20 | 5.21 | +0% |
| sorted-map-assoc-100 | 3,591 (3,570-3,613) | 209.1 (207.0-212.8) | 17.18 | 17.45 | -2% |
| str-sorted-map-20 | 57,034 (56,375-58,293) | 20,488 (20,259-20,777) | 2.78 | 2.75 | +1% |
| bit-and | 1.53 (1.51-1.54) | 0.59 (0.59-0.60) | 2.59 | 2.51 | +3% |
| aget-bit-and | 1.82 (1.81-1.84) | 0.59 (0.59-0.60) | 3.08 | 3.18 | -3% |

**Against the investigation:** the ratios agree within noise.
- The investigation's ratios are from `results/micro-c5dad571-table.md`, and
  for the two loops from the medians of `results/bitand.txt`.
- 54 of the 61 operations in its table, `map-literal-3` aside, are within 10%
  of its ratio, and all 61 are within 17%. The median change is +4%. The two
  loops are within 3%.
- light-cljs's times are a median 7% higher and shadow-cljs's 4% higher, under
  about twice the load.
- **Beyond 10%:**
  - `vec-conj-64` +17%, mostly because shadow-cljs got faster (1,414 to
    1,298 ns);
  - `sort-by-100` +14%;
  - `reduce-map-inc-1000`, `update-in-3` and `seq-walk-vec-1000` +10 to +11%;
  - `vec-invoke-1024` and `closure-alloc-call` -16%. These moved on
    shadow-cljs's side, by about a nanosecond (3.5 to 4.8 ns, and 7.1 to
    8.5 ns).
- No operation moved between the groups below.

**What the gate shows** is what the investigation found:
- **At parity or close:**
  - plain calls: `multi-arity-call` 1.00, `unknown-fn-call` 0.82,
    `closure-alloc-call` 1.08;
  - `reduce-range-1000` 0.90, `vec-invoke-1024` 1.39, `protocol-on-record` 1.44.
- **2 to 5 times:**
  - record fields: `record-kw-field` 2.37, `record-dot-field` 2.25;
  - vectors: `vec-assoc-1024` 2.17, `reduce-vec-1000` 2.11, `vec-peek` 4.54,
    `vec-conj-64` 4.86;
  - `swap-inc` 2.23, `reduce-kv-small` 4.02;
  - printing: `str-nested-map` 3.63, `str-sorted-map-20` 2.78;
  - `hash-fresh-map-7` 2.74;
  - updates of the 100-key hash map: 4.7 to 5.0.
- **5 to 55 times:** every other operation.
  - These are the rest of the map, vector, seq and transient operations, plus
    `str-concat` 10.1, `multimethod` 11.1, `apply-vec` 12.7 and
    `variadic-call` 23.0.
  - The largest are `seq-walk-vec-1000` 54.6, `eq-vec-10` 48.5, `swap-assoc`
    30.7, `variadic-call` and `assoc-small-existing` 21.1.
- **The loop itself is 2.6 to 3.1 times slower.** At `:advanced`, light-cljs
  emits `(bit-and i 3)` as a call of the runtime's `bitAnd`, where upstream's
  macro emits `&`. The call costs about 1 ns.
  - It is the whole gap of `record-dot-field` (1.96 against 0.87 ns), whose
    loop is otherwise the same array read and field read.
  - It is most of `protocol-on-record`'s gap (3.42 against 2.37 ns).
  - Most small-map, record and call benches index their inputs with
    `(bit-and i 3)`, so they carry it too.
  - light-cljs-1020 emits the bit operations as operators.

## 2026-11-15: the Pants build's runtime (light-cljs-1024)

**Change.** The Pants build's suite was red on master `f533efb1`, and this
change restores it. Two edits reach the release output:
- the store metadata (`storesPredicate`, `payloadStores`, `payloadStore`) is
  read with `Object.getOwnPropertyDescriptor` instead of `Object.hasOwn`.
  This runs once, when the runtime loads;
- an array map's key search (`entryIndex`) tests identity first only for
  keywords, strings and numbers. Other keys go to `equalPair`, which tests
  identity first itself.

The third edit, the shared-heap profile's `storeNativeProtocols`, is not in
release output.

**Setup.** The kit and machine of the sections above. Before is
`light-cljs-f533efb1` (master), after is `light-cljs-aa070edb`. The gate ran
the small-map operations, whose maps the key search walks (`--ops
kw-small,get-small,contains-small,assoc-small,dissoc-small,update-small`), 5
runs. Workload 200k ran 10 runs per count, its output built with each binary
just before its runs, and the two runs back to back. **Unreserved:** peer
workers were active. The 1-minute load average before a run was:
- gate: 8.9 to 83.3 before, 8.6 to 24.8 after;
- workload 200k: 5.3 to 7.1 before, 4.3 to 5.5 after.

Raw results are in `research/output-speed/pants-runtime-1024`.

| Measure | light-cljs before | light-cljs after | change | light/shadow before | light/shadow after |
| --- | ---: | ---: | ---: | ---: | ---: |
| `kw-small` | 128.2 ns | 129.3 ns (128.0-134.7) | +1% | 12.98 | 13.10 |
| `get-small` | 169.7 ns | 171.1 ns (169.5-176.1) | +1% | 11.35 | 11.45 |
| `contains-small` | 176.5 ns | 175.5 ns (173.8-181.4) | -1% | 14.55 | 14.47 |
| `assoc-small-existing` | 664.9 ns | 665.5 ns (662.1-683.7) | +0% | 21.07 | 21.11 |
| `assoc-small-new` | 799.1 ns | 784.9 ns (781.6-786.7) | -2% | 20.23 | 19.89 |
| `dissoc-small` | 1,058 ns | 1,051 ns (1,039-1,066) | -1% | 13.19 | 13.04 |
| `update-small` | 750.9 ns | 746.7 ns (744.0-754.5) | -1% | 17.10 | 17.06 |
| workload 200k, steady state per iteration | 94.3 ms | 92.9 ms | -1% | 3.77 | 3.77 |
| workload 200k, wall time at 40 iterations | 4.161 s (4.056-4.220) | 4.085 s (4.075-4.183) | -2% | 3.17 | 3.16 |

- **No change beyond the ranges.** The gate's operations moved 2% or less.
  Workload 200k moved within its ranges, and shadow-cljs's runs moved by
  about as much (25.1 to 24.7 ms per iteration).
- **Output size:** workload 200k's advanced output grows 64 bytes, from
  3,438,709 to 3,438,773. The size ratchet's changes are noted in
  tests/support/ratchet_fixtures.rs: the readable runtime grows up to 126
  bytes, and `constant-keys` at advanced grows 30 bytes.

## 2026-11-15: the emitter's value recursion (light-cljs-1027)

**Change.** The emitter's `value_expr` now holds only the dispatch on an
expression's kind; each kind that needs more than a call has a function of its
own. This shortens the stack each nesting level of a value uses in an
unoptimized build. It does not change the emitted JavaScript.

**Setup.** The kit and machine of the sections above. Before is
`light-cljs-80b7a37b` (master), after is `light-cljs-52191888`, a draft of
this change with the same source code. Workload 200k was built clean
(`release bench`, its cache is off) 10 times with each binary, the two
alternating. **Unreserved:** a peer worker was active, and the 1-minute load
average before a build was 7.9 to 10.5. Raw results are in
`research/output-speed/emitter-stack-1027`.

| Measure | before | after | change |
| --- | ---: | ---: | ---: |
| workload 200k, clean build wall time | 6.568 s (6.382-6.803) | 6.509 s (6.289-6.822) | -1% |
| workload 200k, clean build peak memory | 1.030 GB (1.025-1.056) | 1.038 GB (1.019-1.064) | +1% |

- **Output is byte-identical.** All 20 builds of workload 200k wrote the same
  3,438,773-byte module, and the core-ops gate's `release bench` output is the
  same file with either binary. The gate and workload 200k's steady state run
  the same program before and after, so they were not timed again.
- **Compile time and peak memory moved within their ranges.**

## 2026-11-15: vectors, map entries, subvecs and vector transients as classes (light-cljs-1009)

What landed (docs/value-layout.md, the notes after "Decisions for the user"):
every protocol slot, marker and mask name ends in the build's key
placeholder `$LKEY`; vectors, subvectors, map entries and transient vectors
are instances of internal constructors on the public types' prototypes,
each holding its store in one own keyed field (`store$LKEY`), so building
one is a `new` with one field where it was `Object.create` plus a
private-field stamp; and `nth`, `count`, `peek`, `conj`, `assoc` and `get`
call a vector's keyed slot first. The store algorithms are unchanged, and
the store's own fields are not yet the class's (light-cljs-1026).

Binaries: before `light-cljs-f533efb1` (master), after `light-cljs-2b087a2f`
for the gate and `light-cljs-cd1d5e87` for the workload (the commit before
it; 2b087a2f changes only a vector's own `call`, which the workload's time
does not reach), all in research/shadow-credibility/bin. Unreserved: peer
workers and this pellet's test suites were running. Both commits were
work in progress squashed into this pellet's commit, whose src/ is
2b087a2f's. Raw results:
research/output-speed/vector-classes-1009.

**Gate** (`bb benchmarks/core-ops/run.bb --ops vec-conj,vec-literal,vec-nth,vec-invoke,vec-peek,vec-assoc,transient-vec,count-small,get-small --runs 3`,
the same selection both times; load average 11.8-14.4 before, 9.1-11.6
after), ns per operation, median (range):

| op | light before | light after | change | shadow-cljs (after run) | light/shadow before | light/shadow after |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| vec-nth-1024 | 28.5 (27.8-30.5) | 6.04 (6.04-6.33) | -79% | 5.14 | 5.48 | 1.18 |
| vec-peek | 15.6 (15.6-15.7) | 4.44 (4.44-4.45) | -72% | 3.47 | 4.46 | 1.28 |
| vec-invoke-1024 | 6.90 (6.87-6.95) | 5.95 (5.93-5.99) | -14% | 4.91 | 1.41 | 1.21 |
| vec-assoc-1024 | 168.8 (167.5-180.5) | 98.8 (96.7-100.4) | -41% | 76.4 | 2.16 | 1.29 |
| vec-conj-64 | 6,736 (6,568-6,819) | 3,814 (3,753-3,842) | -43% | 1,486 | 4.34 | 2.57 |
| vec-literal-4 | 45.7 (44.3-47.6) | 23.2 (23.1-23.8) | -49% | 7.91 | 5.75 | 2.93 |
| transient-vec-100 | 8,543 (8,491-9,006) | 5,579 (5,536-5,676) | -35% | 628.3 | 12.85 | 8.88 |
| count-small | 45.7 (45.7-45.9) | 49.1 (48.8-49.5) | +7% | 2.09 | 21.35 | 23.47 |
| get-small | 175.3 (174.6-177.8) | 173.8 (168.5-174.8) | -1% | 14.8 | 11.65 | 11.77 |

- No gate operation is more than 10% slower. `count-small` (+7%, 3.4 ns):
  `count` of a map, which is not converted yet, first reads the keyed kind
  in the slot-first wrapper; it goes when maps convert (light-cljs-1012)
  and the old layers go (light-cljs-1015).
- A first run of the commit before 2b087a2f had `vec-invoke-1024` at 9.60 ns
  (+39%): a vector's `call` tested its kind through `vectorStore`, whose
  property read every kind test shares and so sees every kind of value.
  2b087a2f reads the kind and the store in `call` itself.
- Still above 1.5x shadow-cljs's: the three that allocate (`vec-conj-64`,
  `vec-literal-4`, `transient-vec-100`), which still build a store object
  per value (light-cljs-1026) and pass the old `conj` layers.

**Workload 200k** (`compare.bb workload-200k-seed42 --runs 5 --runtime-only
--runtime 1,10,40`, a copy of the project, load average 14.1-22.9), wall
time, median (range):

| iterations | light before | shadow-cljs (before run) | light after | shadow-cljs (after run) | light/shadow before | light/shadow after |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 0.526 s (0.501-0.634) | 0.370 s | 0.529 s (0.493-0.537) | 0.385 s | 1.42 | 1.37 |
| 10 | 1.910 s (1.584-2.162) | 1.054 s | 1.563 s (1.520-1.671) | 0.700 s | 1.81 | 2.23 |
| 40 | 4.898 s (4.677-6.186) | 1.663 s | 4.542 s (4.291-5.028) | 1.525 s | 2.95 | 2.98 |

Steady state does not move beyond the noise of a load average above 20
(the 10-iteration ratio follows shadow-cljs's own run, 1.054 s against
0.700 s): the workload's time is in maps, seqs and keywords, which are not
converted yet.

**Advanced size** (tests/support/ratchet_fixtures.rs, noted there):
`vector-only` 5,394 to 5,441 bytes, `mapv-count` 37,295 to 39,561,
`features-direct` 77,785 to 81,454 (the slot units and wrappers, the store
helpers every collection test calls, and the keyed names, five bytes each).
re-frame simple and TodoMVC gzip sizes were not measured in this step.

**Review fixes** (the commit after the class layout: slot-first wrappers
take a slot only when it is truthy, the Pants build's slot registry and
switch, unkeyed `IFn` invoke slots, `inspect`'s stamp test). Only the first
of these is on a hot path: each wrapper's test of the slot is `if (method)`
where it was `!== undefined`. The gate ran again with the same `--ops` and
`--runs 3`, before `light-cljs-2b087a2f` (the class layout, the "after"
above) and after `light-cljs-4e1b684d` (the fixes, on master `80b7a37b`;
the later rebase onto light-cljs-1027 leaves output byte-identical).
Unreserved: a peer worker was active, and the 1-minute load average before
a run was 4.1 to 4.9. Raw results: research/output-speed/vector-classes-1009-fixes.

| op | light before | light after | change | shadow-cljs | light/shadow after |
| --- | ---: | ---: | ---: | ---: | ---: |
| vec-nth-1024 | 6.07 | 5.98 (5.85-6.12) | -1% | 4.95 | 1.21 |
| vec-peek | 4.42 | 4.36 (4.33-4.40) | -1% | 3.35 | 1.30 |
| vec-invoke-1024 | 5.85 | 5.81 (5.74-5.93) | -1% | 4.65 | 1.25 |
| vec-assoc-1024 | 99.5 | 95.2 (95.0-95.7) | -4% | 72.2 | 1.32 |
| vec-conj-64 | 3,893 | 3,743 (3,709-3,804) | -4% | 1,451 | 2.58 |
| vec-literal-4 | 23.1 | 23.0 (22.5-23.0) | +0% | 7.56 | 3.04 |
| transient-vec-100 | 5,567 | 5,616 (5,501-5,738) | +1% | 606.5 | 9.26 |
| count-small | 49.7 | 47.4 (47.3-47.4) | -5% | 2.06 | 23.00 |
| get-small | 174.4 | 171.3 (170.1-171.4) | -2% | 14.4 | 11.86 |

No operation moved beyond its range. Advanced size after the rebase onto
light-cljs-1024 and these fixes: `vector-only` 5,503 bytes, `mapv-count`
39,619, `features-direct` 81,469 (tests/support/ratchet_fixtures.rs notes
the ceilings that moved). Workload 200k was not timed again: no change here
touches maps, seqs or keywords, where its time goes.

## 2026-11-15: the per-build runtime key (light-cljs-1025)

What landed (docs/value-layout.md, section 1, "Derivation"): the output
writer writes each build's eight-character key where the runtime text and
emission hold the placeholder `\$LKEY`, four bytes more per keyed name, and
every runtime registers the key page-wide as it loads (the `runtimeKey`
unit: one `Symbol.for`, one global read and one `Map` lookup, about 290
terse bytes, which every `:advanced` slice keeps). No operation's code
changes otherwise; the keyed names are as long as dotted names were in the
measurement that chose them (section 1, "Measured": S against U).

Binaries: before `light-cljs-cf613766` (master), after
`light-cljs-85bf5d44` (this pellet's first commit; the review fixes after it
change only property names that end in the placeholder's spelling without
being keyed, a dev build's identity and size reports, none of which these
outputs hold), in research/shadow-credibility/bin. Unreserved: a peer
worker was active. Raw results: research/output-speed/runtime-key-1025.

**Gate** (`bb benchmarks/core-ops/run.bb --ops
protocol-on-record,vec-nth,vec-conj,count-small,get-small,kw-small,seq-walk-vec,eq-vec,record-kw-field
--runs 3`, before then after; load average 2.85-4.86 before, 2.85-4.39
after), ns per operation, median (range):

| op | light before | light after | change | shadow-cljs before / after run | light/shadow before | light/shadow after |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| protocol-on-record | 3.28 (3.27-3.40) | 3.35 (3.34-3.41) | +2% | 2.36 / 2.39 | 1.39 | 1.40 |
| vec-nth-1024 | 5.78 (5.76-6.05) | 5.99 (5.87-6.07) | +4% | 5.08 / 5.19 | 1.14 | 1.15 |
| vec-conj-64 | 3,610 (3,517-3,675) | 3,724 (3,676-3,779) | +3% | 1,442 / 1,481 | 2.50 | 2.51 |
| count-small | 47.1 (47.0-47.1) | 49.4 (47.8-73.1) | +5% | 2.03 / 2.14 | 23.22 | 23.06 |
| get-small | 165.2 (164.0-179.0) | 170.9 (169.8-171.1) | +3% | 14.4 / 14.9 | 11.45 | 11.48 |
| kw-small | 125.4 (123.9-141.2) | 128.6 (128.4-129.5) | +3% | 9.44 / 9.81 | 13.29 | 13.11 |
| seq-walk-vec-1000 | 404,243 (397,716-408,155) | 460,457 (443,112-487,483) | +14% | 7,572 / 7,737 | 53.39 | 59.51 |
| eq-vec-10 | 3,459 (3,457-3,489) | 3,536 (3,502-3,646) | +2% | 54.2 / 56.3 | 63.83 | 62.80 |
| record-kw-field | 13.5 (13.4-13.7) | 13.6 (13.3-13.6) | +0% | 3.76 / 3.90 | 3.60 | 3.48 |

- shadow-cljs's own output ran 2-5% slower in the second run, as light-cljs's
  did: the light/shadow ratios do not move, except `seq-walk-vec-1000`.
- `seq-walk-vec-1000` again, with `lazy-walk-1000` and `reduce-vec-1000`,
  before and after alternated twice (`--runs 3` each, load average 3.4-8.8
  in the first pair and 14.2-27.3 in the second): light/shadow 53.26 before
  and 53.71 after, then 54.75 and 54.91; `lazy-walk-1000` 13.54 and 13.76,
  then 12.67 and 13.36; `reduce-vec-1000` 2.20 and 2.22, then 2.22 and 2.17.
  The first pair's +14% was noise; no operation changes.

**Workload 200k** (`compare.bb workload-200k-seed42-1025 --runs 5
--runtime-only --runtime 1,10,40`, a copy of the project, each output
built by its binary; load average 4.6-6.3), wall time, median (range):

| iterations | light before | shadow-cljs (before run) | light after | shadow-cljs (after run) | light/shadow before | light/shadow after |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 0.460 s (0.455-0.466) | 0.332 s | 0.414 s (0.408-0.449) | 0.300 s | 1.39 | 1.38 |
| 10 | 1.372 s (1.354-1.408) | 0.618 s | 1.297 s (1.288-1.299) | 0.567 s | 2.22 | 2.29 |
| 40 | 3.987 s (3.747-4.073) | 1.287 s | 3.728 s (3.682-3.828) | 1.166 s | 3.10 | 3.20 |

Both compilers' outputs ran 7-10% faster in the second run; the ratios
stay within that drift. Startup (1 iteration), which now registers the
key, does not move.

**Advanced size** (tests/support/ratchet_fixtures.rs, noted there): every
slice grows by the registration (about 290 bytes) and four bytes per keyed
name, `vector-only` 5,503 to 5,879 bytes, `mapv-count` 39,619 to 40,088,
`features-direct` 81,469 to 82,006. The workload's output grows from
3,442,997 to 3,444,422 bytes (+0.04%), 859,129 to 859,420 gzipped.
re-frame simple and TodoMVC gzip sizes were not measured in this step.

## 2026-11-15: seqs over arrays and strings, and rest arguments, as classes (light-cljs-1010)

What landed (docs/value-layout.md, the light-cljs-1010 notes after
"Decisions for the user"): every seq the runtime built as a view (indexed
seqs over arrays, strings, short vectors, queues and sets, a vector's
chunked seq, and the map seqs over an array of entries) is one object of
its type's class, with its storage, offset and kind in own keyed fields,
where it was `Object.create`, a private stamp and a frozen six-field
payload, and a chunked seq also a `publicCollectionSlots` entry per step.
Indexed and chunked seqs have keyed `first`, `rest`, `next` and `seq` slots
(an indexed seq also `count`), a vector a `seq` slot, and the four
operations call them first. Rest arguments are an indexed seq over one
fresh array: a direct call's array literal is taken as it is, a
dispatcher's `arguments` copied once, nothing frozen. Lazy seqs, chunked
conses, chunks and the list cells are not converted yet (light-cljs-1029,
light-cljs-1030).

Binaries: before `light-cljs-cf613766` (master), after `light-cljs-e7ef321b`
(this pellet's runtime; later commits change tests and comments only), in
research/shadow-credibility/bin. Unreserved: one peer worker was active.
Raw results: research/output-speed/seq-classes-1010.

**Gate** (`bb benchmarks/core-ops/run.bb --ops seq-walk-vec,lazy-walk,reduce-map-inc,filter-count,mapv,into-xform,variadic-call,apply-vec --runs 3`,
the same selection both times; 1-minute load average 3.62-4.82 before,
2.81-4.64 after), ns per operation, median (range):

| op | light before | light after | change | shadow-cljs (after run) | light/shadow before | light/shadow after |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| seq-walk-vec-1000 | 562,294 (562,292-586,120) | 21,089 (21,005-21,683) | -96% | 9,587 | 57.43 | 2.20 |
| variadic-call | 342.8 (340.4-342.9) | 9.23 (8.88-9.36) | -97% | 8.85 | 36.78 | 1.04 |
| apply-vec | 1,050 (1,044-1,059) | 352.6 (349.8-358.1) | -66% | 51.3 | 19.57 | 6.88 |
| lazy-walk-1000 | 620,992 (620,194-622,153) | 584,091 (574,747-588,441) | -6% | 38,028 | 15.48 | 15.36 |
| reduce-map-inc-1000 | 628,944 (627,880-634,819) | 582,989 (578,307-593,599) | -7% | 40,677 | 14.82 | 14.33 |
| filter-count-1000 | 330,617 (325,930-330,974) | 297,693 (294,622-304,807) | -10% | 27,275 | 11.79 | 10.91 |
| into-xform-1000 | 72,179 (72,127-73,078) | 72,339 (70,939-73,802) | +0% | 13,172 | 5.39 | 5.49 |
| mapv-1000 | 52,299 (51,931-55,269) | 51,995 (50,910-52,076) | -1% | 12,706 | 3.97 | 4.09 |

- No gate operation is slower beyond its range. The ratios of
  `into-xform-1000` and `mapv-1000` move only with shadow-cljs's own run:
  both reduce a vector through a transient and reach no converted seq.
- `apply-vec` still conses its argument list (a `cells` value) and counts
  it through the old layers; `lazy-walk`, `reduce-map-inc` and
  `filter-count` walk lazy seqs and chunked conses, which pass the new
  slot-first wrappers without a slot and then the old layers. They gain
  only where their source is a vector's chunked seq: light-cljs-1029 and
  light-cljs-1030 convert them.

**Workload 200k** (`compare.bb workload-200k-seed42 --runs 5 --runtime-only
--runtime 1,10,40`, a copy of the project rebuilt with each binary just
before its run; load average 2.97-3.58), wall time, median (range):

| iterations | light before | light after | shadow-cljs (before run) | shadow-cljs (after run) | light/shadow before | light/shadow after |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 0.424 s (0.412-0.436) | 0.417 s (0.402-0.428) | 0.298 s | 0.300 s | 1.42 | 1.39 |
| 10 | 1.352 s (1.318-1.402) | 1.281 s (1.171-1.333) | 0.608 s | 0.602 s | 2.22 | 2.13 |
| 40 | 3.960 s (3.754-4.041) | 3.656 s (3.444-3.731) | 1.217 s | 1.221 s | 3.25 | 2.99 |

Steady state, from the medians at 1 and 40 iterations: light-cljs 90.7 to
83.1 ms per iteration (-8%), shadow-cljs 23.6 both times; light/shadow 3.85
to 3.52. (A first before run timed the output the copied project already
had, built by an older binary, at 122.1 ms per iteration; it is discarded.)

**Advanced size** (tests/support/ratchet_fixtures.rs, noted there): programs
that walk seqs keep the seq classes' slots and the four slot-first
wrappers, about 3 to 4 KB: `rest-fn` 11,079 to 14,003 bytes, `mapv-count`
39,619 to 43,238, `keyword-lookups` 39,028 to 42,846, `constant-keys`
53,037 to 57,117, `event-maps` 35,420 to 39,449; printing programs grow
173 bytes (`vector-printed`, `map-printed`). re-frame simple and TodoMVC
gzip sizes were not measured in this step.

## 2026-11-15: keywords and symbols as classes (light-cljs-1011)

What landed (docs/value-layout.md, the light-cljs-1011 notes): a keyword
is one object of its class with its namespace, name, `fqn` and hash in own
keyed fields, where it was `Object.create` plus a frozen payload in the
`keywords` WeakMap; symbols likewise (`str` for `fqn`). Both stores are
deleted. An array map's search for a keyword key compares each key by
identity, then by kind and `fqn`, and reads the key's `IEquiv` slot once
per search instead of once per entry. `hash` caches in the keyword's own
field, and at `:advanced` each keyword constant carries the hash the
compiler computed. Lookups still pass the old layers (`get`'s protocol
check, `maps.get`, `collectionEntryIndex`) until light-cljs-1012 and
light-cljs-1015.

Binaries: before `light-cljs-f91e0e62` (master), after `light-cljs-c11a4060`
(this pellet's runtime and emitter; later commits change tests and
comments only), in research/shadow-credibility/bin. Unreserved: one peer
worker was active, and this worktree's Pants suite ran during the after
gate. Raw results: research/output-speed/keyword-classes-1011.

**Gate** (`bb benchmarks/core-ops/run.bb --ops kw-small,kw-hash100,destructure-map-3,record-kw-field,multimethod,get-small,get-small-miss,contains-small,map-literal-3,str-nested-map,hash-fresh-map-7,eq-map-7`,
5 runs, the same selection both times; 1-minute load average 3.29-4.46
before, 6.75-16.73 after), ns per operation, median (range):

| op | light before | light after | change | shadow-cljs (after run) | light/shadow before | light/shadow after |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| kw-small | 124.1 (123.0-127.0) | 104.3 (103.3-105.2) | -16% | 9.81 | 12.67 | 10.63 |
| kw-hash100 | 100.1 (98.7-101.0) | 88.5 (86.7-90.2) | -12% | 13.0 | 7.73 | 6.81 |
| destructure-map-3 | 413.2 (411.0-422.0) | 347.7 (338.3-350.1) | -16% | 47.6 | 8.70 | 7.31 |
| record-kw-field | 13.7 (13.6-14.7) | 14.1 (13.9-14.2) | +3% | 4.21 | 3.28 | 3.35 |
| multimethod | 532.2 (526.6-546.4) | 525.6 (521.4-533.8) | -1% | 41.1 | 12.79 | 12.78 |
| get-small | 162.5 (157.3-165.9) | 116.0 (115.0-117.0) | -29% | 15.0 | 10.96 | 7.76 |
| get-small-miss | 210.4 (202.1-213.5) | 126.2 (125.6-126.8) | -40% | 19.3 | 10.95 | 6.53 |
| contains-small | 161.6 (159.8-167.5) | 129.1 (127.8-129.9) | -20% | 12.1 | 13.40 | 10.64 |
| map-literal-3 | 90.1 (89.6-92.5) | 93.9 (88.8-95.2) | +4% | 11.2 | 8.35 | 8.38 |
| str-nested-map | 6,214 (6,084-6,246) | 6,399 (6,249-6,422) | +3% | 2,338 | 2.73 | 2.74 |
| eq-map-7 | 1,782 (1,746-1,802) | 1,532 (1,524-1,540) | -14% | 149.5 | 11.93 | 10.24 |
| hash-fresh-map-7 | 1,486 (1,434-1,569) | 1,472 (1,458-1,476) | -1% | 374.4 | 3.98 | 3.93 |

- No gate operation is more than 10% slower. `record-kw-field` and
  `map-literal-3` stay within their ranges: a record's keyword field read
  goes through `recordKeywordFieldIndex`'s identity cache, and a map
  literal builds its entries without searching. `str-nested-map` is 3%
  slower, just outside its range, measured at three to four times the
  before run's load; printing a keyword still goes through `String`.
- The keyword search itself is upstream's now; what is left of
  `get-small`'s 7.8x is the dispatch in front of it, which light-cljs-1012
  (array maps as classes) and light-cljs-1015 (the old layers) remove.
  `multimethod` dispatches through the hierarchy and the method table's
  maps, which this step does not reach.

**Workload 200k** (`compare.bb workload-200k-seed42-1011 --runs 5
--runtime-only --runtime 1,10,40`, a copy of the project rebuilt with each
binary just before its run; load average 4.65-6.20), wall time, median
(range):

| iterations | light before | light after | shadow-cljs (before run) | shadow-cljs (after run) | light/shadow before | light/shadow after |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 0.425 s (0.421-0.435) | 0.407 s (0.403-0.415) | 0.311 s | 0.300 s | 1.37 | 1.36 |
| 10 | 1.231 s (1.190-1.258) | 1.153 s (1.148-1.197) | 0.572 s | 0.562 s | 2.15 | 2.05 |
| 40 | 3.709 s (3.683-3.961) | 3.600 s (3.581-3.626) | 1.345 s | 1.269 s | 2.76 | 2.84 |

Steady state, from the medians at 1 and 40 iterations: light-cljs 84.4 to
82.0 ms per iteration (-3%), shadow-cljs 26.6 and 25.0; light/shadow 3.17
and 3.28, the change in the ratio being shadow-cljs's own run.

**Advanced size** (tests/support/ratchet_fixtures.rs, noted there): each
keyword constant carries its hash (10 to 13 bytes), and `keywordOf`,
`symbolOf` and the keyword class replace two `WeakMap` declarations:
`keyword-only` 4,354 bytes, `vector-only` 6,067, `keyword-lookups`
44,624, `constant-keys` 59,003, `features` 88,550; the features example's
startup chunk 7,744. re-frame simple and TodoMVC gzip sizes were not
measured in this step.

After the Sol review's fixes (a keyword or symbol is told by its keyed
kind and the presence of its `fqn` or `str` field, so the public
constructor's undefined spelling still makes one; a keyword's `IFn` arity
entries call its `call`), binary `light-cljs-69d007bb` against
`light-cljs-c11a4060`, the same keyword operations but `map-literal-3`,
`str-nested-map`, `eq-map-7` and `hash-fresh-map-7`, 3 runs, load average
13.02-24.72: every median within 3% of the after run above (`kw-small`
105.4 ns, `get-small-miss` 124.2, `kw-hash100` 91.1, `destructure-map-3`
352.7, `record-kw-field` 14.1, `multimethod` 522.4). The keyword programs
at advanced grow by the two arity entries (125 bytes): `keyword-only`
4,479 bytes, `keyword-case` 4,575, the features startup chunk 7,869.

## 2026-11-16: lazy seqs, chunked conses and chunks as classes (light-cljs-1029)

**What.** A lazy seq, a chunked cons, an array chunk and a chunk buffer are
each one object of an internal class on its type's prototype (`LazySeqCell`,
`ChunkedConsCell`, `ArrayChunkCell`, `ChunkBufferCell`, kinds 16 to 19), with
upstream's fields as their own keyed fields, where each was `Object.create`
plus a private-field payload (`lazyValues`, `chunks`) or a `WeakMap` entry
(`chunkBuffers`), and a chunked cons also a cell object of its own. Lazy seqs
and chunked conses have keyed `first`, `rest`, `next` and `seq` slots, a
chunked cons and a vector's chunked seq the chunk protocols, a chunk `count`,
`nth` and `-drop-first`, and the chunk operations that `map`, `filter` and
`keep` call read those slots first. A step of a lazy pipeline now builds one
object per value and reaches no older dispatch layer. docs/value-layout.md
has the notes.

Binaries: before `light-cljs-f91e0e62` (master), after `light-cljs-349cfb5f`
(a work-in-progress commit of this pellet whose src/ is this pellet's
commit's; later changes were tests and docs only), in
research/shadow-credibility/bin. Unreserved: one peer worker was active, and
the machine's load rose during the after runs. Raw results:
research/output-speed/lazy-classes-1029.

**Gate** (`bb benchmarks/core-ops/run.bb --ops seq-walk-vec,lazy-walk,reduce-map-inc,filter-count,mapv,into-xform,variadic-call,apply-vec,reduce-range,group-by,sort-by --runs 3`,
the same selection both times; 1-minute load average 3.95-7.96 before,
10.59-12.76 after), ns per operation, median (range):

| op | light before | light after | change | shadow-cljs (after run) | light/shadow before | light/shadow after |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| lazy-walk-1000 | 579,980 (578,892-594,349) | 37,937 (37,847-38,156) | -93% | 39,880 | 14.68 | 0.95 |
| reduce-map-inc-1000 | 589,880 (588,610-596,780) | 42,591 (42,336-42,615) | -93% | 42,663 | 13.54 | 1.00 |
| filter-count-1000 | 301,145 (298,653-301,895) | 36,062 (36,034-36,362) | -88% | 28,033 | 10.80 | 1.29 |
| into-xform-1000 | 72,610 (72,139-73,948) | 80,471 (80,046-80,887) | +11% | 13,526 | 5.34 | 5.95 |
| mapv-1000 | 53,758 (52,574-56,882) | 62,930 (61,947-62,963) | +17% | 13,166 | 4.06 | 4.78 |
| seq-walk-vec-1000 | 21,583 (21,464-21,636) | 24,686 (24,423-24,832) | +14% | 9,929 | 2.16 | 2.49 |
| reduce-range-1000 | 5,057 (4,975-5,086) | 4,911 (4,908-4,933) | -3% | 5,496 | 0.89 | 0.89 |
| variadic-call | 9.08 (9.04-9.45) | 10.3 (10.2-10.3) | +13% | 9.16 | 0.99 | 1.12 |
| apply-vec | 348.4 (348.0-352.7) | 312.6 (309.6-315.2) | -10% | 53.8 | 6.46 | 5.81 |
| group-by-100 | 85,971 (85,675-86,190) | 80,134 (79,237-82,186) | -7% | 4,113 | 20.80 | 19.48 |
| sort-by-100 | 5,307 (5,234-5,329) | 6,068 (6,010-6,155) | +14% | 3,430 | 1.59 | 1.77 |

- `lazy-walk` and `reduce-map-inc` reach shadow-cljs's times, and
  `filter-count` comes within 1.3x of it: each step is one chunked cons and
  one chunk, whose slots `first`, `next`, `count` and `nth` call first. What
  `filter-count` keeps is `count`'s walk, which tests each step for
  `ICounted` through the older layers.
- **The five operations above 10% slower are not slower on their own.**
  Their code does not change; in the selection they run after the lazy
  operations, which now pass lazy seqs, chunked conses and chunks through
  the slot-first wrappers and `reduce` they share, so V8's feedback at those
  sites differs when they run. Timed alone (`--ops
  into-xform,mapv,seq-walk-vec,variadic-call,sort-by --runs 2`, load
  4.7-5.5), before then after: `into-xform` 84,098 and 82,275 ns,
  `mapv` 64,565 and 62,121, `seq-walk-vec` 17,677 and 17,233,
  `variadic-call` 9.20 and 9.15, `sort-by` 4,860 and 4,725. The before
  binary timed again with the full selection just before the after runs
  (load 5.12-9.29; `gate-before-rerun` in the raw results) gives the same
  numbers as the first before run within 3%.
- **The whole gate** (all 64 operations, `--runs 3`; load 2.28-4.29 before,
  4.58-20.57 after): the lazy operations gain 86-92% in that order too
  (`lazy-walk` 643,802 to 54,438 ns, light/shadow 16.83 to 1.38;
  `reduce-map-inc` 664,789 to 58,227; `filter-count` 334,424 to 45,533).
  Ten operations read more than 10% slower (`destructure-vec-3`,
  `hash-fresh-vec-4`, `eq-map-7`, `seq-walk-vec`, `sort-by`, `str-concat`,
  `multi-arity-call`, `variadic-call`, `closure-alloc-call`, `swap-inc`),
  among them plain calls this pellet does not reach, while shadow-cljs's
  own times rose as much (`variadic-call` 9.2 to 14.3 ns). Timed again back
  to back with one selection of those ten (`--runs 3`, load 11.3-12.4),
  before then after, none differs by more than 3% (`recheck-before`,
  `recheck-after` in the raw results).

**Workload 200k** (`compare.bb workload-200k-seed42-1029 --runs 5
--runtime-only --runtime 1,10,40`, a copy of the project rebuilt with each
binary just before its run; load average 11.5-13.2 before, 13.3-14.0 after),
wall time, median (range):

| iterations | light before | light after | shadow-cljs (before run) | shadow-cljs (after run) | light/shadow before | light/shadow after |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 0.465 s (0.463-0.474) | 0.460 s (0.453-0.469) | 0.336 s | 0.338 s | 1.38 | 1.36 |
| 10 | 1.337 s (1.304-1.383) | 1.270 s (1.248-1.273) | 0.647 s | 0.636 s | 2.08 | 2.00 |
| 40 | 3.868 s (3.799-3.981) | 3.566 s (3.540-3.610) | 1.434 s | 1.370 s | 2.71 | 2.60 |

Steady state, from the medians at 1 and 40 iterations: light-cljs 87.3 to
79.6 ms per iteration (-9%), shadow-cljs 28.1 and 26.5; light/shadow 3.10 to
3.01. Both runs were under a load of 11 to 14, which slowed shadow-cljs's
steady state from light-cljs-1010's 23.6 ms.

**Advanced size** (tests/support/ratchet_fixtures.rs, noted there): programs
that keep lazy seqs grow 3 to 5 KB, mostly the 17 slots with their long keyed
names: `seq-pipeline` 37,748 bytes, `mapv-count` 48,444, `keyword-lookups`
48,027, `constant-keys` 62,372, `event-maps` 44,974, `features` 91,466;
programs that keep no lazy seq less, through the kind tests the older layers
call (`rest-fn` 15,603, `map-printed` 15,275, `atom-str` 3,738). re-frame
simple and TodoMVC gzip sizes were not measured in this step.

## 2026-11-16: vectors, subvectors, map entries and transient vectors hold their own data (light-cljs-1026)

**What.** A vector is its own store: `VectorCell` has upstream's fields,
`meta`, `cnt`, `shift`, `root`, `tail` and `hash`, as its own keyed fields,
where light-cljs-1009 gave it one keyed field holding a store object, and its
metadata and cached hash were entries in the `metadataValues` and
`constructorHashes` WeakMaps. `conj`, `assoc`, `pop` and vector literals
build one object per vector instead of a store object and a vector; `nth`,
`count` and `peek` read the vector's own fields; `meta`, `with-meta`, `hash`
and the metadata `conj` and `assoc` carry over read and write fields instead
of WeakMap entries. A subvector is a view with `sub`, `begin` and `cnt`
fields of its own (the `VectorView` store class is gone), a map entry has the
vector fields, and a transient vector is its own state. docs/value-layout.md
has the notes.

Binaries: before `light-cljs-ef5c9df2` (master), after `light-cljs-e1631216`
(a work-in-progress commit of this pellet; the review fixes after it only
add a presence test behind an undefined `cnt`, which a defined count skips,
own data properties in `toJSON` and a compile-time check of multimethod
tables), in research/shadow-credibility/bin. Unreserved: one peer worker
was active.

**Gate** (`bb benchmarks/core-ops/run.bb --ops vec-conj,vec-literal,vec-assoc,transient-vec,vec-nth,vec-peek,vec-invoke,destructure-vec,eq-vec,hash-fresh-vec,mapv,reduce-vec,reduce-entries-small`,
5 runs, the same selection both times; 1-minute load average 8.17-11.29
before, 2.56-7.10 after), ns per operation, median (range):

| op | light before | light after | change | shadow-cljs (after run) | light/shadow before | light/shadow after |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| vec-conj-64 | 3,144 (3,014-3,230) | 2,154 (2,133-2,400) | -32% | 1,651 | 1.93 | 1.30 |
| vec-literal-4 | 22.9 (22.3-23.2) | 24.5 (24.3-24.6) | +7% | 7.94 | 2.99 | 3.09 |
| vec-nth-1024 | 7.45 (7.17-7.77) | 6.40 (5.93-7.13) | -14% | 5.16 | 1.46 | 1.24 |
| vec-invoke-1024 | 5.83 (5.68-6.70) | 5.99 (5.70-6.04) | +3% | 4.84 | 1.20 | 1.24 |
| vec-peek | 4.50 (4.28-4.52) | 4.23 (4.19-4.24) | -6% | 3.48 | 1.29 | 1.22 |
| vec-assoc-1024 | 86.3 (82.5-87.3) | 63.0 (61.1-65.7) | -27% | 76.1 | 1.16 | 0.83 |
| destructure-vec-3 | 19.3 (17.9-20.0) | 16.3 (15.8-16.3) | -16% | 9.88 | 1.96 | 1.65 |
| reduce-vec-1000 | 5,277 (5,016-5,542) | 5,325 (5,227-5,439) | +1% | 2,511 | 2.11 | 2.12 |
| mapv-1000 | 62,737 (59,654-62,956) | 64,267 (64,022-65,253) | +2% | 13,073 | 4.87 | 4.92 |
| reduce-entries-small | 1,264 (1,195-1,288) | 1,319 (1,312-1,323) | +4% | 89.3 | 14.28 | 14.78 |
| eq-vec-10 | 618.7 (591.8-621.4) | 636.0 (635.2-638.2) | +3% | 55.5 | 11.17 | 11.45 |
| hash-fresh-vec-4 | 571.9 (549.1-575.0) | 495.7 (493.4-499.3) | -13% | 35.3 | 16.45 | 14.03 |
| transient-vec-100 | 5,309 (5,263-5,534) | 5,521 (5,444-5,652) | +4% | 1,024 | 5.21 | 5.39 |

- `vec-conj` and `vec-assoc` gain 27-32%: each step builds one object, and
  copying the metadata reads a field instead of probing `metadataValues`;
  `assoc` of a large vector is now faster than shadow-cljs's (0.83).
  `vec-nth` reads the vector's own fields with no store indirection, and
  tells a view by its missing own `tail` field instead of a `sub` lookup
  that walks the vector's prototype chain. `hash-fresh-vec` caches the hash
  in the vector's field instead of a WeakMap entry.
- No operation is more than 10% slower. `vec-literal`, `transient-vec` and
  `eq-vec`, 3-7% slower here, are within what runs of the same binaries
  vary: back to back with one selection (`--ops
  vec-nth,vec-invoke,vec-literal,transient-vec`, an earlier commit of this
  pellet, load 6.34-8.16) `vec-literal` was 14% faster and `transient-vec`
  2% slower. What `transient-vec` and `vec-literal` spend is elsewhere
  (`conj!`'s protocol dispatch, the literal's array copy); the transient
  itself no longer builds a state object.
- The first gate of this pellet (commit `d621ed82`, before `vectorNth` read
  the fields directly) had `vec-invoke` 26% slower with a wide range
  (5.79-9.58, load up to 7.0); back to back it was 6% slower, and with the
  direct field reads it is 3% slower in the final run above.

**Workload 200k** (`compare.bb workload-200k-seed42-1026 --runs 5
--runtime-only --runtime 1,10,40`, a copy of the project rebuilt with each
binary just before its run; load average 12.5-13.1 before, 11.4-13.2 after),
wall time, median (range):

| iterations | light before | light after | shadow-cljs (before run) | shadow-cljs (after run) | light/shadow before | light/shadow after |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 0.445 s (0.435-0.456) | 0.423 s (0.413-0.436) | 0.328 s | 0.317 s | 1.36 | 1.33 |
| 10 | 1.325 s (1.215-1.369) | 1.219 s (1.146-1.245) | 0.620 s | 0.614 s | 2.14 | 1.99 |
| 40 | 3.462 s (3.417-3.545) | 3.464 s (3.432-3.476) | 1.328 s | 1.314 s | 2.61 | 2.64 |

Steady state, from the medians at 1 and 40 iterations: light-cljs 77.4 and
78.0 ms per iteration (unchanged within the runs' spread), shadow-cljs 25.6
both times; light/shadow 3.02 and 3.05. The workload's steady state is not
vector-bound: its maps, keyword lookups and seqs are later pellets'. One
iteration (startup and the first pass) is 5% faster.

**Advanced size** (tests/support/ratchet_fixtures.rs, noted there): every
keyed field read spells the key, so programs with vectors grow: `vector-only`
6,051 to 6,824 bytes, `vector-printed` 12,633 to 13,607, `map-only` (its
entries) 9,895 to 10,313, `mapv-count` 48,549 to 51,189, `constant-keys`
63,142 to 66,159, `features` 92,705 to 96,306; programs without a vector move
by under 100 bytes. re-frame simple and TodoMVC gzip sizes were not measured
in this step. Measured afterwards (light-cljs-1032, master `ef5c9df2` to
`518edac7`, light-cljs and esbuild bundle with React, gzip -9): simple 112,321
to 112,906 (+585), TodoMVC 142,331 to 142,911 (+580), flow 340,990 to
341,552 (+562); see "re-frame simple, TodoMVC and flow sizes across the
group" below.

Raw results (the core-ops EDN and Markdown of every run above, the first
gate and the back-to-back recheck included, and both workload runs):
research/output-speed/vector-fields-1026.

## 2026-11-16: lists, conses, ranges, repeat, iterate and cycle as classes (light-cljs-1030)

**What.** A list, the empty list, a cons, an integer or float range, a
repeat, an iterate, a cycle, a map's key or value seq, a `re-seq` step and
a sorted map's seq are each one object of an internal class on its type's
prototype (`ListCell`, `ConsCell` and the others, kinds 12 to 15 and 22 to
27), with their data in own keyed fields and getters of the prototype where
a step computes `first` or `rest`. Each was `Object.create` plus a frozen
payload in the private-field store `cells`, often with closures; the store
is deleted. They have keyed `first`, `rest`, `next` and `seq` slots, and
lists and integer ranges `count`, which the slot-first wrappers call.
docs/value-layout.md has the notes.

Binaries: before `light-cljs-518edac7` (master, with light-cljs-1026),
after `light-cljs-20d3a2e8` (this pellet's src/), in
research/shadow-credibility/bin. Unreserved: one peer worker had just
finished. Raw results: research/output-speed/list-classes-1030 (`*-before2`,
`*-after2`; the `*-before`/`*-after` files are the same comparison before
the rebase onto light-cljs-1026, `ef5c9df2` against `d0521f4e`).

**Gate** (`bb benchmarks/core-ops/run.bb --ops seq-walk-vec,lazy-walk,reduce-range,variadic-call,apply-vec,filter-count,into-xform --runs 3`,
the same selection both times; 1-minute load average 2.81-3.15 before,
3.70-4.09 after), ns per operation, median (range):

| op | light before | light after | change | shadow-cljs (after run) | light/shadow before | light/shadow after |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| apply-vec | 348.9 (348.0-354.7) | 250.2 (246.7-252.7) | -28% | 51.3 | 6.85 | 4.88 |
| variadic-call | 9.74 (9.68-9.76) | 9.90 (9.87-9.97) | +2% | 9.03 | 1.09 | 1.10 |
| seq-walk-vec-1000 | 23,425 (23,374-23,603) | 28,554 (28,354-28,868) | +22% | 9,786 | 2.40 | 2.92 |
| lazy-walk-1000 | 41,911 (41,825-42,137) | 46,944 (46,729-47,776) | +12% | 38,744 | 1.09 | 1.21 |
| reduce-range-1000 | 4,719 (4,638-4,798) | 5,056 (4,924-5,110) | +7% | 5,338 | 0.89 | 0.95 |
| filter-count-1000 | 35,485 (34,716-36,198) | 36,275 (36,054-36,948) | +2% | 30,912 | 1.17 | 1.17 |
| into-xform-1000 | 76,806 (75,019-78,298) | 81,017 (80,945-82,018) | +5% | 12,991 | 5.89 | 6.24 |

- `apply-vec` (`(apply + i v)`, a cons of `i` onto the vector's seq per
  call) gains 28%: the cons is one object, and `apply`'s walk of it calls
  its `first`, `rest` and `next` slots, where each step probed the private
  stores and reached the cells' kind table branches.
- **`seq-walk-vec` and `lazy-walk` read slower: a loss, recorded as found.**
  Timed as a pair (`--ops seq-walk-vec,lazy-walk --runs 3`, load 9.26-9.88,
  `recheck2-*`), before then after: `lazy-walk` 36,233 and 39,905 ns
  (+10%), `seq-walk-vec` 26,497 and 31,147 (+18%). Neither builds a cell
  in its loop, but `lazy-walk` maps over a range, which now takes the
  slot-first path with slots shared by the cell classes, and the empty list
  that ends both walks now has a kind and slots, so the wrappers' property
  reads see more classes; `seq-walk-vec` runs after `lazy-walk` and inherits
  that feedback. Before the rebase, `seq-walk-vec` timed with no lazy
  operation before it was unchanged (18,456 and 18,568 ns, +1%,
  `recheck-*`). In the whole gate below, neither moves by more than 10%.
  light-cljs-1016 (direct slot calls at `:advanced`) removes the shared
  wrappers these walks go through.
- **The whole gate** (all 64 operations, `--runs 3`; load 2.60-4.10 before,
  3.08-4.58 after): no operation reads more than 10% slower; `apply-vec`
  343.3 to 284.6 ns, `vec-conj-64` 2,724 to 2,266 and `transient-map-100`
  23,054 to 19,082 read 17% faster. Before the rebase, the three ops that
  read more than 10% slower there (`vec-invoke-1024`, `destructure-vec-3`,
  `sort-by-100`) were within 3% timed back to back (`recheck-*`).

**Workload 200k** (`compare.bb workload-200k-seed42-1030 --runs 5
--runtime-only --runtime 1,10,40`, a copy of the project rebuilt with each
binary just before its run), wall time, median (range):

| iterations | light before | light after | shadow-cljs (before run) | shadow-cljs (after run) |
| --- | ---: | ---: | ---: | ---: |
| 1 | 0.415 s (0.411-0.431) | 0.437 s (0.397-0.459) | 0.305 s | 0.306 s |
| 40 | 3.279 s (3.099-3.579) | 3.044 s (3.007-3.083) | 1.137 s | 1.270 s |

Steady state, from the medians at 1 and 40 iterations: light-cljs 73.4 to
66.9 ms per iteration (-9%); shadow-cljs 21.3 and 24.7, the after run under
more load, so light/shadow 3.45 to 2.71 overstates the gain. Before the
rebase (`ef5c9df2` against `d0521f4e`, load 3.71-4.33 and 2.69-3.04):
light-cljs 72.7 to 62.3 ms (-14%), shadow-cljs 22.7 and 21.6, light/shadow
3.21 to 2.88.

**Advanced size** (tests/support/ratchet_fixtures.rs, noted there):
programs that keep the cells grow 4 to 5 KB, mostly the 43 slots with their
long keyed names and the cell classes: `seq-pipeline` 42,513 bytes,
`mapv-count` 53,201, `keyword-lookups` 52,734, `constant-keys` 68,266,
`event-maps` 50,318, `features` 98,251 (before the rebase; 51,306 and
102,500 on light-cljs-1026, with the sorted map seq's slots); programs that keep none less,
through the cells' test the older layers call (`rest-fn` 16,668,
`atom-str` 4,047). re-frame simple and TodoMVC gzip sizes were not measured
in this step.

## 2026-11-16: protocol masks, markers and slots of the vector classes (light-cljs-1028)

**What.** The prototypes of vectors, subvectors, map entries and transient
vectors declare the core protocols their values implement, as upstream's
deftypes do: keyed protocol mask bits and the keyed `IFind` marker, exactly
the protocols the native tests answered, so `implements?` and `satisfies?`
answer as before. Subvectors and map entries get the keyed `-nth`, `-count`,
`-peek`, `-conj`, `-assoc`, `-lookup` and `-seq` slots vectors have, and
transient vectors `-nth`, `-count` and `-lookup`, so the slot-first wrappers
serve them where they took the older layers before. docs/value-layout.md
has the notes.

Binaries: before `light-cljs-518edac7` (master), after
`light-cljs-cb060c15` (a work-in-progress commit of this pellet; what
follows it changes tests and documents only), in research/shadow-credibility/bin.
Unreserved: one peer worker was active.

**Gate** (`bb benchmarks/core-ops/run.bb --ops vec-conj,vec-literal,vec-assoc,transient-vec,vec-nth,vec-peek,vec-invoke,destructure-vec,eq-vec,hash-fresh-vec,mapv,reduce-vec,reduce-entries-small,reduce-kv-small,seq-walk-vec`,
5 runs, the same selection both times; 1-minute load average 2.47-2.83
before, 3.15-4.28 after), ns per operation, median (range):

| op | light before | light after | change | shadow-cljs (after run) | light/shadow before | light/shadow after |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| vec-conj-64 | 2,227 (2,067-2,308) | 2,146 (2,089-2,419) | -4% | 1,421 | 1.37 | 1.51 |
| vec-literal-4 | 23.6 (23.1-23.7) | 23.8 (23.6-24.0) | +1% | 7.67 | 3.07 | 3.10 |
| vec-nth-1024 | 5.87 (5.82-6.59) | 6.02 (5.83-7.41) | +3% | 5.03 | 1.18 | 1.20 |
| vec-invoke-1024 | 5.77 (5.67-6.89) | 5.82 (5.74-6.00) | +1% | 4.72 | 1.19 | 1.23 |
| vec-peek | 4.09 (4.07-4.16) | 4.09 (4.08-4.23) | +0% | 3.38 | 1.21 | 1.21 |
| vec-assoc-1024 | 63.1 (59.7-64.1) | 61.8 (59.6-65.4) | -2% | 73.9 | 0.87 | 0.84 |
| destructure-vec-3 | 15.8 (15.2-16.1) | 15.4 (15.0-15.8) | -2% | 9.63 | 1.64 | 1.60 |
| reduce-vec-1000 | 5,190 (5,153-5,277) | 5,235 (5,148-5,303) | +1% | 2,431 | 2.13 | 2.15 |
| mapv-1000 | 62,248 (61,325-62,984) | 61,879 (61,037-63,433) | -1% | 12,418 | 4.98 | 4.98 |
| seq-walk-vec-1000 | 17,856 (17,695-17,893) | 17,808 (17,525-18,144) | +0% | 6,654 | 2.64 | 2.68 |
| reduce-kv-small | 162.7 (161.6-163.5) | 162.7 (162.0-167.1) | +0% | 37.2 | 4.36 | 4.37 |
| reduce-entries-small | 1,283 (1,265-1,301) | 703.7 (690.0-731.5) | -45% | 86.0 | 14.95 | 8.18 |
| eq-vec-10 | 680.4 (674.3-682.2) | 584.4 (577.8-596.9) | -14% | 54.4 | 12.61 | 10.75 |
| hash-fresh-vec-4 | 499.3 (492.9-502.0) | 519.9 (518.5-534.6) | +4% | 44.2 | 11.24 | 11.75 |
| transient-vec-100 | 5,474 (5,261-5,570) | 5,421 (5,388-5,441) | -1% | 997.7 | 5.45 | 5.43 |

- `reduce-entries-small` gains 45%: destructuring each map entry the
  reduction hands its function (`[_ v]`) calls `nth`, which the entry now
  answers from its own slot instead of the kind tables and the protocol
  lookup. `eq-vec` gains 14% in each of the three gates of this pellet: the
  protocol tests that `=` makes of each vector (`sequential?` and the like,
  through `ownProtocol`) answer from the mask bit in one read, where they
  took the native test, which probes the payload stores (not profiled).
- No operation is more than 10% slower. `hash-fresh-vec`, 4% slower in each
  of the three gates of this pellet, keeps its path (the hash of a vector
  reads `meta` and the items through the older layers): the masks put one
  more keyed-kind read on the older layers' protocol lookup of a runtime
  class's value.
- The first gate of this pellet (commit `b0d64d43`, load 3.22-3.70) had
  `vec-conj` 23% and `vec-assoc` 14% slower: `conj` and `assoc` copy the
  metadata through `meta`, whose protocol lookup for a program's method
  (`ownMethod`) found the new `IMeta` mask bit and then looked for a slot by
  computed name on every call. The older layers' lookups now read a mask
  bit only for a value without a keyed kind (docs/value-layout.md, the 1028
  notes), and both operations are back at their times (the table above).

**Workload 200k** (`compare.bb workload-200k-seed42-1028 --runs 5
--runtime-only --runtime 1,10,40`, a copy of the project rebuilt with each
binary just before its run, the two runs back to back; load average
3.71-4.12 before, 3.62-4.25 after), wall time, median (range):

| iterations | light before | light after | shadow-cljs (before run) | shadow-cljs (after run) | light/shadow before | light/shadow after |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 0.406 s (0.401-0.414) | 0.403 s (0.399-0.404) | 0.307 s | 0.299 s | 1.32 | 1.35 |
| 10 | 1.131 s (1.119-1.138) | 1.113 s (1.106-1.132) | 0.559 s | 0.552 s | 2.02 | 2.02 |
| 40 | 3.237 s (3.219-3.259) | 3.186 s (3.153-3.205) | 1.175 s | 1.178 s | 2.75 | 2.70 |

Steady state, from the medians at 1 and 40 iterations: light-cljs 72.6 ms
per iteration before and 71.4 after (2% faster, within the runs' spread),
shadow-cljs 22.2 and 22.5; light/shadow 3.26 and 3.17. An earlier pair of
runs (the after one with `b0d64d43`), the first under a Pants test run
(load 14.4-15.0) and the second at load 3.2-3.4, is not comparable and is
kept with the raw results.

**Advanced size** (tests/support/ratchet_fixtures.rs, noted there): every
program with vectors carries the masks and the marker (`vector-only` 6,824
to 7,089 bytes, `vector-printed` 13,607 to 13,875), and a program with maps
the entries' slots and the slot-first wrappers of the operations it keeps
(`map-only` 10,313 to 11,372, `mapv-count` 51,189 to 52,079,
`constant-keys` 66,159 to 67,882, `features` 96,306 to 99,368). TodoMVC
(research/todomvc-port, one clean `release client --no-cache` per binary):
light-cljs's own module, before esbuild adds React, 280,888 to 286,232
bytes (+1.9%), 80,260 to 80,917 with gzip -9 (+657 bytes, +0.8%). re-frame
simple was not measured: its port could not be generated in this worktree
(`re-frame.bb port simple` failed in the porter's shadow-cljs run).

Raw results (the core-ops EDN and Markdown of every gate above, the first
two included, and every workload run): research/output-speed/vector-protocols-1028.

## 2026-11-16: array, hash and sorted maps and map transients as classes (light-cljs-1012)

**What.** An array map, a hash map and a sorted map are each one object of an
internal class on its type's prototype (`ArrayMapCell`, `HashMapCell`,
`TreeMapCell`, kinds 28 to 30), holding its entries (an array map's and a
sorted map's entry array, a hash map's trie) unfrozen in an own keyed field,
and a sorted map its comparator; a map transient (`TransientArrayMapCell`,
`TransientHashMapCell`, kinds 31 and 32) holds its state likewise. They were
`Object.create` plus a frozen entry array in the private-field store `maps`,
a kind in `mapKinds`, a sorted map's comparator in the `sortedComparators`
WeakMap and a hash map's trie in a private field; those stores are gone for
the maps (records keep theirs until light-cljs-1013). The maps have keyed
`-lookup`, `-count`, `-assoc`, `-contains-key?`, `-conj` and (array and hash
maps) `-dissoc` slots, which the slot-first wrappers call; an array map's
`assoc` copies its entries once and its `dissoc` removes in one loop.
Upstream's flat key, value array and hash map fields are light-cljs-1034's,
and a sorted map as a red-black tree light-cljs-1035's. docs/value-layout.md
has the notes.

Binaries: before `light-cljs-1bd59e34` (master, with light-cljs-1028), after
`light-cljs-d630b6f5` (this pellet's src/), in research/shadow-credibility/bin.
Unreserved: one peer worker was active at first, then none. Raw results:
research/output-speed/map-classes-1012 (`*-before2`, `*-after`; the
`*-before` files are master `bb6e8fad`, before light-cljs-1028 landed, with
TodoMVC under more load).

**Gate** (`bb benchmarks/core-ops/run.bb --ops kw,get,contains,count-small,assoc,dissoc,update,map-literal-3,reduce-map-inc-1000,reduce-kv-small,reduce-entries-small,eq-map-7,hash-fresh-map-7,destructure-map-3,swap-assoc,merge-small,select-keys-3,group-by-100,transient-map-100,sorted-map-assoc-100,str-sorted-map-20,str-nested-map --runs 3`,
the same selection both times; 1-minute load average 2.31-3.87 before,
3.31-4.72 after), ns per operation, median (range):

| op | light before | light after | change | shadow-cljs (after run) | light/shadow before | light/shadow after |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| kw-small | 94.8 (94.6-98.4) | 19.6 (19.2-19.9) | -79% | 9.73 | 10.05 | 2.01 |
| get-small | 106.5 (104.3-108.3) | 22.9 (22.4-23.5) | -79% | 14.8 | 7.46 | 1.55 |
| get-small-miss | 114.3 (113.6-118.3) | 26.5 (26.1-27.3) | -77% | 19.2 | 6.20 | 1.38 |
| contains-small | 134.8 (131.9-135.5) | 25.0 (24.8-26.0) | -81% | 11.9 | 11.63 | 2.10 |
| count-small | 31.8 (30.5-32.2) | 5.45 (5.45-5.60) | -83% | 2.11 | 15.36 | 2.58 |
| assoc-small-existing | 579.2 (577.4-595.7) | 61.5 (60.1-63.1) | -89% | 31.0 | 19.27 | 1.99 |
| assoc-small-new | 613.7 (611.4-637.0) | 81.8 (80.4-84.7) | -87% | 39.5 | 15.98 | 2.07 |
| dissoc-small | 907.6 (896.4-925.4) | 187.6 (182.1-191.1) | -79% | 80.3 | 11.45 | 2.34 |
| update-small | 643.3 (638.8-664.0) | 62.4 (60.0-63.3) | -90% | 44.4 | 15.07 | 1.41 |
| map-literal-3 | 86.2 (84.2-86.8) | 31.3 (29.6-31.6) | -64% | 10.3 | 9.08 | 3.05 |
| get-hash100 | 62.5 (62.4-63.2) | 34.9 (34.1-35.1) | -44% | 12.2 | 5.38 | 2.86 |
| kw-hash100 | 79.5 (79.3-79.5) | 57.6 (56.4-58.7) | -28% | 13.4 | 6.09 | 4.28 |
| assoc-hash100 | 375.7 (375.3-380.9) | 272.7 (264.2-278.6) | -27% | 79.3 | 4.99 | 3.44 |
| dissoc-hash100 | 327.3 (326.7-345.8) | 130.6 (130.3-134.0) | -60% | 77.0 | 4.48 | 1.70 |
| update-hash100 | 443.1 (442.9-454.7) | 322.7 (320.2-328.5) | -27% | 95.7 | 4.86 | 3.37 |
| get-in-3 | 246.1 (241.7-246.3) | 77.3 (76.4-80.3) | -69% | 56.0 | 4.56 | 1.38 |
| assoc-in-3 | 1,540 (1,535-1,584) | 225.2 (223.9-236.7) | -85% | 131.9 | 11.79 | 1.71 |
| update-in-3 | 1,611 (1,605-1,631) | 250.0 (244.2-265.7) | -84% | 134.2 | 12.13 | 1.86 |
| destructure-map-3 | 288.9 (281.5-289.6) | 127.0 (126.8-128.9) | -56% | 48.9 | 6.11 | 2.60 |
| reduce-map-inc-1000 | 47,054 (46,649-48,540) | 46,764 (46,258-46,784) | -1% | 39,653 | 1.23 | 1.18 |
| reduce-kv-small | 136.7 (136.2-141.3) | 83.9 (82.4-85.4) | -39% | 37.5 | 3.73 | 2.24 |
| reduce-entries-small | 559.4 (557.2-573.1) | 490.4 (479.3-497.2) | -12% | 74.9 | 7.25 | 6.54 |
| str-nested-map | 5,820 (5,776-5,891) | 5,707 (5,617-5,722) | -2% | 2,165 | 2.74 | 2.64 |
| eq-map-7 | 1,547 (1,545-1,591) | 513.5 (503.6-515.6) | -67% | 148.3 | 10.70 | 3.46 |
| hash-fresh-map-7 | 1,328 (1,322-1,437) | 799.9 (785.9-807.5) | -40% | 401.4 | 3.32 | 1.99 |
| swap-assoc | 612.1 (605.5-615.8) | 140.5 (137.3-145.1) | -77% | 37.8 | 16.86 | 3.72 |
| merge-small | 5,803 (5,783-5,811) | 4,771 (4,560-4,791) | -18% | 788.6 | 7.57 | 6.05 |
| select-keys-3 | 1,934 (1,928-1,935) | 534.9 (520.3-539.4) | -72% | 200.9 | 10.06 | 2.66 |
| group-by-100 | 67,233 (66,525-68,900) | 13,389 (13,123-13,811) | -80% | 4,315 | 15.78 | 3.10 |
| transient-map-100 | 19,077 (19,049-19,287) | 15,740 (15,561-15,901) | -17% | 3,831 | 5.02 | 4.11 |
| sorted-map-assoc-100 | 3,232 (3,227-3,237) | 622.7 (610.3-647.8) | -81% | 201.2 | 16.50 | 3.10 |
| str-sorted-map-20 | 41,815 (41,726-43,590) | 41,817 (41,497-43,130) | +0% | 20,569 | 2.11 | 2.03 |

- **No operation reads slower.** The small-map operations gain 77-90%
  (`get-small` 106.5 to 22.9 ns, 1.55 times shadow-cljs's;
  `assoc-small-existing` 579.2 to 61.5, 1.99 times; `update-small` 1.41
  times): a lookup, `assoc`, `count` or `contains?` of a map takes its keyed
  slot, where it reached the native code through the kind tables, the
  private-field store test and `publicMethod`, and an array map's `assoc`
  slices an unfrozen array, where `copyEntries` sliced a frozen one, read
  `mapKinds`, `sortedComparators` and the record brand, and froze the copy
  again.
- `group-by-100`, `select-keys-3`, `assoc-in-3` and `update-in-3` gain
  72-85% through the same `get` and `assoc`; `sorted-map-assoc-100` gains
  81% (its `assoc` slot skips the store probes; the insertion is still an
  array splice, light-cljs-1035).
- What moves little: `reduce-map-inc-1000`, `str-nested-map` and
  `str-sorted-map-20` (printing and reduction walk the entries as before),
  `merge-small` (-18%) and `transient-map-100` (-17%), whose `conj` and
  `assoc!` paths still take the older layers, and `reduce-entries-small`
  (-12%), which builds map entries.

**Workload 200k** (`compare.bb workload-200k-seed42-1012 --runs 5
--runtime-only --runtime 1,10,40`, the project rebuilt with each binary just
before its run; load 2.23-2.89 before, 3.27-3.65 after), wall time, median
(range):

| iterations | light before | light after | shadow-cljs (before run) | shadow-cljs (after run) |
| --- | ---: | ---: | ---: | ---: |
| 1 | 0.398 s (0.394-0.400) | 0.388 s (0.367-0.389) | 0.307 s | 0.295 s |
| 40 | 2.879 s (2.670-2.911) | 2.681 s (2.552-2.716) | 1.204 s | 1.201 s |

Steady state, from the medians at 1 and 40 iterations: light-cljs 63.6 to
58.8 ms per iteration (-8%); shadow-cljs 23.0 and 23.2; light/shadow 2.77 to
2.53.

**The re-frame-shaped app** (`compare.bb re-frame-200k-seed42-1012 --runs 5
--runtime-only --runtime 1,10`; load 2.52-3.11 before, 2.89-3.68 after):
1 iteration 2.466 s (2.437-2.496) to 2.344 s (2.272-2.409), 10 iterations
9.472 s (8.963-9.554) to 7.710 s (7.685-7.809); steady state 778.4 to 596.2
ms per iteration (-23%), shadow-cljs 264.0 and 244.5, light/shadow 2.95 to
2.44.

**TodoMVC** (`re-frame.bb runtime todomvc --runs 5`, ported with
`shadow-port-tool-983`; load 4.00-4.19 before, 3.40-3.80 after): the add
step (1,000 todos) 4,826.6 ms (4,742.8-4,888.6) to 3,922.8 ms
(3,893.7-3,980.7), -19%, light/shadow 5.26 to 4.30 (shadow-cljs 918.3 and
912.3 ms); toggle all 100.7 to 65.4 ms; page load to interactive 44.2 to
41.3 ms; the heap with 1,000 todos 9.18 and 9.20 MB.

**Advanced size** (tests/support/ratchet_fixtures.rs, noted there): programs
with maps keep the maps' 17 slot units, their helpers and two more
slot-first wrappers (`map-only` 12,893 bytes, `map-printed` 19,471,
`constant-keys` 76,472, `event-maps` 54,662, `features` 110,364); programs
without maps keep the maps' and map transients' tests that `count` reads
(`vector-only` 7,282, `keyword-only` 4,746).

**re-frame simple and TodoMVC sizes** (`re-frame.bb port APP` then
`re-frame.bb size simple todomvc`, each with the binary named;
`size-1bd59e34.txt` and `size-d630b6f5.txt` in the raw results), the
light-cljs and esbuild bundle with React, before and after:

| app | raw bytes | gzip -9 | brotli -q 11 |
| --- | ---: | ---: | ---: |
| re-frame simple | 372,127 to 377,242 (+5,115) | 114,215 to 115,070 (+855) | 99,247 to 99,855 (+608) |
| TodoMVC | 473,022 to 478,146 (+5,124) | 144,153 to 145,058 (+905) | 124,430 to 125,393 (+963) |

shadow-cljs's whole release files are 101,065 and 137,126 bytes gzip.

## 2026-11-16: the empty list off `seq`'s slot path (light-cljs-1033)

**What.** After light-cljs-1030 the empty list has a kind and slots, so
`seq`'s slot-first wrapper called its slot at the end of every walk (a
`rest` at the end gives the empty list, and `next` takes its `seq`). V8's
inline-cache log showed it as a fifth class at the wrapper's slot read and
call, beside a vector, its chunked seq, a lazy seq and a chunked cons:
once a process had walked a vector and a lazy seq, both went megamorphic,
and every step of every later walk paid. The wrapper now sends the empty
list to the operation, as before light-cljs-1030. Its first form made the
wrapper's bytecode 102 bytes, over the 100 that V8's mid tier (Maglev)
inlines, and seq-walk-vec alone was then 20% slower (round 2 in the raw
results, `33314f17`); the empty list and `noBranch` are now parameters of
the wrapper's maker, which need no check for an uninitialized `const`, and
the wrapper is 94 bytes. docs/value-layout.md has the notes.

Binaries in research/shadow-credibility/bin: `light-cljs-518edac7` (before
light-cljs-1030, the pellet's reference), `light-cljs-765a92e1` (master)
and `light-cljs-04d4aa12` (this change, before its commit gained these
notes; the runtime is the same). Unreserved: one peer worker was
active.

**Gate** (`bb benchmarks/core-ops/run.bb --runs 3`, each selection as
light-cljs-1030 timed it, two rounds with the binaries' order reversed;
1-minute load average 3.2-4.2 in the first, 3.9-5.4 in the second), light
ns per operation, median of the round's three runs, round one / round two:

| selection | op | 518edac7 | master 765a92e1 | this change | change against 518edac7 |
| --- | --- | ---: | ---: | ---: | ---: |
| pair | lazy-walk-1000 | 34,263 / 35,738 | 38,671 / 39,138 | 34,364 / 35,076 | +0% / -2% |
| pair | seq-walk-vec-1000 | 25,246 / 26,387 | 29,623 / 30,247 | 25,300 / 26,051 | +0% / -1% |
| 7-op | lazy-walk-1000 | 41,803 / 42,237 | 45,912 / 46,295 | 43,748 / 42,901 | +5% / +2% |
| 7-op | seq-walk-vec-1000 | 23,704 / 23,463 | 27,598 / 28,435 | 24,847 / 24,087 | +5% / +3% |
| 7-op | apply-vec | 349.0 / 345.8 | 199.1 / 205.6 | 216.3 / 208.8 | -38% / -40% |
| alone | seq-walk-vec-1000 | 19,129 / 18,289 | 19,242 / 18,792 | 19,333 / 18,963 | +1% / +4% |

The pair is `--ops seq-walk-vec,lazy-walk`, the 7-op selection
`--ops filter-count,into-xform,lazy-walk,seq-walk-vec,reduce-range,variadic-call,apply-vec`.
Master was 9-13% (lazy-walk) and 15-21% (seq-walk-vec) slower than
`518edac7` in these runs; this change is within 5% in every one. The
7-op selection's first round of this change ran while the shadow-cljs
numbers also rose 3-5%. `apply-vec` keeps light-cljs-1030's gain, but is
2-9% slower than master: each call takes `seq` of the empty list once or
twice, now through the operation's path (about 27 ns, where the slot took
11 and the older layers 36). The whole gate (all 64 operations in one
process, master `1bd59e34` at load 5.5-14.0 against `583567da`, the same
runtime change before the rebase, at load 2.1-6.6) has no operation more
than 4% slower (filter-count), lazy-walk +1% and seq-walk-vec -1%: with
every operation in one process the wrappers see many classes either way.

**Workload 200k** (`compare.bb workload-200k-seed42-1033 --runs 5
--runtime-only --runtime 1,10,40`, a copy of the project rebuilt with each
binary just before its run, the two runs back to back; load 3.9-5.2
before, 3.2-3.7 after), wall time, median (range):

| iterations | master 765a92e1 | this change | shadow-cljs (master run) | shadow-cljs (this run) |
| --- | ---: | ---: | ---: | ---: |
| 1 | 0.378 s (0.371-0.390) | 0.374 s (0.371-0.396) | 0.293 s | 0.295 s |
| 10 | 0.951 s (0.940-0.976) | 0.932 s (0.923-0.959) | 0.541 s | 0.529 s |
| 40 | 2.552 s (2.514-2.580) | 2.595 s (2.523-2.598) | 1.140 s | 1.134 s |

Steady state, from the medians at 1 and 40 iterations: light-cljs 55.8 ms
per iteration before and 56.9 after (2% slower, within the runs' spread;
10 iterations are 2% faster), shadow-cljs 21.7 and 21.5; light/shadow 2.57
and 2.64. An earlier pair of runs (master `1bd59e34` against `583567da`,
load 20-23) gave 73.0 and 73.8 ms.

**Advanced size** (tests/support/ratchet_fixtures.rs, noted there):
programs that keep the wrapper grow 17 bytes (`seq-pipeline` 43,707 to
43,724, `features` 110,364 to 110,381), under every ceiling; the readable
runtime grows 473 bytes. TodoMVC and re-frame simple were not rebuilt for
17 bytes.

Raw results (every gate run's EDN and Markdown, rounds 1-3, the whole
gate and both workload pairs): research/output-speed/empty-list-seq-1033.

## 2026-11-16: array maps' flat key, value arrays and hash maps' own fields (light-cljs-1034)

**What.** An array map (`ArrayMapCell`) holds upstream's `cnt` and flat key,
value array `arr` (`[k0, v0, k1, v1, ...]`) in its own keyed fields, where
light-cljs-1012 left an array of entry objects in one `entries` field; a hash
map (`HashMapCell`) holds its count, its trie's root, its nil entry and a
builder's entries (`cnt`, `root`, `nil`, `lit`), where 1012 left a trie object
`{root, nil, length}`. `get`, `assoc`, `dissoc`, `count`, `seq`, `reduce-kv`,
map equality and hashing read the fields (`arrayIndex`, `mapPairs`, the hash
map slots' own trie paths), a hash map's `assoc` builds the map and the path of
new nodes and no trie object, and leaf entries are no longer frozen. Advanced
output's constant lookups read `rt$mapValueAt(m, i)`. The older layers,
printing and transients still read entries through `maps` (light-cljs-1036 has
the map transients' fields and the rest). docs/value-layout.md has the notes.

Binaries: before `light-cljs-765a92e1` (master), after `light-cljs-9a5cde99`
(this pellet's src/), in research/shadow-credibility/bin. Unreserved: one peer
worker was active. Raw results: research/output-speed/map-fields-1034
(`*-before`, `*-after`; `gate-c212*` and `gate-d6f9*` are intermediate
binaries of this pellet). The review's fixes after `9a5cde99` change the
Pants profile and, in `arrayIndex`, only a key without an `fqn` field.

**Gate** (`bb benchmarks/core-ops/run.bb --ops kw,get,contains,count-small,assoc,dissoc,update,map-literal-3,reduce-map-inc-1000,reduce-kv-small,reduce-entries-small,eq-map-7,hash-fresh-map-7,destructure-map-3,swap-assoc,merge-small,select-keys-3,group-by-100,transient-map-100,sorted-map-assoc-100,str-sorted-map-20,str-nested-map --runs 3`,
the same selection as light-cljs-1012's; 1-minute load average 3.55-6.86
before, 2.37-3.53 after), ns per operation, median (range):

| op | light before | light after | change | shadow-cljs (after run) | light/shadow before | light/shadow after |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| kw-small | 20.0 (19.4-20.0) | 20.1 (20.0-20.4) | 0% | 9.44 | 2.11 | 2.13 |
| get-small | 23.0 (22.9-23.4) | 22.4 (22.4-24.5) | -2% | 14.3 | 1.61 | 1.57 |
| get-small-miss | 26.7 (26.5-26.9) | 29.2 (29.1-29.6) | +9% | 18.5 | 1.44 | 1.58 |
| contains-small | 25.3 (25.2-25.5) | 26.7 (26.4-26.7) | +6% | 11.6 | 2.18 | 2.30 |
| count-small | 5.45 (5.43-5.55) | 2.84 (2.82-2.87) | -48% | 2.03 | 2.68 | 1.40 |
| assoc-small-existing | 60.8 (59.5-61.5) | 58.1 (57.9-59.2) | -4% | 29.5 | 2.07 | 1.97 |
| assoc-small-new | 80.6 (79.2-81.2) | 81.6 (80.6-81.8) | +1% | 37.7 | 2.13 | 2.16 |
| dissoc-small | 185.2 (184.8-188.7) | 103.4 (102.5-107.9) | -44% | 76.3 | 2.43 | 1.35 |
| update-small | 61.0 (60.6-61.2) | 60.4 (59.3-61.5) | -1% | 42.1 | 1.43 | 1.43 |
| map-literal-3 | 31.1 (29.9-31.4) | 19.5 (19.4-20.4) | -37% | 9.51 | 3.29 | 2.05 |
| get-hash100 | 35.5 (34.9-35.8) | 34.9 (34.3-34.9) | -2% | 11.7 | 3.13 | 2.98 |
| kw-hash100 | 57.8 (57.0-57.9) | 56.9 (55.8-57.5) | -2% | 12.9 | 4.51 | 4.41 |
| assoc-hash100 | 280.3 (267.9-281.7) | 115.3 (114.7-117.8) | -59% | 75.7 | 3.74 | 1.52 |
| dissoc-hash100 | 132.6 (132.1-133.7) | 124.0 (123.6-127.8) | -6% | 73.0 | 1.82 | 1.70 |
| update-hash100 | 322.2 (314.7-326.4) | 145.5 (145.4-149.9) | -55% | 92.8 | 3.44 | 1.57 |
| get-in-3 | 77.0 (76.7-77.3) | 72.5 (70.8-72.9) | -6% | 52.9 | 1.45 | 1.37 |
| assoc-in-3 | 228.2 (218.3-230.2) | 216.6 (215.3-223.3) | -5% | 128.3 | 1.77 | 1.69 |
| update-in-3 | 250.6 (248.9-253.0) | 242.2 (239.4-242.6) | -3% | 131.0 | 1.92 | 1.85 |
| destructure-map-3 | 123.5 (122.7-125.6) | 114.4 (113.8-118.2) | -7% | 47.5 | 2.61 | 2.41 |
| reduce-map-inc-1000 | 48,175 (47,905-48,243) | 46,045 (45,870-46,782) | -4% | 38,952 | 1.27 | 1.18 |
| reduce-kv-small | 84.6 (82.7-84.7) | 48.2 (47.8-49.7) | -43% | 36.8 | 2.32 | 1.31 |
| reduce-entries-small | 489.6 (487.1-492.1) | 500.3 (497.0-504.7) | +2% | 77.0 | 6.35 | 6.50 |
| str-nested-map | 5,706 (5,690-5,738) | 5,780 (5,769-5,826) | +1% | 2,102 | 2.68 | 2.75 |
| eq-map-7 | 517.6 (509.4-521.6) | 374.7 (365.8-378.3) | -28% | 145.6 | 3.58 | 2.57 |
| hash-fresh-map-7 | 801.5 (772.8-802.1) | 781.3 (761.3-806.5) | -3% | 394.4 | 2.02 | 1.98 |
| swap-assoc | 138.2 (137.5-138.8) | 129.5 (124.2-137.0) | -6% | 36.1 | 3.73 | 3.58 |
| merge-small | 4,637 (4,471-4,655) | 3,433 (3,415-3,443) | -26% | 763.0 | 6.05 | 4.50 |
| select-keys-3 | 526.0 (522.9-529.7) | 472.0 (469.5-492.7) | -10% | 190.3 | 2.76 | 2.48 |
| group-by-100 | 13,335 (13,242-13,939) | 12,126 (12,016-12,342) | -9% | 4,166 | 3.22 | 2.91 |
| transient-map-100 | 15,865 (15,450-16,187) | 13,337 (13,228-14,021) | -16% | 3,855 | 4.09 | 3.46 |
| sorted-map-assoc-100 | 633.1 (628.0-635.4) | 647.1 (628.4-665.9) | +2% | 200.3 | 3.20 | 3.23 |
| str-sorted-map-20 | 41,757 (41,187-42,337) | 41,929 (41,691-44,492) | 0% | 19,847 | 2.11 | 2.11 |

- **No operation is more than 10% slower.** The slowest, `get-small-miss`
  (+9%) and `contains-small` (+6%), search a 7-key keyword map without the
  key: the flat array's mixed keys and values tell V8 nothing of a key's
  class, where an entry's `key` field did, so each key that is not the one
  sought is a load and an `fqn` read of an untyped value. Reading `fqn`
  before the kind test (upstream's `array-index-of-keyword?`) brought the
  first measured version's +14% and +20% to these.
- **Hash maps:** `assoc-hash100` and `update-hash100` gain 55-59% (1.52
  and 1.57 times shadow-cljs's, from 3.74 and 3.44): an `assoc` no longer
  builds a trie object, freezes its entry or goes through `updatedMap`.
- **Array maps:** `count-small` reads `cnt` (-48%), `dissoc-small` copies
  the pairs around the key (-44%, 1.35 times shadow-cljs's), `reduce-kv`
  walks `arr` without a `[k, v]` array per entry (-43%), map literals push
  their pairs (-37%), map equality walks the pairs (-28%), and `merge`,
  `select-keys`, `group-by` and transients gain 9-26% through them.
- What moves little: printing (`str-nested-map` +1%) and `reduce` over a
  map's entries (+2%), which read entries made of the pairs, and sorted maps
  (light-cljs-1035).

**Workload 200k** (`compare.bb workload-200k-seed42-1034 --runs 5
--runtime-only --runtime 1,10,40`, the project rebuilt with each binary just
before its run; load 3.20-4.54 before, 2.30-2.43 after), wall time, median
(range):

| iterations | light before | light after | shadow-cljs (before run) | shadow-cljs (after run) |
| --- | ---: | ---: | ---: | ---: |
| 1 | 0.412 s (0.408-0.416) | 0.370 s (0.368-0.390) | 0.322 s | 0.293 s |
| 40 | 2.883 s (2.735-2.945) | 2.501 s (2.486-2.547) | 1.292 s | 1.129 s |

Steady state, from the medians at 1 and 40 iterations: light-cljs 63.3 to
54.6 ms per iteration; shadow-cljs 24.9 and 21.4, as fast again on the
quieter machine, so light/shadow stays 2.55: the workload's maps gain no
more than the load moved.

**The re-frame-shaped app** (`compare.bb re-frame-200k-seed42-1034 --runs 5
--runtime-only --runtime 1,10`; load 4.30-4.54 before, 2.43-2.78 after):
1 iteration 2.446 s (2.360-2.858) to 2.268 s (2.193-2.370), 10 iterations
8.636 s (8.148-8.833) to 7.594 s (7.532-7.718); steady state 687.8 to 591.9
ms per iteration (-14%), shadow-cljs 256.0 and 236.5, light/shadow 2.69 to
2.50.

**TodoMVC** (`re-frame.bb runtime todomvc --runs 5`, ported with
`shadow-port-tool-983`; load 3.62-3.99 before, 5.92-6.30 after): the add step
(1,000 todos) 3,995.2 ms (3,894.1-4,051.2) to 4,031.8 ms (3,964.9-4,069.9),
+1%, light/shadow 4.38 to 4.41 (shadow-cljs 913.1 and 914.8 ms): no change
beyond the load; toggle all 66.7 to 67.5 ms; page load to interactive 41.8 to
42.5 ms; the heap with 1,000 todos 9.20 to 9.05 MB.

**Advanced size** (tests/support/ratchet_fixtures.rs, noted there): programs
with maps keep the flat search and the field paths (`map-only` 14,270 bytes,
`map-printed` 21,365, `event-maps` 56,355, `features` 113,191); programs
without maps keep the longer `maps` and `mapOf` and `count`'s and `hash`'s
array-map branches (`vector-only` 7,519, `keyword-only` 5,102). TodoMVC's
bundle with React (`re-frame.bb size todomvc`): raw 478,192 to 481,129
bytes, gzip -9 145,067 to 146,013, brotli -q 11 125,364 to 126,047; re-frame
simple was not ported in this run.

## 2026-11-16: a symbol's metadata in its own field (light-cljs-1031)

A symbol keeps its metadata in its own keyed `meta` field, not the
runtime's `metadataValues` store, so that a second copy of one build reads
the metadata the first gave (the Sol review of light-cljs-1011, finding 1).
`ownMeta`, the test of the values that hold their metadata and hash in
their own fields, now answers kind 21 (symbols) as well as kinds 1 to 3
(vectors, subvectors and map entries). No gate operation uses a symbol's
metadata; the operations below are the ones whose path runs `ownMeta`
(`conj` through `conjMeta`, `preserveMeta` and `meta`; `hash`).

**Gate** (`bb benchmarks/core-ops/run.bb --ops
vec-conj-64,vec-assoc-1024,hash-fresh-vec-4,hash-fresh-map-7,eq-map-7,group-by-100`,
5 runs per compiler, alternated; unreserved, load 4.9-7.5), master
`6f5beb29` against this change's timing binary `a0c72a04`, whose runtime
differs from the change's only in comments and the `SymbolValue.fields`
unit (the gate's output differs only in that unit, which defines five
accessors once as it loads), ns per operation, median of the runs:

| op | master, round 1 | change, round 1 | master, round 2 | change, round 2 (before / after master's run) |
| --- | ---: | ---: | ---: | ---: |
| vec-conj-64 | 2,023 | 2,086 | 2,117 | 2,131 / 2,114 |
| vec-assoc-1024 | 62.6 | 64.7 | 65.7 | 65.7 / 65.7 |
| eq-map-7 | 465.2 | 468.6 | 481.6 | 483.7 / 478.1 |
| hash-fresh-vec-4 | 531.4 | 534.1 | 533.2 | 533.7 / 532.7 |
| hash-fresh-map-7 | 769.4 | 819.0 | 796.4 | 801.6 / 798.4 |
| group-by-100 | 13,063 | 13,712 | 13,651 | 13,581 / 13,545 |

Round 1 ran master first; its change run saw shadow-cljs's numbers rise
1-4% too (load 5.1-7.5 against 4.9-6.4). Round 2 ran the change, then
master, then the change again: every operation is within 1% of master.

Rebased onto light-cljs-1034 (master `87e5db4b` against `175c47ae`, the
same runtime change; one round, master first, load 3.0-5.8 and 3.0-4.5):
vec-conj-64 2,094 to 2,058 ns, vec-assoc-1024 64.2 to 62.7, eq-map-7
393.0 to 384.5, hash-fresh-vec-4 536.2 to 521.1, hash-fresh-map-7 787.4 to
743.2 and group-by-100 12,832 to 12,252 (2-6% faster, as the load fell).
Alone, vec-conj-64 takes 2,003-2,328 ns on master and 2,015-2,276 with
the change (10 runs each, alternated).

**A loss found and avoided.** The first form of `ownMeta`
(`(kind === 1 || kind === 2 || kind === 3) && (...) || kind === 21 &&
"str" in value`, 71 bytes of bytecode against 53) made vec-conj-64 6-10%
slower in the gate (`b2de61eb`: 2,140 to 2,276 and 2,074 to 2,273 ns) and
9% alone, with the other five operations within 2%. In a program with
protocol implementations (`protocolHooks.builtin`), `conj` of a vector
calls `preserveMeta`, which inlines `meta` and `ownMeta`; V8's cumulative
inlining budget (920 bytes) for `conj`'s wrapper had 9 bytes to spare,
and past them the vector's constructor (`VectorCell`) is no longer inlined.
With `--max-inlined-bytecode-size-cumulative=3000` both builds take 1,920
ns. Any growth there lost: a symbol test at the end of `meta` (vec-conj
2,330 against 2,150), 55 bytes with one comparison for kinds 1 to 3, and
47 bytes testing the vector's `cnt` field with `in` alone (fast in 5 of 14
runs against master's 10 of 14, under load 10). The form kept tests
`kind < 4 ? value.cnt !== undefined || "cnt" in value : kind === 21` (45
bytes; fast in 12 of 12 runs against master's 11 of 12): a value made by
reflection from a symbol's prototype now reads an undefined field as its
metadata.

**Advanced size** (tests/support/ratchet_fixtures.rs, noted there, against
master `87e5db4b`): the programs that keep `ownMeta` shrink 13 bytes
(`mapv-count` 58,216), `protocol-params` grows 12 and `features` 28
(113,287); the readable runtime grows 132 bytes. TodoMVC and re-frame
simple were not rebuilt for these bytes.

**Workload 200k** (`compare.bb PROJECT --runs 5 --runtime-only --runtime
1,10,40`). The disk was short, so the project was not copied: PROJECT held
symbolic links to the original's sources, its own `out/` with the
original's shadow-cljs output, and a light-cljs output rebuilt with each
binary just before its run. The two runs were back to back; load was
5.3-6.4 before and 4.5-5.6 after. Wall time, median (range):

| iterations | master 6f5beb29 | this change 72599e59 | shadow-cljs (master run) | shadow-cljs (this run) |
| --- | ---: | ---: | ---: | ---: |
| 1 | 0.396 s (0.394-0.431) | 0.399 s (0.395-0.407) | 0.314 s | 0.308 s |
| 10 | 1.018 s (1.010-1.033) | 1.001 s (0.998-1.022) | 0.578 s | 0.573 s |
| 40 | 2.740 s (2.698-2.757) | 2.724 s (2.590-2.733) | 1.233 s | 1.206 s |

Steady state, from the medians at 1 and 40 iterations: light-cljs 60.1 ms
per iteration before and 59.6 after (within the runs' spread), shadow-cljs
23.6 and 23.0; light/shadow 2.55 and 2.59.

Raw results (each gate run's EDN and Markdown, for the first attempt
`b2de61eb`, the `in` form `7fc05b66`, the kept form `a0c72a04` and the
rebased `175c47ae`, and both workload runs):
research/output-speed/symbol-meta-1031.

## 2026-11-16: map transients' own fields and the maps' field readers (light-cljs-1036)

**What.** A transient array map holds `editable` and `arr`, upstream's flat
key, value array, which `assoc!` and `dissoc!` edit in place; a transient hash
map holds `edit`, `cnt`, `root` and `nil`, where light-cljs-1012 left the old
state object in one `state` field with an array of entry objects or a trie
object. The public hash map constructor's map holds its count and nil entry as
fields, so `count`, `assoc` and `dissoc` of it take the hash map paths instead
of a `publicCollectionSlots` lookup and entries. `maps` no longer answers an
array or hash map's entries: the older layers of `assoc`, `dissoc`, `count`,
`get`, `find`, `hash` and `transient` call the maps' slot functions or read
their fields, and printing (where no protocol implementation can see an
array's seq) and `mapIterator` walk the flat pairs. docs/value-layout.md has
the notes.

Binaries: before `light-cljs-87e5db4b` (master), after `light-cljs-b0854175`
(this pellet's runtime but for a later fix to a public constructor's
odd-length array's transient), in research/shadow-credibility/bin.
Unreserved: one peer worker was active. Raw results:
research/output-speed/map-transients-1036 (`*-before`, `*-after`).

**Gate** (`bb benchmarks/core-ops/run.bb --ops transient-map-100,assoc-small,assoc-hash100,dissoc-small,dissoc-hash100,get-small,get-small-miss,get-hash100,count-small,merge-small,group-by-100,str-nested-map,reduce-entries-small,reduce-kv-small,eq-map-7,hash-fresh-map-7,select-keys-3,update-small,update-hash100,contains-small --runs 3`;
1-minute load average 4.29-10.24 before, 10.82-12.55 after), ns per
operation, median (range):

| op | light before | light after | change | shadow-cljs (after run) | light/shadow before | light/shadow after |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| transient-map-100 | 14,092 (14,036-14,138) | 13,368 (13,366-13,395) | -5% | 3,902 | 3.63 | 3.43 |
| group-by-100 | 13,378 (13,219-13,494) | 13,092 (12,742-13,135) | -2% | 4,424 | 3.07 | 2.96 |
| merge-small | 3,708 (3,662-3,715) | 3,621 (3,617-3,665) | -2% | 826.6 | 4.49 | 4.38 |
| assoc-small-existing | 63.2 (62.6-63.4) | 63.2 (63.0-63.6) | 0% | 31.2 | 2.04 | 2.02 |
| assoc-small-new | 87.8 (87.2-87.9) | 88.2 (85.3-88.8) | 0% | 39.0 | 2.21 | 2.26 |
| assoc-hash100 | 121.2 (121.1-122.8) | 116.0 (115.5-116.8) | -4% | 80.4 | 1.53 | 1.44 |
| dissoc-small | 112.3 (111.3-114.8) | 113.6 (113.1-116.9) | +1% | 79.7 | 1.41 | 1.43 |
| dissoc-hash100 | 137.5 (136.0-144.0) | 128.9 (123.5-129.3) | -6% | 77.6 | 1.78 | 1.66 |
| update-small | 63.4 (63.1-63.5) | 62.8 (62.6-63.7) | -1% | 43.5 | 1.44 | 1.44 |
| update-hash100 | 153.7 (153.3-162.4) | 148.1 (146.8-149.3) | -4% | 99.5 | 1.57 | 1.49 |
| get-small | 27.1 (26.9-27.2) | 27.1 (27.0-27.2) | 0% | 15.0 | 1.82 | 1.81 |
| get-small-miss | 32.8 (32.5-33.0) | 32.8 (32.6-33.0) | 0% | 19.4 | 1.69 | 1.70 |
| get-hash100 | 35.8 (34.9-36.1) | 35.7 (35.7-35.9) | 0% | 12.1 | 2.93 | 2.96 |
| contains-small | 28.0 (27.6-28.1) | 27.8 (27.7-27.8) | -1% | 12.1 | 2.30 | 2.29 |
| count-small | 2.97 (2.94-2.97) | 2.96 (2.96-2.97) | 0% | 2.10 | 1.42 | 1.41 |
| str-nested-map | 6,537 (6,519-8,063) | 6,475 (6,399-6,513) | -1% | 2,320 | 2.85 | 2.79 |
| reduce-entries-small | 551.2 (548.4-556.8) | 554.6 (547.5-562.6) | +1% | 89.6 | 8.34 | 6.19 |
| reduce-kv-small | 45.8 (45.7-46.8) | 45.8 (45.1-46.1) | 0% | 38.6 | 1.19 | 1.19 |
| eq-map-7 | 443.2 (438.9-444.7) | 440.9 (438.8-441.2) | -1% | 150.7 | 2.93 | 2.93 |
| hash-fresh-map-7 | 780.1 (763.8-796.3) | 779.8 (777.7-783.1) | 0% | 374.3 | 1.95 | 2.08 |
| select-keys-3 | 537.0 (534.0-539.8) | 531.5 (523.0-537.4) | -1% | 201.5 | 2.67 | 2.64 |

- **No operation is more than 10% slower**; the largest change up is
  `dissoc-small`'s +1%, inside its ranges. `reduce-entries-small` and
  `hash-fresh-map-7` move in shadow-cljs's runs (67.2-92.5 and 368.3-401.7
  ns after), not in light-cljs's.
- **Transients** gain 5% (`transient-map-100`) and 2% (`group-by-100`): the
  state is now fields, but each `assoc!` still goes through its protocol
  dispatcher (`coreMethod`: a rest-argument array and its slice, the
  protocol lookup and the native predicate, `transientAssociativeValue`),
  which is most of what remains of the 3.4 times shadow-cljs's;
  light-cljs-1016's direct slot calls are the change that reaches it.
- **Hash maps** gain 4-6% (`assoc-hash100`, `dissoc-hash100`,
  `update-hash100`): their slots no longer ask `publicCollectionSlots`
  whether a public constructor built them.
- **Printing** (`str-nested-map`) moves 1%: the gate's program has protocol
  implementations, so the printer walks an array's seq as before, of the
  entries `mapTraversal` makes of the pairs; only programs without them
  print the flat pairs directly.

**Workload 200k** (`compare.bb workload-200k-seed42-1036 --runs 5
--runtime-only --runtime 1,10,40`, the project rebuilt with each binary just
before its run; load 9.70-10.03 before, 10.88-11.13 after), wall time, median
(range):

| iterations | light before | light after | shadow-cljs (before run) | shadow-cljs (after run) |
| --- | ---: | ---: | ---: | ---: |
| 1 | 0.423 s (0.415-0.425) | 0.421 s (0.416-0.423) | 0.329 s | 0.337 s |
| 40 | 2.873 s (2.845-2.878) | 2.830 s (2.792-2.849) | 1.322 s | 1.286 s |

Steady state, from the medians at 1 and 40 iterations: light-cljs 62.8 to
61.8 ms per iteration (-2%); shadow-cljs 25.5 and 24.3; light/shadow 2.47 and
2.54, within what the load moved.

**Advanced size** (tests/support/ratchet_fixtures.rs, noted there): programs
with maps keep the transients' paths and the older layers' field branches
(`map-only` 14,270 to 14,613, `map-printed` 21,365 to 22,038, `event-maps`
56,372 to 56,871, `features` 113,208 to 116,130); `println-hi` takes the
printer's pairs path (5,360); programs without maps keep the shorter `maps`
(`vector-only` 7,519 to 7,368, `keyword-only` 5,102 to 5,005). TodoMVC and
re-frame simple were not ported in this run.

## 2026-11-16: sets, queues, atoms and the other store-held values as classes (light-cljs-1013)

**What.** Hash and array sets, set transients, queues, reduced values,
iterator seqs, delays, UUIDs and eductions hold their data in their own
fields, and atoms, volatiles and multimethods are told by their prototypes'
keyed kinds, where private-field payload stores and WeakMaps held or branded
each value. An atom's `swap!` with at most two arguments for its function
calls it directly, without a rest-argument seq. Sorted sets and records keep
their stores (light-cljs-1038 and light-cljs-1037). docs/value-layout.md has
the notes.

Binaries: before `light-cljs-31578fd9` (master), after `light-cljs-5d32355d`
(a commit of this pellet before it was squashed: its runtime but for the
notes and an engine-atom fix to `swap!`'s fast path in the Pants build), in
research/shadow-credibility/bin. Unreserved: one peer worker was active.
Raw results: research/output-speed/remaining-classes-1013 (`gate-*`,
`workload-*`).

**Gate** (`bb benchmarks/core-ops/run.bb --ops
protocol-on-record,record-kw-field,record-dot-field,record-assoc,multimethod,swap-inc,swap-assoc
--runs 3`; 1-minute load average 3.31-3.54 before, 5.18-6.06 after), ns per
operation, median (range). The gate has no set operation.

| op | light before | light after | change | shadow-cljs (after run) | light/shadow before | light/shadow after |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| swap-inc | 15.2 (14.5-15.4) | 4.87 (4.60-4.95) | -68% | 5.68 | 2.74 | 0.86 |
| swap-assoc | 122.2 (119.9-123.4) | 68.5 (67.9-68.9) | -44% | 38.3 | 3.32 | 1.79 |
| multimethod | 173.1 (169.9-173.8) | 150.9 (150.3-152.7) | -13% | 42.9 | 4.11 | 3.52 |
| protocol-on-record | 3.34 (3.29-3.37) | 3.45 (3.40-3.46) | +3% | 2.41 | 1.46 | 1.43 |
| record-kw-field | 13.6 (12.9-13.6) | 13.7 (13.5-13.8) | +1% | 2.80 | 5.07 | 4.88 |
| record-dot-field | 1.89 (1.88-1.89) | 1.96 (1.91-1.97) | +4% | 0.88 | 2.25 | 2.23 |
| record-assoc | 89.4 (84.9-89.8) | 94.0 (89.5-94.1) | +5% | 8.47 | 11.13 | 11.10 |

- **No operation is more than 10% slower.** The record operations move
  1-5% up with shadow-cljs's own runs (+4-6% between the two runs, at the
  higher load of the after run), so light/shadow is unchanged or lower for
  each; records are not converted yet (light-cljs-1037).
- **`swap!`** no longer builds a rest-argument seq and turns it back into an
  array for an atom's function of at most two arguments, and an atom is
  told by its kind rather than a WeakMap lookup: `swap-inc` is now faster
  than shadow-cljs's, and `swap-assoc`'s remaining time is the map's
  `assoc`.
- **`multimethod`** gains 13%: its method cache and hierarchy are atoms, and
  the multimethod and atom tests are kind reads.

**Sets.** The gate has no set operation, so a small program times five
(research/output-speed/remaining-classes-1013/sets.cljs, built at
`:advanced` by each binary, three runs each alternated, light-cljs only;
load 7.19-7.40), ns per operation, median (range):

| op | before | after | change |
| --- | ---: | ---: | ---: |
| `conj` onto a 4-member set | 399.9 (396.8-401.5) | 78.6 (78.1-81.0) | -80% |
| `conj` onto a 64-member set | 134.6 (133.8-136.3) | 110.2 (109.6-111.3) | -18% |
| `contains?` of a 64-member set | 49.9 (49.3-50.7) | 29.4 (29.4-29.7) | -41% |
| `disj` from a 64-member set | 168.8 (167.2-171.8) | 119.2 (118.9-119.7) | -29% |
| `(into #{} (range 100))` | 5,965 (5,889-5,971) | 4,758 (4,740-4,815) | -20% |

A set is one object of its class where it was an object, a frozen payload
and its private-field brand, and a set test reads its kind where it probed
the stores.

**Workload 200k** (`compare.bb workload-200k-seed42-1013 --runs 5
--runtime-only --runtime 1,40`, the project's light-cljs output rebuilt with
each binary just before its run; load 8.04-9.50 before, 9.50-9.94 after),
wall time, median (range):

| iterations | light before | light after | shadow-cljs (before run) | shadow-cljs (after run) |
| --- | ---: | ---: | ---: | ---: |
| 1 | 0.581 s (0.427-0.752) | 0.423 s (0.417-0.429) | 0.360 s | 0.332 s |
| 40 | 2.922 s (2.863-2.974) | 2.757 s (2.746-2.786) | 1.343 s | 1.336 s |

The 40-iteration run is 6% faster, with shadow-cljs's within 1%; light/shadow
2.18 to 2.06 there. The before run's 1-iteration median was disturbed by the
load (its range is 0.43-0.75 s), so the steady state from the two medians
(60.0 and 59.8 ms per iteration) hides the change; from each run's fastest
1-iteration time it is 64.0 to 60.0 ms per iteration (shadow-cljs 25.8 and
25.9).

## 2026-11-16: sorted maps as red-black trees of map entries (light-cljs-1035)

**What.** A sorted map is upstream's PersistentTreeMap: a comparator, a
red-black tree and a count in its own fields, where light-cljs-1012 kept its
entries in one sorted array that every `assoc` copied and spliced. Its nodes
are map entries (upstream's BlackNode and RedNode), its `assoc`, `dissoc`,
lookup and `find` walk one path, and its `seq`, `rseq` and `subseq` are
upstream's seq over a stack of nodes. docs/value-layout.md has the notes.

Binaries: before `light-cljs-31578fd9` (master), after `light-cljs-400143f1`
(this pellet's runtime but for the review's fixes of a list stack's protocols and
an entry's field writes, which no measured operation reaches), in
research/shadow-credibility/bin. Unreserved: one
peer worker was active. Raw results: research/output-speed/sorted-maps-1035
(`*-before`, `*-after`).

**Gate** (`bb benchmarks/core-ops/run.bb --ops sorted-map-assoc-100,str-sorted-map-20,reduce-entries-small,reduce-kv-small,str-nested-map,eq-map-7,hash-fresh-map-7,get-small,assoc-small-new,count-small,destructure-vec-3 --runs 3`;
1-minute load average 10.61-12.13 before, 6.91-9.98 after), ns per
operation, median (range):

| op | light before | light after | change | shadow-cljs (after run) | light/shadow before | light/shadow after |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| sorted-map-assoc-100 | 650.8 (647.4-655.4) | 193.8 (191.0-194.7) | -70% | 197.0 | 3.20 | 0.98 |
| str-sorted-map-20 | 42,130 (42,123-42,237) | 39,701 (39,499-39,743) | -6% | 19,203 | 2.12 | 2.07 |
| reduce-entries-small | 515.7 (507.3-521.0) | 514.4 (511.4-520.4) | 0% | 89.6 | 5.56 | 5.74 |
| reduce-kv-small | 45.7 (45.6-46.0) | 43.6 (43.2-44.7) | -5% | 36.4 | 1.19 | 1.20 |
| str-nested-map | 6,362 (6,293-6,483) | 6,156 (6,069-6,318) | -3% | 2,166 | 2.79 | 2.84 |
| eq-map-7 | 396.2 (395.2-401.8) | 382.1 (381.9-391.0) | -4% | 96.0 | 3.93 | 3.98 |
| hash-fresh-map-7 | 520.9 (520.1-522.2) | 484.8 (484.3-498.1) | -7% | 311.8 | 1.59 | 1.56 |
| get-small | 27.3 (27.2-27.6) | 26.0 (26.0-27.0) | -5% | 14.3 | 1.84 | 1.81 |
| assoc-small-new | 90.3 (90.1-90.6) | 85.5 (84.1-85.9) | -5% | 40.8 | 2.15 | 2.10 |
| count-small | 2.97 (2.96-2.97) | 2.82 (2.82-2.88) | -5% | 2.03 | 1.39 | 1.39 |
| destructure-vec-3 | 11.4 (11.4-11.4) | 10.8 (10.7-10.8) | -5% | 9.22 | 1.19 | 1.17 |

- **No operation is slower.** The load fell between the two runs, and
  shadow-cljs's own times fell 3-7% with it, so the 3-7% gains of the
  operations this pellet does not touch are the load; their light/shadow
  ratios move by 0.05 or less.
- **`sorted-map-assoc-100` is at parity** with shadow-cljs's (0.98, from
  3.20): an `assoc` builds the nodes of one path of a 100-key tree, where it
  copied and spliced a 100-entry array, and calls the comparator about seven
  times, as upstream's.
- **`str-sorted-map-20`** moves as shadow-cljs's does (light/shadow 2.12 to
  2.07): printing walks the tree in order, building the entries it prints
  where it read the stored array, and the rest of the time is the printer's,
  as for the other maps (`str-nested-map` 2.84).
- The map entries' slots and readers (`reduce-entries-small`,
  `destructure-vec-3`) see a second class of map entry only in a program that
  builds sorted maps; the gate's do, and they do not move.

**Workload 200k** (`compare.bb workload-200k-seed42-1035 --runs 5
--runtime-only --runtime 1,10,40`, the project rebuilt with each binary just
before its run; load 4.21-4.99 before, 3.59-3.96 after), wall time, median
(range):

| iterations | light before | light after | shadow-cljs (before run) | shadow-cljs (after run) |
| --- | ---: | ---: | ---: | ---: |
| 1 | 0.385 s (0.376-0.392) | 0.419 s (0.393-0.446) | 0.295 s | 0.323 s |
| 40 | 2.604 s (2.589-2.649) | 2.539 s (2.531-2.544) | 1.177 s | 1.156 s |

Steady state, from the medians at 1 and 40 iterations: light-cljs 56.9 to
54.4 ms per iteration (-4%); shadow-cljs 22.6 and 21.4; light/shadow 2.52 and
2.54, within what the load moved (the workload builds few sorted maps).

**Advanced size** (tests/support/ratchet_fixtures.rs, noted there): the
red-black tree's algorithms, its node class and its seqs are about 3,000
bytes in a program that builds sorted maps (`features` 119,868, its editor
chunk, which holds the sorted collections, 11,930 from under 8,600);
programs with other maps keep the generic operations' tests of the sorted
map's class (`map-only` 14,926).

**TodoMVC** (`re-frame.bb runtime todomvc --runs 5`, ported with
`shadow-port-tool-983`; load 4.66-5.28 before, 4.44-4.85 after; its add step
prints a sorted map after every event): the add step (1,000 todos) 4,005.8 ms
(3,974.4-4,013.1) to 3,822.3 ms (3,740.9-3,857.3), -5%, light/shadow 4.31 to
4.19 (shadow-cljs 929.5 and 913.3 ms); toggle all 67.2 to 64.0 ms; page load
to interactive 44.1 to 43.0 ms; the heap with 1,000 todos 9.03 to 8.75 MB.
Its bundle with React (`re-frame.bb size todomvc`): raw 484,186 to 489,777
bytes, gzip -9 146,709 to 147,874, brotli -q 11 126,600 to 127,602; re-frame
simple, which builds no sorted map, was not ported in this run.

## 2026-11-16: records, program types and `reify` told by their keyed kinds (light-cljs-1037)

**What.** A record, a `deftype` and a `reify` value are told by the keyed
kind on their prototype (64 to 66), which `defineType` and `makeType` give
it, where a private-field brand of each instance (`TypeBrand`, or a class's
own field) and the `typeFields`, `recordTypes`, `reifyValues` and
`recordMaps` stores told them. No constructor writes a brand, and a record
is no longer a map to `maps` (its methods answer; the native protocol tests
read its kind). Where a program keeps the slot-first wrappers, `get`,
`assoc`, `count` and the others call a record's or type's slot first.
docs/value-layout.md has the notes.

Binaries: before `light-cljs-daea1afc` (master), after
`light-cljs-5ad9f96d` (a commit of this pellet before it was squashed: its
runtime and emitter but for comments), in research/shadow-credibility/bin.
Unreserved: one or two peer workers were active. Raw results:
research/output-speed/record-kinds-1037 (`gate-*`, the projects' logs,
`load.txt`).

**Gate** (`bb benchmarks/core-ops/run.bb --ops
protocol-on-record,record-kw-field,record-dot-field,record-assoc --runs 5`;
1-minute load average 6.47-6.95 before, 7.32 after), ns per operation,
median (range):

| op | light before | light after | change | shadow-cljs (after run) | light/shadow before | light/shadow after |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| protocol-on-record | 3.29 (3.28-3.36) | 3.28 (3.27-3.42) | +0% | 2.29 | 1.44 | 1.43 |
| record-kw-field | 13.0 (12.8-13.4) | 5.23 (5.23-5.61) | -60% | 2.68 | 4.83 | 1.95 |
| record-dot-field | 1.88 (1.87-1.89) | 1.87 (1.87-1.92) | -1% | 0.84 | 2.24 | 2.23 |
| record-assoc | 85.0 (84.6-88.4) | 53.8 (53.5-54.7) | -37% | 8.04 | 10.56 | 6.69 |

- **No operation is more than 10% slower.**
- **`record-kw-field`** reads `(:w r)` through `get`'s slot-first wrapper,
  which now takes the record's kind and calls its `-lookup` slot, where
  `get.own` read the mask bit, demanded the key and read the slot under
  its computed name. **`record-assoc`** calls the record's `-assoc` slot
  first in the same way, and its new record writes no brand; the rest of
  its time is the method's own copy of the fields (`Type.members.map` and
  a spread into the constructor), which light-cljs-1023's review of the
  gate takes up. `protocol-on-record` and `record-dot-field` never read a
  brand: unchanged (light-cljs-1022 is the protocol call).

The map operations whose `maps.has` lost the record wrapper (`--ops
kw-small,get-small,contains-small,count-small,assoc-small-new,dissoc-small,merge-small,eq-map-7,str-nested-map,reduce-kv-small
--runs 3`; load 5.68-6.64 before, 6.59-9.25 after):

| op | light before | light after | change |
| --- | ---: | ---: | ---: |
| kw-small | 21.0 (20.4-21.2) | 20.9 (20.8-21.2) | +0% |
| get-small | 25.0 (24.7-25.8) | 25.5 (25.0-25.8) | +2% |
| contains-small | 25.7 (25.4-26.6) | 26.2 (25.4-26.5) | +2% |
| count-small | 2.88 (2.83-2.96) | 2.93 (2.82-2.97) | +2% |
| assoc-small-new | 78.5 (78.4-82.3) | 81.3 (78.3-82.7) | +4% |
| dissoc-small | 102.7 (102.4-107.8) | 104.0 (101.9-106.5) | +1% |
| reduce-kv-small | 43.4 (43.2-45.6) | 44.8 (43.5-46.0) | +3% |
| str-nested-map | 5,633 (5,623-5,837) | 4,711 (4,686-4,895) | -16% |
| eq-map-7 | 368.8 (368.5-382.2) | 226.7 (225.2-230.5) | -39% |
| merge-small | 3,119 (2,867-3,160) | 2,647 (2,625-2,892) | -15% |

The slower medians are within each other's ranges, at the higher load of
the after run. Equality, `str` and `merge` of maps ask `maps.has` of
every map they meet, which no longer calls the record wrapper (a record
test by private-field brand and, where a program had class brands, a walk
of the prototype chain) after the map test.

**Workload 200k** (`compare.bb workload-200k-seed42-1037 --runs 5
--runtime-only --runtime 1,40`, the project's light-cljs output rebuilt
with each binary just before its run; load 5.37-6.14 before, 4.00-4.69
after), wall time, median (range):

| iterations | light before | light after | shadow-cljs (before run) | shadow-cljs (after run) |
| --- | ---: | ---: | ---: | ---: |
| 1 | 0.373 s (0.370-0.394) | 0.378 s (0.367-0.383) | 0.293 s | 0.305 s |
| 40 | 2.444 s (2.434-2.483) | 2.396 s (2.361-2.437) | 1.176 s | 1.167 s |

Steady state 53.1 to 51.8 ms per iteration (-2%), shadow-cljs 22.6 and
22.1; light/shadow 2.35 to 2.34.

**The re-frame-shaped app** (`compare.bb re-frame-200k-seed42-1037 --runs 5
--runtime-only --runtime 1,10`; load 3.98-5.74 before, 3.80-7.17 after): 1
iteration 2.309 s (2.265-2.454) to 2.357 s (2.274-2.534), 10 iterations
8.164 s (7.674-9.814) to 8.186 s (7.671-8.317); steady state 650.6 to 647.6
ms per iteration, unchanged; shadow-cljs 278.0 and 256.5 (its own runs
moved 8%), so light/shadow 2.34 to 2.53 moves with shadow-cljs alone. The
app builds no record on its hot path.

**Advanced size** (tests/support/ratchet_fixtures.rs, noted there): the
native tests that answered a record as a map read its kind (`recordCells`)
in every program that keeps them (`vector-only` 7,813, `constant-lookups`
5,913, `constant-keys` 82,214, `features-direct` 121,353); record programs
keep the kinds and `recordTypeOf` but no brand (`record-protocol` 5,814,
`three-records` 5,809). TodoMVC and re-frame simple were not ported in
this run.

## 2026-11-16: sorted sets, vars, functions with metadata and string writers as classes (light-cljs-1038)

**What.** A sorted set is upstream's PersistentTreeSet: its own field holds
its tree map, the red-black sorted map of light-cljs-1035 whose keys are its
members, where a private-field store held a sorted array of its members and
a WeakMap its comparator. `conj`, `disj`, `contains?`, `get` and `count`
are its map's `assoc`, `dissoc`, node lookup and count, and its seq is the
keys of its map's seq, as upstream's. A var holds its `val` and `sym`, a
function with metadata its `afn` and a string writer its `sb` in their own
fields, told by their prototypes' keyed kinds, where WeakMaps and a WeakSet
held or branded them. docs/value-layout.md has the notes.

Binaries: before `light-cljs-daea1afc` (master), after `light-cljs-acbf41e0`
(this pellet's runtime but for the Sol review's fixes: a var's and a
function with metadata's fields made not enumerable, a public sorted set
counted through its map's `count`, a public constructor's undefined
arguments kept, which no measured operation but `@` of a var reaches), in
research/shadow-credibility/bin. Unreserved: one
peer worker was active, and the 1-minute load average was high throughout
(10 to 21). Raw results: research/output-speed/sorted-sets-1038 (`*-before`,
`*-after3` for the gate, `*-before2`/`*-after2` for the workload).

**Gate** (`bb benchmarks/core-ops/run.bb --ops sorted-map-assoc-100,str-sorted-map-20,multimethod,str-nested-map,str-concat --runs 3`;
1-minute load average 14.19-19.46 before, 17.44-20.74 after), ns per
operation, median (range):

| op | light before | light after | change | shadow-cljs before / after | light/shadow before | light/shadow after |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| sorted-map-assoc-100 | 183.4 (182.7-187.2) | 202.3 (193.2-206.4) | +10% | 184.3 / 201.1 | 1.00 | 1.01 |
| str-sorted-map-20 | 36,704 (36,606-37,189) | 36,738 (36,669-39,278) | 0% | 19,033 / 20,525 | 1.93 | 1.79 |
| multimethod | 140.4 (140.4-142.0) | 142.3 (141.7-142.7) | +1% | 37.5 / 39.4 | 3.74 | 3.61 |
| str-nested-map | 5,671 (5,462-8,438) | 5,307 (5,276-5,522) | -6% | 2,181 / 2,435 | 2.60 | 2.18 |
| str-concat | 190.0 (189.9-202.1) | 205.5 (204.0-212.3) | +8% | 15.0 / 15.6 | 12.68 | 13.17 |

- **`sorted-map-assoc-100` +10% is the load**: shadow-cljs's own time rose
  9% between the runs, the light/shadow ratio stays at parity (1.00 to
  1.01), and the gate builds no sorted set, so no changed path runs. An
  earlier run of the same change at a lower load (11.78-12.21, binary
  `light-cljs-c8bed362`, which differs only in a sorted set's seq) gave
  193.5 ns, +5%, with shadow-cljs's +4%.
- **`str-concat` +8%** (`(str "a" i "b" s)`): `str` asks `sets.has` of
  each argument, which now reads a non-string's keyed kind once to tell
  both set classes, where the payload store's test of a number read
  nothing; shadow-cljs's own time rose 4%. Under 10%.
- **`multimethod`** reads a var's or a function with metadata's kind in its
  dispatch tests where WeakMaps were probed: -2% at the lower load, +1% here.
- **`str-nested-map` -6%**: printing builds its string writer as a class,
  where it made an object and added it to a WeakSet, and the writer's test
  is its kind.

**Sorted sets, vars and functions with metadata** (the gate has none; a small
program, research/output-speed/sorted-sets-1038/sorted-sets.cljs, built at
advanced by each binary and run alternately three times, load 13.81-18.31),
ns per operation, median (range):

| op | before | after | change |
| --- | ---: | ---: | ---: |
| `conj` onto a 4-member sorted set | 553.2 (547.4-583.2) | 73.7 (72.3-125.7) | -87% |
| `conj` onto a 64-member sorted set | 1,997.3 (1,996.2-2,073.5) | 120.9 (120.7-127.4) | -94% |
| `contains?` of a 64-member sorted set | 128.8 (128.5-201.6) | 42.6 (42.2-58.9) | -67% |
| `disj` from a 64-member sorted set | 2,640.0 (1,980.7-2,965.4) | 181.5 (172.3-367.2) | -93% |
| `into` an empty sorted set of 100 | 262,629 (252,569-285,762) | 23,201 (22,921-38,666) | -91% |
| `first` of a 64-member sorted set | 622.4 (619.0-627.7) | 49.1 (48.7-64.5) | -92% |
| `first` of a `subseq` | 1,686.0 (1,663.8-1,758.3) | 389.5 (388.0-472.0) | -77% |
| `str` of a new 20-member sorted set | 24,494 (23,735-26,789) | 5,525.6 (5,405.3-6,806.9) | -77% |
| `@` of a var | 16.0 (15.5-25.9) | 5.0 (4.9-5.5) | -69% |
| calling a function with metadata | 185.4 (184.0-260.5) | 177.8 (177.4-253.9) | -4% |
| `pr-str` of a 3-item vector | 761.7 (748.9-1,291.4) | 563.7 (557.6-565.9) | -26% |

**Workload 200k** (`compare.bb workload-200k-seed42-1038 --runs 5
--runtime-only --runtime 1,10,40`, the project rebuilt with each binary just
before its run, the two runs back to back; load 15.38 and 15.09 at most),
wall time, median (range):

| iterations | light before | light after | shadow-cljs (before run) | shadow-cljs (after run) |
| --- | ---: | ---: | ---: | ---: |
| 1 | 0.462 s (0.453-0.466) | 0.463 s (0.458-0.468) | 0.363 s | 0.363 s |
| 40 | 2.962 s (2.875-3.011) | 2.923 s (2.833-3.229) | 1.482 s | 1.476 s |

Steady state, from the medians at 1 and 40 iterations: light-cljs 64.1 to
63.1 ms per iteration (-2%); shadow-cljs 28.7 and 28.5; light/shadow 2.23
and 2.21. The workload builds few sorted sets or vars. (A first pair of runs
at different loads, 8.18 before and 16.60 after, gave 59.3 and 65.6 ms, with
shadow-cljs's 25.9 and 29.3: the load, light/shadow 2.29 and 2.24.)

**Advanced size** (tests/support/ratchet_fixtures.rs, noted there), of the
final change rebased onto master `c88f22ae` (light-cljs-1037's records),
against that master: most programs shrink about 450 bytes, since no store
inventory reads a private-field kind any more and `payloads` is gone
(`set-only` 8,093 to 7,797, `keyword-only` 5,472 to 5,025, `map-only`
15,459 to 15,057); `println-hi` grows 177 (5,537: the string writer's class
and kind test outweigh its WeakSet); the features example, whose sorted
sets now hold the tree map, about 1,100 (`features-direct` 121,353 to
122,470, its editor chunk 12,435). A program that builds a sorted set keeps
the sorted map's tree algorithms, as upstream's does (the small program
above: 122,501 to 132,086 bytes, against `daea1afc`). re-frame simple and
TodoMVC were not ported in this run.

## 2026-11-16: protocol calls test their receiver as upstream's (light-cljs-1022)

**What.** A program protocol method of fixed arities gets its own
dispatcher at `:advanced` (light-cljs-265), which calls the receiver's
slot where it holds something. It now tests the receiver as upstream's
protocol functions do, `r0 != null && r0[m1] != null ? r0[m1](r0) :
m0(r0)`, where it read `r0?.[m1] != null`. Nothing else changes: the
slots, the records' classes, `extend-type`, `specify!`, `satisfies?` and
`implements?` are as they were.

Binaries, in research/shadow-credibility/bin: before `light-cljs-c88f22ae`
(master), after `light-cljs-59d6fa08` (a commit of this pellet before it
was squashed: its emitter as landed). To measure the operation with the
loop shadow-cljs has, light-cljs-1020's emitter change (`src/emitter.rs`
of its commit `6bdde1ae`, not on master: the bit operations as operators)
was applied to each, `light-cljs-c88f22ae-with-1020` and
`light-cljs-59d6fa08-with-1020`. Unreserved: one peer worker was active.
Raw results: research/output-speed/protocol-call-1022.

**Gate** (`bb benchmarks/core-ops/run.bb --ops protocol-on-record --runs
5`), ns per operation, median (range):

| loop | light before | light after | change | shadow-cljs (after run) | light/shadow before | light/shadow after |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| master's (`bitAnd` call) | 3.47 (3.46-3.48) | 2.93 (2.88-2.94) | -16% | 2.52 (2.50-2.54) | 1.44 | 1.16 |
| with light-cljs-1020 | 3.70 (3.67-3.71) | 2.74 (2.59-2.77) | -26% | 2.57 (2.40-2.59) | 1.32 | 1.07 |

1-minute load average 12.17 before and 7.59-7.65 after on master's loop,
6.35-6.65 and 6.33-6.38 with light-cljs-1020. The operation is within
1.1x of shadow-cljs's once light-cljs-1020 lands; on master the loop's
`bitAnd` call, which that pellet removes, keeps it at 1.16.

**Where the time went.** The profile (`--pseudo-names protocol-on-record`)
has the same parts in both outputs: the loop, the dispatcher (V8 inlines
light-cljs's into the loop, `--trace-turbo-inlining`) and the two records'
methods, which take the same time in both. Edits of light-cljs's
release output, each timed against shadow-cljs's in alternated processes
(scratch measurements, not the gate; shadow-cljs 2.45 to 2.79 ns as the
machine's state moved):

- **Optional chaining** in the dispatcher was most of the gap: `r0 != null
  && r0[m1] != null` took 2.83-2.91 against 3.51-3.56 ns for `r0?.[m1] !=
  null`. A dotted slot name instead of the computed `r0[m1]` made no
  difference, a strict `!== null && !== undefined` test was 0.1 ns slower,
  and `var` in place of the program's `let` bindings at most 0.15 ns
  faster.
- **The loop's `bitAnd` call** was 0.2 to 0.3 ns of the rest, as
  light-cljs-1020 found.
- **The call through the dispatcher's binding.** With both fixed, writing
  the dispatcher's test at the call site, `(o = r, o != null && o.slot !=
  null ? o.slot(o) : area(o))`, took 1.93 against 2.62 ns (0.78 against
  1.06 times shadow-cljs's), and so did a dispatcher declared as a
  module-level function (2.05): V8 calls a closure held in a `let` of the
  program's function with a check of the binding and of the closure on
  every call. This pellet leaves the call-site form out. It grew workload
  200k by 82,274 bytes (2.3%, 16,280 gzipped; its 360 calls of program
  protocol methods) with no change to its steady state, and a slot that
  holds nothing would be read twice, at the call site and in the
  dispatcher, where the dispatcher reads it once
  (`protocol_dispatchers_call_host_slots_and_keep_their_length`). The diff
  is research/output-speed/protocol-call-1022/call-site-dispatch.diff for
  light-cljs-1023's review of the gate.

**Workload 200k** (`compare.bb workload-200k-seed42-1022 --runs 5
--runtime-only --runtime 1,40`, the project's light-cljs output rebuilt
with each binary just before its run, two before/after pairs; load
6.28-7.22 and 13.59-14.20 before, 14.42-22.30 and 15.06-15.62 after), wall
time, median (range):

| iterations | light before | light after | shadow-cljs (before runs) | shadow-cljs (after runs) |
| --- | ---: | ---: | ---: | ---: |
| 1, pair 1 | 0.444 s (0.439-0.448) | 0.455 s (0.427-1.089) | 0.358 s | 0.365 s |
| 40, pair 1 | 2.833 s (2.756-2.925) | 2.877 s (2.867-4.846) | 1.386 s | 1.480 s |
| 1, pair 2 | 0.475 s (0.458-0.480) | 0.531 s (0.477-0.571) | 0.368 s | 0.449 s |
| 40, pair 2 | 2.926 s (2.877-2.999) | 2.876 s (2.868-2.903) | 1.498 s | 1.488 s |

Steady state 61.3 to 62.1 and 62.8 to 60.1 ms per iteration: unchanged
within the load's noise, as the scratch edit of its output for this change
alone measured (2,844 against 2,839 ms at 40 iterations). The load rose to
22 during the first after run.

**Advanced size** (tests/support/ratchet_fixtures.rs, noted there): 7
bytes per slot test, one per arity of each method's dispatcher; a
two-method protocol costs 330 bytes where it cost 316, `eight-records`
52,598 to 52,605, workload 200k 3,550,336 to 3,553,276 (904,978 to 905,108
gzipped). No ceiling moves. TodoMVC and re-frame simple were not ported in
this run.

## 2026-11-15: bit operations as operators (light-cljs-1020)

**Change.** At `:advanced`, light-cljs emitted `(bit-and i 3)` as a call of
the runtime's `bitAnd`, a rest-argument function over `numericFold`. Upstream's
macro emits `(i & 3)`. Each bit operation's call is now the JavaScript
operator its runtime definition uses, as `+` and `<` already were:
- `bit-and`, `bit-or`, `bit-xor` and `bit-and-not` on two operands. A wider
  call is the operators only when the operands after the second are literals,
  locals that nothing assigns or globals defined once. The runtime reads every
  operand before it coerces any, and a coercion runs `valueOf`. So a later
  operand that runs code, or a parameter or local that a `js*` hole may
  assign, keeps the call.
- `bit-not`, `bit-shift-left`, `bit-shift-right`, `unsigned-bit-shift-right`,
  `bit-shift-right-zero-fill`, `bit-clear`, `bit-flip`, `bit-set` and
  `bit-test` at their fixed arity.
- Other arities, higher-order uses such as `(reduce bit-and ...)`, and a
  program that replaces the operation keep the runtime's function.
- `none` and `prepare` keep the call, as they do for `+`. They read the live
  core binding, which a later `set!`, `with-redefs` or REPL form may replace.

`type_inference::bit_operations_are_operators_for_every_operand` compares each
lowered call with the runtime's (`apply`) at every level over 26 values. These
include `NaN`, `-0`, strings, collections, an object with `valueOf`, and a
BigInt that throws when mixed with numbers.

**Setup.** The gate (`benchmarks/core-ops`) with the operations the pellet
names, the same `--ops` before and after:
- before is `light-cljs-f533efb1` (master) and after is `light-cljs-aed9845e`,
  this change before the Sol review narrowed wider chains. Every bit
  operation in the gate takes two operands, so its output is the landed one;
- 5 runs per compiler, alternated, shadow-cljs 3.5.3 in each.

**Unreserved:** a peer worker was active. The 1-minute load average before a
run was 6.7 to 7.1 before and 8.1 after. Raw results are in
`research/output-speed/bit-operations-1020`.

| op | light-cljs before | light-cljs after | change | shadow-cljs after | light/shadow before | light/shadow after |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| bit-and | 1.47 (1.38-1.54) | 0.58 (0.53-0.60) | -61% | 0.55 (0.53-0.59) | 2.72 | 1.05 |
| aget-bit-and | 1.79 (1.74-1.81) | 0.73 (0.70-0.75) | -59% | 0.59 (0.57-0.69) | 3.09 | 1.24 |
| record-dot-field | 1.89 (1.87-1.94) | 0.86 (0.83-0.88) | -54% | 0.85 (0.84-0.88) | 2.22 | 1.01 |
| protocol-on-record | 3.31 (3.29-3.53) | 3.11 (2.99-3.19) | -6% | 2.34 (2.30-2.41) | 1.45 | 1.33 |

Nanoseconds per operation, medians over the runs, ranges in brackets.

- **At parity:** `bit-and` and `record-dot-field`. The call was their whole gap.
- **`aget-bit-and` stays at 1.24.** Its loop is now the same as shadow-cljs's,
  `acc + ms[i & 3]`. What differs is the array: `(array s 1 2 3)` is the
  runtime's `array`, `(...args) => args`, where upstream's macro writes the
  literal `[s, 1, 2, 3]`. In Node, the same loop over a rest-argument array
  takes 0.75 ns and over the literal 0.60 ns (`rest-array-vs-literal.mjs` with
  the raw results). That is the remaining gap, closed in the next section.
- **`protocol-on-record` stays at 1.33.** The call was 0.2 ns of its 1.0 ns
  gap, not most of it. The rest is in the record's protocol dispatch, which
  light-cljs-1013 changes when it makes records classes.
- **Workload 200k and the re-frame-shaped 200k app:** neither program calls a
  bit operation that reaches the output. Both compile to byte-identical
  advanced output with the two binaries, so their steady state is unchanged
  and was not timed again.
- **Output size:** a program that only calls the bit operations keeps none of
  their units. The gate's own output shrinks from 123,907 to 123,772 bytes
  (gzip 36,293 to 36,251). No size ratchet fixture calls one, so the ratchet
  is unchanged (noted in tests/support/ratchet_fixtures.rs).

## 2026-11-15: array calls as literals (light-cljs-1020, light-cljs-1021)

**Change.** At `:advanced`, a call of `array` is now the JavaScript array
literal of its arguments, as upstream's macro writes it: `(array s 1 2 3)` is
`[s, 1, 2, 3]`. The runtime's `array` is `(...args) => args`, a fresh array
of the evaluated arguments in order, and the literal is the same. The
literal's elements keep their kind, such as small integers, where a rest
parameter's are generic. Higher-order uses such as `(apply array ...)` and
`(map array ...)`, and a program that replaces `array`, keep the runtime's
function. `none` and `prepare` keep the call, as they do for `+` and the bit
operations.

`type_inference::array_calls_are_array_literals` compares lowered calls with
`(apply array ...)` and `#js [...]` at every level. It checks a fresh array per
evaluation, argument order in value and statement position, `nil` and
`undefined` elements, and the prototype.

**Setup.** The gate (`benchmarks/core-ops`) with the four operations
light-cljs-1020 names, the same `--ops` before and after:
- before is `light-cljs-f533efb1` (master);
- after is `light-cljs-92ce8f11`, which has the bit operations of the
  previous section and the array literals;
- 5 runs per compiler, alternated, shadow-cljs 3.5.3 in each.

**Unreserved:** a peer worker was active. The 1-minute load average before a
run was 20.4 to 20.5 before and 20.9 after. Raw results are in
`research/output-speed/array-literals-1020`.

| op | light-cljs before | light-cljs after | change | shadow-cljs after | light/shadow before | light/shadow after |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| bit-and | 1.56 (1.55-1.57) | 0.61 (0.60-0.80) | -61% | 0.60 (0.60-0.61) | 2.60 | 1.02 |
| aget-bit-and | 1.88 (1.86-2.07) | 0.60 (0.60-0.86) | -68% | 0.61 (0.60-0.61) | 3.13 | 0.98 |
| record-dot-field | 1.98 (1.97-2.08) | 0.90 (0.89-1.41) | -55% | 0.88 (0.88-0.89) | 2.13 | 1.02 |
| protocol-on-record | 3.55 (3.46-4.55) | 3.23 (3.19-3.28) | -9% | 2.42 (2.41-2.48) | 1.45 | 1.33 |

Nanoseconds per operation, medians over the runs, ranges in brackets.

- **At parity:** `bit-and`, `aget-bit-and` and `record-dot-field`. With the
  array literal, `aget-bit-and`'s loop and array are both shadow-cljs's.
  Selected alone (`--ops aget-bit-and`, the same binaries, load average 19.7
  to 19.9), it goes from 1.86 to 0.60 ns (0.60-0.61) against shadow-cljs's
  0.61 (0.60-0.62): 3.10 to 0.98.
- **`protocol-on-record` stays at 1.33**, as in the previous section. Its gap
  is the record's protocol dispatch, not the loop. light-cljs-1022 carries its
  1.1x target to after light-cljs-1013 makes records classes.
- **Workload 200k:** its advanced output is byte-identical with the two
  binaries, so its steady state is unchanged and was not timed again.
- **The re-frame-shaped 200k app:** three `array` calls change, in reagent's
  `ratom` (a reaction's captured derefs, its on-dispose callbacks) and in the
  `goog.array` compatibility namespace. Its output is 26 bytes smaller (gzip
  -9: 27 bytes). Not timed.
- **Output size** of light-cljs's output as written, without React, for both
  changes together (gzip -9):
  - the gate: 123,907 to 123,735 bytes (gzip 36,288 to 36,233);
  - re-frame simple: 210,644 to 210,621 bytes (gzip 61,827 to 61,817);
  - TodoMVC: 258,986 to 258,961 bytes (gzip 75,716 to 75,719).

  No size ratchet fixture calls `array` or a bit operation, so the ratchet is
  unchanged (noted in tests/support/ratchet_fixtures.rs).

## 2026-11-16: bit operators with the receiver test (light-cljs-1020, light-cljs-1022)

**What.** The two previous sections' changes landed together, rebased onto
master `5cd32c66`: the bit operations and `array` calls as JavaScript
operators and literals at `:advanced` (light-cljs-1020, light-cljs-1021),
and each protocol method's dispatcher testing its receiver as `r0 != null
&& r0[m1] != null` (light-cljs-1022). light-cljs-1020's gate includes
`protocol-on-record`, which needs both.

Binaries, in research/shadow-credibility/bin: before `light-cljs-5cd32c66`
(master), after `light-cljs-a224bc05` (both changes, the emitter as
landed). Unreserved: one peer worker was active. Raw results:
research/output-speed/combined-1020-1022.

**Gate** (`bb benchmarks/core-ops/run.bb --ops
bit-and,aget-bit-and,record-dot-field,protocol-on-record --runs 5`), two
before/after pairs run one after the other, each run alternating the
compilers; 1-minute load average before a run 5.51 and 5.61 in the first
pair, 6.86-7.29 and 6.50-6.71 in the second. ns per operation, median
(range), first pair then second:

| op | light before | light after | shadow-cljs (after run) | light/shadow before | light/shadow after |
| --- | ---: | ---: | ---: | ---: | ---: |
| protocol-on-record | 3.86 (3.67-3.88), 3.92 (3.85-3.93) | 2.86 (2.85-2.87), 2.86 (2.67-2.88) | 2.71, 2.72 | 1.43, 1.45 | 1.06, 1.05 |
| record-dot-field | 2.22 (1.99-2.23), 2.24 (2.23-2.25) | 0.99 (0.98-1.00), 0.95 (0.91-1.00) | 1.00, 0.99 | 2.22, 2.24 | 0.99, 0.96 |
| bit-and | 1.73 (1.61-1.74), 1.73 (1.70-1.75) | 0.67 (0.64-0.68), 0.67 (0.61-0.68) | 0.68, 0.68 | 2.58, 2.54 | 0.99, 0.99 |
| aget-bit-and | 2.06 (2.01-2.07), 2.08 (2.06-2.09) | 0.68 (0.65-0.70), 0.67 (0.63-0.69) | 0.68, 0.69 | 3.03, 3.06 | 1.00, 0.97 |

All four operations are within 1.1x of shadow-cljs's output, which meets
light-cljs-1020's and light-cljs-1022's gates. The per-call-site slot test
that light-cljs-1022 measured and left out stays out: the dispatcher alone
reaches the gate.

**Workload 200k** (`compare.bb workload-200k-seed42-1020x1022 --runs 5
--runtime-only --runtime 1,40`, the project's light-cljs output rebuilt
with each binary just before its run, two before/after pairs, load 4.6 to
6.4). Its advanced output differs from master's only in the dispatchers'
receiver tests (no bit operation or `array` call reaches it), as
light-cljs-1022's section measured. Wall time, median:

| pair | light before (1 / 40) | light after (1 / 40) | shadow-cljs (1 / 40) | light steady state, ms per iteration |
| --- | ---: | ---: | ---: | ---: |
| 1 | 0.437 / 2.690 s | 0.429 / 2.684 s | 0.344-0.349 / 1.348-1.349 s | 57.8 to 57.8 |
| 2 | 0.434 / 2.702 s | 0.437 / 2.720 s | 0.349-0.350 / 1.361-1.364 s | 58.2 to 58.5 |

Steady state is unchanged within the load's noise.

**Output size** (gzip -9):
- the gate: 173,499 to 173,334 bytes (gzip 44,369 to 44,304);
- workload 200k: 3,549,831 to 3,552,771 (gzip 895,066 to 895,214), the
  receiver tests;
- TodoMVC: 372,560 to 372,640 bytes (gzip 99,932 to 99,930);
- re-frame simple: 255,425 to 255,458 bytes (gzip 69,074 to 69,072).

  TodoMVC and re-frame simple are light-cljs's own release output of the
  ports (`shadow-port-tool-983`), without React. The size ratchet moves
  only by light-cljs-1022's receiver test (`eight-records` 52,245 to
  52,252); no fixture calls a bit operation or `array`.

## 2026-11-16: vectors, maps and sets compare and hash by their own slots (light-cljs-1014)

**What.** Each runtime class has its own keyed `-equiv` and `-hash` slots,
which `=` and `hash` call first through slot-first wrappers (87 and 68
bytes of bytecode). A vector or a map entry compares counts, then items
index by index, and hashes its items index by index into its own `hash`
field, where `=` and `hash` walked their seqs. A map compares counts, then
looks each key up in the other map. Sets, seqs and every mixed comparison
(a vector against a list, a map against a record) take the structural
comparison directly instead of through the protocol lookups, with today's
answers. `hash` of a number or a string reads its prototype's `IHash` mask
bit and marker by name, where the protocol lookup read them by computed
names. The comparisons by fields stop, and the generic walk answers, once a
program's `extend-type` or `specify!` reaches a runtime type.
docs/value-layout.md has the notes.

Binaries: before `light-cljs-5cd32c66` (master), after `light-cljs-cd6e833f`
(this pellet's runtime; the final commit differs only in comments and in
reading an array map's `arr` after its kind test), in
research/shadow-credibility/bin. Unreserved: one peer worker was active.
Raw results: research/output-speed/eq-hash-1014.

**Gate** (`bb benchmarks/core-ops/run.bb`, every operation, 5 runs; 1-minute
load average 4.83-8.74 before, 6.34-11.21 after), ns per operation, median
(range):

| op | light before | light after | change | shadow-cljs before / after | light/shadow before | light/shadow after |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| eq-vec-10 | 491.0 (482.5-532.3) | 44.1 (40.2-46.3) | -91% | 85.4 / 86.9 | 5.75 | 0.51 |
| eq-map-7 | 324.6 (322.8-348.2) | 219.4 (218.8-229.8) | -32% | 157.4 / 157.6 | 2.06 | 1.39 |
| hash-fresh-vec-4 | 426.7 (424.9-473.6) | 51.0 (49.1-59.8) | -88% | 75.7 / 73.0 | 5.64 | 0.70 |
| hash-fresh-map-7 | 818.0 (806.6-867.4) | 278.0 (275.6-289.0) | -66% | 576.0 / 574.3 | 1.42 | 0.48 |
| get-hash100 | 30.2 (29.7-30.6) | 19.2 (18.6-20.3) | -37% | 12.6 / 12.6 | 2.39 | 1.52 |
| kw-hash100 | 52.2 (50.5-54.6) | 41.5 (39.7-43.3) | -20% | 14.3 / 14.0 | 3.64 | 2.98 |
| update-hash100 | 136.0 (133.5-138.7) | 115.3 (113.3-121.7) | -15% | 101.4 / 101.5 | 1.34 | 1.14 |
| transient-map-100 | 13,204 (13,029-14,135) | 11,158 (11,035-11,494) | -15% | 4,412 / 4,300 | 2.99 | 2.59 |
| group-by-100 | 13,035 (12,786-13,430) | 12,401 (12,104-13,129) | -5% | 5,350 / 5,339 | 2.44 | 2.32 |
| kw-small | 22.2 (22.1-24.5) | 13.4 (12.9-13.6) | -40% | 10.4 / 10.2 | 2.14 | 1.32 |
| get-small | 26.9 (26.8-29.8) | 17.9 (17.3-18.8) | -34% | 15.7 / 15.3 | 1.72 | 1.17 |
| multimethod | 153.1 (152.6-168.2) | 188.2 (183.7-196.6) | +23% | 51.4 / 49.8 | 2.98 | 3.78 |
| seq-walk-vec-1000 | 29,052 (28,860-32,741) | 31,469 (29,689-32,362) | +8% | 10,471 / 10,514 | 2.77 | 2.99 |

- **The four operations of this pellet**: `=` of two 10-item vectors is now
  twice as fast as shadow-cljs's (its count and index loop, where the walk
  built a seq step per item), and `hash` of a fresh 4-item vector or 7-key
  map is faster than shadow-cljs's. The vector's hash was mostly its items'
  hashes: `hash` of a number spent most of its time reading the `IHash`
  marker of a primitive by a computed name (56% of the profile), which it
  now reads by name. `=` of two 7-key array maps is 1.39x: each key's
  search of the other map (`arrayIndex`, 45% of the profile) still reads
  the key's own `IEquiv` slot through the protocol lookup once per search.
- **Hash maps** (`get-hash100` and the rest) hash keyword keys through the
  keyword's slot, its cached field, where `hash` reached the field after
  the protocol lookup and nine kind tests.
- **`kw-small` and `get-small`** (array maps of keyword keys, no hashing or
  equality) are faster on no path this change touches; the first operation
  of the process, so not the order of the operations. Not investigated.
- **`multimethod` +23% is the multimethod tables the compiler now chooses.**
  The release build keeps a multimethod's method table and cache in the
  atoms of full tables where a program "can build a value whose hash it
  chose", which src/rta.rs (`multi_tables`, `chosen_hash`) tells by an
  operation whose unit requires `constructorHashes`; `hash`'s unit did, so
  every program that read `hash` kept the full tables. Its split
  (`hashStructural` requires it now, not `hash`) ends that false trigger,
  and the gate program takes the lean tables, arrays walked with `isa?`
  on each call (`bestMethod`, `isaPlain`), which compare the dispatch
  keyword with each method's by `=`. Alone, in a small program
  (research/output-speed/eq-hash-1014/mm.cljs, `mm-nohash.cljs`), the lean
  tables dispatch faster than the full ones: 64.4 to 64.6 ns after against
  69.2 to 69.9 before, and 63.5 to 67.4 without `hash` in either. In the
  gate's process, where the earlier operations leave `=`'s call sites
  megamorphic, the walk's three or four `=` of two keywords, which have no
  `-equiv` slot of their own and take the old protocol lookup, cost more
  than the full tables' cache lookup. The same operation alone with three
  others, before and after (`--ops multimethod,record-assoc,seq-walk-vec-1000,multi-arity-call --runs 3`,
  load 12.12 and 12.72): 147.3 to 183.2 ns (+24%), the others unchanged
  (`seq-walk-vec-1000` 20,769 to 20,696, `record-assoc` 60.4 to 60.0,
  `multi-arity-call` 2.10 to 2.04). Filed as light-cljs-1040: keywords
  compare by their own slot, which also makes the lean walk cheap.
- **`seq-walk-vec-1000` +8%** is within the load: alone it is unchanged
  (above).

**Workload 200k** (`compare.bb workload-200k-seed42-1014 --runs 5
--runtime-only --runtime 1,10,40`, the project rebuilt with each binary just
before its run, the two runs back to back; load at most 12.38), wall time,
median (range):

| iterations | light before | light after | shadow-cljs (before run) | shadow-cljs (after run) |
| --- | ---: | ---: | ---: | ---: |
| 1 | 0.446 s (0.440-0.461) | 0.588 s (0.519-1.021) | 0.352 s | 0.511 s |
| 10 | 1.081 s (1.073-1.108) | 1.081 s (1.066-1.371) | 0.679 s | 0.704 s |
| 40 | 2.688 s (2.682-2.729) | 2.696 s (2.684-3.334) | 1.355 s | 1.424 s |

The after run's single iterations met a load spike (both compilers' medians
rose by a third), so the steady state comes from 10 and 40 iterations:
light-cljs 53.6 to 53.8 ms per iteration (0%), shadow-cljs 22.5 and 24.0;
light/shadow 2.38 and 2.24. The workload's equality and hashing are not
where its time goes.

**Advanced size** (tests/support/ratchet_fixtures.rs, noted there), against
master `5cd32c66`: about 1,000 to 2,000 bytes per program, the slots of the
classes it builds and the two wrappers (`vector-printed` 15,062 to 16,037,
`map-printed` 22,905 to 24,328, `map-only` 15,057 to 16,934, `set-only`
7,797 to 9,214); the features example about 4,000 (`features-direct`
122,470 to 126,491, its shared chunk 98,209 to 101,762). re-frame simple and
TodoMVC were not ported in this run.

## 2026-11-16: core operations call their call-site entries (light-cljs-1016)

**What.** At `:advanced`, a call of `count`, `nth`, `get` (and `(:k m)`,
`(:k m d)`), `first`, `rest`, `next`, `seq`, `peek`, `conj`, `assoc`,
`dissoc` or `contains?` at the arity its slot-first wrapper serves calls
that operation's call-site entry, `rt$countSite(x)`, `rt$getSite(m, k)`: a
47 to 75-byte runtime unit that V8 inlines into the site, which calls the
value's keyed slot (dotted, `x.cljs$core$ICounted$_count$arity$1$<key>(x)`)
and else, or where the slot answers `noBranch`, the operation. It reads no
keyed kind, so it is used only where every value that can hold a core
protocol slot has one (`ProgramFacts::core_slots_runtime_only`: core
protocols intact, no core protocol member written by name or spelled in raw
text); elsewhere the operations are called as before. `(:k m)` is one call,
`rt$getSite(m, k)`. docs/value-layout.md, "What light-cljs-1016 landed",
has the forms measured and left out (the slot test written at each site:
no faster once it kept `noBranch`, and about half again workload 200k's
output).

Binaries, in research/shadow-credibility/bin: before `light-cljs-1bd3ac3d`
(master, with light-cljs-1014), after `light-cljs-28019e68` (a commit of
this pellet before it was squashed: its emitter and runtime as landed).
Unreserved: one peer worker had been active. Raw results:
research/output-speed/call-sites-1016.

**Gate** (`bb benchmarks/core-ops/run.bb --ops
kw-small,get-small,get-small-miss,contains-small,count-small,assoc-small,dissoc-small,kw-hash100,get-hash100,vec-conj,vec-nth,vec-peek,destructure-vec,destructure-map,lazy-walk,seq-walk-vec,filter-count,reduce-entries-small
--runs 5`), 1-minute load average before a run 3.87-5.18 before and
4.34-5.12 after. ns per operation, median (range):

| op | light before | light after | change | shadow-cljs (after run) | light/shadow before | light/shadow after |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| kw-small | 13.4 (12.5-14.0) | 13.2 (13.1-13.7) | -1% | 11.1 | 1.23 | 1.19 |
| get-small | 18.3 (17.3-18.8) | 17.9 (17.4-17.9) | -3% | 16.8 | 1.11 | 1.06 |
| get-small-miss | 24.4 (23.5-24.7) | 23.3 (22.0-24.0) | -5% | 21.7 | 1.14 | 1.07 |
| contains-small | 19.5 (18.5-19.8) | 16.9 (16.6-17.4) | -13% | 13.4 | 1.43 | 1.26 |
| count-small | 2.99 (2.70-3.02) | 2.15 (2.15-2.22) | -28% | 2.39 | 1.25 | 0.90 |
| assoc-small-existing | 50.7 (46.0-51.1) | 51.1 (51.0-53.9) | +1% | 33.9 | 1.48 | 1.51 |
| assoc-small-new | 81.7 (75.8-81.9) | 84.5 (83.6-88.2) | +3% | 43.9 | 1.86 | 1.92 |
| dissoc-small | 92.0 (85.9-92.5) | 92.4 (91.3-96.6) | +0% | 87.8 | 1.02 | 1.05 |
| get-hash100 | 20.5 (19.0-21.0) | 20.0 (19.8-20.9) | -2% | 13.4 | 1.56 | 1.50 |
| kw-hash100 | 44.1 (41.8-45.9) | 41.1 (39.6-42.8) | -7% | 14.9 | 2.93 | 2.75 |
| vec-conj-64 | 3,008 (2,966-3,152) | 3,095 (2,971-3,152) | +3% | 1,555 | 1.87 | 1.99 |
| vec-nth-1024 | 6.31 (5.92-6.69) | 5.53 (5.29-5.73) | -12% | 5.82 | 1.12 | 0.95 |
| vec-peek | 4.40 (3.98-4.43) | 4.02 (3.82-4.19) | -9% | 3.71 | 1.14 | 1.08 |
| destructure-vec-3 | 15.6 (14.1-16.0) | 7.85 (7.55-8.31) | -50% | 11.1 | 1.42 | 0.71 |
| destructure-map-3 | 107.2 (97.1-109.4) | 104.7 (101.2-111.5) | -2% | 54.5 | 1.92 | 1.92 |
| filter-count-1000 | 48,673 (46,575-49,091) | 43,854 (43,576-47,092) | -10% | 31,756 | 1.50 | 1.38 |
| lazy-walk-1000 | 58,858 (56,821-60,664) | 50,407 (49,517-51,562) | -14% | 44,528 | 1.32 | 1.13 |
| seq-walk-vec-1000 | 32,043 (29,766-33,582) | 27,465 (26,798-28,506) | -14% | 11,268 | 2.89 | 2.44 |
| reduce-entries-small | 420.6 (400.1-445.5) | 390.6 (360.9-393.2) | -7% | 82.3 | 4.99 | 4.75 |

- **At or below shadow-cljs:** `count-small`, `vec-nth-1024` and
  `destructure-vec-3`, whose `nth` calls are the entries.
- **No operation is 10% slower.** `vec-conj-64` and `assoc-small-new` (+3%)
  are inside their before ranges' spread; their `conj` and `assoc` call
  their slots through the entries as through the wrappers.
- **`kw-small` and `get-small` gain under 1 ns**: an array map's lookup of
  a keyword reads the key's `IEquiv` through the protocol hooks once per
  lookup (`arrayIndex`), about 4.5 ns of `kw-small`'s 12.4 in a scratch
  edit of the output; light-cljs-1041.
- **`seq-walk-vec-1000`** stays 2.44 times shadow-cljs's, its gap now in
  the slots its steps call (not profiled here). Selected with four other
  operations only (`--ops
  kw-small,get-small,count-small,vec-nth,seq-walk-vec`, 3 alternated scratch
  runs of the outputs, load 8-12) it went from 20,500-21,000 to
  13,900-14,000 ns.

**Workload 200k and the re-frame-shaped 200k app** (`compare.bb PROJECT
--runs 5 --runtime-only --runtime 1,40`, the project's light-cljs output
rebuilt with each binary just before its run, two before/after pairs each;
load 4.1 to 8.4), wall time, median (range), and light-cljs's steady state:

| project, pair | light before (1 / 40) | light after (1 / 40) | shadow-cljs at 40 (before / after runs) | steady state, ms per iteration |
| --- | ---: | ---: | ---: | ---: |
| workload 200k, 1 | 0.448 / 2.735 s (2.710-2.889) | 0.428 / 2.655 s (2.646-2.681) | 1.455 / 1.353 s | 58.6 to 57.1 |
| workload 200k, 2 | 0.433 / 2.671 s (2.658-2.686) | 0.431 / 2.720 s (2.698-2.734) | 1.367 / 1.456 s | 57.4 to 58.7 |
| re-frame-shaped, 1 | 2.575 / 26.863 s (26.609-27.217) | 2.443 / 27.116 s (26.656-27.257) | 12.645 / 12.661 s | 622.8 to 632.6 |
| re-frame-shaped, 2 | 2.463 / 26.536 s (26.436-26.811) | 2.460 / 26.345 s (26.270-26.844) | 12.403 / 12.323 s | 617.2 to 612.4 |

Steady state is unchanged within the runs' noise in both programs: their
time is in the runtime's own operations, which call the operations and
not the entries, and the entries only replace the wrapper at each program
call site.

**Advanced size** (gzip -9):
- the gate: 170,584 to 172,677 bytes (gzip 43,277 to 43,679);
- workload 200k: 3,556,829 to 3,559,707 (gzip 896,001 to 896,683);
- the re-frame-shaped 200k app: 5,915,357 to 5,918,979 (gzip 1,198,864 to
  1,199,571);
- the size ratchet (tests/support/ratchet_fixtures.rs, noted there): 154
  to 328 bytes where a fixture calls the operations in its functions, 939
  for the features example; the readable runtime of `none` and `prepare`
  about 4,500 bytes. TodoMVC and re-frame simple were not ported in this
  run. Measured afterwards (light-cljs-1042, master `1bd3ac3d` to
  `15b858bf`, light-cljs and esbuild bundle with React, gzip -9): simple
  116,176 to 116,549 (+373), TodoMVC 148,774 to 149,052 (+278), flow
  unchanged at 347,830; see "re-frame simple, TodoMVC and flow sizes
  across the group" below.

## 2026-11-16: keywords and symbols compare by their own slots (light-cljs-1040)

**What.** Keywords and symbols have their own keyed `-equiv` slots, which
`=`'s slot-first wrapper calls: two keywords compare by kind and `fqn`, two
symbols by kind and `str`, where `=` of two keywords that were not identical
took the old protocol lookup and the structural comparison. The keyword
searches of array maps (`arrayIndex`, `entryIndex`, `arrayMapDissoc`) read
the key's slot once and take the class's own for no program method, with no
lookup. docs/value-layout.md has the notes.

Binaries: before `light-cljs-1bd3ac3d` (master), after `light-cljs-213006f5`
(this pellet's runtime; the final commit differs in comments, docs and the
size ratchet), in research/shadow-credibility/bin, and `light-cljs-5cd32c66`
(master before light-cljs-1014) for `multimethod`'s reference.
Unreserved: one peer worker was active. Raw results:
research/output-speed/kw-equiv-1040.

**Gate** (`bb benchmarks/core-ops/run.bb`, every operation, 5 runs; 1-minute
load average 4.89-5.82 before, 4.24-5.88 after), ns per operation, median
(range), the operations that moved by 5% or more:

| op | light before | light after | change | shadow-cljs before / after | light/shadow before | light/shadow after |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| multimethod | 183.3 (179.2-193.8) | 135.1 (133.0-137.9) | -26% | 51.7 / 52.1 | 3.55 | 2.60 |
| kw-small | 13.4 (12.8-13.5) | 8.98 (8.82-9.03) | -33% | 10.5 / 10.6 | 1.27 | 0.84 |
| get-small | 18.0 (17.3-18.1) | 13.0 (12.5-13.3) | -27% | 15.8 / 15.7 | 1.14 | 0.83 |
| get-small-miss | 23.7 (23.4-23.8) | 21.9 (21.3-22.9) | -7% | 20.4 / 20.6 | 1.16 | 1.06 |
| dissoc-small | 84.2 (82.3-85.0) | 69.6 (68.7-73.2) | -17% | 61.2 / 62.0 | 1.38 | 1.12 |
| destructure-map-3 | 101.5 (99.5-104.5) | 72.9 (71.2-76.7) | -28% | 53.3 / 53.4 | 1.90 | 1.37 |
| eq-map-7 | 234.5 (228.2-237.4) | 135.9 (133.3-138.1) | -42% | 163.3 / 162.5 | 1.44 | 0.84 |
| eq-vec-10 | 56.2 (55.3-57.6) | 43.7 (43.1-44.4) | -22% | 88.8 / 88.4 | 0.63 | 0.49 |
| kw-hash100 | 44.0 (43.0-44.6) | 22.4 (21.6-23.4) | -49% | 14.6 / 15.0 | 3.02 | 1.50 |
| get-in-3 | 72.2 (72.0-74.1) | 68.9 (68.0-71.1) | -5% | 60.7 / 61.9 | 1.19 | 1.11 |
| swap-assoc | 81.2 (80.5-83.4) | 69.9 (68.2-71.4) | -14% | 40.5 / 40.9 | 2.00 | 1.71 |
| select-keys-3 | 543.2 (530.5-546.2) | 485.1 (471.8-499.6) | -11% | 267.5 / 273.2 | 2.03 | 1.78 |
| record-kw-field | 6.98 (6.46-7.03) | 9.09 (8.78-9.56) | +30% | 4.42 / 4.60 | 1.58 | 1.98 |
| record-assoc | 58.8 (57.5-60.7) | 63.2 (60.4-64.1) | +7% | 14.7 / 14.9 | 4.01 | 4.23 |

`contains-small` -4%; every other operation moved less than 5%.

**The operations of this pellet, alone** (`--ops multimethod,kw-small,get-small-miss,eq-map-7,destructure-map-3,dissoc-small,get-small,contains-small`,
5 runs, the three binaries one after another; load 8.53-10.33, 11.28-11.65
and 11.83-12.10), light-cljs ns per operation, median (range):

| op | 5cd32c66 | 1bd3ac3d (before) | 213006f5 (after) | shadow-cljs (after run) |
| --- | ---: | ---: | ---: | ---: |
| multimethod | 138.0 (135.9-138.4) | 137.3 (136.3-138.0) | 94.1 (93.4-94.6) | 40.1 |
| kw-small | 21.6 (21.3-22.2) | 12.8 (12.7-12.8) | 8.43 (8.35-8.47) | 10.1 |
| get-small | 26.4 (26.1-27.5) | 17.2 (17.0-17.3) | 12.2 (12.1-12.3) | 15.3 |
| get-small-miss | 32.0 (31.6-33.8) | 22.5 (22.4-22.6) | 20.7 (20.5-20.9) | 19.6 |
| contains-small | 27.3 (26.8-28.2) | 18.1 (18.0-18.2) | 17.6 (15.6-17.7) | 12.3 |
| dissoc-small | 112.4 (110.2-146.9) | 86.4 (86.0-87.2) | 76.0 (75.5-78.1) | 85.2 |
| destructure-map-3 | 71.3 (69.9-77.9) | 51.1 (51.0-51.3) | 49.2 (48.1-52.4) | 35.3 |
| eq-map-7 | 247.3 (241.2-248.1) | 121.5 (120.9-122.2) | 100.1 (99.6-100.8) | 102.4 |

- **`multimethod` is back under its `5cd32c66` time**: 135.1 ns in the whole
  gate against 153.1 there (light-cljs-1014's section) and 183.3 before, and
  94.1 alone against 138.0 and 137.3. The lean multimethod tables compare
  the dispatch keyword with each method's by `=` (`isaPlain`), three or four
  times a call, which is now the wrapper and the keyword's slot. Alone, where
  the earlier operations leave `=`'s call sites less polymorphic, the lean
  tables were no slower than 5cd32c66's full ones before this change either.
- **Keyword searches** (`kw-small`, `get-small`, `destructure-map-3`,
  `dissoc-small`, and `eq-map-7`, whose map comparison searches the other
  map once per key) no longer call the protocol lookup (`keyMethod` and
  `externalMethod`) at the first key that is not the searched one: the
  search reads the key's slot and finds the class's own. `eq-map-7` is now
  faster than shadow-cljs's; `get-small-miss` (a miss compares every key by
  `fqn`) gains least. `kw-hash100` halves: its keys are built at run time
  (`keyword`), so a hash map's lookup of the literal `:k42` compares them
  with `=`, which now takes the slot.
- **`record-kw-field` +30% is the gate process's inlining, not the record
  lookup.** Alone (`--ops record-kw-field,record-dot-field,record-assoc`,
  3 runs, load 12.95 and 14.11) it is unchanged, 5.65 to 5.63 ns, and so is
  `record-assoc` (60.1 to 59.3); after the equality operations too (5.70 to
  5.67). It slows only after the hash map operations (the gate's
  `map-literal-3` to `update-in-3` first: 6.97 to 8.98 ns in one process,
  pseudo-names builds 6.59 to 8.66). The benchmark's `(:w r)` calls `get`,
  whose slot-first wrapper calls the value's `-lookup` slot, and in that
  process the call has seen three targets: the array map's, the hash map's
  and the record's. TurboFan's trace (`--trace-turbo-inlining`) shows the
  hash map's lookup, whose optimized code now inlines `=`'s keyword slot,
  at 636 bytes of inlined bytecode, so the three-target call is no longer
  inlined, and the record's two-argument `-lookup` method runs as a call of
  its own (17% of the operation's profile; before, it was inlined into the
  loop and only the three-argument method was a call). A program whose one
  `get` site takes hash maps and records alike pays this; one that keeps
  them apart does not. Not fixed here.

**On light-cljs-1016's call-site entries.** Rebased onto master `15b858bf`
(light-cljs-1016), the same change (`light-cljs-9c37a6b1`; the final commit
differs in tests and notes) against `light-cljs-15b858bf`, `--ops
kw-small,get-small,get-small-miss,contains-small,dissoc-small,kw-hash100,destructure-map-3,eq-map-7,record-kw-field,multimethod`,
5 runs each (load 5.36-6.99, then 4.29-5.24), ns per operation, median
(range):

| op | light before | light after | change | shadow-cljs before / after | light/shadow after |
| --- | ---: | ---: | ---: | ---: | ---: |
| kw-small | 13.6 (12.8-13.8) | 9.13 (8.80-9.21) | -33% | 11.1 / 10.9 | 0.83 |
| get-small | 18.1 (16.9-18.4) | 14.4 (14.3-14.6) | -20% | 16.3 / 16.9 | 0.85 |
| get-small-miss | 24.1 (21.9-24.3) | 19.2 (18.8-19.7) | -20% | 21.0 / 21.9 | 0.88 |
| contains-small | 17.4 (16.3-17.6) | 15.8 (14.8-16.0) | -9% | 12.9 / 13.8 | 1.15 |
| dissoc-small | 95.5 (87.5-97.2) | 77.6 (75.1-80.5) | -19% | 89.6 / 91.2 | 0.85 |
| kw-hash100 | 43.2 (38.9-43.9) | 23.0 (21.5-23.5) | -47% | 14.1 / 14.5 | 1.59 |
| destructure-map-3 | 104.9 (98.8-106.4) | 79.9 (61.8-80.8) | -24% | 51.9 / 53.0 | 1.51 |
| eq-map-7 | 236.2 (224.5-241.2) | 170.5 (161.3-172.4) | -28% | 168.0 / 168.3 | 1.01 |
| record-kw-field | 6.38 (6.21-6.66) | 6.88 (6.43-7.00) | +8% | 4.75 / 4.74 | 1.45 |
| multimethod | 167.9 (158.5-170.9) | 108.8 (98.2-110.5) | -35% | 46.6 / 46.7 | 2.33 |

This is most of light-cljs-1041 (filed by light-cljs-1016: an array map's
lookup of a keyword reads the key's `IEquiv` through the protocol hooks
once per lookup): the search now reads the key's slot by name and takes
the class's own for none, so `kw-small`, `get-small` and `get-small-miss`
are under shadow-cljs's. `contains-small` stays at 1.15.

**Compile time** (the change's other side: the compile-time protocol lookup
returns the runtime's own slots, so a keyword slot could have sent every
macro-time keyword search through `=`). A macro that searches a 7-key array
map for keywords built at expansion time a million times (`get`, `=`,
`contains?`, `dissoc`; research/output-speed/kw-equiv-1040/macro), built
with `--compile --entry --no-cache --stats`, 10 builds per binary
alternating, load 10.64-11.17: the expansion's engine time 626.7 ms
(623.5-630.8) before, 510.7 ms (508.4-514.0) after, -19%; the build's wall
time 657.1 ms to 540.3 ms. Workload 200k, built clean (`release bench`, its
cache off) 5 times per binary alternating, load 11.22-11.95: wall time
7.06 s (6.82-7.19) before, 7.07 s (6.79-7.19) after; peak memory 1.038 GB
(1.030-1.053) and 1.041 GB (1.033-1.064).

**Workload 200k** (`compare.bb workload-200k-seed42-1040 --runs 5
--runtime-only --runtime 1,10,40`, the project rebuilt with each binary just
before its run; load 12.22-12.99), wall time, median (range):

| iterations | light before | light after | shadow-cljs (before run) | shadow-cljs (after run) |
| --- | ---: | ---: | ---: | ---: |
| 1 | 0.446 s (0.441-0.453) | 0.446 s (0.442-0.450) | 0.356 s | 0.355 s |
| 10 | 1.065 s (1.054-1.068) | 1.048 s (1.044-1.057) | 0.672 s | 0.674 s |
| 40 | 2.704 s (2.686-2.730) | 2.686 s (2.669-2.705) | 1.436 s | 1.425 s |

Steady state, from 1 and 40 iterations: light-cljs 57.9 to 57.5 ms per
iteration (-1%), shadow-cljs 27.7 and 27.4; light/shadow 2.09 both times.

**Advanced size** (tests/support/ratchet_fixtures.rs, noted there), against
master `1bd3ac3d`: about 500 bytes for a program of keywords alone, which now
keeps `=`'s wrapper (`keyword-only` 5,090 to 5,599), 240 to 400 for one with
maps (`map-printed` 24,423 to 24,665, `set-only` 9,251 to 9,572); the
features example about 500 (`features-direct` 127,104). Workload 200k's
module grows 423 bytes (3,556,829 to 3,557,252). re-frame simple and TodoMVC
were not ported in this run.

## 2026-11-16: protocol markers shared by copies of one build (light-cljs-1018)

**What.** The value every protocol marker holds (`protocolSentinel`,
upstream's `PROTOCOL_SENTINEL`) was a frozen object per runtime load; it is
now one per runtime key on the page, `Symbol.for("light-cljs.protocol" +
key)`, so that two copies of one build, which share the key and both write
the keyed markers of an `extend-type` of a host constructor, recognise each
other's (docs/value-layout.md, section 1, "Two runtimes on one page"). Only
the marker tests (`satisfies?` of a value without mask bits, the hash and
lookup paths that read a marker) see the value; they compare it with `===`
as before.

Binaries, in research/shadow-credibility/bin: before `light-cljs-15b858bf`
(master), after `light-cljs-a3347d18` (a commit of this pellet before it
was squashed: its runtime as landed). Unreserved: one peer worker and this
pellet's QuickJS reference suite were running. Raw results:
research/output-speed/two-runtimes-1018.

**Gate** (`bb benchmarks/core-ops/run.bb --ops
hash-fresh-vec-4,hash-fresh-map-7,eq-map-7,eq-vec-10,protocol-on-record,kw-hash100,multimethod
--runs 5`), 1-minute load average before a run 12.89-13.48 before and
12.67-13.22 after. ns per operation, median (range):

| op | light before | light after | change | shadow-cljs (after run) | light/shadow before | light/shadow after |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| kw-hash100 | 39.5 (39.2-41.7) | 39.7 (39.2-41.3) | +0% | 12.4 | 3.21 | 3.21 |
| eq-map-7 | 218.8 (217.7-220.6) | 219.2 (215.9-219.5) | +0% | 154.8 | 1.41 | 1.42 |
| eq-vec-10 | 76.5 (75.7-76.7) | 75.8 (75.4-77.1) | -1% | 61.3 | 1.25 | 1.24 |
| hash-fresh-vec-4 | 51.9 (51.4-53.6) | 51.9 (51.2-54.1) | +0% | 64.4 | 0.81 | 0.81 |
| hash-fresh-map-7 | 237.3 (236.1-237.6) | 235.3 (234.0-237.3) | -1% | 482.6 | 0.49 | 0.49 |
| protocol-on-record | 2.57 (2.56-2.61) | 2.57 (2.55-2.63) | +0% | 2.41 | 1.04 | 1.07 |
| multimethod | 183.6 (182.0-186.1) | 184.8 (181.3-187.1) | +1% | 43.4 | 4.20 | 4.26 |

No operation moved outside its before range.

**Workload 200k** (`compare.bb PROJECT --runs 5 --runtime-only --runtime
1,40`, the project's light-cljs output rebuilt with each binary just before
its run, two before/after pairs; load 9.3 to 14.0), wall time, median
(range), and light-cljs's steady state:

| pair | light before (1 / 40) | light after (1 / 40) | shadow-cljs at 40 (before / after runs) | steady state, ms per iteration |
| --- | ---: | ---: | ---: | ---: |
| 1 | 0.441 / 2.772 s (2.625-3.130) | 0.431 / 2.640 s (2.581-2.764) | 1.427 / 1.361 s | 59.8 to 56.6 |
| 2 | 0.493 / 2.729 s (2.612-2.915) | 0.436 / 2.644 s (2.598-2.901) | 1.427 / 1.364 s | 57.3 to 56.6 |

Unchanged within the runs' noise: shadow-cljs's runs beside the after runs
were as much faster (light/shadow steady state 2.16 and 2.12 before, 2.17
and 2.17 after).

**Advanced size** (gzip -9): 19 bytes more where a slice keeps the marker
value, which is written as `Symbol.for(...)`:
- the gate: 172,677 to 172,696 bytes (gzip 43,679 to 43,667);
- workload 200k: 3,559,707 to 3,559,726 (gzip 896,683 to 896,656);
- the size ratchet (tests/support/ratchet_fixtures.rs, noted there):
  `defmulti` 19,471 to 19,490; the readable runtime of `none` and
  `prepare` 52 bytes more with the unit's comment. TodoMVC and re-frame
  simple were not ported in this run.

**After the measurements** (the Sol review of this pellet): the identity
hash of a host object (`identityHashes`, the hash of a value with no hash
of its own) became one `WeakMap` per runtime key, held by the key registry,
so that copies of one build hash a collection holding the same host object
alike. It is reached only by `hash` of such a value, which none of the
gate's operations hashes, and it was not timed. It adds the table's lookup
at load to slices that keep it, about 100 bytes at advanced: the
`constant-keys` ratchet fixture went 71 bytes past its 87,000 ceiling, and
the chunked build of `chunks::terse_chunks_write_their_runtime_units_at_module_level`
is 130,400 bytes.

## 2026-11-16: the kind tables and protocol wrappers removed (light-cljs-1015)

**What.** The kind tables (`kindOps`, `kindTables`, the nine wrappers that
read them and their 61 branch units) and the protocol wrappers of `seq`,
`first`, `rest`, `next` and `count` (`behindProtocol`) are deleted: a value
whose class has the operation's slot takes the slot-first wrapper or the
call-site entry, every other value the operation's own path, which looks a
program's implementation up after its nil case. `publicMethod` is
`extendedMethod`, which a host value takes without the payload stores'
probes; a runtime class's value without a slot still probes them
(light-cljs-1046). docs/value-layout.md, "What light-cljs-1015 landed".

Binaries, in research/shadow-credibility/bin: before `light-cljs-0b473fdb`
(master), after `light-cljs-a2c54b1c` (a commit of this pellet before it was
squashed: its runtime as landed but for the narrower extension test of
`fieldComparisons`, which only programs extending a runtime type reach;
the gate does not). Unreserved: one peer worker and its suites ran
throughout. Raw results: research/output-speed/remove-layers-1015.

**Gate** (`bb benchmarks/core-ops/run.bb --runs 5`, every operation, the
before binary run first and the after binary next), 1-minute load average
before a run 8.10-18.57 before and 11.94-23.38 after. ns per operation,
median (range):

| op | light before | light after | change | shadow (before / after runs) | light/shadow before | light/shadow after |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| kw-small | 8.15 (8.13-8.24) | 8.19 (8.13-15.7) | +0% | 9.91 / 9.95 | 0.82 | 0.82 |
| get-small | 11.5 (11.5-11.7) | 12.2 (11.5-16.3) | +6% | 15.0 / 15.0 | 0.77 | 0.81 |
| get-small-miss | 16.0 (15.9-16.1) | 16.1 (15.9-22.0) | +1% | 19.5 / 19.4 | 0.82 | 0.83 |
| contains-small | 14.0 (12.0-14.1) | 14.1 (13.9-20.7) | +1% | 12.2 / 12.1 | 1.15 | 1.17 |
| count-small | 1.99 (1.91-1.99) | 2.01 (1.93-2.61) | +1% | 2.14 / 2.14 | 0.93 | 0.94 |
| assoc-small-existing | 37.8 (37.3-38.2) | 37.9 (36.9-59.5) | +0% | 32.3 / 31.2 | 1.17 | 1.22 |
| assoc-small-new | 65.0 (64.8-66.1) | 65.6 (64.1-83.3) | +1% | 40.3 / 39.6 | 1.61 | 1.66 |
| dissoc-small | 65.2 (64.7-69.6) | 72.8 (65.0-259.0) | +12% | 58.0 / 57.9 | 1.12 | 1.26 |
| update-small | 48.4 (47.7-49.0) | 48.4 (47.1-60.6) | +0% | 44.8 / 44.7 | 1.08 | 1.08 |
| map-literal-3 | 21.2 (20.9-27.0) | 21.6 (20.7-86.8) | +2% | 10.6 / 10.6 | 2.00 | 2.04 |
| get-hash100 | 19.9 (19.8-20.0) | 19.9 (19.8-22.0) | +0% | 12.2 / 12.2 | 1.62 | 1.63 |
| kw-hash100 | 22.3 (21.9-29.6) | 22.2 (21.8-34.9) | -0% | 13.8 / 13.6 | 1.62 | 1.63 |
| assoc-hash100 | 98.3 (94.3-103.4) | 96.6 (92.2-129.3) | -2% | 80.6 / 81.8 | 1.22 | 1.18 |
| dissoc-hash100 | 105.8 (101.8-112.1) | 104.3 (99.8-123.0) | -1% | 78.5 / 78.1 | 1.35 | 1.33 |
| update-hash100 | 111.9 (111.0-119.2) | 111.7 (107.2-143.7) | -0% | 97.1 / 99.3 | 1.15 | 1.13 |
| get-in-3 | 57.4 (56.5-61.9) | 57.0 (55.9-66.9) | -1% | 59.0 / 60.7 | 0.97 | 0.94 |
| assoc-in-3 | 211.7 (210.7-223.4) | 210.5 (206.7-233.7) | -1% | 133.6 / 135.8 | 1.58 | 1.55 |
| update-in-3 | 234.9 (233.8-237.0) | 232.2 (225.7-356.5) | -1% | 142.0 / 140.5 | 1.65 | 1.65 |
| vec-conj-64 | 2,257 (2,154-2,407) | 2,199 (2,128-2,523) | -3% | 1,330 / 1,401 | 1.70 | 1.57 |
| vec-literal-4 | 21.5 (20.6-22.0) | 21.5 (20.8-22.7) | +0% | 6.90 / 6.85 | 3.11 | 3.13 |
| vec-nth-1024 | 5.15 (4.91-5.17) | 5.10 (4.89-5.28) | -1% | 5.28 / 5.15 | 0.98 | 0.99 |
| vec-invoke-1024 | 5.17 (5.14-5.29) | 5.11 (5.04-5.68) | -1% | 4.87 / 4.92 | 1.06 | 1.04 |
| vec-peek | 3.71 (3.69-3.74) | 3.75 (3.68-3.77) | +1% | 3.48 / 3.49 | 1.07 | 1.07 |
| vec-assoc-1024 | 68.2 (67.8-68.4) | 67.9 (66.3-69.5) | -0% | 75.2 / 76.8 | 0.91 | 0.88 |
| destructure-vec-3 | 7.40 (7.39-7.42) | 7.41 (7.20-7.44) | +0% | 9.89 / 9.86 | 0.75 | 0.75 |
| destructure-map-3 | 66.2 (62.6-67.4) | 64.5 (62.6-65.2) | -3% | 49.8 / 49.5 | 1.33 | 1.30 |
| reduce-vec-1000 | 5,283 (5,235-5,455) | 5,272 (5,246-5,477) | -0% | 2,515 / 2,523 | 2.10 | 2.09 |
| reduce-map-inc-1000 | 49,477 (48,807-50,102) | 49,819 (49,517-50,868) | +1% | 41,741 / 41,823 | 1.19 | 1.19 |
| filter-count-1000 | 41,862 (41,349-42,397) | 41,101 (40,849-43,342) | -2% | 29,355 / 30,144 | 1.43 | 1.36 |
| into-xform-1000 | 69,575 (68,488-70,604) | 69,413 (65,341-73,779) | -0% | 18,642 / 18,849 | 3.73 | 3.68 |
| mapv-1000 | 48,906 (48,167-50,456) | 49,471 (46,951-52,280) | +1% | 14,414 / 14,551 | 3.39 | 3.40 |
| lazy-walk-1000 | 44,118 (43,514-44,568) | 43,922 (43,333-45,142) | -0% | 40,036 / 40,652 | 1.10 | 1.08 |
| seq-walk-vec-1000 | 25,268 (25,097-25,592) | 25,022 (24,495-25,429) | -1% | 10,284 / 10,113 | 2.46 | 2.47 |
| reduce-range-1000 | 4,942 (4,882-5,007) | 4,908 (4,807-5,080) | -1% | 5,528 / 5,520 | 0.89 | 0.89 |
| reduce-kv-small | 50.2 (49.9-51.2) | 50.2 (49.8-50.8) | +0% | 38.2 / 38.2 | 1.32 | 1.32 |
| reduce-entries-small | 357.4 (349.5-363.2) | 359.7 (355.8-374.2) | +1% | 77.1 / 80.0 | 4.63 | 4.50 |
| str-nested-map | 4,148 (4,073-4,260) | 4,103 (4,054-4,136) | -1% | 2,254 / 2,277 | 1.84 | 1.80 |
| str-concat | 171.0 (167.0-182.8) | 169.6 (167.2-173.9) | -1% | 14.8 / 14.9 | 11.55 | 11.35 |
| eq-map-7 | 124.8 (122.2-127.4) | 121.8 (119.1-122.5) | -2% | 153.3 / 151.1 | 0.81 | 0.81 |
| eq-vec-10 | 40.4 (40.2-41.6) | 39.4 (38.5-39.8) | -2% | 82.6 / 82.0 | 0.49 | 0.48 |
| hash-fresh-vec-4 | 52.1 (51.3-53.3) | 51.3 (47.5-51.4) | -2% | 72.5 / 72.5 | 0.72 | 0.71 |
| hash-fresh-map-7 | 285.5 (274.0-286.8) | 277.7 (270.5-285.5) | -3% | 557.3 / 555.6 | 0.51 | 0.50 |
| protocol-on-record | 2.55 (2.47-2.92) | 2.55 (2.50-2.55) | +0% | 2.40 / 2.40 | 1.06 | 1.06 |
| record-kw-field | 8.04 (8.03-9.06) | 8.01 (7.94-8.07) | -0% | 4.23 / 4.24 | 1.90 | 1.89 |
| record-dot-field | 0.88 (0.84-0.93) | 0.88 (0.86-0.89) | +0% | 0.88 / 0.88 | 1.00 | 1.00 |
| record-assoc | 57.2 (56.2-60.7) | 56.5 (55.2-57.0) | -1% | 14.1 / 14.3 | 4.05 | 3.96 |
| multimethod | 116.4 (115.2-120.1) | 117.1 (115.8-118.4) | +1% | 48.9 / 48.4 | 2.38 | 2.42 |
| multi-arity-call | 2.04 (2.03-2.04) | 2.02 (2.01-2.04) | -1% | 2.04 / 2.03 | 1.00 | 1.00 |
| variadic-call | 15.1 (14.9-15.3) | 13.8 (13.5-14.3) | -9% | 14.5 / 14.4 | 1.04 | 0.95 |
| apply-vec | 191.7 (179.2-198.0) | 192.0 (184.2-194.5) | +0% | 93.4 / 92.8 | 2.05 | 2.07 |
| unknown-fn-call | 4.51 (4.47-4.52) | 4.53 (4.39-4.55) | +0% | 6.62 / 6.65 | 0.68 | 0.68 |
| closure-alloc-call | 9.25 (9.22-9.31) | 9.31 (9.09-9.31) | +1% | 8.58 / 8.77 | 1.08 | 1.06 |
| swap-inc | 4.82 (4.74-4.90) | 4.80 (4.67-4.93) | -0% | 5.65 / 5.71 | 0.85 | 0.84 |
| swap-assoc | 60.4 (60.3-60.5) | 61.1 (59.8-64.6) | +1% | 38.6 / 38.8 | 1.57 | 1.57 |
| merge-small | 2,804 (2,784-3,287) | 2,852 (2,766-2,951) | +2% | 916.7 / 970.2 | 3.06 | 2.94 |
| select-keys-3 | 387.8 (379.4-400.2) | 392.6 (388.6-407.9) | +1% | 253.9 / 280.2 | 1.53 | 1.40 |
| group-by-100 | 11,381 (11,280-12,094) | 11,789 (11,401-12,216) | +4% | 5,139 / 5,573 | 2.21 | 2.12 |
| sort-by-100 | 7,508 (7,376-7,782) | 7,295 (7,227-7,374) | -3% | 4,767 / 5,366 | 1.57 | 1.36 |
| transient-vec-100 | 4,945 (4,865-5,094) | 4,933 (4,904-5,007) | -0% | 1,167 / 1,342 | 4.24 | 3.68 |
| transient-map-100 | 10,855 (10,520-20,037) | 10,590 (10,509-10,810) | -2% | 4,273 / 4,336 | 2.54 | 2.44 |
| sorted-map-assoc-100 | 210.8 (203.0-310.8) | 206.7 (205.0-213.8) | -2% | 211.6 / 223.6 | 1.00 | 0.92 |
| str-sorted-map-20 | 32,053 (31,862-45,529) | 31,809 (31,261-32,054) | -1% | 20,857 / 22,230 | 1.54 | 1.43 |
| bit-and | 0.60 (0.59-0.61) | 0.60 (0.58-0.64) | +0% | 0.60 / 0.60 | 1.00 | 1.00 |
| aget-bit-and | 0.60 (0.60-0.61) | 0.60 (0.59-0.62) | +0% | 0.60 / 0.60 | 1.00 | 1.00 |

- **No operation is more than 10% slower but `dissoc-small`** (+12%): its
  after range is 65.0-259.0, one run at the highest load of the session,
  and its lowest run equals the before median; `dissoc` takes its
  slot-first wrapper and call-site entry, which this pellet does not touch,
  and shadow-cljs's `dissoc-small` moved by 0.2%. Every other change is
  within 9% and inside the runs' spread (`variadic-call` -9%, unrelated).
- **No gate operation reached the removed layers' fast paths**: its values
  take their classes' slots, and the gate keeps no protocol lookup.
- **What got slower off the gate**, in scratch runs of a small program
  (each operation 2 to 5 million times): `count` of a cons about 65%
  slower (35 to 60 ns), after `count` regained the branch's walk on its
  own path (three times slower without it), and of a queue about 10%; a
  `next` of a vector and `conj` and `seq` of a set within 12%. None has a
  slot (upstream's `Cons` does not implement `ICounted`); light-cljs-1046
  gives the classes the slots they lack.

**Workload 200k and the re-frame-shaped 200k app** (`compare.bb PROJECT
--runs 5 --runtime-only --runtime 1,40`, the project's light-cljs output
rebuilt with each binary just before its run, one before/after pair each;
load 7.8 to 23), wall time at 40 iterations, median (range), and
light-cljs's steady state:

| project | light before | light after | shadow-cljs (before / after runs) | steady state, ms per iteration |
| --- | ---: | ---: | ---: | ---: |
| workload 200k | 3.340 s (3.235-3.902) | 2.646 s (2.567-2.730) | 2.116 s (1.454-2.801) / 1.383 s (1.339-1.447) | 70.6 to 56.2 |
| re-frame-shaped | 26.872 s (26.307-27.146) | 27.351 s (26.718-33.377) | 13.020 / 13.154 s | 628.2 to 636.9 |

The workload 200k before run was disturbed (shadow-cljs's own range
1.45-2.80 s); its after numbers match light-cljs-1016's (2.655-2.720 s at
40, shadow-cljs 1.35-1.46 s). Steady state is unchanged within noise in
both programs, as the gate's operations are: their time is in the slots,
which this pellet does not change.

**Advanced size** (the runtime slice, `--size-report`):
- the gate: 153,386 to 147,541 runtime bytes (1,015 to 961 units);
- workload 200k: 186,552 to 179,975 (1,314 to 1,253 units);
- the re-frame-shaped 200k app: 203,015 to 196,074 (1,443 to 1,377 units);
- the readable runtime of `none`, `prepare` and dev builds: 674,075 to
  651,992 bytes; the size ratchet (tests/support/ratchet_fixtures.rs,
  noted there) 75 to 5,640 bytes smaller where a fixture keeps
  collections, `rest-fn` 735 larger. TodoMVC and re-frame simple were not
  ported in this run. Measured afterwards (light-cljs-1032, master `56bf7795`
  to `685f86dd`, light-cljs and esbuild bundle with React, gzip -9): simple
  116,640 to 115,247 (-1,393), TodoMVC 149,378 to 147,758 (-1,620), flow
  348,003 to 346,561 (-1,442); see "re-frame simple, TodoMVC and flow sizes
  across the group" below.
## 2026-11-16: re-frame simple, TodoMVC and flow sizes across the group (light-cljs-1032, light-cljs-1042)

Most sections above say re-frame simple's and TodoMVC's gzip sizes were not measured. benchmarks/re-frame-vs-shadow.md, "2026-11-16: release size across the output-speed-parity group", now measures them for all 24 master commits of the group that have a binary in research/shadow-credibility/bin, from `f533efb1` to `685f86dd`. It covers simple, TodoMVC and flow, with each commit's change, and shadow-cljs 3.5.3 beside them. Raw results are in research/output-speed/app-sizes-1032.

The light-cljs and esbuild bundle with React, gzip -9, from the group's start to master now:

| App | `f533efb1` | `685f86dd` | Change | shadow-cljs | light/shadow, start to now |
|---|---:|---:|---:|---:|---:|
| re-frame simple | 108,631 | 115,247 | +6,616 (+6.1%) | 101,065 | 1.075 to 1.140 |
| TodoMVC | 138,015 | 147,758 | +9,743 (+7.1%) | 137,102 | 1.007 to 1.078 |
| flow | 328,645 | 346,561 | +17,916 (+5.5%) | 358,659 | 0.916 to 0.966 |

- **Raw growth** was 11.4% for simple, 11.6% for TodoMVC and 5.2% for flow.
- **Per pellet:** most conversion pellets added 500 to 1,400 bytes after gzip.
  - light-cljs-1011, which gives every keyword constant its hash, added 1,378, 2,011 and 9,758 bytes in simple, TodoMVC and flow.
  - light-cljs-1038 gave simple back 2,752 bytes: simple no longer keeps the sorted map's tree code.
  - light-cljs-1015 gave back 1,393, 1,620 and 1,442 bytes in simple, TodoMVC and flow.
- **The browser behaviour checks** pass 3/3 on master (`56bf7795` and `685f86dd`) for TodoMVC, simple and flow, on both compilers (`bb benchmarks/re-frame.bb browser todomvc simple flow`).

## 2026-11-17: one store test for the runtime's values (light-cljs-1046)

**What.** The protocol lookup of `extend-type` programs (`extendedMethod`),
the `satisfies` and `implements` hooks and the store inventories
(`ownedProtocolValue`, `seqableValue` and the other native tests) read a
value's keyed kind once and test its class's store (`kindTable`), where
they probed the stores in turn (`payloadStore`, `storesPredicate`); the
hash sets and queues have `-count`, `-seq` and `-conj` slots, the sets
`-lookup`, and the vectors, subvectors and map entries `-contains-key?`.
docs/value-layout.md, "What light-cljs-1046 landed".

Binaries, in research/shadow-credibility/bin: before `light-cljs-685f86dd`
(master), after `light-cljs-f40b8d79` (this pellet's runtime as landed but
for comments and tests). Unreserved: one peer worker and its suites ran
throughout, with the 1-minute load average up to 122 during the workload
runs. Raw results: research/output-speed/store-test-1046.

**Gate** (`bb benchmarks/core-ops/run.bb --runs 5`, every operation, the
before binary run first and the after binary next), 1-minute load average
before a run 5.92-28.75 before and 3.84-17.61 after. ns per operation,
median (range):

| op | light before | light after | change | shadow (before / after runs) | light/shadow before | light/shadow after |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| kw-small | 8.14 (8.11-8.17) | 8.16 (8.02-8.21) | +0% | 9.81 / 9.75 | 0.83 | 0.84 |
| get-small | 11.5 (11.5-11.6) | 11.5 (11.4-11.8) | +0% | 15.0 / 14.9 | 0.77 | 0.77 |
| get-small-miss | 16.0 (15.9-16.0) | 15.9 (15.7-16.0) | -1% | 19.2 / 19.3 | 0.83 | 0.82 |
| contains-small | 14.0 (12.0-14.1) | 14.0 (13.9-14.0) | +0% | 12.1 / 12.1 | 1.16 | 1.15 |
| count-small | 1.98 (1.94-2.00) | 1.97 (1.97-2.07) | -1% | 2.12 / 2.13 | 0.93 | 0.92 |
| assoc-small-existing | 38.1 (37.1-38.4) | 37.3 (36.7-38.6) | -2% | 31.2 / 30.9 | 1.22 | 1.21 |
| assoc-small-new | 64.8 (63.5-65.6) | 65.1 (63.5-66.6) | +0% | 39.5 / 39.4 | 1.64 | 1.65 |
| dissoc-small | 66.0 (64.8-68.6) | 65.0 (63.8-66.3) | -2% | 58.1 / 57.2 | 1.14 | 1.14 |
| update-small | 48.2 (46.3-49.5) | 47.7 (47.4-48.2) | -1% | 43.9 / 43.7 | 1.10 | 1.09 |
| map-literal-3 | 20.5 (19.5-21.4) | 19.8 (19.5-21.4) | -3% | 10.2 / 10.1 | 2.00 | 1.96 |
| get-hash100 | 19.7 (19.5-19.9) | 19.7 (19.4-20.1) | +0% | 12.1 / 12.2 | 1.64 | 1.61 |
| kw-hash100 | 22.2 (21.9-22.3) | 22.3 (21.9-22.5) | +0% | 13.6 / 13.7 | 1.63 | 1.63 |
| assoc-hash100 | 96.0 (92.2-96.3) | 92.5 (91.9-95.1) | -4% | 79.7 / 78.4 | 1.21 | 1.18 |
| dissoc-hash100 | 105.3 (101.5-106.4) | 104.1 (103.3-107.3) | -1% | 77.4 / 77.7 | 1.36 | 1.34 |
| update-hash100 | 110.8 (108.0-180.9) | 109.1 (107.9-111.3) | -2% | 98.0 / 96.4 | 1.13 | 1.13 |
| get-in-3 | 57.2 (56.2-232.1) | 56.8 (55.9-58.3) | -1% | 58.0 / 57.6 | 0.99 | 0.99 |
| assoc-in-3 | 211.2 (208.6-397.0) | 206.4 (203.7-213.8) | -2% | 137.1 / 131.7 | 1.54 | 1.57 |
| update-in-3 | 235.5 (233.1-483.8) | 232.8 (226.3-238.0) | -1% | 139.8 / 139.2 | 1.68 | 1.67 |
| vec-conj-64 | 2,203 (2,135-2,460) | 2,509 (2,434-2,886) | +14% | 1,352 / 1,310 | 1.63 | 1.91 |
| vec-literal-4 | 20.8 (20.4-21.8) | 20.7 (20.4-25.9) | +0% | 6.85 / 6.80 | 3.04 | 3.04 |
| vec-nth-1024 | 5.16 (5.13-5.23) | 5.22 (5.12-5.44) | +1% | 5.16 / 5.22 | 1.00 | 1.00 |
| vec-invoke-1024 | 5.11 (4.96-5.19) | 5.22 (5.00-5.44) | +2% | 4.85 / 5.03 | 1.05 | 1.04 |
| vec-peek | 3.73 (3.65-3.78) | 3.71 (3.62-3.78) | -1% | 3.46 / 3.47 | 1.08 | 1.07 |
| vec-assoc-1024 | 68.6 (67.2-71.1) | 72.7 (71.5-76.8) | +6% | 74.8 / 74.5 | 0.92 | 0.98 |
| destructure-vec-3 | 7.39 (7.34-7.82) | 7.40 (7.35-7.48) | +0% | 10.3 / 9.87 | 0.72 | 0.75 |
| destructure-map-3 | 66.5 (62.9-70.7) | 64.4 (62.3-67.9) | -3% | 50.0 / 49.1 | 1.33 | 1.31 |
| reduce-vec-1000 | 5,328 (5,242-5,485) | 5,254 (5,200-5,584) | -1% | 2,516 / 2,528 | 2.12 | 2.08 |
| reduce-map-inc-1000 | 51,518 (49,069-51,838) | 49,479 (48,589-68,413) | -4% | 41,543 / 40,898 | 1.24 | 1.21 |
| filter-count-1000 | 41,888 (41,173-44,461) | 41,785 (40,240-69,497) | +0% | 28,670 / 28,665 | 1.46 | 1.46 |
| into-xform-1000 | 68,641 (66,751-71,707) | 68,126 (67,629-143,606) | -1% | 18,421 / 18,529 | 3.73 | 3.68 |
| mapv-1000 | 49,148 (47,569-50,631) | 48,231 (47,463-67,824) | -2% | 14,458 / 14,462 | 3.40 | 3.33 |
| lazy-walk-1000 | 44,205 (43,729-46,990) | 44,676 (43,174-51,806) | +1% | 39,856 / 40,796 | 1.11 | 1.10 |
| seq-walk-vec-1000 | 25,511 (24,746-26,536) | 25,244 (24,437-43,104) | -1% | 9,998 / 9,964 | 2.55 | 2.53 |
| reduce-range-1000 | 4,873 (4,855-5,102) | 5,109 (4,841-8,263) | +5% | 5,538 / 5,485 | 0.88 | 0.93 |
| reduce-kv-small | 50.4 (49.7-53.8) | 51.0 (49.9-99.5) | +1% | 38.0 / 38.1 | 1.33 | 1.34 |
| reduce-entries-small | 362.3 (354.9-439.0) | 357.4 (353.9-544.5) | -1% | 79.7 / 79.8 | 4.55 | 4.48 |
| str-nested-map | 4,105 (4,033-4,865) | 4,108 (4,079-6,729) | +0% | 2,256 / 2,276 | 1.82 | 1.80 |
| str-concat | 171.2 (168.8-172.2) | 165.7 (164.9-182.2) | -3% | 15.0 / 14.8 | 11.41 | 11.18 |
| eq-map-7 | 124.5 (121.7-126.1) | 122.4 (118.9-129.2) | -2% | 151.4 / 152.2 | 0.82 | 0.80 |
| eq-vec-10 | 40.1 (39.9-42.2) | 39.0 (38.8-39.8) | -3% | 81.8 / 82.4 | 0.49 | 0.47 |
| hash-fresh-vec-4 | 53.0 (50.9-65.0) | 51.7 (50.1-53.1) | -2% | 73.4 / 73.1 | 0.72 | 0.71 |
| hash-fresh-map-7 | 293.3 (260.9-323.1) | 268.9 (257.6-301.1) | -8% | 552.1 / 554.1 | 0.53 | 0.49 |
| protocol-on-record | 2.56 (2.53-2.70) | 2.54 (2.52-2.56) | -1% | 2.38 / 2.40 | 1.08 | 1.06 |
| record-kw-field | 8.12 (8.01-10.3) | 8.04 (7.88-8.12) | -1% | 4.24 / 4.23 | 1.92 | 1.90 |
| record-dot-field | 0.88 (0.88-0.88) | 0.88 (0.87-0.89) | +0% | 0.88 / 0.88 | 1.00 | 1.00 |
| record-assoc | 57.3 (55.7-58.5) | 55.9 (54.2-58.5) | -2% | 13.8 / 14.2 | 4.14 | 3.94 |
| multimethod | 116.6 (113.3-119.3) | 115.8 (114.8-118.6) | -1% | 48.0 / 47.4 | 2.43 | 2.44 |
| multi-arity-call | 2.02 (1.99-2.04) | 2.03 (1.95-2.56) | +0% | 2.02 / 2.01 | 1.00 | 1.01 |
| variadic-call | 13.8 (13.3-14.3) | 14.1 (13.6-19.6) | +2% | 14.5 / 14.6 | 0.95 | 0.97 |
| apply-vec | 192.9 (190.4-198.0) | 191.0 (188.1-236.3) | -1% | 93.8 / 94.2 | 2.06 | 2.03 |
| unknown-fn-call | 4.54 (4.50-4.62) | 4.53 (4.40-4.85) | +0% | 6.52 / 6.60 | 0.70 | 0.69 |
| closure-alloc-call | 9.31 (9.04-10.6) | 9.11 (9.10-13.1) | -2% | 8.46 / 8.46 | 1.10 | 1.08 |
| swap-inc | 4.91 (4.74-5.16) | 4.79 (4.68-5.09) | -2% | 5.70 / 5.69 | 0.86 | 0.84 |
| swap-assoc | 62.0 (60.9-63.9) | 62.0 (60.3-66.6) | +0% | 38.5 / 38.5 | 1.61 | 1.61 |
| merge-small | 2,788 (2,605-2,856) | 2,682 (2,610-3,632) | -4% | 894.7 / 912.7 | 3.12 | 2.94 |
| select-keys-3 | 397.0 (390.2-409.4) | 384.4 (379.2-421.8) | -3% | 253.0 / 255.5 | 1.57 | 1.50 |
| group-by-100 | 11,819 (11,437-12,393) | 12,518 (11,894-14,343) | +6% | 5,091 / 5,207 | 2.32 | 2.40 |
| sort-by-100 | 7,398 (7,152-7,442) | 7,270 (7,185-10,156) | -2% | 4,759 / 4,748 | 1.55 | 1.53 |
| transient-vec-100 | 4,934 (4,867-5,343) | 5,061 (4,845-6,099) | +3% | 1,163 / 1,172 | 4.24 | 4.32 |
| transient-map-100 | 10,756 (10,415-11,703) | 10,571 (10,345-13,736) | -2% | 4,132 / 4,306 | 2.60 | 2.46 |
| sorted-map-assoc-100 | 216.1 (203.0-225.5) | 213.1 (208.5-316.4) | -1% | 210.8 / 212.5 | 1.03 | 1.00 |
| str-sorted-map-20 | 32,021 (31,286-35,388) | 32,197 (31,089-60,988) | +1% | 20,578 / 21,108 | 1.56 | 1.53 |
| bit-and | 0.59 (0.56-0.60) | 0.59 (0.59-1.84) | +0% | 0.59 / 0.60 | 1.00 | 0.98 |
| aget-bit-and | 0.60 (0.59-0.62) | 0.60 (0.59-2.06) | +0% | 0.60 / 0.60 | 1.00 | 1.00 |

- **One operation more than 10% slower in this run**: `vec-conj-64`
  (+14%, after range 2,434-2,886). Two rechecks of a build of the five
  operations that moved over 5% (`--ops`, before and after alternated,
  load 4-12) gave it +9% and +5% (before 2,347 and 2,332, after 2,555 and
  2,448), and `vec-assoc-1024` +8% and `group-by-100` +7% and +6% in both,
  where `reduce-range-1000` and `hash-fresh-map-7` were within 2%. No
  unit on their paths changed (`conj`'s and `assoc`'s call-site entries and
  the vector slots are as before; the gate's slice differs by the stores'
  kinds, the table and the vectors' `-contains-key?` slot), and renaming
  that slot away in the built output changed nothing (`vec-conj-64`
  2,432-2,553 without it, 2,450-2,672 with it). The cause of the 5-9% is not
  found and is recorded as a loss (light-cljs-1056). Every other change is
  within 8% and inside the runs' spread; the gate keeps no `extend-type`,
  so none of its operations takes the lookup this pellet changes.
- **Off the gate**, in a scratch program with one `extend-type` (a record
  extended with `ICounted`), each operation 1 to 5 million times, median
  of 7 rounds, three processes each (load 4.5-16.6 before, 9.8-11.8
  after): `count` of a hash set 185 to 10.4 ns, of a queue 187 to 10.1,
  `conj` of a set 589 to 56, `get` of a set 138 to 12, `seq` of a set 193
  to 49, `contains?` of a vector 93 to 4.8, `reduce` of a 3-item vector 225
  to 166 and of a set 674 to 331, `meta` of a map 94 to 60, `empty` of a
  vector 194 to 124, `counted?` of a cons 154 to 44, `seqable?` of a set 91
  to 31, `count` of a two-item cons 401 to 204; `count` of a record
  unchanged (14.8). Without `extend-type`, `count` of a set 34 to 10.4 and
  of a queue 33 to 10.2, and `count` of a two-item cons 34 as before (40 at
  `0b473fdb`, before light-cljs-1015: the 65% loss light-cljs-1015 measured
  does not reproduce in this program).

**Workload 200k and the re-frame-shaped 200k app** (`compare.bb PROJECT
--runs 5 --runtime-only --runtime 1,40`, the project's light-cljs output
rebuilt with each binary just before its run, one before/after pair each),
wall time at 40 iterations, median (range), and light-cljs's steady state:

| project | light before | light after | shadow-cljs (before / after runs) | steady state, ms per iteration | load before / after |
| --- | ---: | ---: | ---: | ---: | ---: |
| workload 200k | 2.753 s (2.669-2.851) | 2.345 s (2.335-2.440) | 1.458 / 1.364 s | 59.3 to 49.4 (shadow-cljs 28.4 / 26.0) | 41-53 / 27-33 |
| re-frame-shaped | 26.129 s (25.326-26.516) | 21.562 s (20.822-22.536) | 12.528 / 12.441 s | 608.6 to 498.8 (shadow-cljs 283.1 / 281.7) | 7-17 / 6-7 |

The re-frame-shaped app keeps `extend-type`, so its operations on sets,
queues and the other runtime values took the lookup: its steady state is
18% lower, light/shadow 2.15 to 1.77, with shadow-cljs's unchanged. The
generated workload's 17% is part load (the before run was at a load of 41
to 53 and shadow-cljs's own time fell 6% between the runs): light/shadow
2.09 to 1.90.

**Advanced size** (the runtime slice, `--size-report`):
- the gate: 147,575 to 148,438 runtime bytes (961 to 968 units);
- workload 200k: 180,078 to 181,776 (1,254 to 1,264 units);
- the re-frame-shaped 200k app: 196,177 to 198,497 (1,378 to 1,392 units);
- the readable runtime of `none`, `prepare` and dev builds: about 6,000
  bytes larger (`add` at none 660,822 to 666,777); the size ratchet
  (tests/support/ratchet_fixtures.rs, noted there): `keyword-case` at
  advanced 5,738, past its 5,700. TodoMVC and re-frame simple were not
  ported in this run.

## 2026-11-17: an array map's keyword search inlined, and `contains?` of an array map (light-cljs-1041)

**What.** First re-measured on master (`d0c26527`): light-cljs-1040 had
already made the keyword search of an array map read the key's `IEquiv`
slot once, so `kw-small`, `get-small` and `get-small-miss` were under
shadow-cljs's (0.80, 0.75, 0.81 in the gate). What remained was
`contains-small` (1.14) and `record-kw-field` (1.87 in the gate, +30% from
light-cljs-1040). This pellet splits the search (`arrayIndex`, 381 to 246
bytes of bytecode; `keywordProgramEquiv`, 180 to 71) so that TurboFan inlines
`getSite`'s three-target `-lookup` call again, and gives an array map's
`-contains-key?` slot its own search where its `-lookup` slot is its own
(`arrayMapContains`). docs/value-layout.md has the notes.

Binaries in research/shadow-credibility/bin: `light-cljs-685f86dd` (master
`d0c26527`'s runtime) and `light-cljs-c67ae112` (this change on it), then,
after rebasing onto light-cljs-1046, `light-cljs-e0168930` (master) and
`light-cljs-e327b4ed` (this change; the final commit differs in notes and
tests). `light-cljs-fc1dd3fa` is the split without the `contains?` slot,
for the comparison below. Unreserved: one or two peer workers were active.
Raw results: research/output-speed/kw-index-1041.

**Gate** (`bb benchmarks/core-ops/run.bb`, every operation, 5 runs;
`685f86dd` load 3.72-7.59, `c67ae112` 5.57-11.08), ns per operation,
median (range), the operations of this pellet and every one that moved by
5% or more:

| op | light before | light after | change | shadow-cljs before / after | light/shadow before | light/shadow after |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| kw-small | 7.80 (7.76-8.06) | 7.99 (7.88-8.02) | +2% | 9.79 / 9.78 | 0.80 | 0.82 |
| get-small | 11.1 (11.0-11.4) | 11.4 (11.1-11.5) | +2% | 14.8 / 14.9 | 0.75 | 0.77 |
| get-small-miss | 15.6 (15.2-15.9) | 15.8 (15.4-15.9) | +1% | 19.2 / 19.2 | 0.81 | 0.82 |
| contains-small | 13.5 (13.5-13.9) | 9.77 (9.60-9.98) | -28% | 11.9 / 12.1 | 1.14 | 0.81 |
| kw-hash100 | 21.7 (21.1-25.2) | 22.0 (21.9-22.3) | +2% | 13.4 / 13.6 | 1.62 | 1.61 |
| destructure-map-3 | 62.7 (59.6-64.6) | 67.2 (64.6-68.6) | +7% | 49.2 / 49.1 | 1.28 | 1.37 |
| record-kw-field | 7.81 (7.70-8.08) | 5.94 (5.82-5.97) | -24% | 4.17 / 4.19 | 1.87 | 1.42 |
| get-hash100 | 19.3 (18.8-19.4) | 20.3 (19.9-20.5) | +5% | 12.1 / 12.1 | 1.60 | 1.68 |
| get-in-3 | 55.5 (54.0-56.0) | 62.8 (62.4-63.6) | +13% | 57.4 / 57.2 | 0.97 | 1.10 |
| hash-fresh-vec-4 | 49.3 (48.6-51.7) | 52.0 (49.6-55.8) | +6% | 71.5 / 70.3 | 0.69 | 0.74 |
| hash-fresh-map-7 | 238.0 (230.3-282.4) | 275.4 (251.5-290.7) | +16% | 547.9 / 540.3 | 0.43 | 0.51 |
| merge-small | 2,573 (2,531-2,799) | 2,689 (2,571-2,728) | +5% | 883 / 891 | 2.91 | 3.02 |

Every other operation moved by less than 5%, most by +1 to +3% (`bit-and`
+4%, which runs no map code): the after run's load was about 3 higher.

**The same three outputs in one process each, alternated** (`node
run-light.mjs 7 5 all 1` on the `:bench` outputs of `685f86dd`, `fc1dd3fa`
and `c67ae112`, 4 rounds, load 6.90-9.67), light-cljs ns per operation,
median of the 4:

| op | before | split only | after (split and `contains?` slot) |
| --- | ---: | ---: | ---: |
| contains-small | 14.05 | 10.80 | 9.85 |
| update-small | 47.78 | 48.00 | 45.27 |
| get-hash100 | 19.80 | 20.23 | 20.36 |
| kw-hash100 | 22.21 | 22.19 | 22.20 |
| get-in-3 | 56.46 | 56.55 | 63.00 |
| destructure-map-3 | 65.58 | 67.06 | 66.98 |
| hash-fresh-map-7 | 257.37 | 261.63 | 257.88 |
| record-kw-field | 8.07 | 5.98 | 5.97 |

- **`record-kw-field`** is the inlining: TurboFan's trace
  (`--trace-turbo-inlining`) of the benchmark's loop shows the `-lookup`
  call's three targets (the array map's, 17 bytes of bytecode with 636
  inlined in its own optimized code; the hash map's, 17; the record's, 25
  with 67) refused at 48 of 920 bytes used, as 1.2 times their 762 would
  pass the budget, and inlined now that the array map's is about 350
  (`c67ae112`; about 390 in the final commit, below).
- **`contains-small`** gains 3.2 ns from the split (the `contains?`
  path's `get` now inlines the search too) and 1 ns more from the slot.
- **The final commit** (`light-cljs-61e0bc5c`) also reads the value that
  `contains?` finds, and each key of the search once (Sol review), which
  makes `arrayIndex` 246 bytes: alternated twice with `685f86dd`'s output
  in one process each, `contains-small` 10.1 and 10.1 against 14.1 and
  11.9, `record-kw-field` 6.0 and 6.1 against 8.1 and 8.1, `get-in-3` 63.7
  and 64.3 against 57.1 and 56.6.
- **`get-in-3` +13% is the gate process's history of `get`, not its
  lookups.** Only the `contains?` slot moves it (the split alone leaves it
  at 56.5), and only after the gate's other operations: alone it is 61.7,
  60.2 and 59.9 ns (before, split, after), after `contains-small` 57.9, 57.3
  and 56.5, after the small-map operations through `update-small` 54.1,
  54.4 and 52.2. `get-in` calls `get`'s slot-first wrapper, which
  `contains?` of an array map no longer calls, so in the gate the wrapper is
  first optimized later (during `update-small`, which gains 5%), with other
  feedback; in `get-in-3` the array map's `-lookup` and `keywordOf` then run
  as frames of their own (a profile of the benchmark's frames: 3.9% and
  4.5%), where they were inlined. The inlining decisions inside `getIn`
  itself are the same.
- **`hash-fresh-map-7`, `hash-fresh-vec-4` and `get-hash100`** are within
  their spread in the alternated runs (`get-hash100` +3%, its hash map
  lookups unchanged); `destructure-map-3`, which spread from 57 to 71 in
  this pellet's runs before the change, is +2% there.

**The operations of this pellet, alone** (`--ops
kw-small,get-small,get-small-miss,contains-small,kw-hash100,destructure-map,record-kw-field`,
5 runs), on master `e0168930` (load 2.75-3.06 before, 2.95-3.32 after), ns
per operation, median (range):

| op | light before | light after | change | shadow-cljs before / after | light/shadow after |
| --- | ---: | ---: | ---: | ---: | ---: |
| kw-small | 7.90 (7.77-8.06) | 7.63 (7.62-8.08) | -3% | 9.41 / 9.60 | 0.79 |
| get-small | 12.2 (12.2-12.5) | 12.2 (12.1-12.4) | +0% | 14.3 / 14.3 | 0.85 |
| get-small-miss | 16.5 (16.5-17.0) | 16.4 (16.4-16.7) | -1% | 18.5 / 18.5 | 0.89 |
| contains-small | 13.5 (13.4-13.7) | 10.8 (10.7-11.1) | -20% | 11.6 / 11.6 | 0.93 |
| kw-hash100 | 19.9 (19.9-20.2) | 21.8 (21.5-21.9) | +10% | 12.9 / 12.7 | 1.72 |
| destructure-map-3 | 67.9 (67.4-68.8) | 62.8 (61.4-64.8) | -8% | 45.8 / 45.8 | 1.37 |
| record-kw-field | 5.75 (5.71-5.93) | 5.75 (5.73-5.83) | +0% | 4.01 / 4.09 | 1.41 |

On `d0c26527` the same selection gave the same picture (load 4.06-4.20 and
7.24): `contains-small` 13.9 to 11.2, `kw-hash100` 20.7 to 22.5 (+9%),
`destructure-map-3` 69.9 to 64.5. **`kw-hash100` +10% here is budget the
array map's search now takes:** in this process the `-lookup` call of the
benchmark's loop has seen array maps and hash maps; before, the array map's
target was too large to inline and the hash map's lookup took the budget
(`hashMapLookup`, `hashNodeFind`, `equalPair`, `keywordEquiv` inlined),
now the array map's search is inlined and `hashNodeFind` and the comparison
are calls. In the whole gate, where the call has more targets, it is
unchanged. light-cljs-1057 compares a keyword key by its spelling in hash
map lookups, as upstream's `key-test` does, which is most of the
remaining 1.6 times.

**Workload 200k** (`compare.bb workload-200k-seed42-1041 --runs 5
--runtime-only --runtime 1,10,40`, the project rebuilt with each binary just
before its run; `e0168930` load 2.41-2.92, `e327b4ed` 3.36-3.90), wall
time, median (range):

| iterations | light before | light after | shadow-cljs (before run) | shadow-cljs (after run) |
| --- | ---: | ---: | ---: | ---: |
| 1 | 0.365 s (0.363-0.377) | 0.373 s (0.366-0.385) | 0.304 s | 0.311 s |
| 10 | 0.844 s (0.835-0.847) | 0.842 s (0.829-0.911) | 0.567 s | 0.573 s |
| 40 | 2.119 s (2.090-2.157) | 2.109 s (2.088-2.125) | 1.215 s | 1.214 s |

Steady state, from 1 and 40 iterations: light-cljs 45.0 to 44.5 ms per
iteration (-1%), shadow-cljs 23.4 and 23.2; light/shadow 1.93 and 1.92.

**Advanced size** (tests/support/ratchet_fixtures.rs, noted there), against
master `e0168930`: about 290 bytes where a program keeps array maps
(`map-printed` 24,598 to 24,889, `keyword-lookups` 59,298 to 59,590), 350
for `constant-keys` and the features example; no ceiling moves. re-frame
simple and TodoMVC were not ported in this run.

## 2026-11-17: the side stores in the values' fields (light-cljs-1053)

Metadata, cached hashes, a public constructor's slots, a subvector's source and a vector's root token are each value's own keyed fields. The six WeakMap stores that held them are deleted (docs/value-layout.md, "What light-cljs-1053 landed").

- **Binaries:** before is master `d8a28b21` (`light-cljs-d8a28b21`). After is `light-cljs-1053-4fd7638c`. Both are V8 builds.
- **Load:** the machine was unreserved, with other workers building. The 1-minute load average before each run is given below.

**The gate** (`bb benchmarks/core-ops/run.bb`, all 64 operations, 5 runs per compiler, alternated; before load 3.3-7.9, after 3.6-7.9):
- **No operation is slower by more than 1%.** `map-literal-3` and `destructure-map-3` are +1%.
- **47 operations moved by less than 3%** either way.
- **17 are 4% or more faster.** The faster ones update maps.
  - Before, every update probed the metadata store twice, through `preserveMeta`, `meta` and `ownMeta`, and `hash` probed the hash store.
  - Now they read one field.

| op | light before (ns) | light after (ns) | change | light/shadow before | light/shadow after |
| --- | ---: | ---: | ---: | ---: | ---: |
| assoc-small-existing | 37.5 | 24.4 | -35% | 1.21 | 0.80 |
| assoc-small-new | 60.8 | 49.5 | -19% | 1.55 | 1.28 |
| dissoc-small | 64.3 | 49.4 | -23% | 1.13 | 0.88 |
| update-small | 44.4 | 29.2 | -34% | 1.01 | 0.68 |
| assoc-hash100 | 93.3 | 79.8 | -14% | 1.19 | 1.03 |
| dissoc-hash100 | 102.4 | 89.1 | -13% | 1.34 | 1.19 |
| update-hash100 | 108.2 | 94.2 | -13% | 1.13 | 1.00 |
| assoc-in-3 | 209.1 | 163.5 | -22% | 1.56 | 1.25 |
| update-in-3 | 233.6 | 188.9 | -19% | 1.68 | 1.38 |
| hash-fresh-map-7 | 257.8 | 137.4 | -47% | 0.48 | 0.26 |
| swap-assoc | 57.4 | 40.6 | -29% | 1.54 | 1.11 |
| select-keys-3 | 373.1 | 309.7 | -17% | 1.50 | 1.28 |
| group-by-100 | 12,548 | 10,218 | -19% | 2.48 | 2.07 |

**Workload 200k** (`compare.bb workload-200k-seed42-1053 --runs 5 --runtime-only --runtime 1,10,40`; the project was rebuilt with each binary just before its run; before load 3.75-5.4, after 4.25-7.0). Wall time, median (range):

| iterations | light before | light after | shadow-cljs (before run) | shadow-cljs (after run) |
| --- | ---: | ---: | ---: | ---: |
| 1 | 0.367 s (0.355-0.377) | 0.376 s (0.369-0.388) | 0.303 s | 0.310 s |
| 10 | 0.808 s (0.796-0.831) | 0.847 s (0.842-0.852) | 0.542 s | 0.574 s |
| 40 | 2.116 s (2.063-2.433) | 2.113 s (2.090-2.162) | 1.239 s | 1.216 s |

- **Steady state, from 1 and 40 iterations:** light-cljs went from 44.9 to 44.5 ms per iteration (-1%); shadow-cljs measured 24.0 and 23.2. Light/shadow was 1.87, then 1.92.
- **The 10-iteration row** moved by as much in shadow-cljs's runs as in light-cljs's (+6% and +5%), which points at the machine's load.

**Size.**
- **Advanced:** the fixture sizes are noted in tests/support/ratchet_fixtures.rs. The ceilings of `vector-only` and `closed-params` move down.
- **re-frame apps:** the whole bundles shrink after gzip by 60 bytes (simple), 161 (TodoMVC) and 112 (flow), against the 600 to 900 bytes light-cljs-1047 expected. benchmarks/re-frame-vs-shadow.md says why.

## 2026-11-17: vectors and maps reduce by their own slots (light-cljs-1055)

**What.** A vector, a subvector, an array map and a hash map have a
`-reduce` slot, and a vector, a subvector and the three maps a `-kv-reduce`
slot; `reduce` and `reduce-kv` call them before any protocol lookup
(`reduce.slots`, 96 bytes of bytecode, and `reduceKv.slots`, 100). Where
`extend-type` or `specify!` gave the value a `-seq`, the slot takes
`reduce`'s own path, as `reduce` walks a program's seq.
docs/value-layout.md, "What light-cljs-1055 landed".

Binaries in research/shadow-credibility/bin: before `light-cljs-d8a28b21`
(master), after `light-cljs-2de6cf66` (the full gate and the workloads) and
`light-cljs-07ee60f8` (this pellet's runtime before rebasing onto light-cljs-1053, after the review's
fixes to public constructors' maps and clones; the touched operations and
the sizes below). Unreserved: peer workers built and tested throughout.
Raw results: research/output-speed/reduce-slots-1055.

**Gate** (`bb benchmarks/core-ops/run.bb --runs 5`, every operation; before
load 4.04-8.02, after 2.90-9.27), ns per operation, median (range), the
reductions and every operation that moved by 5% or more:

| op | light before | light after | change | shadow-cljs before / after | light/shadow before | light/shadow after |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| reduce-vec-1000 | 5,262 (5,242-5,323) | 5,116 (4,916-5,153) | -3% | 2,511 / 2,506 | 2.10 | 2.04 |
| reduce-map-inc-1000 | 48,964 (48,263-49,590) | 46,957 (46,305-49,318) | -4% | 39,666 / 40,352 | 1.23 | 1.16 |
| reduce-kv-small | 50.4 (49.9-50.4) | 6.83 (6.69-7.04) | -86% | 38.0 / 37.9 | 1.33 | 0.18 |
| reduce-entries-small | 356.7 (356.2-362.4) | 90.0 (87.9-94.8) | -75% | 77.1 / 78.5 | 4.63 | 1.15 |
| mapv-1000 | 47,982 (47,779-48,446) | 46,518 (45,568-48,006) | -3% | 14,248 / 14,308 | 3.37 | 3.25 |
| into-xform-1000 | 65,935 (64,751-66,722) | 66,906 (63,140-67,668) | +1% | 18,375 / 18,185 | 3.59 | 3.68 |
| group-by-100 | 12,248 (12,214-12,445) | 11,551 (11,321-12,425) | -6% | 5,123 / 5,049 | 2.39 | 2.29 |
| filter-count-1000 | 41,231 (40,659-41,982) | 39,555 (39,032-41,633) | -4% | 28,376 / 28,791 | 1.45 | 1.37 |
| transient-vec-100 | 4,931 (4,843-4,985) | 4,611 (4,528-4,808) | -6% | 1,166 / 1,165 | 4.23 | 3.96 |

- **No operation is slower by more than 3%** (`vec-literal-4`, +3%).
- `reduce-kv-small` and `reduce-entries-small` reduce array maps of 7 keys:
  the `-kv-reduce` slot walks the map's pairs (`mapKvReduce`), which V8
  inlines with the reducing function, and the `-reduce` slot builds an
  entry per pair without the seq of all of them, which `reduce` built
  first.
- **The landed runtime** (`07ee60f8`, `--ops` of the eight reductions, 5
  runs, alternated with a master run; master load 9.66-11.31, after
  10.48-12.88): reduce-vec-1000 5,278 to 5,201 (-1%), reduce-map-inc-1000
  39,810 to 36,937 (-7%), filter-count-1000 34,693 to 34,548, into-xform-1000
  67,356 to 65,690 (-2%), mapv-1000 47,783 to 47,463, reduce-kv-small 49.9 to
  7.02, reduce-entries-small 369.0 to 94.5, group-by-100 13,055 to 13,343
  (+2%).
- **No call-site entry**: a reduction's cost is its steps, and the wrapper
  costs a few nanoseconds once per reduction.

**Off the gate**, in a scratch program with one `extend-type` (a record
extended with `ICounted`, as light-cljs-1046's), `07ee60f8` against
`d8a28b21`, 3 processes each, median of 7 rounds (load 6-11), ns per call:
`reduce` of a 3-item vector 132 to 20.5 (arity 2: 134 to 16), of a 40-item
vector 301 to 216, of a 5-item subvector 133 to 32, of an array map of 3
keys 263 to 46, of a hash map of 20 keys 723 to 251; `reduce-kv` of the
array map 62 to 16, of the hash map 164 to 116, of a 3-item vector 269 to
19, of a sorted map of 3 keys 71 to 23. Without `extend-type`: `reduce` of
the 3-item vector 72 to 20.5, of the array map 204 to 46, `reduce-kv` of the
array map 32 to 16. A map entry, which has no slot, 135 to 120. In the same
readable release output, the protocol lookups of these reductions, public
constructors' vectors and maps included, are 28 calls on master and none
after (`reduce_of_vectors_and_maps_takes_no_protocol_lookup`).

**Workload 200k and the re-frame-shaped 200k app** (`compare.bb PROJECT
--runs 5 --runtime-only --runtime 1,40`, the light-cljs output rebuilt with
each binary just before its runs), wall time at 40 iterations, median
(range), and light-cljs's steady state:

| project | light before | light after (`2de6cf66`) | shadow-cljs (before / after runs) | steady state, ms per iteration | load before / after |
| --- | ---: | ---: | ---: | ---: | ---: |
| workload 200k | 2.324 s (2.309-2.339) | 2.200 s (2.182-2.236) | 1.354 / 1.330 s | 49.2 to 46.1 (shadow-cljs 26.1 / 25.6) | 10.6-10.8 / 10.8-11.2 |
| re-frame-shaped | 22.064 s (21.719-23.058) | 21.344 s (21.300-21.516) | 12.237 / 12.190 s | 511.9 to 494.2 (shadow-cljs 276.2 / 275.3) | 9.3-12.3 / 10.0-10.9 |

Light/shadow steady state 1.89 to 1.80 for the workload, 1.85 to 1.80 for
the re-frame-shaped app.

**Size** (`07ee60f8` against `d8a28b21`):
- advanced runtime slices (`--size-report`): the gate 148,808 to 152,646
  bytes (972 to 987 units), workload 200k 182,146 to 185,986 (1,268 to
  1,283), the re-frame-shaped app 198,867 to 203,150 (1,396 to 1,413); most
  of it is the slots' keyed names, written out in full until
  light-cljs-1048 shortens them;
- the size ratchet (tests/support/ratchet_fixtures.rs, noted there): 1,158
  to 1,937 bytes more where a program reduces vectors or maps, no ceiling
  moves; the readable runtime about 10,100 bytes more;
- TodoMVC (the port's light-cljs output alone, gzip -9): 85,692 to 86,377
  bytes, +685 (+0.8% of that output, about 0.46% of the 147,758-byte bundle
  with React of light-cljs-1032's table). Re-frame simple and flow were not
  ported in this run.
- **Rebased onto light-cljs-1053** (master `51f38606`, its runtime built as
  `light-cljs-51f38606`): the public constructors' slots are a field the
  copies keep, which the array map's slot reads; the ratchet fixtures grow
  as before against it (1,158 to 1,908 bytes, noted in
  tests/support/ratchet_fixtures.rs), and the lookups of the reductions
  above are 28 on `51f38606` and none after. The timings above were not
  repeated on the rebased runtime.

## 2026-11-17: short keyed names at `:advanced` (light-cljs-1048)

At `:advanced` the output writer now writes every keyed name short: `$`, the key, and a code of one or two letters, the most used first (docs/value-layout.md, section 1, "Short names at `:advanced`"). The runtime's `protocolSlot` reads a table of the codes of the protocol slots and markers the text spells, once per key and arity. A name's length changes nothing V8 does with a dotted access, which section 1 measured, so no operation should move.

**Binaries:**
- `light-cljs-ceede268`: master before light-cljs-1046 landed.
- `light-cljs-1048-18bc7d4f`: this pellet on that master.

The pellet was then rebased onto `e0168930` (light-cljs-1046). Rebasing changed none of the renaming, so the timings were not repeated.

**Machine:** Node v26.12.1, 24 CPUs. Every run is unreserved: the load average was 6.3 to 12.0 for the gate and 7.7 to 7.8 for the workload.

**The gate** (`bb benchmarks/core-ops/run.bb`, all 64 operations, 5 runs per compiler, alternated, the after run with `--baseline` the before's):
- **No operation is more than 3% slower.** The largest are `get-hash100` (19.3 to 19.9 ns, +3%, whose runs' ranges overlap, 18.9 to 19.7 and 19.4 to 20.0), `bit-and` (+2%), and `kw-small`, `count-small` and 9 others (+1%).
- **`contains-small` is 13% faster** (13.7 to 11.9 ns). Its before run's range, 11.7 to 14.1, covers that, so it is noise.
- **Everything else** is within 2%.
- **Raw results:** benchmarks/core-ops/out/{before,after}.md, which are not committed.

**Workload 200k** (`compare.bb PROJECT --runs 10 --runtime-only --runtime 1,10,40`; the project's light-cljs output was rebuilt with each binary just before its run; shadow-cljs's output was the same for both). Wall time is the median, with the range in brackets:

| iterations | light before | light after | shadow-cljs (before / after runs) |
| ---: | ---: | ---: | ---: |
| 1 | 0.396 s (0.390-0.406) | 0.391 s (0.384-0.398) | 0.324 / 0.318 s |
| 10 | 0.942 s (0.934-0.955) | 0.950 s (0.937-0.967) | 0.596 / 0.595 s |
| 40 | 2.458 s (2.430-2.502) | 2.428 s (2.419-2.455) | 1.270 / 1.267 s |

- **Steady state:** 52.9 ms to 52.2 ms per iteration. shadow-cljs's is 24.3 ms in both runs, so light/shadow went from 2.18 to 2.15.
- **Advanced size:**
  - **workload 200k:** 3,553,784 to 3,542,213 bytes raw, and 895,481 to 895,084 after gzip -9.
  - **The re-frame apps** (benchmarks/re-frame-vs-shadow.md, "2026-11-17: short keyed names at `:advanced`"), whole bundles with React after gzip -9, against master `4fbc0e65` (with light-cljs-1053 and light-cljs-1055): simple -703, TodoMVC -883 and flow -1,001 bytes (`light-cljs-1048-74d8d955`, after the Sol review's fixes).
  - **The size ratchet** (tests/support/ratchet_fixtures.rs, noted there): 359 to 7,319 bytes smaller where a fixture keeps collections, and `record-protocol` and `three-records` 20 bytes larger. The review's fixes add 28 bytes to programs keeping `unkeyedJSON`; the timings above predate them.

## 2026-11-17: `conj` and `assoc` of a vector lose light-cljs-1046's lookup steps (light-cljs-1056)

**What.** After light-cljs-1046, `vec-conj-64`, `vec-assoc-1024` and
`group-by-100` ran 5-14% slower although no unit on their own paths had
changed. The cause: 1046 let `ownMethod`, the protocol lookup of a program
that implements protocols without `extend-type` or `specify!` (the gate's
records), take a runtime class's mask bit as a declaration. A vector's mask
declares `IMeta`, and `conj` and `assoc` of a vector read its metadata with
`meta` wherever `installMethods` is kept (`conjMeta`, the `-assoc` slot's
`preserveMeta`), so each went on to `keyMethod` (`keySlot`, then a
megamorphic read of the absent `-meta` slot) and `ownDeclared` (a
`reifyCells` WeakMap probe) to answer nothing, where the kind test had sent
it to `keyMarker`'s one read. `ownMethod` tests the kind before the mask bit
again, and a program that writes a core protocol method's member by name,
the one way to put its method in a runtime class's slot without
`extend-type` or `specify!`, keeps `extend-type`'s lookup at `:advanced`
(the Sol review found that the kind test alone made `meta` of such a vector
answer differently at `:advanced`, as before light-cljs-1046). The gate
writes no member by name, so its output is the same with or without that
second part. docs/value-layout.md, "What light-cljs-1056 found".

Evidence (readable builds, `bb benchmarks/core-ops/run.bb --pseudo-names
vec-conj-64`): `keyMethod`, `keySlot` and `ownDeclared` took 3.6%, 4.0% and
3.7% of the light-cljs samples on master `4fbc0e65` and none on
`685f86dd` (before 1046) or with the fix; the code of `conjSite`, the
vector's `-conj` and `-assoc` slots, `conjMeta`, `vectorConj` and
`VectorCell` was the same in all three. `node --trace-turbo-inlining`
shows `meta`, `ownMethod`, `keyMarker` and `ownMeta` inlined into
`preserveMeta` and the slots with the fix, so the loss was the extra work,
not a lost inlining.

Binaries in research/shadow-credibility/bin: `light-cljs-685f86dd` (before
light-cljs-1046), master `light-cljs-4fbc0e65` and the fix on it
`light-cljs-e6362ee1` (the full gate and workload 200k); after rebasing
onto light-cljs-1048, master `light-cljs-1048-74d8d955` (the runtime of
`1df0a071`) and the fix `light-cljs-b1296bca` (the recheck). Unreserved:
peer workers built and tested throughout. Raw results:
research/output-speed/vec-slowdown-1056.

**Gate** (`bb benchmarks/core-ops/run.bb --runs 5`, every operation; load
before a run 7.82-17.97 before, 11.89-14.99 after), ns per operation,
median (range), the touched operations and every one that moved 5% or
more:

| op | light before (`4fbc0e65`) | light after (`e6362ee1`) | change | shadow-cljs before / after | light/shadow before | light/shadow after |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| vec-conj-64 | 2,404 (2,392-2,447) | 2,051 (2,042-2,068) | -15% | 1,357 / 1,295 | 1.77 | 1.58 |
| vec-assoc-1024 | 71.3 (70.3-71.9) | 66.1 (65.6-66.4) | -7% | 73.8 / 74.4 | 0.97 | 0.89 |
| group-by-100 | 10,204 (10,142-10,284) | 9,868 (9,780-9,947) | -3% | 5,131 / 5,098 | 1.99 | 1.94 |
| kw-hash100 | 22.2 (22.0-22.2) | 20.9 (19.7-22.1) | -6% | 13.6 / 13.7 | 1.64 | 1.53 |

- **No operation is slower by more than 2%** (`map-literal-3`, +2%, ranges
  overlapping). `kw-hash100`'s ranges overlap too.

**Recheck against `685f86dd`** (the five operations of light-cljs-1046's
recheck, `--ops vec-conj-64,group-by-100,vec-assoc-1024,reduce-range-1000,hash-fresh-map-7
--runs 5`, the binaries alternated, two passes, load 4.64-7.70), light-cljs
ns per operation, medians of pass 1 / pass 2:

| op | `685f86dd` | master `74d8d955` | fix `b1296bca` | fix against `685f86dd` |
| --- | ---: | ---: | ---: | ---: |
| vec-conj-64 | 2,427 / 2,367 | 2,477 / 2,478 | 2,127 / 2,204 | -12% / -7% |
| vec-assoc-1024 | 66.2 / 64.4 | 70.5 / 69.4 | 64.3 / 65.5 | -3% / +2% |
| group-by-100 | 11,217 / 10,700 | 9,981 / 9,820 | 9,315 / 9,445 | -17% / -12% |

- All three are within 2% of `685f86dd` or faster. `group-by-100` had
  already recovered on master: light-cljs-1055's `-reduce` slots took
  more from it than the lookup added. On `4fbc0e65`, before rebasing, the
  same recheck gave `vec-conj-64` 2,555 / 2,498 against `685f86dd`'s
  2,286 / 2,324, and the fix 2,160 / 2,171 against 2,446 / 2,309.

**Workload 200k** (`compare.bb PROJECT --runs 5 --runtime-only --runtime
1,40`, the light-cljs output rebuilt with each binary just before its runs,
two passes in opposite orders), light-cljs's steady state in ms per
iteration, with shadow-cljs's in the same run: before 42.2 (22.7) and 44.5
(23.7), after 46.8 (27.2) and 45.8 (24.7); light/shadow 1.86 and 1.87
before, 1.72 and 1.86 after. Unchanged within the machine's noise (load
5.5-12.2; shadow-cljs's own steady state moved 20% between runs).

**Size.** The gate's advanced output 162,000 to 162,020 bytes (42,654 to
42,658 after gzip -9). The size ratchet (tests/support/ratchet_fixtures.rs,
noted there): 20 bytes more where a program keeps `ownMethod`, no ceiling
moves.

**What is left.** `conj` and `assoc` of a vector still read the vector's
metadata through `meta`'s lookup (the mask, kind and marker reads, about a
tenth of `vec-conj-64`'s samples with the fix) in every program that
implements a protocol, though only `extend-type`, `specify!` or a method
written by name could give a vector its own `IMeta` (light-cljs-1060).

**Review.** The Sol review's one finding (a blocker, fixed): the kind test
alone made `meta` of a vector whose `-meta` member a program wrote by name
answer the vector's own metadata at `:advanced` and the written method at
the other levels. Such a program now keeps `extend-type`'s lookup
(`light-cljs-4ec8334a`, whose gate output is byte for byte
`light-cljs-b1296bca`'s).

## 2026-11-17: a hash map's keyword keys compared by their spelling (light-cljs-1057)

**What.** A hash map's trie compares a keyword key with a leaf's by
identity, then by kind and `fqn` (`hashKeyTest`, upstream's `key-test`),
where `equalPair`'s wrapper called the key's `-equiv` slot through a call
V8 does not inline; a program's `IEquiv` method on keywords still decides
(`keywordProgramEquiv`, read once per comparison), and any other key
compares by `equalPair` as before. The walk (`hashNodeFind`) tells a
collision node by its missing `slots` and an entry by its missing `kind`,
and its loop tests no node, which is most of the gain. `assoc`
(`hashNodeAssoc`) compares keys the same way.
docs/value-layout.md, "What light-cljs-1057 landed".

Binaries in research/shadow-credibility/bin: before `light-cljs-4fbc0e65`
(master), after `light-cljs-51855d30` (this change on it, with an identity
test before the key test, which the Pants build's suite later refused for
an occurrence of NaN); then, rebased onto light-cljs-1048, before
`light-cljs-1df0a071` (master) and after `light-cljs-7a2b2a5e` (the landed
runtime; "The landed runtime" below). Unreserved: peer workers built and
tested throughout. Raw results: research/output-speed/kw-hash-1057.

**The operations of this pellet, alone** (`--ops
kw-small,get-small,get-hash100,kw-hash100,assoc-hash100,dissoc-hash100,update-hash100,destructure-map-3,record-kw-field`,
5 runs; load 7.37-9.18 before, 9.98-12.19 after), ns per operation, median
(range):

| op | light before | light after | change | shadow-cljs before / after | light/shadow before | light/shadow after |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| kw-small | 8.00 (7.91-8.05) | 7.92 (7.81-8.06) | -1% | 9.82 / 9.80 | 0.81 | 0.81 |
| get-small | 12.7 (12.6-12.8) | 12.6 (12.6-12.7) | +0% | 14.9 / 14.9 | 0.85 | 0.85 |
| get-hash100 | 20.9 (20.9-21.1) | 11.4 (11.4-11.7) | -45% | 12.0 / 12.1 | 1.74 | 0.94 |
| kw-hash100 | 25.2 (25.0-25.5) | 16.1 (15.8-16.4) | -36% | 13.4 / 13.5 | 1.87 | 1.19 |
| assoc-hash100 | 83.0 (81.5-84.8) | 80.5 (80.1-85.0) | -3% | 79.6 / 78.7 | 1.04 | 1.02 |
| dissoc-hash100 | 95.9 (95.0-97.1) | 93.8 (92.6-95.2) | -2% | 75.8 / 74.7 | 1.27 | 1.26 |
| update-hash100 | 93.5 (92.7-95.2) | 87.0 (86.0-89.6) | -7% | 97.0 / 96.5 | 0.96 | 0.90 |
| destructure-map-3 | 64.0 (63.2-65.9) | 68.3 (67.5-69.4) | +7% | 49.3 / 49.4 | 1.30 | 1.38 |
| record-kw-field | 5.98 (5.89-5.99) | 5.97 (5.87-6.00) | +0% | 6.39 / 6.32 | 0.94 | 0.94 |

- **`destructure-map-3` +7% here is TurboFan's budget, not the lookups.**
  Its loop makes three `getSite` calls of an array map, whose `-lookup`
  call has also seen hash maps in this process. `--trace-turbo-inlining`
  of the readable build: before, `getSite`'s own optimized code had 601
  bytes of bytecode inlined (both `-lookup` targets), too much to inline
  into the loop, which called it three times; after, it has 161 (the hash
  map's target, with the key test inlined, is about 100 bytes larger, and
  the two no longer fit together), so the loop inlines `getSite` at all
  three sites, the first site's two targets take 800 of the loop's 920
  bytes, and the other two `-lookup` calls stay calls. The same readable
  builds give 46.7 ns alone before and after, 68 before and after behind
  the hash map operations alone, and 64 against 70 only behind the
  small-map operations too (this selection). In the full gate below,
  whose process has more history, it is 4% faster.

**Gate** (`bb benchmarks/core-ops/run.bb`, every operation, 5 runs; before
load 7.07-14.87, after 3.40-9.38), ns per operation, median (range), the
operations of this pellet and every one that moved by 5% or more:

| op | light before | light after | change | shadow-cljs before / after | light/shadow before | light/shadow after |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| kw-small | 8.02 (7.83-8.06) | 7.90 (7.78-8.01) | -1% | 9.92 / 9.83 | 0.81 | 0.80 |
| get-small | 11.4 (11.1-11.4) | 11.3 (11.0-11.4) | -1% | 15.0 / 14.9 | 0.76 | 0.76 |
| get-hash100 | 20.0 (18.1-20.3) | 11.4 (11.2-11.5) | -43% | 12.2 / 12.1 | 1.63 | 0.94 |
| kw-hash100 | 22.2 (19.7-22.3) | 15.8 (15.8-16.1) | -29% | 14.0 / 13.5 | 1.59 | 1.17 |
| assoc-hash100 | 81.9 (77.3-82.9) | 80.6 (79.7-82.2) | -2% | 79.8 / 76.9 | 1.03 | 1.05 |
| dissoc-hash100 | 91.9 (86.8-92.1) | 88.6 (87.7-89.4) | -4% | 77.5 / 76.2 | 1.19 | 1.16 |
| update-hash100 | 96.7 (91.6-97.0) | 88.4 (88.0-90.6) | -9% | 97.5 / 92.7 | 0.99 | 0.95 |
| destructure-map-3 | 64.2 (64.0-65.9) | 62.0 (60.3-63.9) | -4% | 50.2 / 48.7 | 1.28 | 1.27 |
| record-kw-field | 5.97 (5.93-5.98) | 5.82 (5.78-5.96) | -3% | 4.24 / 4.19 | 1.41 | 1.39 |
| map-literal-3 | 21.3 (19.5-21.8) | 20.3 (19.4-20.6) | -5% | 10.3 / 9.78 | 2.06 | 2.08 |
| reduce-map-inc-1000 | 49,281 (46,269-49,876) | 45,376 (44,861-46,407) | -8% | 40,601 / 39,411 | 1.21 | 1.15 |
| filter-count-1000 | 41,516 (38,882-41,856) | 37,625 (37,522-38,973) | -9% | 28,841 / 28,105 | 1.44 | 1.34 |
| str-sorted-map-20 | 32,979 (31,910-48,390) | 31,462 (30,402-32,047) | -5% | 20,788 / 20,203 | 1.59 | 1.56 |

- **No operation is slower by more than 1%** (`select-keys-3`, +1%).
- `get-hash100` (the map's own keys) and `kw-hash100` (`(:k42 m)`, the
  constant keyword against keys `keyword` built) both gain the walk's 8 to
  9 ns; `kw-hash100` also loses the `-equiv` slot's call. `update-hash100`
  and `dissoc-hash100` find the key first.
- `reduce-map-inc-1000`, `filter-count-1000`, `map-literal-3` and
  `str-sorted-map-20` take no hash map; they moved with the load, which was
  about 4 lower in the after run (shadow-cljs's own times moved by 2 to 5%).

**Workload 200k** (`compare.bb workload-200k-seed42-1057 --runs 5
--runtime-only --runtime 1,10,40`, the project rebuilt with each binary just
before its run; before load 5.82-6.44, after 5.04-6.16), wall time, median
(range):

| iterations | light before | light after | shadow-cljs (before run) | shadow-cljs (after run) |
| --- | ---: | ---: | ---: | ---: |
| 1 | 0.368 s (0.363-0.387) | 0.365 s (0.360-0.391) | 0.309 s | 0.308 s |
| 10 | 0.890 s (0.819-1.008) | 0.833 s (0.812-0.846) | 0.614 s | 0.566 s |
| 40 | 2.069 s (2.034-2.160) | 2.054 s (2.035-2.358) | 1.217 s | 1.196 s |

Steady state, from 1 and 40 iterations: light-cljs 43.6 to 43.3 ms per
iteration (-1%, within the spread), shadow-cljs 23.3 and 22.8; light/shadow
1.87 and 1.90. The workload's maps are mostly array maps of a few keys.

**Size** (advanced): workload 200k 3,559,758 to 3,560,043 bytes raw and
896,479 to 896,551 after gzip -9 (+72, under 0.01%); the size ratchet
(tests/support/ratchet_fixtures.rs, noted there), rebased onto master
`1df0a071`: 267 bytes where a program keeps hash maps (`constant-keys`,
`features`, `features-direct`), for `keywordProgramEquiv`, which the key
test now keeps; the ceilings of `constant-keys` and `features-direct` move
to the next 1,000. The timings above are on the runtime before
light-cljs-1048's short keyed names, which change no access V8 makes.

**After the Sol review's fix** (`keywordProgramEquiv` reads the key's slot
without the protocol lookup, 70 to 79 bytes of bytecode), the gate's
readable builds of master `1df0a071` (light-cljs-1048's short names
included) and of the final runtime, one process each, alternated twice
(load 5-7), ns per operation: `kw-hash100` 25.0 and 25.3 to 16.0 and 16.5,
`get-hash100` 20.8 and 20.8 to 11.4 and 11.3, `kw-small` 7.90 and 7.93 to
7.95 and 7.95, `get-small` 12.6 and 12.5 to 12.6 and 12.6,
`contains-small` 11.4 and 11.3 to 11.3 and 11.4, `record-kw-field` 5.97 and
5.95 to 5.93 and 6.04: the same as before the fix.

**The landed runtime** (`light-cljs-7a2b2a5e`: the key test tests
identity on the keyword path only, so that any other key compares by
`equalPair` alone, which the Pants build's suite required, and reads a
slot written by name, the Sol review's fix), against master `1df0a071`
(with light-cljs-1048), the operations of this pellet alone, 5 runs (load
5.34-6.38 before, 9.56-10.48 after), ns per operation, median (range):

| op | light before | light after | change | shadow-cljs before / after | light/shadow before | light/shadow after |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| kw-small | 7.94 (7.64-7.96) | 8.01 (7.78-8.03) | +1% | 9.79 / 9.92 | 0.81 | 0.81 |
| get-small | 12.5 (12.1-12.6) | 12.7 (12.4-12.8) | +2% | 14.9 / 15.0 | 0.84 | 0.85 |
| get-hash100 | 20.4 (20.0-20.7) | 12.9 (12.5-12.9) | -37% | 12.1 / 12.2 | 1.69 | 1.05 |
| kw-hash100 | 24.7 (24.2-25.3) | 15.8 (14.4-16.8) | -36% | 13.3 / 13.6 | 1.86 | 1.17 |
| assoc-hash100 | 81.1 (78.6-83.5) | 85.3 (82.2-87.1) | +5% | 79.8 / 81.1 | 1.02 | 1.05 |
| dissoc-hash100 | 93.7 (91.5-95.3) | 94.3 (93.7-98.2) | +1% | 74.3 / 77.0 | 1.26 | 1.23 |
| update-hash100 | 93.0 (90.1-93.7) | 93.3 (92.0-95.8) | +0% | 94.7 / 98.2 | 0.98 | 0.95 |
| destructure-map-3 | 63.5 (61.9-64.5) | 69.9 (69.0-70.5) | +10% | 48.3 / 49.9 | 1.31 | 1.40 |
| record-kw-field | 5.94 (5.72-5.95) | 6.02 (6.00-6.04) | +1% | 6.33 / 6.44 | 0.94 | 0.93 |

`get-hash100` keeps 7.5 of the 9 ns it gained with the identity test
first, `kw-hash100` all of it; `destructure-map-3` is `getSite`'s budget as
above.

**The landed runtime's gate** (every operation, 5 runs; `1df0a071` load
6.82-18.04, `7a2b2a5e` 8.91-18.26), the operations of this pellet and every
one that moved by 5% or more:

| op | light before | light after | change | shadow-cljs before / after | light/shadow before | light/shadow after |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| kw-small | 8.05 (7.94-8.26) | 7.99 (7.88-8.03) | -1% | 9.88 / 9.91 | 0.81 | 0.81 |
| get-small | 11.4 (11.3-11.6) | 11.4 (11.2-11.4) | +0% | 15.1 / 15.0 | 0.76 | 0.76 |
| get-hash100 | 20.2 (18.0-20.6) | 12.7 (12.5-12.8) | -37% | 12.1 / 12.0 | 1.67 | 1.06 |
| kw-hash100 | 22.2 (19.8-24.9) | 15.5 (14.3-16.5) | -30% | 13.8 / 13.8 | 1.61 | 1.13 |
| assoc-hash100 | 82.8 (79.9-85.2) | 83.5 (82.1-95.2) | +1% | 78.9 / 80.0 | 1.05 | 1.04 |
| dissoc-hash100 | 93.0 (91.0-93.7) | 89.6 (87.8-110.7) | -4% | 76.7 / 77.7 | 1.21 | 1.15 |
| update-hash100 | 97.9 (94.8-101.1) | 93.1 (91.8-124.0) | -5% | 96.2 / 99.1 | 1.02 | 0.94 |
| destructure-map-3 | 64.5 (61.6-66.4) | 65.0 (61.6-68.2) | +1% | 49.9 / 49.8 | 1.29 | 1.31 |
| record-kw-field | 5.97 (5.95-5.98) | 6.01 (6.00-6.13) | +1% | 4.23 / 4.23 | 1.41 | 1.42 |
| reduce-map-inc-1000 | 49,894 (46,489-50,896) | 47,174 (46,006-58,752) | -5% | 40,494 / 40,169 | 1.23 | 1.17 |
| filter-count-1000 | 42,098 (39,859-43,795) | 39,407 (38,457-41,141) | -6% | 28,330 / 28,917 | 1.49 | 1.36 |

No operation is slower by more than 2% (`vec-peek`, `transient-vec-100`,
`into-xform-1000`); `reduce-map-inc-1000` and `filter-count-1000` take no
hash map, as in the first gate. Workload 200k was not re-timed on the
landed runtime: its maps are array maps, and the first runtime left it
unchanged.

## 2026-11-17: declarations where they are defined (light-cljs-1051)

At `:advanced` a single file now declares each top-level global where it defines it, `var gq=...`, rather than in a `let` list up front (docs/advanced-optimization.md, Phase 3, "Declarations where they are defined"). Nothing in the runtime changes. A hoisted `var` is a context slot like the `let` it replaces, so no operation should move.

**Binaries:**
- `light-cljs-c49a646a`: master with light-cljs-1056 and light-cljs-1057.
- `light-cljs-1051-d05aa74d`: this pellet on it.

A first version also wrote the runtime slice's units in the program's own function (`light-cljs-1051-bc8abfdf` against `light-cljs-1df0a071`); it was left out (benchmarks/re-frame-vs-shadow.md, "declarations where they are defined"), and its gate run below explains `kw-hash100`.

**Machine:** Node v26.12.1, 24 CPUs. Every run is unreserved: the load average was 4.0 to 7.4 for the gate, 12.2 to 12.7 for master's workload runs and 5.2 to 8.0 for this pellet's.

**The gate** (`bb benchmarks/core-ops/run.bb`, all 64 operations, 5 runs per compiler, alternated, the after run with `--baseline` the before's):
- **57 of 64 operations are within 2%.**
- **`kw-hash100` reads 11% slower** (14.3 to 15.8 ns), which is variance of master's own: three runs of the whole program in one process each gave master 16.2, 14.4 and 16.5 ns and this pellet 14.5, 14.5 and 14.4. Alone, three runs each gave 14.1 to 14.3 against 14.2 to 14.4, and after `get-hash100` 13.9 to 14.3 against 14.2 to 14.3.
  - The first version, with the runtime's units in the program's function, made `kw-hash100` 18% slower right after `get-hash100` (20.9 to 24.7 ns in three runs each, equal alone), from the units alone; it was one reason to leave them out.
- **The rest:** `protocol-on-record` 6% faster; `assoc-small-new`, `eq-map-7` and `variadic-call` 3% slower, `vec-nth-1024` and `reduce-range-1000` 3% faster.
- **Raw results:** benchmarks/core-ops/out/{before2,after2}.md, which are not committed.

**Workload 200k** (`compare.bb PROJECT --runs 10 --runtime-only --runtime 1,10,40`; the project's light-cljs output was rebuilt with each binary just before its run; shadow-cljs's output was the same for both). Medians, with the range in brackets:

| iterations | light before | light after | shadow-cljs (before / after runs) |
| ---: | ---: | ---: | ---: |
| 1, first output | 0.400 s (0.387-0.424) | 0.359 s (0.357-0.365) | 0.334 / 0.301 s |
| 1, wall | 0.406 s (0.392-0.430) | 0.364 s (0.362-0.369) | 0.338 / 0.305 s |
| 10, wall | 0.909 s (0.891-0.917) | 0.832 s (0.804-0.849) | 0.627 / 0.569 s |
| 40, wall | 2.161 s (2.104-2.244) | 2.068 s (1.991-2.222) | 1.282 / 1.205 s |

- **Steady state:** 45.0 to 43.7 ms per iteration; shadow-cljs's 24.2 and 23.1 in the same runs, so light/shadow went from 1.86 to 1.89. The before runs met a load of 12, the after runs 5 to 8: shadow-cljs's own output ran 6 to 10% faster in the after runs, and so did startup, so most of light-cljs's gain is the machine. Nothing is slower.
- **Startup** (first output at one iteration): light/shadow 1.20 before and 1.19 after.

**Advanced size:**
- **workload 200k:** 3,547,313 to 3,529,299 bytes raw, and 896,335 to 883,581 after gzip -9 (-1.42%).
- **The re-frame apps** (benchmarks/re-frame-vs-shadow.md), whole bundles with React after gzip -9: simple -649, TodoMVC -1,085 and flow -4,013 bytes.
- **The size ratchet** (tests/support/ratchet_fixtures.rs, noted there): up to 62 bytes smaller raw where a program defines several globals; no ceiling crosses its rounding.

## 2026-11-17: `conj` and `assoc` of a vector copy its metadata without the lookup (light-cljs-1060)

**What.** `conj` and `assoc` of a vector read the vector's metadata with
`meta` (`conjMeta`, the `-assoc` slots' `preserveMeta`) in every program that
implements a protocol, so each took `meta`'s lookup (`ownMethod`: the mask,
kind and marker reads) before the vector's own field. Without `extend-type`
or `specify!`, and with no core method written by name (which keeps
`extend-type`'s machinery, light-cljs-1056), nothing but the field can
answer it, so `conjMeta` now tests `protocolHooks.extend` instead of
`protocolHooks.builtin` and copies the field otherwise, as upstream's
`-conj` and `-assoc` copy `meta`. The `-assoc` slots of a vector, a
subvector and a map entry and a set's `-conj` slot call it too. Where
`extend-type`, `specify!` or a member written by name is kept, the result
holds what `meta` answers, as before, at every level. docs/value-layout.md,
"What light-cljs-1060 landed".

Evidence (readable builds, `bb benchmarks/core-ops/run.bb --pseudo-names
vec-conj-64`): `conjMeta`, `ownMethod`, `keyMarker`, `preserveMeta`, `meta`
and `ownMeta` took 14.8%, 6.0%, 3.4%, 2.0%, 1.8% and 0.9% of the light-cljs
samples on master `1ffb4d00` (3,404 ns per operation under the profiler);
with the change only `ownMeta` (1.4%) is left, and `vectorConj` holds 69% of
the samples (1,533 ns). `node --trace-turbo-inlining` shows `conjMeta`,
`ownMeta`, `makeVector` and `vectorConj` inlined into the benchmark's loop.

Binaries in research/shadow-credibility/bin: master `light-cljs-1ffb4d00`
and the change on it `light-cljs-eb39513f` (first pass); after rebasing onto
light-cljs-1057, master `light-cljs-c49a646a` and the change
`light-cljs-f2bb68c6` (second pass, recheck and workload; its runtime's
comment was shortened afterwards, which no advanced output keeps).
Unreserved: peer workers built and tested throughout. Raw results:
research/output-speed/conj-meta-1060.

**Gate** (`bb benchmarks/core-ops/run.bb --runs 5`, every operation, two
passes in opposite orders: pass 1 before then after, load 4.26-18.43 and
3.44-4.85; pass 2 after then before, load 3.42-4.48 and 4.00-7.78), ns per
operation, median (range), the touched operations and every one that moved
by 5% or more in both passes:

| op | pass | light before | light after | change | shadow-cljs before / after | light/shadow before | light/shadow after |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| vec-conj-64 | 1 | 2,012 (1,973-2,051) | 1,322 (1,297-1,337) | -34% | 1,320 / 1,289 | 1.52 | 1.03 |
| vec-conj-64 | 2 | 2,036 (1,992-2,098) | 1,313 (1,297-1,318) | -36% | 1,294 / 1,273 | 1.57 | 1.03 |
| vec-assoc-1024 | 1 | 63.4 (62.9-64.6) | 53.9 (52.4-53.9) | -15% | 72.1 / 73.4 | 0.88 | 0.73 |
| vec-assoc-1024 | 2 | 65.3 (64.9-70.4) | 53.5 (52.4-53.6) | -18% | 73.5 / 72.5 | 0.89 | 0.74 |
| group-by-100 | 1 | 9,451 (9,371-9,915) | 8,793 (8,590-8,837) | -7% | 5,033 / 4,976 | 1.88 | 1.77 |
| group-by-100 | 2 | 9,764 (9,676-9,942) | 8,434 (8,330-8,598) | -14% | 5,060 / 5,015 | 1.93 | 1.68 |
| transient-vec-100 | 1 | 4,602 (4,510-4,724) | 4,745 (4,619-4,861) | +3% | 1,157 / 1,144 | 3.98 | 4.15 |
| transient-vec-100 | 2 | 4,818 (4,606-4,979) | 4,643 (4,582-4,828) | -4% | 1,147 / 1,144 | 4.20 | 4.06 |
| swap-assoc | 1 | 38.9 (38.4-41.1) | 45.3 (44.1-45.8) | +16% | 37.4 / 36.6 | 1.04 | 1.24 |
| swap-assoc | 2 | 40.6 (39.5-40.9) | 47.0 (46.7-47.5) | +16% | 37.0 / 37.9 | 1.10 | 1.24 |
| select-keys-3 | 1 | 306.3 (299.0-321.8) | 337.0 (323.4-341.3) | +10% | 244.0 / 241.5 | 1.26 | 1.40 |
| select-keys-3 | 2 | 320.0 (310.8-325.4) | 344.6 (335.4-348.3) | +8% | 250.4 / 245.3 | 1.28 | 1.40 |

- `transient-vec-100` builds its vector with `conj!` and `persistent!`,
  which take no metadata; it is unchanged within the passes' ranges.
- **`swap-assoc` and `select-keys-3` are slower in the whole gate's process
  only.** Their path (`assoc` of an array map: `arrayMapAssoc`,
  `preserveMeta`, `meta`, `ownMethod`; `select-keys` also `meta` of the
  map) is the same code before and after. What changed is that vectors no
  longer reach `preserveMeta`, `meta` and `ownMethod`, so V8 optimizes those
  shared functions on the feedback the gate's earlier map, seq and printing
  operations leave. Run without the whole gate before them, both are faster
  with the change: in the alternated recheck below `swap-assoc` 39.6 to 37.3
  and 38.9 to 37.2 ns, `select-keys-3` 299.8 to 292.5 and 300.0 to 295.7.
  Outside `run.bb`, six processes of the whole gate's order, alternated
  (load 10-11), gave `swap-assoc` 45.6 before and 48.0 after (medians, +5%)
  and `select-keys-3` 339.8 and 350.1 (+3%): the order-dependent loss is
  real but smaller than the two gate passes show. It is recorded as found.
- `multi-arity-call` (+10% in pass 1, -1% in pass 2), `merge-small` (+7%,
  +1%), `hash-fresh-map-7` (+6%, +2%), `str-sorted-map-20` (+6%, -1%) and
  `str-nested-map` (+5%, -1%) moved by 5% or more in one pass only, when
  the loop baselines `bit-and` and `aget-bit-and` moved +4% and +5% (0% and
  -3% in pass 2); every other operation moved by less than 5% in both.

**Recheck** (`--ops vec-conj-64,vec-assoc-1024,group-by-100,transient-vec-100,multi-arity-call,swap-assoc,merge-small,select-keys-3
--runs 5`, `c49a646a` and `f2bb68c6` alternated, two passes in opposite
orders, load 2.87-4.12), light-cljs ns per operation, medians of pass 1 /
pass 2: `vec-conj-64` 2,119 / 2,094 to 1,422 / 1,427 (-33%, -32%),
`vec-assoc-1024` 64.8 / 64.3 to 52.7 / 52.5 (-19%, -18%), `group-by-100`
10,155 / 10,240 to 8,415 / 8,498 (-17%, -17%), `transient-vec-100` 4,663 /
4,628 to 4,549 / 4,620 (-2%, -0%), `multi-arity-call` 2.01 / 1.97 to 1.94 /
1.97, `merge-small` 2,375 / 2,327 to 2,287 / 2,310.

**Workload 200k** (`compare.bb PROJECT --runs 5 --runtime-only --runtime
1,40`, the light-cljs output rebuilt with each binary just before its runs,
the shadow-cljs output the project's, two passes in opposite orders: master,
change, change, master; load 8.88-11.66), light-cljs's steady state in ms
per iteration, with shadow-cljs's in the same run: master 40.5 (22.4) and
46.2 (25.5), change 46.2 (25.9) and 45.5 (25.2); light/shadow 1.80 and 1.81
for master, 1.79 and 1.80 with the change. Unchanged: the steady state
followed the machine's load, as shadow-cljs's did.

**Size.** The gate's advanced output 162,287 to 162,288 bytes (42,744 to
42,746 after gzip -9). The size ratchet (tests/support/ratchet_fixtures.rs,
noted there): `constant-keys` 102 bytes larger, which keeps `conjMeta` for
its vectors' `assoc` and its sets' `conj`; `mapv-count` 1 byte smaller; no
other fixture moves, no ceiling moves.

## 2026-11-18: a program protocol's slots named by the text (light-cljs-1058)

light-cljs-1058 names a program protocol's slots in `:advanced` output where the runtime needs them, in each `protocolMethod` (its last argument, `names`) and in each method entry that does not name its dispatcher, so that the slot-code table `protocolSlot` reads holds only the core protocols' slots and the markers (docs/value-layout.md, section 1, "A program protocol's slots from the text"). It changes what the runtime does as a protocol function, a type, a `reify` or an extension is made, not any call: the slots and the lookups are the same properties as before.

**Binaries:** master `light-cljs-c49a646a` and `light-cljs-1058-7e6d4545`, this pellet on it, both V8 builds. The pellet was then rebased onto light-cljs-1051's declarations (`light-cljs-1058-d4fa8e7d`), which changes no access V8 makes; the timings are on the first pair.

**The gate** (`bb benchmarks/core-ops/run.bb`, all 64 operations, 5 runs per compiler alternated; before load 10.39-15.50, after 13.24-16.01, unreserved): no operation is more than 5% slower. Three moved by more than 3%:

| op | light before | light after | change | light range after | rerun before | rerun after | rerun change |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `sorted-map-assoc-100` | 182.5 | 194.9 | +7% | 174.7-196.5 | 168.2 | 166.9 | -1% |
| `vec-nth-1024` | 4.92 | 5.12 | +4% | 5.07-5.17 | 4.88 | 5.02 | +3% |
| `kw-hash100` | 16.2 | 15.6 | -4% | 14.4-16.5 | 14.6 | 14.5 | -1% |

The rerun (`--ops sorted-map-assoc-100,vec-nth-1024,kw-hash100`, load 13.46 and 13.99-14.25) puts `sorted-map-assoc-100` at -1%: the first after run's range, 174.7-196.5, held the before run's. None of the three reaches a protocol function, a type's methods or `protocolSlot` per call. The operations on protocols and records did not move: `protocol-on-record` 2.55 to 2.54 ns, `record-kw-field` 5.98 to 6.00, `record-assoc` 54.2 to 54.2, `multimethod` 118.4 to 118.5.

**Workload 200k** (`compare.bb workload-200k-seed42-1058 --runs 5 --runtime-only --runtime 1,10,40`, the project rebuilt with each binary just before its run), wall time, median (range). A first pair of runs met a load of 24 during the before binary's 40 iterations (3.94 to 8.98 s) and was run again; these are the second (before load 13.6-15.8, after 13.6-14.0):

| iterations | light before | light after | shadow-cljs (before run) | shadow-cljs (after run) |
| --- | ---: | ---: | ---: | ---: |
| 1 | 0.405 s (0.397-0.412) | 0.391 s (0.387-0.392) | 0.330 s | 0.330 s |
| 10 | 0.897 s (0.894-0.903) | 0.879 s (0.862-0.900) | 0.619 s | 0.606 s |
| 40 | 2.207 s (2.193-2.296) | 2.146 s (2.136-2.173) | 1.342 s | 1.294 s |

Steady state, from 1 and 40 iterations: light-cljs 46.2 to 45.0 ms per iteration (-3%, as shadow-cljs's own 26.0 to 24.7 moved with the load), light/shadow 1.78 and 1.82.

**Size** (advanced): workload 200k 3,547,313 to 3,523,501 bytes raw (-23,812, -0.7%), mostly its method entries naming their slots in place of their keys. The re-frame apps' bundles shrink 61 to 122 bytes after gzip (benchmarks/re-frame-vs-shadow.md, "2026-11-18: a program protocol's slots named by the text"); the size ratchet (tests/support/ratchet_fixtures.rs, noted there) grows 53 to 128 bytes where a small program keeps the protocol machinery.

## 2026-11-17: keyword constants without their hash (light-cljs-1050)

At `:advanced` a keyword constant no longer carries the hash the compiler computed (light-cljs-1011): it is `p(null,"todos")` again, and its first `hash` computes the hash and caches it in the keyword's field, as below `:advanced` and as every keyword built from data already did. The runtime gains no code. The other option, hashing each constant as it is built at load, would add the hashing units to every program that builds a keyword and make startup hash every constant (1,214 in flow); with no cost measured below for hashing on first use, it was not built.

**Binaries:**
- `light-cljs-8c21c1ae`: master with light-cljs-1051.
- `light-cljs-1050-827965c3`: this pellet on it.

They were measured before light-cljs-1060 and light-cljs-1058 landed; this pellet changes none of their code.

**Machine:** Node v26.12.1, 24 CPUs. Every run is unreserved: the load average was 10.1 to 25.0 for the gate, 6.3 to 13.4 for the workload runs and 11.7 to 12.9 for TodoMVC.

**The gate** (`bb benchmarks/core-ops/run.bb`, all 64 operations, 5 runs per compiler, alternated, the after run with `--baseline` the before's):
- **No operation is more than 2% slower.** `assoc-small-new` and `merge-small` read 2% slower; `vec-nth-1024` reads 5% faster, and `update-hash100`, `assoc-in-3` and `vec-literal-4` 3% faster.
- **The keyword operations:** `kw-small` 8.02 to 8.02 ns, `get-small` 11.4 to 11.4, `get-small-miss` 15.8 to 15.9.
- **Hash-map lookups of keyword keys:** `get-hash100` 12.7 to 12.7 ns, `kw-hash100` 14.4 to 14.5 (its literal `:k42` is now hashed on its first lookup, in the warm-up). After the first `hash` a constant's field holds its hash as before, so a lookup reads the same field.
- **Raw results:** benchmarks/core-ops/out/{before,after}.md, which are not committed.

**Workload 200k** (`compare.bb PROJECT --runs 10 --runtime-only --runtime 1,10,40`; the project's light-cljs output was rebuilt with each binary just before its run; shadow-cljs's output was the same for all). Two pairs, the second in the reverse order. Medians, with the range in brackets:

| iterations | light before | light after | light before (2) | light after (2) |
| ---: | ---: | ---: | ---: | ---: |
| 1, first output | 0.541 s (0.425-0.960) | 0.404 s (0.394-0.409) | 0.408 s (0.399-0.413) | 0.403 s (0.396-0.418) |
| 10, wall | 0.905 s (0.899-0.915) | 0.920 s (0.901-0.970) | 0.884 s (0.866-0.918) | 0.913 s (0.894-0.940) |
| 40, wall | 2.285 s (2.256-2.320) | 2.280 s (2.251-2.334) | 2.122 s (2.073-2.183) | 2.258 s (2.232-2.286) |
| light/shadow steady state | 1.87 | 1.81 | 1.85 | 1.84 |

- **Steady state:** compare.bb gave 44.5 and 43.8 ms per iteration before, 47.9 and 47.4 after, but shadow-cljs's unchanged output moved as much in the same runs (23.8 and 23.6 against 26.4 and 25.7), so light/shadow went from 1.87 and 1.85 to 1.81 and 1.84. The first pair's before median at one iteration also met a load spike (range to 0.960 s), which lowers its steady state.
- **The two outputs alternated directly**, to settle it (`node run-light.mjs 42 N` of each output in turn, 12 rounds, one-iteration and 40-iteration runs, load 6.5 to 10.1): one iteration 0.441 s before and 0.442 after; 40 iterations 2.374 and 2.358; steady state 49.6 and 49.1 ms per iteration. Nothing is slower.
- **Startup** (first output at one iteration): 0.408 to 0.403 s in the second pair, 0.441 to 0.442 alternated; no slower beyond the range.

**TodoMVC** (`bb benchmarks/re-frame.bb runtime todomvc --runs 10`, headless Chromium, each binary's run alternated with shadow-cljs's), medians with ranges:
- **Page load to interactive:** 48.1 ms (46.6-54.2) before, 47.3 (45.8-67.8) after; script time 25.95 to 25.20 ms.
- **Adding 1,000 todos:** 3,411 ms (3,387-3,483) before, 3,452 (3,377-3,532) after; light/shadow 3.48 and 3.50, within the range.
- **Toggle all, edit, clear completed:** 64.9, 63.1 and 26.7 ms before; 64.1, 61.6 and 25.6 after.

**Advanced size:**
- **workload 200k:** 3,529,299 to 3,460,653 bytes raw, and 883,581 to 853,838 after gzip -9 (-3.4%).
- **The re-frame apps** (benchmarks/re-frame-vs-shadow.md), whole bundles with React after gzip -9: simple -1,067, TodoMVC -1,522 and flow -9,507 bytes.
- **The size ratchet** (tests/support/ratchet_fixtures.rs, noted there): 9 to 161 bytes smaller where a program has keyword constants; `constant-lookups`' ceiling moves down to 5,700.

## 2026-11-17: sets and indexed seqs reduce by their own slots (light-cljs-1059)

**What.** A hash or array set and an indexed seq (rest arguments, a short
vector's seq, a set's or a queue's seq, a public constructor's) have a
`-reduce` slot, which `reduce.slots` calls before any protocol lookup. A
set's walks its members' keys in traversal order, where `reduce` walked its
iterator over a fresh indexed seq of them; an indexed seq's walks its array
from its offset. Where `extend-type` or `specify!` gave the value an `-seq`
or an `-iterator`, or the indexed seqs an `-first` or `-next`, the slot
takes `reduce`'s own path. The other seqs, a map entry, a sorted map or set
and a queue were measured with slots and left without them (below).
docs/value-layout.md, "What light-cljs-1059 landed".

Binaries in research/shadow-credibility/bin: before `light-cljs-a33f6fef`
(master), after `light-cljs-6fe3af28` (this pellet's runtime on it, after
the review's two fixes) and `light-cljs-2071580c` (before them: a first
gate pass and workload run, within 2% of the second but `merge-small`,
-7%, and `update-in-3`, -5%). Unreserved: peer workers built and tested
throughout. Raw results: research/output-speed/reduce-slots-1059.

**Gate** (`bb benchmarks/core-ops/run.bb --runs 5`, every operation; before
load 11.95-14.75, after 6.35-13.27), ns per operation, median (range), the
operations the pellet names and every one that moved by 3% or more:

| op | light before | light after | change | shadow-cljs before / after | light/shadow before | light/shadow after |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| reduce-map-inc-1000 | 46,649 (45,677-46,943) | 46,059 (45,573-46,839) | -1% | 40,124 / 41,581 | 1.16 | 1.11 |
| reduce-range-1000 | 4,947 (4,918-5,171) | 5,120 (4,859-5,250) | +3% | 5,472 / 5,462 | 0.90 | 0.94 |
| lazy-walk-1000 | 43,691 (42,032-44,043) | 43,520 (43,324-43,821) | +0% | 39,399 / 38,768 | 1.11 | 1.12 |
| group-by-100 | 8,744 (8,674-8,872) | 8,621 (8,565-8,686) | -1% | 5,138 / 5,045 | 1.70 | 1.71 |
| merge-small | 2,682 (2,643-2,728) | 2,375 (2,346-2,437) | -11% | 898.4 / 893.7 | 2.98 | 2.66 |
| str-nested-map | 4,052 (3,994-4,243) | 3,951 (3,918-3,964) | -3% | 2,246 / 2,215 | 1.80 | 1.78 |

- **No operation is slower by more than 3%.** `reduce-range-1000`
  reduces an integer range, which has no slot: its path is unchanged and
  its ranges overlap (+0% in the first pass).
- `merge-small` merges maps given as rest arguments: `merge` reduces its
  indexed seq of them, which now takes the slot.
- The gate holds no `extend-type`, so `reduce.protocol`'s lookup there is
  `ownMethod`'s few reads: the slots pay where the path itself is slow (a
  set's iterator) or long against its walk (a short indexed seq).

**Off the gate**, in a scratch program with one `extend-type` (a record
extended with `ICounted`), `6fe3af28` against `a33f6fef`, 3 processes each,
alternated, median of 7 rounds (load 10.1-10.4), ns per call: `reduce` of
a 3-member set 251 to 20.7, of a 20-member set 896 to 176, of `(seq [1 2
3])` 165 to 24.6, of 3 rest arguments 183 to 43.3; `into []` of a 3-member
set 452 to 173. Without slots, as before: a 3-item list 110 to 100, a cons
143 to 123, a realized 3-item lazy seq 244 to 228, a range of 10 91 to 82,
a repeat of 3 57 to 56, a sorted set of 3 626 to 596, a sorted map of 3
245 to 244, a queue of 3 522 to 503, a map entry 132 to 135. A
program without `extend-type` (a record implementing a program protocol)
measured the same within 5% in an earlier pass. In the readable release
output of `reduce_of_vectors_and_maps_takes_no_protocol_lookup`, the sets'
and indexed seqs' reductions take 14 lookups on master `f8f1ed3f` and none
after; the lists', lazy seqs' and sorted sets' still take theirs.

**Measured and left out.** One slot shared by the other seq classes (it took
`reduce`'s own path, as the classes' walks need) gave, with one
`extend-type`, a 3-item list 111 to 104 ns, a cons 145 to 129, a lazy seq
249 to 235, an indexed seq 167 to 168, a range of 10 90 to 98, a repeat of
3 57 to 67, a chunked seq of 39 405 to 410, a chunked cons of 40 2,220 to
2,290 (`light-cljs-6a18e208` against a runtime with those slots, load
15-18), for 76 bytes of TodoMVC's gzip; the gains match those of the lists
above, which have no slot. A sorted map's tree walk took its 5 lookups to
none for 78 bytes of TodoMVC's gzip, a queue's walk its 13; both are
rarely reduced.

**Workload 200k** (`compare.bb PROJECT --runs 5 --runtime-only --runtime
1,40`, the light-cljs output rebuilt with each binary just before its
runs), wall time at 40 iterations, median (range), and light-cljs's steady
state:

| project | light before | light after (`6fe3af28`) | shadow-cljs (before / after runs) | steady state, ms per iteration | load before / after |
| --- | ---: | ---: | ---: | ---: | ---: |
| workload 200k | 2.152 s (2.143-2.182) | 2.123 s (2.073-2.134) | 1.303 / 1.336 s | 45.3 to 44.9 (shadow-cljs 25.2 / 26.2) | 12.8-13.4 / 8.2-8.6 |

The first pass (`2071580c`) gave 44.8 ms per iteration.

**Size** (`bb benchmarks/re-frame.bb size simple todomvc flow --light-only`,
ports from `shadow-port-tool-983`), the whole bundle with React, gzip -9:

| App | before (`a33f6fef`) | after (`6fe3af28`) | change |
| --- | ---: | ---: | ---: |
| re-frame simple | 113,870 | 114,103 | +233 (+0.20%) |
| TodoMVC | 145,424 | 145,681 | +257 (+0.18%) |
| flow | 333,157 | 333,406 | +249 (+0.07%) |

- light-cljs's output as written, gzip -9: simple 67,442 to 67,684 (+242),
  TodoMVC 97,202 to 97,440 (+238), flow 280,245 to 280,480 (+235).
- Every app keeps sets and indexed seqs, so each pays the two slots, the
  walk they share (`itemsReduce`) and the keyed names of the six slot reads
  their guards make. The review's two guards (an indexed seq's `-first` and
  `-next`, a set's one-arity `IReduce`) cost 40 to 90 of these bytes.
- The size ratchet (tests/support/ratchet_fixtures.rs, noted there): 653 to
  1,197 bytes more at advanced where a program reduces a set or an indexed
  seq, nine advanced ceilings moved; the readable runtime 4,108 bytes more,
  mostly comments.

## 2026-11-17: `=` against a keyword constant calls a method written by name (light-cljs-1061)

In a program that writes `IEquiv`'s method by name (`(set! (.-cljs$core$IEquiv$_equiv$arity$2 k) f)`, or raw text spelling it), `:advanced` no longer lowers `=` and `not=` (a keyword constant's identity and spelling test, a primitive's `===`), folds `=` of constants or compares a keyword set literal's members by spelling, so `=` calls that method as the other levels and shadow-cljs's release output do (docs/value-layout.md, "What light-cljs-1061 landed"). `extend-type` of `IEquiv` on `Keyword` and `specify!` already turned the lowering off. A program without such a write compiles as before.

**Binaries:** master `light-cljs-1061-base-08593058` and `light-cljs-1061-4c7719f0`, this pellet on it, both V8 builds. The landed commit adds the same guard to a second folding path and is rebased onto light-cljs-1059; against master there (`light-cljs-dbf91703`), its binary (`light-cljs-1061-8c860cb2`) also compiles the gate and workload 200k byte-identical.

**Output:** the gate's source (161,978 bytes) and workload 200k (3,436,843 bytes, SHA-256 `d5042b74...` from both) compile byte-identical at `:advanced` with both binaries, so their timings cannot move; the runs below are the check. Neither the gate, workload 200k nor the re-frame apps' sources write the member.

**The gate's keyword operations** (`bb benchmarks/core-ops/run.bb --ops kw-small,get-small,get-small-miss,contains-small,get-hash100,kw-hash100,destructure-map-3,record-kw-field,multimethod`, 5 runs per compiler alternated, two before and after pairs; load 7.3 to 9.1, unreserved), light-cljs ns per operation, medians:

| op | before | after | change | before (2) | after (2) | change (2) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `kw-small` | 8.00 | 8.00 | +0% | 8.01 | 8.02 | +0% |
| `get-small` | 12.7 | 12.7 | +0% | 12.7 | 12.7 | +0% |
| `get-small-miss` | 17.1 | 17.1 | +0% | 17.2 | 17.2 | +0% |
| `contains-small` | 11.5 | 11.4 | +0% | 11.4 | 11.4 | +0% |
| `get-hash100` | 12.8 | 12.8 | +0% | 12.8 | 12.8 | +0% |
| `kw-hash100` | 16.3 | 14.4 | -12% | 16.3 | 16.3 | +0% |
| `destructure-map-3` | 64.4 | 65.8 | +2% | 64.1 | 63.8 | +0% |
| `record-kw-field` | 6.00 | 5.98 | +0% | 6.01 | 6.02 | +0% |
| `multimethod` | 83.7 | 83.1 | -1% | 83.9 | 83.2 | -1% |

`kw-hash100` runs at about 14.4 or 16.3 ns from one process to the next (its ranges, 13.9-16.6, hold both), and `destructure-map-3`'s first after range, 57.4-70.2, holds the before median; the second pair, from the same bytes, has no operation slower.

**Workload 200k** (`compare.bb workload-200k-seed42-1061 --runs 5 --runtime-only --runtime 1,10,40`, the project's light-cljs output rebuilt with each binary just before its run; before load 6.7-7.5, after 5.8-6.4, unreserved), wall time, median (range):

| iterations | light before | light after | shadow-cljs (before run) | shadow-cljs (after run) |
| --- | ---: | ---: | ---: | ---: |
| 1 | 0.372 s (0.368-0.376) | 0.367 s (0.361-0.375) | 0.314 s | 0.301 s |
| 10 | 0.836 s (0.832-0.862) | 0.901 s (0.821-0.922) | 0.580 s | 0.584 s |
| 40 | 2.094 s (2.071-2.107) | 2.118 s (2.074-2.835) | 1.242 s | 1.260 s |

Steady state, from 1 and 40 iterations: light-cljs 44.2 to 44.9 ms per iteration, as shadow-cljs's own 23.8 to 24.6 moved in the same runs; light/shadow 1.86 and 1.83. The output was the same file both times.

**Size:** unchanged where no program writes the member (the outputs above are byte-identical; the size ratchet's fixtures write none).

## 2026-11-18: a runtime class only extended keeps its prototype alone (light-cljs-1052)

light-cljs-1052 changes only what the `:advanced` slice keeps: where a program extends a runtime class it never builds, the class's slots and the helpers only they reach leave the slice (docs/advanced-optimization.md, "Classes only extended"). The runtime's code and every kept unit's text are unchanged, so a program that keeps a class whole runs the same JavaScript.

**Binaries:** for the timings, master `light-cljs-a33f6fef` (master `08593058` since adds only a warning) and `light-cljs-1052-2afe713c`, this pellet on it; the gate's after run used `light-cljs-1052-cedb9a8c`, the same pellet before a fix that only places a chunked build's runtime units. Then, after the review's fixes and light-cljs-1059, master `light-cljs-dbf91703` and `light-cljs-1052-33fc0bab`. All are V8 builds.

**Output:** the core-ops gate source and workload 200k compile byte for byte the same at `:advanced` with master's binary and this pellet's, each pair: 161,978 and 3,436,843 bytes on `a33f6fef`, 162,634 and 3,438,039 on `dbf91703`. Neither extends a runtime class it does not build, so their timings cannot move; the runs below confirm it.

**The gate** (`bb benchmarks/core-ops/run.bb`, all 64 operations, 5 runs per compiler, alternated, the after run with `--baseline` the before's; load 11.87-19.97 before and 11.26-13.63 after, unreserved): one operation read more than 3% slower, `kw-hash100`, 14.4 to 16.3 ns (+13%), its after range 14.4-16.5 holding the before median. Rerun twice alternated (`--ops kw-hash100`, load 7.9-9.3) it read 14.5 to 14.3 and 14.5 to 14.4 ns (-1% both times): the identical JavaScript's two modes, which light-cljs-1058's gate also met (14.4-16.5). `assoc-hash100` and `vec-nth-1024` read 3% faster.

**Workload 200k** (`compare.bb workload-200k-seed42-1052 --runs 5 --runtime-only --runtime 1,10,40`, the project rebuilt with each binary just before its run; load 7.5-8.1, unreserved), wall time, median (range):

| iterations | light before | light after | shadow-cljs (before run) | shadow-cljs (after run) |
| --- | ---: | ---: | ---: | ---: |
| 1 | 0.383 s (0.374-0.411) | 0.379 s (0.370-0.383) | 0.319 s | 0.314 s |
| 10 | 0.846 s (0.842-0.864) | 0.838 s (0.834-0.848) | 0.584 s | 0.586 s |
| 40 | 2.105 s (2.066-2.216) | 2.078 s (2.056-2.082) | 1.261 s | 1.227 s |

Steady state, from 1 and 40 iterations: light-cljs 44.2 to 43.6 ms per iteration, light/shadow 1.83 and 1.86 (shadow-cljs's unchanged output moved from 24.2 to 23.4). Nothing is slower.

**Size** (advanced):
- **The re-frame apps** (benchmarks/re-frame-vs-shadow.md), whole bundles with React after gzip -9: TodoMVC -337 bytes (145,681 to 145,344 on `dbf91703`); simple and flow unchanged.
- **The size ratchet** (tests/support/ratchet_fixtures.rs, noted there): no fixture extended a runtime class, so none moves; the new `extended-classes`, cljs.spec's `specize*` pattern, is 72,416 bytes against master's 109,065.
- **workload 200k:** unchanged.

## 2026-11-18: another copy's ex-info by its keyed kind (light-cljs-1044)

`ExceptionInfo`'s prototype holds kind 48, and `ex-data`, `ex-cause`, `Throwable->map` and the ex-info printer test it where they tested `instanceof`, so a copy of a build reads an ex-info the other copy threw as its own (docs/value-layout.md, "Two runtimes on one page"). `instance?` and `catch` of `ExceptionInfo` stay `instanceof`.

**Binaries:** master `light-cljs-a123bd44` and `light-cljs-10cb454b`, this pellet on it, both V8 builds (research/shadow-credibility/bin; the runs below are in research/output-speed/exinfo-kind-1044).

**The gate:** its source compiles byte-identical with both binaries (SHA-256 `32b50411...`, 162,634 bytes); it makes no ex-info, so no operation of it can move and none was rerun.

**Workload 200k** (`compare.bb PROJECT --runs 5 --runtime-only --runtime 1,10,40`, the project's light-cljs output rebuilt with each binary just before its run, two before and after pairs in turn; unreserved, load falling from 22.4 to 7.5 across the four). Its output grows 29 bytes, 3,438,039 to 3,438,068: the kind assignment, `isExceptionInfo`, and renamed locals; nothing in its loop makes an ex-info. Steady state from the median wall times at 1 and 40 iterations:

| run | load before | light-cljs | shadow-cljs | light/shadow |
| --- | --- | ---: | ---: | ---: |
| before 1 | 17.0-22.4 | 53.0 ms | 27.7 ms | 1.91 |
| after 1 | 10.9-14.5 | 44.8 ms | 25.6 ms | 1.75 |
| before 2 | 8.2-10.2 | 42.5 ms | 24.4 ms | 1.74 |
| after 2 | 6.5-7.5 | 43.9 ms | 26.1 ms | 1.68 |

Light/shadow in each run, which share its load, is 1.74-1.91 before and 1.68-1.75 after: no loss beyond the load's noise.

**Size** (`bb benchmarks/re-frame.bb size todomvc simple --light-only`, each binary):

| app | output as written, gzip -9 | whole bundle with React, raw | whole bundle, gzip -9 |
| --- | ---: | ---: | ---: |
| TodoMVC | 83,555 to 83,582 (+27) | 427,133 to 427,161 (+28) | 130,636 to 130,597 (-39) |
| simple | 67,357 to 67,372 (+15) | 371,359 to 371,387 (+28) | 113,760 to 113,824 (+64, +0.06%) |

- **The size ratchet** (tests/support/ratchet_fixtures.rs, noted there): advanced unchanged in every fixture, none of which makes an ex-info; the readable runtime grows 424 bytes, past fifteen fixtures' none or prepare ceilings, which move to the next 1,000.

## 2026-11-18: map key and value seqs step through slots (light-cljs-1064)

**What.** The map seqs (`PersistentArrayMapSeq`, `NodeSeq`, `ArrayNodeSeq`)
answer `first`, `rest`, `next`, `seq` and, the array map's, `count` from
slots, as upstream's types implement ISeq, INext, ISeqable and ICounted. The
key and value seqs' own slots (light-cljs-1030), which sat on a prototype
their type unit replaced, now reach them, and their steps call the slots of
their map seq and of the entry, where they called the protocols'
dispatchers. In a program with `extend-type` a walk of a map's keys or
values took 9 protocol lookups per item and takes none, nor a walk of a
map's seq (2 before): `walks_of_map_keys_and_values_take_no_protocol_lookup_per_item`.
A key seq's `-reduce` was measured and left out. docs/value-layout.md,
"What light-cljs-1064 landed".

Binaries in research/shadow-credibility/bin: before `light-cljs-9b0028f0-1064`
(master `9b0028f0`, built for this run), after `light-cljs-a45ad1e6` (this
pellet on it). Unreserved: peer workers built and tested throughout.

**Gate** (`bb benchmarks/core-ops/run.bb --runs 5`, every operation, the four
new key and value walks included; the after run with `--baseline` the
before's; load 6.07-21.12 before, 10.30-21.39 after), ns per operation,
median (range), the new operations and every one that moved by 3% or more:

| op | light before | light after | change | shadow-cljs after | light/shadow before | light/shadow after |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| keys-small | 2,529 | 348.0 (342.0-349.2) | -86% | 76.0 | 33.13 | 4.58 |
| vals-small | 2,532 | 384.2 (363.2-389.1) | -85% | 79.2 | 31.89 | 4.85 |
| keys-hash100 | 39,118 | 4,305 (3,812-4,380) | -89% | 5,041 | 7.68 | 0.85 |
| vals-hash100 | 39,232 | 4,358 (3,935-4,544) | -89% | 5,034 | 7.76 | 0.87 |
| lazy-walk-1000 | 49,984 | 45,460 (45,411-46,130) | -9% | 50,872 | 0.98 | 0.89 |
| seq-walk-vec-1000 | 27,329 | 26,442 (26,245-26,781) | -3% | 19,832 | 1.39 | 1.33 |
| reduce-range-1000 | 5,150 | 4,938 (4,789-4,965) | -4% | 5,467 | 0.94 | 0.90 |
| destructure-map-3 | 66.1 | 62.0 (48.8-66.2) | -6% | 49.8 | 1.33 | 1.24 |
| assoc-in-3 | 169.9 | 165.2 (162.4-168.2) | -3% | 134.5 | 1.25 | 1.23 |
| str-nested-map | 4,252 | 4,011 (3,963-4,023) | -6% | 2,359 | 1.80 | 1.70 |
| apply-vec | 248.7 | 200.1 (198.8-202.6) | -20% | 92.4 | 2.68 | 2.16 |
| merge-small | 2,599 | 2,286 (2,212-2,335) | -12% | 907.0 | 2.92 | 2.52 |
| select-keys-3 | 395.3 | 346.8 (342.1-351.5) | -12% | 254.8 | 1.57 | 1.36 |
| transient-map-100 | 10,927 | 10,631 (10,440-10,687) | -3% | 4,590 | 2.38 | 2.32 |
| str-sorted-map-20 | 32,834 | 30,763 (30,720-31,019) | -6% | 20,636 | 1.58 | 1.49 |

- **No operation is slower by more than 3%**: the slowest moves are
  `dissoc-hash100` and `into-xform-1000`, +2% each, inside their ranges.
- The gate holds no `extend-type`, so its key seqs called the dispatchers'
  shorter lookup (`ownMethod`) per step, and still 2,529 ns for 7 keys.
  `keys-hash100` and `vals-hash100` are now faster than shadow-cljs's;
  `keys-small` and `vals-small` stay 4.6 to 4.9 times, most of it the map's
  seq, which builds a frozen array of fresh entries for its 7 entries
  before the first step (`kindHooks.seqMap`, 15% of `--pseudo-names
  keys-small`'s profile with the first pass, and the garbage collector 10%),
  where upstream's builds an entry per `-first`.
- The walks go before `lazy-walk-1000` in the gate (benchmarks/core-ops/README.md),
  so the seq walks after them take their type feedback: with master the key
  seqs' steps reached the map seqs through the dispatchers and the
  operations, and now they call the map seqs' slots from their own getters,
  and no new class reaches the shared wrappers' slot calls from a key walk.
  `lazy-walk-1000` (-9%) and `seq-walk-vec-1000` (-3%) gained; timed alone
  with `--ops keys,vals,lazy-walk,seq-walk-vec` (3 runs, load 13.4-29.6,
  against `light-cljs-dbf91703` and `light-cljs-4f35ece1`, this pellet's
  first pass, whose map seqs had slot methods of their own) they read -13%
  and -20%.
- `apply-vec`, `merge-small` and `select-keys-3`, which walk indexed seqs
  after the key and value walks, gained 12 to 20%. Their code is the same
  but for the indexed seqs' first, seq and count methods, which the map seqs
  now share; the gain was not profiled and may be type feedback the key
  walks no longer leave, as `swap-assoc`'s moves were in light-cljs-1060.

**Off the gate**, a scratch program with one `extend-type` (`Atom` given
`ICounted`), `light-cljs-4f35ece1` against `light-cljs-dbf91703`, median of
7 rounds (load 9.6), ns per call: a `first` and `next` walk of a 7-key map's
values 3,983 to 480, of a 100-key map's 67,705 to 4,482; `reduce` of the
7-key map's values 4,403 to 586, of its keys counted 6,104 to 569, of the
100-key map's values 65,569 to 5,097; `into []` of its keys 5,798 to 1,356;
a walk of the 7-key map's seq 1,720 to 538.

**Measured and left out: a key seq's `-reduce`.** `reduce` of the 7-key
map's values (586 ns above) costs 106 ns more than the walk of its `first`
and `next` (480): `reduce`'s lookup and kind tests, which a slot would skip.
For the 100-key map the gap is 615 ns, 6 per item, `reduce`'s generic loop.
A slot needs guards for the key seq's own `-first`, `-next` and `-seq`, the
map seq's and the entry's, so the key seqs keep `reduce`'s path, as
light-cljs-1059 left the other seqs'.

**Workload 200k** (`compare.bb PROJECT --runs 5 --runtime-only --runtime
1,40`, project `workload-200k-seed42-1064`, the light-cljs output rebuilt
with each binary just before its runs), wall time at 40 iterations, median
(range), and light-cljs's steady state:

| project | light before | light after (`a45ad1e6`) | shadow-cljs (before / after runs) | steady state, ms per iteration | load before / after |
| --- | ---: | ---: | ---: | ---: | ---: |
| workload 200k | 2.202 s (2.171-2.292) | 2.154 s (2.121-2.284) | 1.390 / 1.342 s | 46.1 to 44.9 (shadow-cljs 26.9 / 25.7) | 13.1-13.6 / 13.7-14.4 |

Unchanged: light/shadow 1.72 before and 1.75 after, as shadow-cljs's own
output moved as much.

**Size** (`bb benchmarks/re-frame.bb size simple todomvc flow --light-only`,
ports from `shadow-port-tool-983`), the whole bundle with React, gzip -9:

| App | before (`9b0028f0`) | after (`a45ad1e6`) | change |
| --- | ---: | ---: | ---: |
| re-frame simple | 114,103 | 114,311 | +208 (+0.18%) |
| TodoMVC | 145,344 | 145,640 | +296 (+0.20%) |
| flow | 333,264 | 333,686 | +422 (+0.13%) |

- light-cljs's output as written, gzip -9: simple 67,684 to 67,909 (+225),
  TodoMVC 97,161 to 97,386 (+225), flow 280,486 to 280,735 (+249).
- The first pass, with map seq methods of their own, was 764 bytes larger
  raw than master in a program with an array map's seq; sharing the indexed
  seqs' first, seq and count methods made it 358.
- The size ratchet (tests/support/ratchet_fixtures.rs, noted there): 357 to
  361 bytes more at advanced where a program keeps a map seq, 576 with the
  key seqs, 797 for the features example, 24 with only indexed seqs; three
  advanced ceilings and the features shared chunk's moved; the readable
  runtime 3,643 bytes more, mostly comments.

**After the review** (`light-cljs-33fb62f5`): the key and value seqs read
their entries' seq's slots only where it has a keyed kind, and a sorted
map seq's list stack is peeked and pushed without lookups where a program
can extend lists (docs/value-layout.md), so sorted maps' keys and values
walk with no lookup per item too (2.25 before). Against master `5bb838cd`
(`light-cljs-5bb838cd-1064`, light-cljs-1044 since `9b0028f0`), the gate's
touched operations (`--ops keys,vals,lazy-walk,seq-walk-vec,sorted-map-assoc,str-sorted-map,apply-vec,merge-small,select-keys,str-nested-map`,
5 runs, load 5.43-11.39 before and 2.83-4.75 after):

| op | light before | light after | change | shadow-cljs after | light/shadow after |
| --- | ---: | ---: | ---: | ---: | ---: |
| keys-small | 2,083 | 304.0 (288.6-315.3) | -85% | 66.7 | 4.56 |
| vals-small | 2,232 | 300.4 (295.8-308.3) | -87% | 87.3 | 3.44 |
| keys-hash100 | 36,347 | 3,946 (3,768-4,099) | -89% | 5,307 | 0.74 |
| vals-hash100 | 36,967 | 4,129 (4,031-4,262) | -89% | 5,110 | 0.81 |
| lazy-walk-1000 | 47,530 | 42,587 (41,368-43,221) | -10% | 51,268 | 0.83 |
| seq-walk-vec-1000 | 27,136 | 20,673 (20,348-21,246) | -24% | 19,693 | 1.05 |
| str-nested-map | 4,232 | 3,855 (3,796-3,916) | -9% | 2,323 | 1.66 |
| apply-vec | 241.4 | 173.0 (172.2-181.3) | -28% | 70.3 | 2.46 |
| merge-small | 2,418 | 1,994 (1,926-2,084) | -18% | 763.8 | 2.61 |
| select-keys-3 | 362.2 | 294.5 (280.9-302.3) | -19% | 224.3 | 1.31 |
| sorted-map-assoc-100 | 160.3 | 157.4 (151.6-161.9) | -2% | 193.9 | 0.81 |
| str-sorted-map-20 | 32,524 | 29,378 (29,114-30,720) | -10% | 20,032 | 1.47 |

The re-frame apps against `5bb838cd`, whole bundle with React, gzip -9:
simple 114,102 to 114,343 (+241, +0.21%), TodoMVC 145,402 to 145,750
(+348, +0.24%), flow 333,310 to 333,886 (+576, +0.17%); light-cljs's
output as written 67,713 to 67,943, 97,183 to 97,491 and 280,529 to
280,827. The size ratchet, noted there: the same advanced growth as
above, the features example 1,064 bytes (its editor chunk, which keeps
the sorted maps' seqs, 499), the readable runtime 5,045.

## 2026-11-18: the slice's roots without the binding list (light-cljs-1062)

light-cljs-1062 writes a terse ES module's runtime units in a function around the program's own instead of a factory whose returned array the program destructures (docs/advanced-optimization.md, "The slice's roots without the binding list"). The units' text is unchanged; the program now reads a root from the enclosing function's scope rather than from a constant of its own, and the runtime's top-level bindings are that function's rather than the factory's. light-cljs-1051, which wrote the units in the program's own function, had measured `kw-hash100` 18% slower right after `get-hash100` in one process; that operation's two modes, about 14.4 and 16.3 ns, are noted in the sections above.

**Binaries:** master `light-cljs-9b0028f0` and `light-cljs-1062-879e09d0`, this pellet on it, both V8 builds. The gate's output and workload 200k are ES modules, so both change.

**The gate** (`bb benchmarks/core-ops/run.bb`, all 64 operations, 5 runs per compiler, alternated, the after run with `--baseline` the before's; load 2.63-18.24 before and 11.17-14.01 after, unreserved): no operation is more than 3% slower. The largest changes are `mapv-1000` +3% (48,262 to 49,524 ns, its after range 47,517-63,702 holding the before median), `str-concat`, `vec-literal-4`, `hash-fresh-vec-4` and `reduce-entries-small` +2%, and `map-literal-3` -6%, `reduce-kv-small`, `swap-inc` and `merge-small` -3%. `get-hash100` read 12.9 to 12.8 ns and `kw-hash100` 16.2 to 15.9.

**Workload 200k** (`compare.bb workload-200k-seed42-1062 --runs 5 --runtime-only --runtime 1,10,40`, the project rebuilt with each binary just before its run, two before and after pairs; load 10.7-12.4, unreserved), light-cljs, median (range):

| | before | after | before (2) | after (2) |
| --- | ---: | ---: | ---: | ---: |
| first output, 1 iteration | 0.391 s | 0.394 s | 0.381 s | 0.388 s |
| wall, 1 iteration | 0.397 s (0.390-0.411) | 0.400 s (0.389-0.408) | 0.386 s (0.383-0.394) | 0.394 s (0.390-0.394) |
| wall, 10 iterations | 0.866 s (0.854-0.897) | 0.867 s (0.862-0.895) | 0.864 s (0.853-0.871) | 0.874 s (0.862-0.913) |
| wall, 40 iterations | 2.123 s (2.104-2.128) | 2.093 s (2.066-2.149) | 2.117 s (2.076-2.125) | 2.119 s (2.105-2.136) |
| steady state, ms per iteration | 44.3 | 43.4 | 44.4 | 44.2 |
| light/shadow, steady state | 1.78 | 1.75 | 1.80 | 1.75 |

The steady state does not move. **Startup is about 1.5% slower**, a loss recorded as found: the first output reads 3 and 7 ms later in the two pairs above, and a check that ran the two outputs alternately, 30 Node processes each (`node run-light.mjs 42 1` against copies of both outputs, load 6.8), timed them at 371.0 ms (364.5-515.6) before and 376.9 ms (364.1-604.9) after (+1.6%). Timed inside the process (20 alternated each, load 6.4), loading the module, which parses it and runs the units and the program's definitions, takes 120.7 ms before and 122.9 after (+1.8%), and the iteration itself 199.0 and 200.0 ms. The cause is not isolated; one candidate is that the program now reads each root from the enclosing function's scope rather than from a constant it declares, so the parser resolves every such read one scope further out. Writing the inner function as `function(){...}` rather than an arrow measured the same (122.2 ms).

**Size** (advanced): workload 200k's output is 848,932 to 848,567 bytes after gzip -9 (-365) and 3,438,039 to 3,438,435 raw (+396); the re-frame bundles shrink by 747, 929 and 1,763 bytes after gzip (benchmarks/re-frame-vs-shadow.md, same date). The size ratchet's fixtures are JavaScript expressions, which keep the factory: none moves.

## 2026-11-18: transients take `conj!`, `assoc!` and `persistent!` from slots (light-cljs-1065)

**What.** The transient vectors, maps and sets have keyed `-conj!` and
`-persistent!` slots, and the vectors and maps `-assoc!`, as upstream's
TransientVector, TransientArrayMap, TransientHashMap and TransientHashSet
implement ITransientCollection and ITransientAssociative. `conj!`, `assoc!`
and `persistent!`, and so `into`, `mapv`, `filterv` and `vec` of a seq, call
them before any protocol lookup, and a map transient's `conj!` of a map
entry reads the entry's `-key` and `-val` slots. In a program with
protocol implementations each conjoined item took one lookup (the
`-conj!` or `-assoc!` dispatcher), `into {}` of pairs two and of map entries
four, and takes none: `into_and_transient_updates_take_no_protocol_lookup_per_item`.
`extend-type` and `specify!` keep master's answers at every level
(`transient_slots_keep_the_answers_of_extended_and_specified_transients`).
docs/value-layout.md, "What light-cljs-1065 landed". The gate gains two
operations of `into` without a transducer, `into-vec-keys-small` and
`into-set-100`, after the printing ones (benchmarks/core-ops/README.md).

Binaries in research/shadow-credibility/bin: before `light-cljs-a397e0c9`
(master), after `light-cljs-08a43178` (this pellet on it: a work-in-progress
commit whose compiler sources the landed commit keeps), both V8 builds. The
runs' records are in research/output-speed/transient-slots-1065.
Unreserved: peer workers built and tested throughout.

**Gate** (`bb benchmarks/core-ops/run.bb --runs 5`, every operation, the
after run with `--baseline` the before's; load 8.25-31.37 before and
2.77-10.10 after), ns per operation, median (range), the operations this
touches:

| op | light before | light after | change | shadow-cljs after | light/shadow before | light/shadow after |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| transient-vec-100 | 4,914 (4,512-7,414) | 1,190 (1,130-1,225) | -76% | 1,132 | 4.22 | 1.05 |
| transient-map-100 | 10,996 (10,018-21,737) | 5,724 (5,635-5,812) | -48% | 4,454 | 2.50 | 1.29 |
| into-xform-1000 | 68,152 (64,371-76,254) | 28,445 (27,218-28,681) | -58% | 18,214 | 3.69 | 1.56 |
| mapv-1000 | 47,906 (46,528-52,572) | 13,623 (13,234-13,795) | -72% | 14,025 | 3.31 | 0.97 |
| into-vec-keys-small | 1,233 (1,169-2,166) | 903.9 (872.2-920.4) | -27% | 315.6 | 3.72 | 2.86 |
| into-set-100 | 10,906 (10,161-20,544) | 7,046 (6,950-7,162) | -35% | 4,247 | 2.54 | 1.66 |
| group-by-100 | 8,739 (8,161-12,740) | 8,422 (8,123-8,989) | -4% | 4,940 | 1.71 | 1.70 |

- **No operation is slower by more than 3%**: the slowest move is
  `assoc-small-existing`, +2%, inside its range; `reduce-vec-1000` and
  `sort-by-100` +0%. Most others read 2 to 8% faster, the before run's
  higher load.
- **Two more passes** against that bias. The same command in the other
  order (after first, load 3.98-7.98, then before, 4.15-11.64): the touched
  operations as above, and `vals-hash100` +7%, `vals-small` +5% and
  `select-keys-3` +4%, each inside the other's range, the reverse of the
  first pass (-10%, -7%, -3%): the second binary run read faster both
  times. Then light-cljs alone, the two outputs alternated process by
  process, 8 each (a scratch loop over `run-light.mjs`, load 3.87-12.40):
  `vals-hash100` -3.9%, `vals-small` -0.9%, `select-keys-3` -0.9%, every
  untouched operation within ±2.8% but `kw-hash100`, +12% (14.4 to 16.2 ns,
  ranges 14.3-16.5 and 14.2-17.0). It reads 14.x or 16.x in every pass with
  either binary (16.4, 16.0 and 14.4 before; 15.6, 14.4 and 16.2 after):
  bimodal, not this change, which no keyword lookup reaches.
- The gate has records with protocols and no `extend-type`, so its
  transients' `conj!` and `assoc!` took the dispatchers' shorter lookup
  (`ownMethod`) per item. `mapv-1000` is now as fast as shadow-cljs's and
  `transient-vec-100` within 5%. `group-by` builds persistent maps, as
  before, and takes no transient.
- `into-vec-keys-small` stays 2.9 times shadow-cljs's: per call, `into`
  still takes lookups for the target's `IEditableCollection` and
  `-as-transient`, its metadata and the key seq's reduction (with one
  `extend-type`, 7 per call of 3 keys, where it took 8 and one per key;
  `persistent!` is a slot now), and the 7-key map's seq builds its entries
  first (light-cljs-1064). Those are the persistent collections' slots, left
  for the pellets reshaping the core operations.

**Off the gate**, a scratch program with one `extend-type` (`Atom` given
`ICounted`), the same two binaries, 5 alternated runs of the median of 7
rounds (load 6.55-10.31), ns per call: `into []` of a 7-key map's keys
1,093 to 772 (the 1,356 light-cljs-1064 measured on its own master), `into
#{}` of them 1,253 to 949, `into []` of a 20-item list 1,498 to 681, `into
#{}` of it 2,574 to 1,638, `into {}` of a 13-key map's seq 4,311 to 1,915 (to
3,095 without reading the entries' slots), `into {}` of 13 pairs 4,046 to
2,791, `mapv` of 100 items 5,660 to 1,734, `persistent!` of `reduce conj!`
of 100 items 5,221 to 1,362.

**Workload 200k** (`compare.bb PROJECT --runs 5 --runtime-only --runtime
1,40`, project `workload-200k-seed42-1065`, the light-cljs output rebuilt
with each binary just before its runs), wall time at 40 iterations, median
(range), and light-cljs's steady state:

| project | light before | light after (`08a43178`) | shadow-cljs (before / after runs) | steady state, ms per iteration | load before / after |
| --- | ---: | ---: | ---: | ---: | ---: |
| workload 200k | 1.988 s (1.940-2.022) | 1.858 s (1.846-1.913) | 1.225 / 1.210 s | 41.2 to 38.3 (shadow-cljs 23.2 / 23.1) | 2.84-3.47 / 3.60-3.86 |

Light/shadow 1.78 before and 1.65 after.

**Size** (`bb benchmarks/re-frame.bb size simple todomvc flow --light-only`,
ports from `shadow-port-tool-983`), the whole bundle with React, gzip -9:

| App | before (`a397e0c9`) | after (`08a43178`) | change |
| --- | ---: | ---: | ---: |
| re-frame simple | 114,343 | 114,588 | +245 (+0.21%) |
| TodoMVC | 145,750 | 146,036 | +286 (+0.20%) |
| flow | 333,814 | 333,895 | +81 (+0.02%) |

- light-cljs's output as written, gzip -9: simple 67,943 to 68,169 (+226),
  TodoMVC 97,491 to 97,726 (+235), flow 280,824 to 281,069 (+245).
- At `:advanced` the slot-first `conjBangOwn`, `assocBangOwn` and
  `persistentBang` are about 140 to 150 bytes each, the transient vector's
  `-conj!` 133, `assocEntry` 151, and each slot assignment 27 or 28
  (`--size-report`).
- The size ratchet (tests/support/ratchet_fixtures.rs, noted there): 507
  bytes more at advanced where a program conjoins onto a transient vector
  (`mapv-count`), 1,031 for the features example; the readable runtime
  5,465 bytes more, mostly comments.

**After the review** (Sol): a map transient's `conj!` of a map entry read
the entry's `-val` slot before it called `-key`, where `valProtocol`'s
lookup followed `keyProtocol`'s call, so a `-key` that `specify!`d the
entry a new `-val` had the old one called (`{:a 1}` where master gives
`{:a 99}`). It now reads `-val` after `-key` returns
(`transient_map_conj_observes_value_method_changes_from_key`): 4 bytes more
at advanced, no change to any measured path's work.

## 2026-11-18: the seq walk shaped as upstream's (light-cljs-1054)

`seq`, `first`, `rest` and `next` call a value's keyed slot, then upstream's path for nil and host values, without the slot-first wrappers and the slots' `noBranch` guards (docs/value-layout.md, "What light-cljs-1054 landed"). The lookups, updates, `=` and `hash` keep theirs (light-cljs-1066).

**Binaries:** `light-cljs-a397e0c9` (master before) and `light-cljs-1054-28b22fc0` (this pellet on it), V8 builds. No run waited for the machine; the loads are given with each table, all unreserved.

**The gate** (`bb benchmarks/core-ops/run.bb --light BIN`, all 68 operations, 5 runs per compiler alternated, then the same with the other binary and `--baseline`, twice: pairs a and b, loads 2.8-8.1 and 4.4-13.1), the operations the seq walk touches, light-cljs ns per operation:

| operation | before a | after a | change | before b | after b | change | light/shadow after |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| lazy-walk-1000 | 45,346 | 39,122 | -14% | 45,469 | 38,469 | -15% | 0.77-0.79 |
| seq-walk-vec-1000 | 25,869 | 24,962 | -4% | 26,474 | 25,086 | -5% | 1.28-1.30 |
| keys-small | 340.4 | 264.4 | -22% | 344.5 | 267.1 | -22% | 3.49-3.59 |
| vals-small | 360.4 | 285.5 | -21% | 381.8 | 291.3 | -24% | 3.65-3.74 |
| keys-hash100 | 3,767 | 3,509 | -7% | 4,245 | 3,533 | -17% | 0.71-0.72 |
| vals-hash100 | 3,947 | 3,739 | -5% | 4,363 | 3,710 | -15% | 0.75 |
| merge-small | 2,129 | 2,039 | -4% | 2,264 | 2,108 | -7% | 2.28-2.37 |
| str-concat | 165.4 | 157.5 | -5% | 167.4 | 158.3 | -5% | 10.7-10.8 |
| filter-count-1000 | 38,992 | 38,405 | -2% | 38,719 | 38,801 | +0% | 1.36-1.38 |
| into-xform-1000 | 65,952 | 65,227 | -1% | 68,450 | 66,996 | -2% | 3.55-3.68 |

- **No operation is more than 2% slower in pair b.** In pair a one is 5% slower, `vec-nth-1024` (4.88 to 5.11 ns), which this pellet does not touch (`nth` of a vector calls its slot through `nthSite`, unchanged), and which pair b has 2% faster (4.99 to 4.91): noise of a 5 ns operation.
- **Why the walks gain:** a lazy seq's step called `first`'s and `seq`'s wrappers, 90 to 94 bytes of bytecode with the `noBranch` test, and each slot tested its fields first; the operations are now 49 to 58 bytes and the slots read only what they answer. The key and value seqs' getters call the map seqs' slots, which no longer guard.

**Size** (`bb benchmarks/re-frame.bb size simple todomvc flow --light-only --light BIN`, fresh ports from `shadow-port-tool-983` with the `a397e0c9` binary, built by every binary), the whole bundle with React, gzip -9:

| App | `f533efb1` | master `a397e0c9` | light-cljs-1054 | against master | against `f533efb1` |
| --- | ---: | ---: | ---: | ---: | ---: |
| re-frame simple | 108,631 | 114,343 | 114,103 | -240 (-0.21%) | +5,472 |
| TodoMVC | 138,015 | 145,750 | 145,604 | -146 (-0.10%) | +7,589 |
| flow | 328,641 | 333,715 | 333,497 | -218 (-0.07%) | +4,856 |

- **Raw:** 373,438 to 371,955 (-1,483), 481,187 to 479,727 (-1,460), 1,286,309 to 1,285,226 (-1,083); light-cljs's output as written, gzip, 67,943 to 67,775, 97,491 to 97,350, 280,826 to 280,735.
- **Against the expectation** (2,500 to 4,000 bytes per app for the whole pellet, light-cljs-1047): this is the seq walk alone. The recognisers its old bodies called stay for the other operations' bodies, and all three apps keep `extend-type` (`extendType`), so they keep `firstSeq` for `ownSeqs`; the size ratchet's programs that walk seqs, without those, are 1,460 to 2,503 bytes smaller raw (tests/support/ratchet_fixtures.rs). The user decided on 2026-11-17 that per-pellet targets are reported, not gated; light-cljs-1066 does the other operations.
- The browser behaviour checks pass for all three apps with this pellet's builds (`bb benchmarks/re-frame.bb browser todomvc simple flow --compiler light --runs 1`: 16 of 16).

**Workload 200k and the re-frame-shaped 200k app** (`compare.bb PROJECT --runs 5 --runtime-only --runtime 1,40`, projects `workload-200k-seed42-1054` and `re-frame-200k-seed42-1054`, the light-cljs output rebuilt with each binary just before its runs), wall time at 40 iterations, median (range), and light-cljs's steady state:

| project | light before | light after (`28b22fc0`) | shadow-cljs (before / after runs) | steady state, ms per iteration | load before / after |
| --- | ---: | ---: | ---: | ---: | ---: |
| workload 200k | 2.165 s (2.136-2.215) | 2.055 s (2.047-2.063) | 1.346 / 1.325 s | 45.4 to 42.8 (shadow-cljs 26.1 / 25.8) | 11.9-12.6 / 12.1-12.6 |
| re-frame-shaped | 21.145 s (19.449-21.424) | 19.405 s (18.878-20.689) | 12.165 / 11.388 s | 491.0 to 446.4 (shadow-cljs 274.9 / 252.8) | 12.5 / 6.9-10.0 |

Light/shadow steady state 1.74 to 1.66 for the workload. The re-frame-shaped app's runs moved with the machine's load (shadow-cljs's own steady state 8% faster in the second pair of runs): light/shadow 1.79 to 1.77.

## 2026-11-18: startup's type and keyword setup (light-cljs-1017)

Startup measured by phase, and the program's definitions made cheaper at load (docs/value-layout.md, "What light-cljs-1017 found and landed"): `installMethods` keeps and copies only the `Object/` methods' entries and reads its entries by index, `decorateType` finds a marker's mask bit in `protocolMasks`, the record markers' entries are built once (`recordMarkers`), `mungeMember` replaces by a pattern, `keyword` finds a literal without a namespace by its name, and a host type's `extend-type` implementation calls its method without a rest array.

**Binaries:** master `light-cljs-9c9479b3` and `light-cljs-1017-7dc8274c` (this pellet on it), V8 builds. shadow-cljs 3.5.3, Node v26.12.1. No run waited for the machine: a peer's test suite and builds ran throughout, and each table gives its 1-minute load; all unreserved.

**Where load goes** (workload 200k, in-process: the module's compile until its first statement runs, the runtime's units, the program's top level, then one iteration; 15 runs of each output alternated, load 3.5-3.7), median (range) in ms:

| phase | light-cljs `9c9479b3` | light-cljs-1017 | shadow-cljs |
| --- | ---: | ---: | ---: |
| compile | 73.3 (72.1-77.9) | 72.9 (72.0-77.6) | 68.0 (67.2-69.8) |
| runtime units | 1.4 | 1.4 | (in its top level) |
| program's top level | 42.5 (41.5-46.9) | 24.6 (24.3-27.1) | 13.2 (13.0-14.1) |
| first iteration | 183.1 (179.6-189.2) | 184.1 (182.3-196.2) | 168.6 (166.5-171.2) |
| total | 311.4 (307.7-326.8) | 293.9 (290.9-306.1) | 261.0 (258.8-266.0) |

Workload 800k (9 runs, load 3.3-3.4): compile 301.2, 297.9 and 288.6 ms; top level 132.8 (129.8-142.0), 85.4 (82.1-90.0) and 41.1 (39.8-42.4); first iteration 647.6, 656.1 and 627.3; total 1,098, 1,050 and 971.

- **Compile** is the same per byte of parse as shadow-cljs's (V8's trace: 20.5 and 21.3 ms of `ParseProgram` per MB at 200k). light-cljs's is longer because V8 compiles about 2,100 parenthesized function expressions of the program at once (methods and type constructors passed to the runtime) where shadow-cljs's output has 13; writing them without parentheses moved 6 ms from the compile to the first iteration and gained nothing.
- **The runtime's units** take 1.4 ms (2.2 at 800k): no store, method key or kind table of note is built at load any more.
- **The program's top level** was the gap: its definitions, timed per runtime root on a `--pseudo-names` build of 800k before and after, `defineType` 47.0 to 26.5 ms (2,298 types), `extendType` 20.8 to 12.1 (2,455), `installMethods` (the record kits) 13.6 to 1.1 (1,904), `keyword` 10.8 to 6.6 (24,945 constants), `protocolMethod` 7.7 to 6.8 (981); `derive` (13.4 to 17.1, 1,642) and the multimethods' `addMethod` (5.2 to 7.4, 5,410) are operations this pellet does not change and moved with the machine.
- **The first iteration** is the operations' speed: steady state is unchanged (below).

**First output** at one iteration, from starting the process to its first byte, the three outputs alternated in one session (a Node script spawning `node run-light.mjs 42 1` against copies of the module built by each binary and `node out/shadow.js 42 1`), median (range) in ms:

| project (runs, load) | light-cljs `9c9479b3` | light-cljs-1017 | shadow-cljs | light/shadow before, after |
| --- | ---: | ---: | ---: | ---: |
| workload 200k (20, 4.9-5.7) | 343.0 (338.5-357.2) | 330.9 (326.1-343.0) | 295.5 (292.0-322.8) | 1.16, 1.12 |
| workload 200k (8, 7.6-8.1) | 380.9 (373.9-386.8) | 363.5 (361.1-368.6) | 323.3 (320.3-330.8) | 1.18, 1.12 |
| workload 800k (12, 4.6-5.1) | 1,165.1 (1,110.1-1,289.8) | 1,120.1 (1,072.8-1,199.1) | 1,032.0 (972.2-1,095.3) | 1.13, 1.09 |
| re-frame-shaped 200k app (10, 4.2-4.8) | 1,839.8 (1,779.7-1,989.4) | 1,817.7 (1,779.3-1,936.8) | 1,319.7 (1,284.4-1,501.6) | 1.39, 1.38 |

`compare.bb workload-200k-seed42-1017 --runs 10 --runtime-only --runtime 1`, one session per binary, two pairs: light-cljs 0.352 and 0.360 s before (shadow-cljs 0.300 and 0.311 in the same sessions, light/shadow 1.17 and 1.16), 0.455 (load 3.2-5.5, ranges 0.327-0.529) and 0.358 s after (shadow-cljs 0.395 and 0.319, 1.15 and 1.12); the sessions drift with the machine more than the change, which the alternated runs above separate.

**TodoMVC** (`bb benchmarks/re-frame.bb runtime todomvc --runs 10 --todos 50`, one session per binary, load 4.9-5.1): page load to interactive 41.95 ms (40.60-43.90) before against shadow-cljs's 31.90, 41.40 (39.90-43.20) after against 31.85 (1.32 to 1.30); its script time 22.36 to 21.71 ms against 13.57 and 13.41; the JavaScript heap after load 3.25 to 3.22 MB against 2.74. A Chrome DevTools Protocol profile of a `--pseudo-names` bundle puts the gap in the module's top level (17.0 against 11.0 ms with React's initialization in both), which is the program's own work at load, re-frame's `dispatch-sync` and registrations through the operations, and in React's first render (9.0 against 8.2 ms); type definitions are under 1 ms of it.

**Steady state** (workload 200k, 40 and 1 iterations, the three alternated, 8 runs each, load 5.8-8.1): 39.0 ms per iteration before and 39.0 after, shadow-cljs 25.2. **The gate** (`bb benchmarks/core-ops/run.bb`, the record, protocol, multimethod and keyword operations and the maps' `=`, hash, `str` and `merge`, 5 runs per compiler alternated, after with `--baseline`, load 3.3-9.2): no operation is more than 2% slower (`multimethod` 113.0 to 115.8 ns, its range 113.0-175.5; `kw-hash100` 16.1 to 16.5; the others +0 to +2%).

**Size:** workload 200k's output is 312 bytes larger raw and 105 after gzip -9 (848,858 to 848,963); the re-frame bundles with React grow 40, 12 and 126 bytes after gzip (simple 113,685 to 113,725, TodoMVC 144,845 to 144,857, flow 332,043 to 332,169; 0.01 to 0.04%). The size ratchet's record programs are 213 to 265 bytes larger at advanced and its keyword programs 21 (tests/support/ratchet_fixtures.rs).

The review of this pellet found that a name a macro builds, such as `/1/ab` without a namespace, could share `keyword`'s key with a namespaced literal (`a/b`); a name that starts with a slash now takes another (`keyword_intern_keys` in tests/support/js_runtime.test.js). That one test of the name's first character is not in the measured binary `7dc8274c`; it adds 21 bytes to a program with keyword constants.

**What is left**, profiled and filed: the program's top level is still 1.9 to 2.1 times shadow-cljs's (a record type's about 30 non-enumerable slots and per-type closures, a program protocol's dispatcher factory compiled at load, `derive` rebuilding sets; light-cljs-1069); the first iteration is the operations', most of it in the re-frame-shaped app the extended lookup that predicates, `count` and `meta` take in a program that extends types (219 of its 1,260 ms; light-cljs-1070); light-cljs-1066 and light-cljs-1068 reshape the other operations.

## 2026-11-18: the lookups shaped as upstream's (light-cljs-1066)

`count`, `nth`, `get`, `contains?` and `peek` call a value's keyed slot, then upstream's path for nil and host values (`countOf` and the others), without the slot-first wrappers and the lookup slots' `noBranch` guards; `nth` and `get` are one function per arity, each under light-cljs-1033's 100 bytes of bytecode (docs/value-layout.md, "What light-cljs-1066 landed"). A lookup's slot that a program sets to nil, false or undefined on a runtime class's prototype, or on its value over an `extend-type` method, still answers by the class: only a program that writes a core protocol method's member by name keeps the wrappers that do it (`slotWrites`), so the measured programs, which write none, have none. The updates, `=`, `hash` and the reductions keep their wrappers (light-cljs-1068).

**Binaries** in research/shadow-credibility/bin: before `light-cljs-9c9479b3` (master), after `light-cljs-1066-23cfd103` (this pellet on it; the landed commit rebases it onto light-cljs-1017 and changes only the `slotWrites` wrappers, which no measured program keeps), V8 builds. Records in research/output-speed/lookups-1066. No run waited for the machine; all results are unreserved (peer workers built and tested throughout).

**Gate** (`bb benchmarks/core-ops/run.bb --runs 5`, all 72 operations, the after run with `--baseline` the before's; load 8.74-12.45 before and 6.98-8.67 after), light-cljs ns per operation, median (range), the operations that read through a lookup:

| op | light before | light after | change | shadow-cljs after | light/shadow after |
| --- | ---: | ---: | ---: | ---: | ---: |
| kw-small | 7.98 (7.90-8.06) | 7.71 (7.50-7.74) | -3% | 9.89 | 0.78 |
| get-small | 11.4 (11.2-11.4) | 11.1 (10.8-11.2) | -3% | 15.0 | 0.74 |
| get-small-miss | 15.8 (15.8-15.9) | 15.6 (15.3-15.6) | -2% | 19.4 | 0.80 |
| contains-small | 10.1 (10.0-10.2) | 9.40 (9.33-9.41) | -7% | 12.2 | 0.77 |
| count-small | 1.99 (1.97-2.01) | 2.12 (2.04-2.13) | +7% | 2.13 | 1.00 |
| get-hash100 | 12.7 (12.3-12.8) | 12.5 (12.0-12.7) | -2% | 12.2 | 1.02 |
| kw-hash100 | 14.4 (13.8-16.0) | 14.3 (14.2-14.9) | +0% | 13.6 | 1.05 |
| get-in-3 | 64.2 (63.7-64.6) | 63.5 (62.9-63.7) | -1% | 57.8 | 1.10 |
| update-in-3 | 188.8 (187.5-189.7) | 173.4 (171.2-175.1) | -8% | 137.9 | 1.26 |
| vec-nth-1024 | 5.05 (4.68-5.11) | 4.55 (4.25-4.71) | -10% | 5.15 | 0.88 |
| vec-invoke-1024 | 5.05 (5.00-5.08) | 5.08 (4.82-5.18) | +1% | 4.82 | 1.05 |
| vec-peek | 3.74 (3.70-3.74) | 3.64 (3.60-3.65) | -3% | 3.45 | 1.06 |
| destructure-vec-3 | 7.34 (7.30-7.41) | 5.47 (5.20-5.48) | -25% | 9.84 | 0.56 |
| destructure-map-3 | 63.2 (62.4-65.6) | 62.3 (61.2-62.7) | -1% | 49.8 | 1.25 |
| select-keys-3 | 340.4 (337.6-345.3) | 338.1 (326.4-339.9) | -1% | 247.1 | 1.37 |
| filter-count-1000 | 38,948 (38,801-39,654) | 37,024 (36,701-37,505) | -5% | 28,158 | 1.31 |
| variadic-call | 13.9 (13.6-14.1) | 14.7 (14.6-14.8) | +6% | 14.1 | 1.05 |

- **Two operations are more than 5% slower, both `count`:** `count-small` (`count` of a 7-key array map) and `variadic-call`, whose function counts its rest arguments' indexed seq. A second pair with only these two (`--ops count-small,variadic-call`, load 4.95-5.52) repeats it: 1.94 to 2.06 ns (+6%) and 9.91 to 10.6 ns (+7%). The call is the same in both binaries (`countSite`, which reads the `-count` slot and calls it), and what runs after it is less: no `noBranch` comparison in `countSite`, no field test in the array map's and the indexed seq's `-count` slots. Alone, the same loop over four array maps (20 million `count`s in a program of its own, two runs each) is faster after: 2.71-2.74 ns before and 2.28-2.46 after. So the 0.12 ns (under half a cycle) is the benchmark process's type feedback from the operations around it, not the work `count` does; `count-small` stays at parity with shadow-cljs (1.00).
- **What gains:** `destructure-vec-3` (`nth` of a vector through `nthSite` no longer compares against `noBranch`), `vec-nth-1024`, `update-in-3` and `contains-small`; outside the table `reduce-entries-small` (-12%), `reduce-map-inc-1000` (-11%) and `merge-small` (-7%): destructuring a map entry calls its `-nth` slot without the guard, `merge` counts, and `map inc` over a vector counts its chunks and chunk buffers, which have `-count` slots now; no other operation of the 72 moves more than 5%.

**Size** (`bb benchmarks/re-frame.bb size simple todomvc flow --light-only --light BIN`, fresh ports from `shadow-port-tool-983` with the `9c9479b3` binary, built by every binary), the whole bundle with React, gzip -9:

| App | `f533efb1` | master `9c9479b3` | light-cljs-1066 | against master | against `f533efb1` |
| --- | ---: | ---: | ---: | ---: | ---: |
| re-frame simple | 108,631 | 113,685 | 113,364 | -321 (-0.28%) | +4,733 |
| TodoMVC | 138,015 | 144,845 | 144,573 | -272 (-0.19%) | +6,558 |
| flow | 328,644 | 331,982 | 331,665 | -317 (-0.10%) | +3,021 |

- **Raw:** 372,517 to 370,395 (-2,122), 479,878 to 477,755 (-2,123), 1,283,954 to 1,282,339 (-1,615); light-cljs's output as written, gzip, 67,572 to 67,239, 96,810 to 96,543, 279,672 to 279,514.
- **Against the expectation** (light-cljs-1047's 2,500 to 4,000 bytes per app for the whole of light-cljs-1066 as filed): this is the lookups alone, and the apps stay larger than at `f533efb1`. The recognisers the old bodies called stay for the updates', `=`'s and `hash`'s bodies and the reductions', which light-cljs-1068 reshapes. Per-pellet size targets are reported, not gated (the user, 2026-11-17).
- The browser behaviour checks pass for all three apps with this pellet's builds (`bb benchmarks/re-frame.bb browser todomvc simple flow --compiler light --runs 1`: 16 of 16).
- **On master `2af3dfcb`** (with light-cljs-1017), the landed commit's release binary against master's (`light-cljs-2af3dfcb`), same ports: simple 113,731 to 113,539 (-192), TodoMVC 144,894 to 144,678 (-216), flow 332,098 to 331,847 (-251); none of the three apps writes a core protocol method's member by name, so none keeps `slotWrites` (`--size-report`).

**Workload 200k and the re-frame-shaped 200k app** (`compare.bb PROJECT --runs 5 --runtime-only --runtime 1,40`, projects `workload-200k-seed42-1066` and `re-frame-200k-seed42-1066`, the light-cljs output rebuilt with each binary just before its runs), wall time at 40 iterations, median (range), and light-cljs's steady state:

| project | light before | light after (`23cfd103`) | shadow-cljs (before / after runs) | steady state, ms per iteration | load before / after |
| --- | ---: | ---: | ---: | ---: | ---: |
| workload 200k | 1.753 s (1.734-1.772) | 1.725 s (1.713-1.758) | 1.185 / 1.173 s | 35.9 to 35.3 (shadow-cljs 22.6 / 22.4) | 5.6-6.8 / 4.9-5.6 |
| re-frame-shaped | 18.362 s (18.311-18.985) | 18.181 s (18.163-19.081) | 10.774 / 10.765 s | 423.1 to 420.2 (shadow-cljs 242.8 / 242.5) | 3.4-4.4 / 2.6-3.1 |

Light/shadow steady state 1.59 to 1.57 for the workload and 1.74 to 1.73 for the re-frame-shaped app; the first iteration 0.351 to 0.349 s and 1.863 to 1.792 s. Both are within their ranges of master: the lookups were already slot-first in both programs' hot paths, and this pellet takes the wrapper layer and guards off them, not a lookup.

## 2026-11-18: types, protocols and hierarchies at load (light-cljs-1069)

The program's definitions at load made cheaper, after light-cljs-1017's (docs/value-layout.md, "What light-cljs-1069 found and landed"): a program protocol's dispatcher is written as the function itself, which V8 compiles on its first call, where `protocolMethod` called a factory V8 compiled at load, and a method nothing assigns has no getter; `installMethods` defines each protocol entry's slot in one loop and keeps its records only for `Object/` and `IFn/-invoke` methods; a record type at `:advanced` takes its 15 markers as their masks and its kit as `defineType`'s last argument; `derive` conjoins into its sets.

**Binaries:** master `light-cljs-c786677b` (light-cljs-1066 landed) and `light-cljs-1069-80a86562` (this pellet on it, before the review's fix that keeps a record's markers where a program names a protocol member or mask, which changes these programs' outputs only by `defineType`'s extra parameter, 5 bytes), V8 builds; the gate before the rebase also with `light-cljs-2af3dfcb` and `light-cljs-1069-9a191551`. shadow-cljs 3.5.3, Node v26.12.1. No run waited for the machine; each table gives its 1-minute load; all unreserved.

**Where load goes** (in-process: the module's compile until its first statement runs, the runtime's units, the program's top level, then one iteration; the three outputs alternated), median (range) in ms:

| phase | light-cljs `c786677b` | light-cljs-1069 | shadow-cljs |
| --- | ---: | ---: | ---: |
| workload 200k (15 runs, load 3.1-3.2): compile | 72.6 (71.7-76.6) | 72.1 (71.1-75.3) | 80.8 (79.5-85.6) |
| runtime units | 1.4 | 1.4 | (in its top level) |
| program's top level | 24.2 (23.8-27.2) | 19.7 (19.4-20.2) | 13.1 (13.0-13.5) |
| first iteration | 180.7 (177.5-191.6) | 181.9 (178.8-188.3) | 166.0 (164.4-171.4) |
| workload 800k (9 runs, load 3.0-3.2): compile | 292.0 (289.9-301.3) | 291.3 (287.8-302.9) | 324.4 (323.5-335.9) |
| runtime units | 2.1 | 2.1 | (in its top level) |
| program's top level | 80.9 (79.8-82.3) | 65.2 (64.3-69.7) | 39.7 (39.1-43.7) |
| first iteration | 641.7 (637.2-649.6) | 642.4 (638.3-648.6) | 613.3 (604.9-630.3) |

The top level is 1.85 to 1.50 times shadow-cljs's at 200k and 2.04 to 1.64 at 800k: short of the 1.3 the pellet aimed for; what is left is named below. (Here shadow-cljs's compile is timed from the script's import, which reads a larger file; light-cljs-1017 timed it at 68.0 ms against light-cljs's 73.3.)

**Per runtime root** (`--pseudo-names` builds of workload 800k by `2af3dfcb` and `1069-9a191551`, 7 runs alternated, load 6.6-7.1), median ms: `defineType` 32.7 to 21.7 (2,298 types, 1,904 records) and the record kits' `installMethods` 1.1 to none; `protocolMethod` 6.8 to 1.1 (981 methods), whose factory V8 compiled at load; `extendType` 10.4 to 6.9 (2,455), which called each method's getter, so V8 compiled each; `derive` 13.9 and 16.4 (1,642), `addMethodProtocol` 5.2 and 6.2 (5,410), `keyword` 6.4 and 5.8 (24,945), which this pellet does not make cheaper: `derive` rebuilt from an array, conjoining as upstream's, and hoisted all measured alike within the runs' noise (12 to 19 ms), as its cost is the operations it calls (`get`, `assoc`, `contains?`, hashing) in the interpreter, in a program that extends types.

**First output** at one iteration, from starting the process to its first byte, the three outputs alternated in one session (`node run-light.mjs 42 1` against copies of the module built by each binary, and `node out/shadow.js 42 1`), median (range) in ms:

| project (runs, load) | light-cljs `c786677b` | light-cljs-1069 | shadow-cljs | light/shadow before, after |
| --- | ---: | ---: | ---: | ---: |
| workload 200k (20, 3.0-3.1) | 327.3 (323.9-343.5) | 321.9 (319.1-331.9) | 293.1 (291.0-301.7) | 1.12, 1.10 |
| workload 800k (12, 3.0-3.2) | 1,079.0 (1,069.1-1,104.8) | 1,047.6 (1,042.3-1,067.1) | 975.1 (968.3-985.6) | 1.11, 1.07 |
| re-frame-shaped 200k app (10, 3.0-3.5) | 1,750.8 (1,740.9-1,884.6) | 1,760.0 (1,733.2-1,808.3) | 1,302.8 (1,276.8-1,315.5) | 1.34, 1.35 |

The re-frame-shaped app does not move: its gap is the operations of its first iteration (light-cljs-1017, light-cljs-1070), not its definitions.

**TodoMVC** (`bb benchmarks/re-frame.bb runtime todomvc --runs 10 --todos 50`, one session per binary, load 3.0 and 8.2 at their starts; the port rebuilt with `shadow-port-tool-983`): page load to interactive 43.55 ms (41.00-67.30) before against shadow-cljs's 33.65 in the same session, 42.65 (41.60-46.20) after against 34.80 (1.29 to 1.23); its script time 22.94 to 22.08 ms against 14.76 and 14.86; the JavaScript heap after load 3.20 MB both times against 2.74.

**Steady state** (workload 200k, 40 and 1 iterations, the three alternated, 8 runs each, load 3.3-3.6): 35.2 ms per iteration before and 35.3 after, shadow-cljs 22.3. **The gate** (`bb benchmarks/core-ops/run.bb --ops protocol-on-record,record-kw-field,record-dot-field,record-assoc,multimethod,multi-arity-call`, 5 runs per compiler alternated, after with `--baseline`): on `c786677b` (load 4.2-4.7) every operation +0% but `multimethod` +1% (77.7 to 78.3 ns, its range 76.7-78.6); on `2af3dfcb` (load 13.1-13.6) every one +0% but `multimethod` -2% (88.4 to 86.9). `protocol-on-record` stays 2.33 ns, shadow-cljs's.

**Size** (`bb benchmarks/re-frame.bb size todomvc simple --light-only`, on `c786677b`): the re-frame bundles with React grow 246 and 202 bytes after gzip -9 (TodoMVC 144,678 to 144,924, 0.17%; simple 113,196 to 113,398, 0.18%); flow's port does not build with either binary (its vendored zprint uses `:require-macros`, which light-cljs retired). The size ratchet's record programs are 555 bytes larger at advanced, for the one loop, the record masks and the dispatchers, which spell each slot twice as upstream's do; each extra two-method protocol 375 to 398 bytes (tests/support/ratchet_fixtures.rs, tests/compiler/size_ratchet.rs).

**What is left** of the top level, profiled: the type definitions' about 46,000 non-enumerable slots at 800k, each `Object.defineProperty` about 150 ns (220 where the type's closures are its own) with as much again in their loops, where upstream's output assigns a record about 21 slots as stores; a record's 16 shared methods built as closures per type over `Type` (a shared function reading the prototype's record slot measured 3.7 ms less at 800k, at the cost of a megamorphic read in every record method); and `derive`, `addMethod` and `keyword` constants, whose cost is the operations at load, in the interpreter (shadow-cljs constructs its keyword constants with their hashes and does not intern them).

## 2026-11-18: the updates shaped as upstream's (light-cljs-1068)

`conj`, `assoc`, `dissoc`, `disj` and `pop` call a value's keyed slot, then upstream's path for nil, host values and the other arities (`conjOf` and the others), without the slot-first wrappers, the update slots' `noBranch` guards and the call-site entries' `noBranch` tests; the variadic ones send the arity with a slot to a function of its own (`conj2`, `assoc3`, `dissoc2`, `disj2`), each under light-cljs-1033's 100 bytes of bytecode (docs/value-layout.md, "What light-cljs-1068 landed"). The lists, the sets, the vectors, a map entry and the queue gained the `-conj`, `-disjoin` and `-pop` slots their old paths gave. `=`, `hash` and the reductions keep their wrappers (light-cljs-1072).

**Binaries** in research/shadow-credibility/bin: before `light-cljs-c786677b` (master), after `light-cljs-1068-361997ef` (this pellet on it), V8 builds; sizes also with `light-cljs-f9d52a2c` (master with light-cljs-1069) and `light-cljs-1068-4d5410e5` (the landed commits, which add the Sol review's fix for variadic `assoc` and `disj` and the program types' `disj` and `pop`, which changes no single-key update; the timings were not repeated for it). Records in research/output-speed/updates-1068. No run waited for the machine; all results are unreserved (a peer worker built and tested throughout).

**Gate** (`bb benchmarks/core-ops/run.bb --runs 5`, all 70 operations, the after run with `--baseline` the before's; load 6.58-7.77 before and 6.60-11.40 after), light-cljs ns per operation, median (range), the operations that update:

| op | light before | light after | change | shadow-cljs after | light/shadow after |
| --- | ---: | ---: | ---: | ---: | ---: |
| assoc-small-existing | 24.2 | 22.2 (21.4-22.3) | -8% | 30.8 | 0.72 |
| assoc-small-new | 49.5 | 48.4 (47.2-48.9) | -2% | 39.4 | 1.23 |
| dissoc-small | 50.1 | 50.8 (50.5-52.4) | +1% | 57.5 | 0.88 |
| update-small | 29.0 | 28.3 (28.1-28.9) | -2% | 43.6 | 0.65 |
| assoc-hash100 | 82.7 | 83.3 (82.7-83.9) | +1% | 78.8 | 1.06 |
| dissoc-hash100 | 88.6 | 88.2 (85.7-88.3) | -1% | 76.6 | 1.15 |
| update-hash100 | 91.6 | 91.8 (90.8-92.1) | +0% | 96.1 | 0.96 |
| assoc-in-3 | 154.3 | 160.1 (157.9-164.0) | +4% | 130.0 | 1.23 |
| update-in-3 | 176.5 | 178.3 (176.5-179.2) | +1% | 137.7 | 1.29 |
| vec-conj-64 | 1,332 | 1,326 (1,317-1,572) | -1% | 1,303 | 1.02 |
| vec-assoc-1024 | 53.7 | 53.9 (53.7-54.2) | +0% | 74.1 | 0.73 |
| swap-assoc | 47.3 | 43.6 (42.7-44.2) | -8% | 38.0 | 1.15 |
| merge-small | 2,056 | 2,171 (2,090-2,229) | +6% | 879.8 | 2.47 |
| select-keys-3 | 337.9 | 326.0 (324.0-327.9) | -4% | 250.0 | 1.30 |
| group-by-100 | 8,839 | 8,548 (8,501-8,628) | -3% | 5,061 | 1.69 |
| sorted-map-assoc-100 | 190.0 | 191.8 (191.1-199.5) | +1% | 206.4 | 0.93 |
| into-set-100 | 7,078 | 7,157 (6,910-7,971) | +1% | 4,270 | 1.68 |

- **One operation is more than 5% slower in the full pass, `merge-small` (+6%), and it is noise:** two more alternated pairs of only `merge-small`, `assoc-in-3` and `str-concat` (`--ops`, 5 runs each, load 8.03-12.53) give `merge-small` 1,767 to 1,875 ns (+6%) and 1,705 to 1,623 (-5%), `assoc-in-3` +0% and -1%, `str-concat` +0% and -1%. Its per-process spread (1,623 to 2,229 ns over the four binaries' runs) is wider than the change; `merge` conjoins each map onto the first, through the map's `-conj` slot as before, now without the `noBranch` comparison and the map slot's field test.
- **What gains:** `assoc-small-existing` and `swap-assoc` (-8%: `assoc` of an array map reaches its slot through `assoc3`, no `noBranch` comparison and no `arr` test), `select-keys-3` (-4%) and `group-by-100` (-3%); no other operation of the 70 moves more than 3%.

**Size** (`bb benchmarks/re-frame.bb size simple todomvc flow --light-only --light BIN`, fresh ports from `shadow-port-tool-983` with the `c786677b` binary, built by each binary), the whole bundle with React, gzip -9:

| App | master `c786677b` | light-cljs-1068 on it (`361997ef`) | master `f9d52a2c` (light-cljs-1069) | landed (`4d5410e5`) | against `f9d52a2c` |
| --- | ---: | ---: | ---: | ---: | ---: |
| re-frame simple | 113,539 | 113,031 (-508) | 113,757 | 113,552 | -205 (-0.18%) |
| TodoMVC | 144,678 | 144,126 (-552) | 144,912 | 144,779 | -133 (-0.09%) |
| flow | 331,794 | 331,428 (-366) | 332,368 | 332,286 | -82 (-0.02%) |

- **Raw:** on `c786677b` 370,667 to 369,649 (-1,018), 478,024 to 476,934 (-1,090), 1,282,686 to 1,281,762 (-924); on `f9d52a2c` 371,458 to 371,478 (+20), 478,967 to 478,915 (-52), 1,284,136 to 1,284,296 (+160). The old bodies (`conjPath` and the others) and the hooks only they call (`kindHooks.conjVector`, `conjMap`) leave release slices that write no slot by name; none of the three apps does. The landed commit gives back about 300 bytes after gzip of the first measurement: the Sol review's fix keeps a vector's variadic `assoc` and a set's variadic `disj` to one metadata read, as the old paths, which keeps `kindHooks.assocVector` where a program keeps `assoc` and vectors.
- **Size ratchet:** no advanced size grows against master; four advanced ceilings move down (`constant-keys` 74,000 to 73,000, `mapv-count` 53,000 to 52,000, `features` and `features-direct` 117,000 to 116,000), and the features example's chunks are about 1,050 bytes smaller together (its shared chunk 93,836 to 92,179, `app.reports.js` 4,830 to 5,458 as the hash map's statics move into it).

**Workload 200k and the re-frame-shaped 200k app** (`compare.bb PROJECT --runs 5 --runtime-only --runtime 1,40`, projects `workload-200k-seed42-1068` and `re-frame-200k-seed42-1068`, the light-cljs output rebuilt with each binary just before its runs), wall time at 40 iterations, median (range), and light-cljs's steady state:

| project | light before | light after | shadow-cljs (before / after runs) | steady state, ms per iteration | load before / after |
| --- | ---: | ---: | ---: | ---: | ---: |
| workload 200k | 1.785 s (1.728-1.834) | 1.743 s (1.737-1.787) | 1.230 / 1.246 s | 36.1 to 35.8 (shadow-cljs 23.2 / 23.9) | 5.7-6.5 / 5.0-5.3 |
| re-frame-shaped | 20.473 s (18.565-31.607) | 18.312 s (17.543-23.069) | 11.832 / 11.800 s | 477.3 to 422.2 (shadow-cljs 268.3 / 266.9) | 4.5-15.2 / 6.0-14.5 |

The workload's steady state is within its range of master (light/shadow 1.56 to 1.50). The re-frame-shaped app's before runs met a load spike (one 40-iteration run 31.6 s, load up to 15.2), so its 477.3 is not master's level: light-cljs-1066 measured 420.2 for the same program, and this pellet's 422.2 is that, not a gain of 11%.

## 2026-11-19: the seq walk's cleared slots answer as master's (light-cljs-1071)

light-cljs-1071 adds four `slotWrites` wrappers (`seqOf`, `firstOf`, `restOf`, `nextOf`) that only a program writing a core protocol method's member by name keeps at `:advanced` (docs/value-layout.md, "What light-cljs-1071 landed"). Every other program's release output is unchanged, so no timing was run: built with the release binaries of master `76061c5c` and of this change, the gate's output (`light-cljs release bench` in benchmarks/core-ops), workload 200k's and the re-frame-shaped app's (`workload-200k-seed42`, `re-frame-200k-seed42`, `release bench`) are byte-identical (`diff -r`). The gate's times, steady state and advanced sizes are therefore those of the light-cljs-1068 section above.

**Size.** The size ratchet's advanced sizes do not move; the readable runtime of `none` and `prepare` builds grows 2,271 bytes. A program that clears the four slots on 23 classes' prototypes (the test `seq_walk_slots_a_program_clears_answer_as_master`) grows 2,989 bytes at advanced (150,202 to 153,191; 40,469 to 41,083 after gzip -9), the wrappers and the old paths they call.

## 2026-11-19: `pop`, `disj` and the variadic updates in the gate (light-cljs-1073)

light-cljs-1068 reshaped `pop`, `disj` and the variadic updates, but the gate timed none of them, so its section above has no number for them. The gate now has six more operations, after `into-set-100` and before the loop baselines, so no earlier operation's number moves (benchmarks/core-ops/README.md):
- `vec-pop`: vectors of 3, 10, 33 and 100 items. Popping the 33-item one takes the tail from the tree.
- `list-pop`: lists of 2 to 10 items.
- `disj-set-7`: one item from 7-item hash sets.
- `disj-sorted-set-100`: one item from a 100-item sorted set.
- `disj-variadic-set-7`: two items from the 7-item sets.
- `vec-assoc-variadic-1024`: two indexes of a 1,024-item vector.

This pellet changes no runtime or emitter code, so it has no workload or size run.

**Binaries** in research/shadow-credibility/bin, V8 builds:
- before: `light-cljs-f9d52a2c`, master just before light-cljs-1068 landed;
- after: `light-cljs-76061c5c`, master with light-cljs-1068.

Master `f5164c11` adds only light-cljs-1071. A program keeps its wrappers only when it writes a core protocol method's member by name, and the gate never does. Records are in research/output-speed/pop-disj-1073. No run waited for the machine, and all results are unreserved: a peer worker built and tested throughout.

**Commands:** two alternated passes, each a before run then an after run. Each run has 5 Node processes per compiler.

```sh
BIN=research/shadow-credibility/bin R=research/output-speed/pop-disj-1073
OPS=vec-pop,list-pop,disj,vec-assoc-variadic-1024   # N is the pass, 1 or 2
bb benchmarks/core-ops/run.bb --light $BIN/light-cljs-f9d52a2c --ops $OPS --out $R/before-N
bb benchmarks/core-ops/run.bb --light $BIN/light-cljs-76061c5c --ops $OPS --out $R/after-N --baseline $R/before-N.edn
```

**Load averages** before each run:

| pass | before | after |
| --- | ---: | ---: |
| 1 | 17.90-18.62 | 18.38-19.30 |
| 2 | 16.90-18.80 | 15.55-17.99 |

**Results:** light-cljs's ns per operation, median (range), from pass 1. The last column gives pass 2's change and light/shadow after.

| op | light before | light after | change | shadow-cljs after | light/shadow before / after | pass 2 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| vec-pop | 46.3 (45.2-47.4) | 19.1 (18.6-19.2) | -59% | 19.4 | 2.37 / 0.98 | -60%, 0.94 |
| list-pop | 26.1 (26.0-27.1) | 5.87 (5.84-5.89) | -78% | 5.40 | 4.84 / 1.09 | -77%, 1.08 |
| disj-set-7 | 83.7 (82.1-85.1) | 71.6 (70.5-73.6) | -14% | 112.4 | 0.74 / 0.64 | -15%, 0.62 |
| disj-sorted-set-100 | 234.1 (230.0-261.2) | 185.3 (179.6-192.0) | -21% | 271.7 | 0.88 / 0.68 | -22%, 0.69 |
| disj-variadic-set-7 | 131.6 (129.2-132.7) | 153.0 (148.0-159.1) | +16% | 234.0 | 0.54 / 0.65 | +17%, 0.67 |
| vec-assoc-variadic-1024 | 188.4 (182.1-189.7) | 189.8 (184.6-191.5) | +1% | 185.8 | 1.00 / 1.02 | -1%, 1.01 |

- **No operation is over 1.2x shadow-cljs's after light-cljs-1068.** The highest is `list-pop` at 1.08-1.09. Before it, `vec-pop` was at 2.37x and `list-pop` at 4.84x.
  - In master's `pop`, every call first asked `protocolHooks.builtin` whether a program had given the value a `-pop`, then probed the kinds (`holdsVector`, `vectorStore`).
  - The `-pop` slots of light-cljs-1068 answer from the vector or list itself.
  - Single-key `disj` gains 14% and 21% the same way, through the sets' `-disjoin` slots.
- **One loss: `disj-variadic-set-7` is 16% and 17% slower.** It is still 0.65-0.67x shadow-cljs's, so no pellet is filed.
  - The cause: the call now passes its `arguments` through `disj` to `disjOf` (by `apply`), copies them (`slice`), and reaches `setDisjoin`. That is two more calls than master's one rest-parameter `disj`.
  - The profiles show the work moving out of master's single function. Under `--pseudo-names disj-variadic-set-7`, master's `disj` had 44% of self time, inlining the entry search (`collectionEntryIndex` 1.4%). Now `setDisjoin` has 38%, `disjOf` 13.9% and `collectionEntryIndex` 6.5% (124.4 to 150.1 ns per operation under the profiler).
- **The variadic `assoc` of a vector is unchanged** (+1%, -1%) and at parity (1.01-1.02).

## 2026-11-19: the reductions shaped as upstream's (light-cljs-1072)

`reduce` and `reduce-kv` call a value's keyed slot, then upstream's path (`reduceOf`, `reduceKvOf`), without their slot-first wrappers and their slots' `noBranch` guards; `reduce` sends each arity to a function of its own (`reduce2`, `reduce3`), each under light-cljs-1033's 100 bytes of bytecode (docs/value-layout.md, "What light-cljs-1072 landed"). A vector's chunked seq, the integer range, repeat, iterate, cycle, an array chunk, a lazy seq and a key or value seq gained the `-reduce` slots for the walks the old path gave them; every other runtime class takes, from `reduceOf`, the walk the old path gave it, told by its keyed kind. `=` and `hash` keep their wrappers (light-cljs-1074).

**Binaries** in research/shadow-credibility/bin: before `light-cljs-76061c5c` (master), after `light-cljs-1072-f80a11ab` (this pellet's main commit `66aa4e68` on it; the review's fix `74386487` changes only the walk of a value without a `-reduce` slot that a program gave an `IIterable` iterator, which no measured program has), V8 builds. Records in research/output-speed/reductions-1072. No run waited for the machine; all results are unreserved (a peer worker built and tested throughout, and this pellet's full test suite ran during the after pass).

**Gate** (`bb benchmarks/core-ops/run.bb --runs 5`, all 70 operations, the after run with `--baseline` the before's; load 12.6-14.6 before and 13.2-29.1 after), light-cljs ns per operation, median (range), the operations that reduce:

| op | light before | light after | change | shadow-cljs after | light/shadow after |
| --- | ---: | ---: | ---: | ---: | ---: |
| reduce-vec-1000 | 5,096 | 5,231 (5,169-5,326) | +3% | 2,518 | 2.08 |
| reduce-map-inc-1000 | 39,425 | 39,867 (39,388-40,575) | +1% | 40,945 | 0.97 |
| reduce-range-1000 | 4,931 | 4,919 (4,686-7,888) | +0% | 5,551 | 0.89 |
| reduce-kv-small | 6.90 | 6.53 (6.47-14.3) | -5% | 38.2 | 0.17 |
| reduce-entries-small | 83.7 | 87.1 (82.2-127.9) | +4% | 81.8 | 1.06 |
| into-xform-1000 | 30,418 | 30,354 (30,274-30,574) | +0% | 19,066 | 1.59 |
| mapv-1000 | 14,249 | 14,310 (14,162-14,398) | +0% | 14,393 | 0.99 |
| transient-vec-100 | 1,189 | 1,210 (1,191-1,736) | +2% | 1,210 | 1.00 |
| into-vec-keys-small | 928.1 | 830.2 (817.1-1,471) | -11% | 334.9 | 2.48 |
| into-set-100 | 7,239 | 7,385 (6,988-10,866) | +2% | 4,352 | 1.70 |
| group-by-100 | 8,586 | 8,723 (8,625-11,529) | +2% | 5,227 | 1.67 |
| merge-small | 2,234 | 2,302 (2,249-3,910) | +3% | 914.0 | 2.52 |
| select-keys-3 | 330.4 | 316.0 (311.2-459.7) | -4% | 251.1 | 1.26 |
| filter-count-1000 | 38,084 | 35,122 (34,623-35,353) | -8% | 29,425 | 1.19 |

- **No operation of the 70 is more than 5% slower.** The reductions over a vector, a map's entries and through `merge` are 3-4% slower within ranges that the after pass's load widened (its upper ends 1.5 to 1.7 times its medians); their slot call is as before, now through `reduce3` and without the wrapper's `noBranch` comparison.
- **What gains:** `into-vec-keys-small` (-11%: `into` of a map's key seq reaches its new `-reduce` slot, where it took the protocol lookup and the old body's recognisers), `filter-count-1000` (-8%) and `reduce-kv-small` (-5%: `reduce-kv` reads the slot without the wrapper's `noBranch` comparison); two operations that do not reduce, `multimethod` (-11%) and `swap-assoc` (-9%), moved as much, which is the passes' noise.

**Size** (`bb benchmarks/re-frame.bb size simple todomvc flow --light-only --light BIN`, fresh ports from `shadow-port-tool-983` with the `76061c5c` binary, built by each binary; the after binary is the main commit's code), the whole bundle with React, gzip -9:

| App | master `76061c5c` | light-cljs-1072 (`f80a11ab`) | change |
| --- | ---: | ---: | ---: |
| re-frame simple | 113,552 | 113,417 | -135 (-0.12%) |
| TodoMVC | 144,779 | 144,560 | -219 (-0.15%) |
| flow | 332,454 | 332,099 | -355 (-0.11%) |

- **Raw:** 371,478 to 371,240 (-238), 478,915 to 478,670 (-245), 1,284,175 to 1,284,026 (-149). The old bodies (`reducePath`, `reduceKvPath`), the wrappers and `reduce.protocol` leave release slices that write no slot by name; the walks the old body held stay as functions the slots share (`reduceWalk`, `seqReduce`, `rangeReduce`, ...). A first version with a `-reduce` slot for every class the old path recognised (21 slot units) changed the apps by -59 to +9 bytes after gzip and grew them 775 to 1,235 bytes raw: the plain seq and view walks are a keyed-kind test in `reduceOf` instead, and the slots are made by one factory (`reduceSlot`).
- **Size ratchet:** `eight-records` moves down (48,000 to 47,000). Programs that keep the recognisers for their other operations pay the upstream-shaped dispatch: `mapv-count` 51,926 to 52,069 (ceiling 52,000 to 53,000) and the features example as one module 117,689 to 117,902 (its ceiling holds); the readable runtime grows about 9,600 bytes (none and prepare ceilings up), as the old paths stay for the compile-time runtime and the shared-heap profile.

**Workload 200k** (`compare.bb PROJECT --runs 3 --runtime-only --runtime 1,40`, two copies of `workload-200k-seed42`, `-1072b` built by the before binary and `-1072` by the after, the two run in turn twice: before, after, before, after), wall time at 40 iterations, median (range), and light-cljs's steady state:

| round | light before | light after | shadow-cljs (before / after runs) | steady state, ms per iteration | load before / after |
| --- | ---: | ---: | ---: | ---: | ---: |
| 1 | 1.954 s (1.928-1.973) | 1.876 s (1.853-1.901) | 1.398 / 1.376 s | 39.5 to 38.7 (shadow-cljs 26.6 / 26.1) | 28.9 / 26.0 |
| 2 | 2.044 s (2.019-2.160) | 2.096 s (1.975-2.314) | 1.529 / 1.441 s | 43.0 to 44.0 (shadow-cljs 30.2 / 28.0) | 22.9 / 21.2 |

The steady state is unchanged within the rounds' spread (one round 2% faster, the other 2% slower; light/shadow 1.49 and 1.43 before, 1.48 and 1.57 after), at a load four times light-cljs-1068's runs (35.8 ms there at load 5-6), where this pellet's own test suite and a peer's ran.

## 2026-11-20: count, meta and the predicates without the extended lookup (light-cljs-1070)

In a program that extends types, `count` of a string or an array, `meta` of the runtime's collections and seqs (one `-meta` slot, `metaSlot`), the predicates of 30 runtime classes (keyed masks, `classMasks`), the predicates of nil, primitives and program types, `deref` of an atom and `counted?` of a seq view answer without the extended lookup (`extendedMethod`, `storeProtocol`, `runtimeKind`, `coreProtocolImplementation`, `keyMethod`, the `satisfies` and `implements` hooks' store tests); `extend-type`, `specify!`, `satisfies?` and `implements?` keep master's answers (docs/value-layout.md, "What light-cljs-1070 landed").

**Binaries** in research/shadow-credibility/bin: before `light-cljs-c09a181b` (master), after `light-cljs-1070-b01b100d` (this pellet's last runtime commit), V8 builds; shadow-cljs 3.5.3, Node v26.12.1. Records in research/output-speed/predicates-1070-b (attempt one's in predicates-1070 overlapped its test suites and are not used). No run waited for the machine: peers' builds ran throughout, each figure gives its 1-minute load; all unreserved.

**Where the time was** (the re-frame-shaped 200k app, a `--pseudo-names` build by each binary, `node --cpu-prof --cpu-prof-interval 100 run-profile.mjs 42 1`, the samples under a frame of the pellet's list: `extendedMethod`, `storeProtocol`, `runtimeKind`, `coreProtocolImplementation`, `keyMethod`, the two hooks, `storeHolds`; 5 profiles of each alternated, load 9.6-11.1), median (range) in ms:

| | master `c09a181b` | light-cljs-1070 | after / before |
| --- | ---: | ---: | ---: |
| whole run to first output (load and first iteration) | 233.4 (224.1-240.4) | 78.7 (74.7-79.3) | 0.34 |
| first iteration alone (`run`) | 46.1 (42.9-47.4) | 21.3 (19.2-22.1) | 0.46 |

An earlier build of the same changes, profiled at load 7.3-7.9 against master: 69.7-72.5 ms against 216-229 (0.31-0.33). The pellet's 219 ms (light-cljs-1017) was the whole run; its target, a third, is 73 to 78 ms on these figures, which the whole run meets at the lower load and misses by about a millisecond at the higher.

- **What is left** (the 5 profiles, ms per run): the `builtin` lookup of the core dispatchers (`-transient`, `-conj!`, `-reduce` of runtime values), `=` (light-cljs-1074's) and a program type's `deref`, 12.8; the predicates of values whose answer the hooks still compute (`map?`, `sequential?`, `vector?`, `fn?`, `coll?`, `set?`), about 28, most of it in the hook's own body; host values' `nth`, `seq`, `first` and `reduce`, about 13; `cons` 3.0 and `bounded-count` 2.3. light-cljs-1075 holds these.
- **What did not pay:** a variant of the hooks that read each mask once and skipped the declaration demands where none is made measured no faster (71.0 against 71.9 ms medians) and changed what an effectful getter of a host object answers (the review's finding), so it was removed.

**First output and steady state** (one Node script alternating the three outputs, each run's checksum checked against the project's oracle, 8 runs each at 1 and 40 iterations; steady state is the difference of the medians over 39 iterations), median (range):

| project (load) | light-cljs master | light-cljs-1070 | shadow-cljs | light/shadow before, after |
| --- | ---: | ---: | ---: | ---: |
| re-frame-shaped 200k, first output (8.8-16.7) | 2,038 ms (1,929-2,137) | 1,809 ms (1,727-6,886) | 1,535 ms (1,479-7,794) | 1.33, 1.18 |
| re-frame-shaped 200k, steady state | 473 ms per iteration | 444 | 314 | 1.51, 1.42 |
| workload 200k, first output (15.1-16.0) | 373 ms (354-394) | 373 ms (361-383) | 345 ms (333-368) | 1.08, 1.08 |
| workload 200k, steady state | 41.1 ms per iteration | 36.1 | 27.6 | 1.49, 1.31 |

A first session with the review's earlier binary (`1070-2b676390`, load 9.6-15.4) gave the same picture: re-frame 1,981 to 1,813 ms first output (shadow-cljs 1,507) and 470 to 428 ms per iteration (306); workload 372 to 375 ms (352) and 37.6 to 33.3 ms per iteration (25.1). The workload's steady state gains where its program extends types: its iterations call the predicates and `count` that this pellet answers.

**Gate** (`bb benchmarks/core-ops/run.bb --runs 5 --ops ...`, the operations whose paths this pellet reaches, before with master and after with `--baseline`; load 9.1-9.4 before, 15.1-15.8 after), light-cljs ns per operation, median (range):

| op | light before | light after | change | shadow-cljs after | light/shadow before, after |
| --- | ---: | ---: | ---: | ---: | ---: |
| count-small | 2.12 | 2.13 (2.13-2.17) | +0% | 2.12 | 1.00, 1.00 |
| filter-count-1000 | 32,783 | 33,873 (32,396-34,816) | +3% | 32,568 | 1.03, 1.04 |
| into-xform-1000 | 29,398 | 30,837 (30,069-31,543) | +5% | 13,887 | 2.14, 2.22 |
| mapv-1000 | 14,029 | 14,199 (14,160-14,454) | +1% | 13,464 | 1.06, 1.05 |
| lazy-walk-1000 | 36,386 | 37,120 (36,497-37,630) | +2% | 40,356 | 0.93, 0.92 |
| seq-walk-vec-1000 | 19,628 | 19,797 (19,179-20,790) | +1% | 10,422 | 1.92, 1.90 |
| protocol-on-record | 2.40 | 2.40 (2.39-2.41) | +0% | 2.40 | 1.00, 1.00 |
| multimethod | 78.8 | 79.8 (75.1-81.3) | +1% | 39.1 | 2.04, 2.04 |
| variadic-call | 14.8 | 15.2 (14.9-15.5) | +3% | 9.66 | 1.56, 1.57 |
| apply-vec | 161.9 | 164.9 (164.0-168.7) | +2% | 59.5 | 2.78, 2.77 |
| swap-inc | 4.74 | 4.77 (4.71-5.00) | +1% | 5.77 | 0.84, 0.83 |
| swap-assoc | 33.4 | 21.9 (21.1-22.6) | -34% | 35.5 | 0.96, 0.62 |

- **No touched operation is slower** where before and after ran at the same load: the after pass above ran at a load 6 higher, and `into-xform-1000` (+5%), `filter-count-1000` and `variadic-call` (+3%) and `apply-vec` (+2%), rechecked in two rounds of before then after (`--runs 5` each, load 16.3-17.5), moved +0% and +0%, -1% and +1%, +1% and +0%, -1% and +1% (light-cljs 31,174 to 31,023 and 30,110 to 30,065 ns for `into-xform-1000`; 168.2 to 167.3 and 164.4 to 166.4 for `apply-vec`). The gate program extends no type, so these reach only `countedValue` (`apply`'s arguments) of this pellet's changes; `swap-assoc`'s -34% (33.4 ns in the master session, 21.2 and 21.9 in the two after sessions) is not this change: `swap!` reads an atom's `state` without `deref`, through no path this pellet changes.

**Size** (`bb benchmarks/re-frame.bb size simple todomvc flow --light-only --light BIN`, fresh ports from `shadow-port-tool-983` with the master binary, built by each binary), the whole bundle with React, gzip -9:

| App | master `c09a181b` | first attempt `1070-be661268` | light-cljs-1070 `b01b100d` | change |
| --- | ---: | ---: | ---: | ---: |
| re-frame simple | 113,485 | 114,187 | 114,153 | +668 (+0.59%) |
| TodoMVC | 144,693 | 145,384 | 145,337 | +644 (+0.45%) |
| flow | 332,032 | 332,938 | 332,978 | +946 (+0.28%) |

- **Raw:** 371,278 to 373,719 (+2,441), 478,708 to 481,337 (+2,629), 1,284,065 to 1,286,907 (+2,842).
- **Why re-frame simple grows more than 0.5%:** the keyed masks of each runtime class the app keeps (`classMasks`, two literals per class) and their `-meta` slots (`metaSlot`, one assignment per class), which upstream's classes also carry; the second attempt's steps (nil, primitives and program types in the `satisfies` hook, `hostSatisfies`, `deref` of an atom, the review's fix) are 34 to 47 bytes smaller to 40 bytes larger than the first attempt's.
- **Size ratchet:** the readable runtime grows to 765,178 bytes (`console-log-sum` at none; the first attempt's ceiling was 763,000), a program that extends types about 250 bytes at advanced (`extended-classes` 71,656), and the exceeded none, prepare and advanced ceilings move up (tests/support/ratchet_fixtures.rs).

## 2026-11-20: `=` and `hash` shaped as upstream's (light-cljs-1074)

`=` and `hash` call a value's keyed `-equiv` or `-hash` slot, read once, then upstream's path (`equalPairOf`, `hashOf`), without their slot-first wrappers (`equalPair.slots`, `hash.slots`) and the wrappers' `noBranch` comparisons, each under light-cljs-1033's 100 bytes of bytecode (71 and 52); the classes' own comparisons and hashes call their class's directly, where they asked the old path's recognisers (`structuralEqualPair`, `hashStructural`, `comparedSequential`) to tell the class again; the slotless seqs, the queue, the UUIDs and the vars are told by their keyed kind (docs/value-layout.md, "What light-cljs-1074 landed").

**Binaries** in research/shadow-credibility/bin: before `light-cljs-c09a181b` (master), after `light-cljs-1074-4ad8a055` for the timings (this pellet with the first Sol review's fixes; the second review's fixes in `d8a80401` change only the order of the fallbacks for a value without a slot and add an effect kept only where a program writes a slot by name, so the slots and the two small functions the timed operations take are the same) and `light-cljs-1074-d8a80401` for the sizes, V8 builds. Records in research/output-speed/eq-hash-1074. No run waited for the machine; all results are unreserved: the before pass ran at load 8.5-31.5 (this pellet's own builds and a peer's overlapped it), the after pass at 6.9-11.1, so operations this pellet does not touch moved too (`vec-conj-64` -27%, `multimethod` -12%); the touched operations were timed again in two alternated before/after pairs at load about 8.

**Gate** (`bb benchmarks/core-ops/run.bb --runs 5`, all 76 operations, the after run with `--baseline` the before's), light-cljs ns per operation, median (range), the operations that compare or hash:

| op | light before | light after | change | shadow-cljs after | light/shadow after |
| --- | ---: | ---: | ---: | ---: | ---: |
| eq-map-7 | 124.8 (116.5-297.1) | 134.4 (133.5-135.1) | +8% (pairs: -1%, -2%) | 150.7 | 0.89 |
| eq-vec-10 | 44.1 | 38.3 (38.2-41.1) | -13% (pairs: -15%, -16%) | 81.4 | 0.47 |
| hash-fresh-vec-4 | 58.0 | 47.2 (46.2-47.8) | -19% | 116.0 | 0.41 |
| hash-fresh-map-7 | 140.3 | 116.6 (114.3-118.6) | -17% (pairs: -18%, -18%) | 653.8 | 0.18 |
| get-small | 11.4 | 11.1 (11.0-11.1) | -3% | 14.9 | 0.74 |
| get-hash100 | 13.4 | 11.6 (11.6-12.9) | -13% | 12.1 | 0.96 |
| kw-small | 7.93 | 7.72 (7.66-7.75) | -3% | 9.84 | 0.78 |
| kw-hash100 | 16.6 | 13.0 (12.8-13.1) | -22% | 13.6 | 0.95 |
| assoc-small-existing | 25.9 | 22.4 (22.0-22.9) | -14% | 29.5 | 0.76 |
| assoc-small-new | 50.9 | 48.7 (47.6-49.5) | -4% | 39.5 | 1.23 |
| assoc-hash100 | 95.3 | 83.6 (83.1-90.9) | -12% | 79.8 | 1.05 |
| group-by-100 | 8,890 | 8,802 (8,691-8,838) | -1% | 5,120 | 1.72 |
| into-set-100 | 7,726 | 6,857 (6,805-8,116) | -11% | 4,304 | 1.59 |

- **No touched operation is more than 5% slower.** `eq-map-7` is the one operation of the 76 slower in the full passes (+8%), where the before pass's range was 116.5 to 297.1 ns; in two alternated pairs of the touched operations alone (`--ops eq-map-7,eq-vec-10,hash-fresh-map-7`, light-cljs 92.0 to 90.9 and 92.5 to 90.9 ns) it is 1% and 2% faster. Its comparison is the array map's slot (`mapEquiv`) as before, now without the wrapper's kind test and `noBranch` comparison, and its values' `=` takes the identity test first.
- **What gains:** hashing a fresh vector or map (-17 to -19%: `hash` of their numbers and keywords reaches `hashOf`'s primitive test or the keyword's slot without the wrapper and the old body's second dispatch), `=` of vectors (-13 to -16%), and the hash map lookups and updates whose keys `hash` (`kw-hash100` -22%, `get-hash100` -13%, `assoc-hash100` -12%), less certainly given the passes' loads.

**Size** (`bb benchmarks/re-frame.bb size simple todomvc flow --light-only --light BIN`, fresh ports from `shadow-port-tool-983` with the `c09a181b` binary, built by each binary), the whole bundle with React, gzip -9:

| App | master `c09a181b` | light-cljs-1074 (`d8a80401`) | change |
| --- | ---: | ---: | ---: |
| re-frame simple | 113,485 | 113,401 | -84 (-0.07%) |
| TodoMVC | 144,693 | 144,527 | -166 (-0.11%) |
| flow | 332,166 | 332,248 | +82 (+0.02%) |

- **Raw:** 371,278 to 371,241 (-37), 478,708 to 478,789 (+81), 1,284,063 to 1,284,137 (+74). The wrappers, the old bodies and their recognisers leave every release slice (flow's runtime: `hashStructural` 1,354 bytes, `structuralEqualPair` and its symbols' wrapper 461, the two wrappers 413, `comparedSequential` and `cons.compared` 150), but the classes' hashes and comparisons they held move into the slots (`mapHash` 460, `setHash` 165, `primitiveHash` 254) and the upstream-shaped paths (`equalPairOf` about 460, `hashOf` about 650) take their place. A first version saved 68 to 121 bytes after gzip per app; the Sol reviews' fixes (UUIDs and vars told by their kind so that `extend-type js/Object` still reaches them, `.equiv` of an identical slotless seq comparing its items, the old path's order in the fallbacks) cost about that again, so flow, which keeps UUIDs, vars and most seq classes, grows 82 bytes. Most of the size ratchet's programs that compare or hash, which keep little more of the runtime than that, shrink 30 to 1,170 bytes (`keyword-case` 5,364 to 4,192); `atom-str` and `map-printed` grow 14 and 41, the `.equiv` fix's cost.

**Workload 200k** (`compare.bb PROJECT --runs 3 --runtime-only --runtime 1,40`, two copies of `workload-200k-seed42`, `-1074b` built by the before binary and `-1074` by the after, run in turn twice: before, after, before, after), wall time at 40 iterations, median (range), and light-cljs's steady state:

| round | light before | light after | shadow-cljs (before / after runs) | steady state, ms per iteration | load before / after |
| --- | ---: | ---: | ---: | ---: | ---: |
| 1 | 1.783 s (1.742-1.787) | 1.722 s (1.713-1.761) | 1.279 / 1.272 s | 36.4 to 35.1 (shadow-cljs 24.4 / 24.4) | 8.5 / 7.8 |
| 2 | 1.724 s (1.697-1.744) | 1.765 s (1.762-1.771) | 1.266 / 1.265 s | 35.1 to 36.5 (shadow-cljs 24.2 / 24.6) | 7.4 / 6.5 |

The steady state is unchanged within the rounds' spread (one round 4% faster, the other 4% slower; light/shadow 1.49 and 1.45 before, 1.44 and 1.49 after).

## 2026-11-20: `into` without a lookup per call (light-cljs-1067)

The vector, array map, hash map and hash set classes have `-as-transient` and `-with-meta` slots (`asTransientSlot`, `withMetaSlot`); `transient` and `-with-meta` call a value's slot first; `into` reads its target's `IEditableCollection` mask bit, as upstream's `implements?` does, and is one function per arity (docs/value-layout.md, "What light-cljs-1067 landed"). In a program that implements protocols (the gate's records make it one), each `into` of a small collection took three protocol lookups (`-as-transient`'s dispatcher, `-with-meta`'s support test and its own lookup) and takes none.

**Binaries** in research/shadow-credibility/bin: before `light-cljs-8903e693` (master), after `light-cljs-1067-a0a85250` (this pellet's runtime on `8903e693`; the rebase onto light-cljs-1074 changed none of its code), V8 builds; shadow-cljs 3.5.3, Node v26.12.1. Records in research/output-speed/into-slots-1067. No run waited for the machine: a peer worker built and tested during the first gate pass; each figure gives its 1-minute load; all unreserved.

**Gate** (`bb benchmarks/core-ops/run.bb --runs 5`). Pass 1 is all 78 operations, before then after with `--baseline` (load 5.3-9.5 before, 4.2-4.8 after); pass 2 is the operations whose paths this pellet reaches, after, before, after (load 4.1-4.5, 4.6-5.0, 5.4-5.9). light-cljs ns per operation, medians:

| op | pass 1 before | pass 1 after | change | pass 2 before | pass 2 after (two runs) | change | shadow-cljs (pass 2) | light/shadow before, after |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| into-vec-small | 300.7 | 194.2 | -35% | 278.0 | 170.1, 170.7 | -39% | 160.4 | 1.77, 1.06 |
| into-map-small | 951.2 | 792.7 | -17% | 898.6 | 731.9, 732.4 | -19%, -18% | 328.1 | 2.75, 2.23 |
| into-vec-keys-small | 816.7 | 686.5 | -16% | 668.4 | 584.5, 588.5 | -13%, -12% | 318.2 | 2.12, 1.85 |
| into-set-100 | 7,335 | 7,006 | -4% | 6,520 | 6,363, 6,328 | -2%, -3% | 3,951 | 1.64, 1.60 |
| into-xform-1000 | 29,594 | 28,981 | -2% | 29,530 | 29,228, 29,260 | -1%, -1% | 13,750 | 2.15, 2.13 |
| transient-vec-100 | 1,185 | 1,138 | -4% | 1,192 | 1,114, 1,123 | -7%, -6% | 1,213 | 0.99, 0.93 |
| transient-map-100 | 5,763 | 5,722 | -1% | 5,160 | 5,098, 5,094 | -1%, -1% | 3,679 | 1.39, 1.38 |
| mapv-1000 | 14,270 | 14,112 | -1% | 14,000 | 13,953, 14,051 | -0%, +0% | 13,198 | 1.06, 1.06 |
| select-keys-3 | 252.9 | 226.1 | -11% | 240.9 | 222.9, 216.1 | -7%, -10% | 167.5 | 1.44, 1.29 |
| group-by-100 | 6,399 | 6,375 | -0% | 5,954 | 5,920, 6,084 | -1%, +2% | 4,447 | 1.34, 1.37 |
| merge-small | 2,124 | 2,100 | -1% | 1,917 | 1,818, 1,858 | -5%, -3% | 684.1 | 2.82, 2.72 |
| sort-by-100 | 7,444 | 7,368 | -1% | 5,310 | 5,336, 5,337 | +0%, +1% | 3,489 | 1.53, 1.53 |
| filter-count-1000 | 34,030 | 34,230 | +1% | 33,110 | 33,081, 33,191 | -0%, +0% | 31,994 | 1.04, 1.04 |

- **No operation of the 78 is more than 1% slower** in pass 1 (`dissoc-small`, `filter-count-1000`, `kw-hash100`, `swap-inc` and `vec-nth-1024` +1%); in pass 2 the largest move up is `group-by-100`'s +2% in one after run, -1% in the other.
- **The new operations** `into-vec-small` (`(into [] v)` of vectors of 2 to 8 items) and `into-map-small` (`(into {} m)` of the 7-key maps), added after light-cljs-1073's so that no earlier operation's number moves, are the per-call cost: a small vector's `into` is within 6% of shadow-cljs's (1.77 before). `into-map-small` stays at 2.2 times shadow-cljs's: its readable profile (`--pseudo-names into-map-small`, research/output-speed/into-slots-1067/profile-into-map-small) has its time in each entry's `conj!` onto the map transient (`arrayIndex` 16%, `kindHooks.assocTransientMap` 14%, the entry's `nth` 10%, `editableTransient` 3%), where shadow-cljs's is in `array-index-of` and `-assoc!`, and 2.5% in `into2`'s own steps.
- **Other gains:** `transient` of a vector reads its slot (`transient-vec-100` -4 to -7%), and `select-keys-3` (-7 to -11%) and `merge-small` (-1 to -5%) give their results metadata through `collectionWithMeta`, whose lookup now finds the class's `-with-meta` slot, which answers a value already holding the metadata at once, where the old path first tested eight stores.

**Recheck with the final binary** `light-cljs-1067-f33cd4e3` (the review's fix, `withMetaSlot` reading `meta` once, rebased onto light-cljs-1074) against master `light-cljs-b86a231e`, the same operations but `transient-map-100`, `merge-small`, `sort-by-100` and `filter-count-1000`, run final, master, final (load 12-23, a peer's suites running), light-cljs ns per operation:

| op | master | final (two runs) | change | shadow-cljs | light/shadow master, final |
| --- | ---: | ---: | ---: | ---: | ---: |
| into-vec-small | 207.2 | 124.3, 124.2 | -40% | 110.0 | 1.88, 1.13 |
| into-map-small | 526.4 | 389.8, 387.6 | -26% | 272.6 | 1.93, 1.42 |
| into-vec-keys-small | 499.8 | 411.2, 415.4 | -18%, -17% | 191.8 | 2.61, 2.15 |
| into-set-100 | 5,121 | 4,925, 5,018 | -4%, -2% | 3,189 | 1.61, 1.55 |
| into-xform-1000 | 30,141 | 29,887, 30,301 | -1%, +1% | 14,228 | 2.12, 2.11 |
| transient-vec-100 | 1,202 | 1,161, 1,162 | -3%, -3% | 1,075 | 1.12, 1.08 |
| select-keys-3 | 184.3 | 177.9, 180.2 | -3%, -2% | 144.4 | 1.28, 1.24 |
| mapv-1000 | 14,111 | 13,957, 14,035 | -1%, -1% | 13,380 | 1.05, 1.04 |
| group-by-100 | 5,972 | 6,149, 6,192 | +3%, +4% | 4,361 | 1.37, 1.41 |

- `group-by-100` calls none of the changed code (`groupBy` is `reduce`, `assoc`, `conj` and `get`); alone in its process (`--ops group-by-100`, master, final, master, final, load 11.5-13.0) it measured 6,867 to 6,792 (-1%) and 6,892 to 6,998 (+2%), so the +3 to +4% beside the `into` operations is their feedback in the shared process, as light-cljs-1060 found for `swap-assoc`. An earlier final-then-master attempt at load 9.5-16.3, whose shadow-cljs numbers rose 6 to 9% between its two runs, is not used.
- `into-map-small` and `into-vec-keys-small` gain more here than against `8903e693` in percent, not in time: master `b86a231e` (with light-cljs-1074's `=` and `hash`) runs them faster than `8903e693` did (`into-map-small` 526 against 899 to 951 ns), and the per-call time this pellet removes is about the same (137 ns here, 158 to 167 there).

**Workload 200k** (`compare.bb PROJECT --runs 3 --runtime-only --runtime 1,40`, two copies of `workload-200k-seed42`, `-1067b` built by the before binary and `-1067` by the after, run before, after, before, after), wall time at 40 iterations, median (range), and light-cljs's steady state:

| round | light before | light after | shadow-cljs (before / after runs) | steady state, ms per iteration | load before / after |
| --- | ---: | ---: | ---: | ---: | ---: |
| 1 | 1.751 s (1.738-1.811) | 1.748 s (1.688-1.761) | 1.368 / 1.413 s | 35.6 to 35.6 (shadow-cljs 26.6 / 27.5) | 5.2-5.3 / 5.5-5.7 |
| 2 | 1.749 s (1.738-1.750) | 1.687 s (1.665-1.699) | 1.369 / 1.344 s | 35.7 to 34.2 (shadow-cljs 26.4 / 26.0) | 5.0-5.5 / 4.8 |

The steady state is unchanged in one round and 4% faster in the other (light/shadow 1.34 and 1.35 before, 1.29 and 1.31 after); the workload's `into` calls are a small share of its iterations.

**Size** (`bb benchmarks/re-frame.bb size simple todomvc flow --light-only --light BIN`, fresh ports from `shadow-port-tool-983` with the before binary, built by each binary), the whole bundle with React, gzip -9:

| App | master `8903e693` | light-cljs-1067 (`a0a85250`) | change |
| --- | ---: | ---: | ---: |
| re-frame simple | 114,153 | 114,362 | +209 (+0.18%) |
| TodoMVC | 145,337 | 145,610 | +273 (+0.19%) |
| flow | 332,995 | 333,121 | +126 (+0.04%) |

- **Raw:** 373,719 to 374,597 (+878), 481,337 to 482,215 (+878), 1,286,907 to 1,287,796 (+889): the four classes' slot assignments a program keeps, the two slots, `transient`'s and `-with-meta`'s slot-first entries and `into`'s functions per arity (tests/support/ratchet_fixtures.rs has the advanced breakdown for `mapv-count`, +700 bytes).
- **Size ratchet:** the readable runtime grows 5,482 bytes (every none and prepare ceiling moves to the next 1,000 above); `mapv-count`, `features` and `features-direct` move to the next 100 above their sizes, and the features example's shared chunk to 95,200.

## 2026-11-20: host values' and program types' predicates and the remaining lookups (light-cljs-1075)

In a program that extends types, `coll?`, `seq?`, `sequential?`, `vector?`, `map?`, `set?`, `chunked-seq?` and `cons`'s and `nth`'s `implements?` of `ISeq` read the protocol's mask bit by its keyed name before the hooks, which go on from the marker where it is clear; `deref` of a program type calls its `-deref` slot; `empty?` reads `ICounted`'s bit and calls `-count`'s slot; `builtin` answers a runtime class's value whose slot holds its class's method and whose class declares the protocol by its mask bit without the external lookup's native test; the lookup reads the kind once, answers an array or a string past its marker where its caller takes only a direct method, answers a class's value whose mask leaves out a `kinds` protocol, reads the slot before a native test that differs within a class, and asks a function its host tables without `satisfies` again (docs/value-layout.md, "What light-cljs-1075 landed"). `extend-type`, `specify!`, `satisfies?` and `implements?` keep master's answers; values made by reflection change (section 5, item 37).

**Binaries** in research/shadow-credibility/bin: before `light-cljs-b86a231e` (master, light-cljs-1074), after `light-cljs-1075-97606841` (this pellet on `b86a231e`, before the Sol review's fixes) for the timings and the gate; the final commit `light-cljs-1075-8965cfeb` (rebased onto light-cljs-1067's `86dc4aa0`, with the review's fixes) against `light-cljs-86dc4aa0` for the final profiles and the sizes. The fixes take the `builtin` shortcut only where a value's slot holds a runtime method (so `name` of a keyword takes the external lookup again) and give `empty?` a front; V8 builds; shadow-cljs 3.5.3, Node v26.12.1. Records in research/output-speed/predicates-1075. No run waited for the machine: peers' test suites and another project's builds ran throughout, each figure gives its 1-minute load; all unreserved.

**Where the time was** (the re-frame-shaped 200k app, a `--pseudo-names` build by each binary, `node --cpu-prof --cpu-prof-interval 100 run-profile.mjs 42 1`, the samples under a frame of light-cljs-1070's list: `extendedMethod`, `storeProtocol`, `runtimeKind`, `coreProtocolImplementation`, `keyMethod`, the two hooks, `storeHolds`; 5 profiles of each alternated, load 12.1-13.6, this pellet's own builds overlapping the first two rounds), median (range) in ms:

| | master `b86a231e` | light-cljs-1075 | after / before |
| --- | ---: | ---: | ---: |
| whole run to first output (load and first iteration) | 85.7 (79.6-97.6) | 60.7 (59.8-74.8) | 0.71 |
| first iteration alone (`run`) | 22.5 (21.6-25.3) | 16.8 (13.8-20.6) | 0.75 |

The final commit against master `86dc4aa0`, 5 profiles of each alternated at load 4.4-4.8: whole run 81.3 (74.8-88.0) to 53.1 (51.4-54.0) ms, 0.65; first iteration 21.7 (20.7-25.2) to 14.8 (13.6-18.2), 0.68 (profiles-86dc4aa0-summary.log; against `b86a231e` at load 39-64, 83.0 to 52.3).

Earlier sessions of the same method with this pellet's steps as they landed gave 83.0 against 64.9 (3 rounds, load 15-18) and 81.3 against 58.5 (3 rounds, load 13-23); the target, under 73, holds in every session's median, and one profile of the five above (74.8, a load spike that also slowed its whole run to 2.95 s) is over it.

- **What was left** on master: the hooks' own bodies (28 and 11 ms of self time), over a million calls from the predicates, three in five answered at their first test, the mask bit; `builtin` 15 ms (`=` of numbers, light-cljs-1074's, 4.7; `deref` of a program type 3.2; `empty?` of the queue; `name` of a keyword; `with-meta`, `conj` and `nth` of runtime values through the external lookup's native test); `first` of a vector, `seq` of an array and `nth` of an indexed seq through the lookup's store test and native test.
- **What is left:** `=` of numbers and `compare` of strings through the external lookup (about 4 ms, light-cljs-1074's `=` among them; a host table answer cached per protocol and type was not taken, see docs/value-layout.md); the predicates' misses on program types and host values, which still take the hooks (`isSequential` and `isMap` of re-frame's reactions, `fn?` of vectors and strings, about 12 ms); `bounded-count` of `apply`'s arguments and of lazy seqs (3 to 4.5).

**First output and steady state** (one Node script alternating the three outputs, each run's checksum checked against the project's oracle, 8 runs each at 1 and 40 iterations; steady state is the difference of the medians over 39 iterations), median (range):

| project (session, load) | light-cljs master | light-cljs-1075 | shadow-cljs | light/shadow before, after |
| --- | ---: | ---: | ---: | ---: |
| re-frame-shaped 200k, first output (1, 10.5-18) | 1,908 ms (1,779-3,335) | 1,915 ms (1,747-3,374) | 1,708 ms (1,455-3,736) | 1.12, 1.12 |
| re-frame-shaped 200k, steady state (1) | 463 ms per iteration | 483 | 327 | 1.42, 1.48 |
| re-frame-shaped 200k, first output (2, 13.5-16.8) | 2,050 ms (1,971-3,178) | 2,008 ms (1,849-2,105) | 1,647 ms (1,610-1,932) | 1.24, 1.22 |
| re-frame-shaped 200k, steady state (2) | 476 ms per iteration | 469 | (523; its 40-iteration runs 14.6-32.9 s) | |
| workload 200k, first output (1, 10.5-15.9) | 373 ms (367-391) | 376 ms (365-405) | 346 ms (341-511) | 1.08, 1.09 |
| workload 200k, steady state (1) | 37.0 ms per iteration | 36.7 | 28.3 | 1.31, 1.30 |

The re-frame app's first output and steady state are unchanged within their spread: its 40-iteration runs ranged 18.2 to 26.6 s in the first session, and the two sessions moved the steady state +4% and -1%. The layers' 25 ms of a 1.9 s first output and 6 ms of a 470 ms iteration are below these sessions' resolution.

**Gate** (`bb benchmarks/core-ops/run.bb --runs 5 --ops ...`, the operations whose paths this pellet reaches, before with master and after with `--baseline`; load 9.3-11.2), light-cljs ns per operation, median (range):

| op | light before | light after | change | shadow-cljs after | light/shadow before, after |
| --- | ---: | ---: | ---: | ---: | ---: |
| count-small | 2.17 | 2.15 (2.14-2.23) | -1% | 2.15 | 1.01, 1.00 |
| vec-nth-1024 | 4.75 | 4.60 (4.48-4.98) | -3% | 5.22 | 0.91, 0.88 |
| destructure-vec-3 | 5.51 | 5.49 (5.48-5.53) | +0% | 9.91 | 0.56, 0.55 |
| destructure-map-3 | 35.0 | 34.6 (34.2-34.7) | -1% | 35.7 | 1.01, 0.97 |
| filter-count-1000 | 34,584 | 33,420 (32,889-34,362) | -3% | 32,454 | 1.06, 1.03 |
| mapv-1000 | 14,437 | 14,601 (14,340-14,818) | +1% | 12,727 | 1.14, 1.15 |
| lazy-walk-1000 | 37,562 | 37,379 (37,136-38,708) | +0% | 41,676 | 0.90, 0.90 |
| seq-walk-vec-1000 | 20,472 | 20,316 (19,683-20,790) | -1% | 10,424 | 1.97, 1.95 |
| reduce-range-1000 | 4,929 | 4,908 (4,878-4,998) | +0% | 5,520 | 0.88, 0.89 |
| str-nested-map | 4,104 | 4,188 (4,075-4,269) | +2% | 2,324 | 1.74, 1.80 |
| protocol-on-record | 2.40 | 2.41 (2.39-2.60) | +0% | 2.42 | 1.00, 1.00 |
| multimethod | 64.2 | 68.2 (64.9-68.5) | +6% | 39.8 | 1.58, 1.71 |
| variadic-call | 15.7 | 15.9 (15.6-16.1) | +1% | 10.0 | 1.53, 1.58 |
| apply-vec | 181.5 | 169.4 (168.2-171.3) | -7% | 77.3 | 2.31, 2.19 |
| swap-inc | 4.84 | 4.84 (4.82-4.94) | +0% | 5.74 | 0.85, 0.84 |
| swap-assoc | 22.2 | 22.1 (21.8-22.5) | +0% | 36.1 | 0.60, 0.61 |
| group-by-100 | 6,244 | 6,188 (6,161-6,195) | -1% | 4,212 | 1.48, 1.47 |
| sort-by-100 | 5,120 | 5,076 (5,002-5,837) | -1% | 4,818 | 1.05, 1.05 |

- **No touched operation is slower** where before and after ran in turn: `multimethod` (+6%), `str-nested-map` (+2%), `mapv-1000` and `variadic-call` (+1%), rechecked in two pairs of before then after (`--runs 5` each), moved +0% (68.5 to 68.8 ns), +1%, -1% and +0% in the pair at load 18.4-20.1; the other pair's after pass ran into a load spike (ranges up to five times the medians) and is not used (gate-recheck.log). The gate program extends no type, so it reaches the fronts of the predicates, `cons`, `concat` and `nth` and `countSlot`, not the external lookup's or the lookup's steps; `apply-vec`'s -7% is likely `cons`'s front, whose tail, `apply`'s arguments, declares `ISeq` by its bit.

**Workload 200k** steady state: 37.0 to 36.7 ms per iteration (the first-output session above; shadow-cljs 28.3), unchanged within its spread.

**Size** (`bb benchmarks/re-frame.bb size simple todomvc flow --light-only --light BIN`, fresh ports from `shadow-port-tool-983` with the master binary, built by each binary), the whole bundle with React, gzip -9:

| App | master `86dc4aa0` | light-cljs-1075 `8965cfeb` | change |
| --- | ---: | ---: | ---: |
| re-frame simple | 114,271 | 114,496 | +225 (+0.20%) |
| TodoMVC | 145,538 | 145,780 | +242 (+0.17%) |
| flow | 333,311 | 333,414 | +103 (+0.03%) |

- **Raw:** 374,578 to 375,604 (+1,026), 482,313 to 483,343 (+1,030), 1,287,891 to 1,288,928 (+1,037): the fronts of the predicates and `empty?` the apps keep, `countSlot`, `implementsSeq` and the lookups' steps; no per-class code. Before the review's fixes, on `b86a231e`: +263, +121 and +187 after gzip.
- **Size ratchet** (the final commit, with the review's fixes): the readable runtime grows 6,422 bytes on `86dc4aa0` (every none and prepare ceiling moves to the next 1,000 above); advanced programs that keep a front grow 121 to 395 bytes and `extended-classes` 566 (tests/support/ratchet_fixtures.rs).

## 2026-11-20: `into` onto other collections through `-conj` (light-cljs-1076)

`into` without a transducer conjoins each item onto a target that is no editable collection (a sorted map or set, a list or `()`, a queue, a subvector, a map entry, a record, a program type) through the target's `-conj` slot (`conjOwn`), as upstream's `into` calls `-conj`, where `-conj`'s dispatcher (`conjProtocol`) looked it up per item; and a map's `-conj` of a vector or a map entry reads the entry's items through their `-nth` slots (`entryNth`), where `-nth`'s dispatcher looked each up (docs/value-layout.md, "What light-cljs-1076 landed"). In a program that implements protocols (the gate's records make it one), each item onto those targets took one protocol lookup, three onto a sorted map, and takes none.

**Binaries** in research/shadow-credibility/bin: before `light-cljs-86dc4aa0` (master), after `light-cljs-1076-86e2b4b3` (this pellet's first runtime on `86dc4aa0`) and the final `light-cljs-1076-5111cbc9` (with the Sol review's fix, which changes only the paths of a slot that holds no method, `conjRead` and `entryNthRead`), V8 builds; shadow-cljs 3.5.3, Node v26.12.1. Records in research/output-speed/into-conj-1076. No run waited for the machine; each figure gives its 1-minute load; all unreserved.

**New gate operations** `into-sorted-map-small` (`(into (sorted-map) m)` of the 7-key maps) and `into-list-small` (`(into () v)` of vectors of 2 to 8 items), added after light-cljs-1067's, before the loop baselines, so that no earlier operation's number moves (80 operations). The sorted operations before them build their data with a transducer, which already called `conj`, so the gate did not time this path.

**Gate** (`bb benchmarks/core-ops/run.bb --runs 5`). Pass 1 is all 80 operations, before then after with `--baseline`, during a peer's load of 18 to 608 (before 18-572, after 18-608): no operation is slower than +2% (`get-small-miss`; `bit-and` and `aget-bit-and` +0%), and its gains on operations this pellet does not reach (`into-xform-1000` -26%, `vec-conj-64` -24%) are that load, not the change. Pass 2 is the operations whose paths it reaches, after, before, after (load 4.3-4.8, 13.7-15.5, 5.2-15.5); light-cljs ns per operation, medians:

| op | pass 1 before | pass 1 after | pass 2 before | pass 2 after (two runs) | change | shadow-cljs (pass 2 before) | light/shadow before, after |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| into-sorted-map-small | 7,796 | 2,853 | 3,996 | 2,298, 2,278 | -42%, -43% | 784.1 | 5.10, 2.88 |
| into-list-small | 788.4 | 196.6 | 442.5 | 122.8, 124.4 | -72%, -72% | 117.5 | 3.77, 1.05 |
| merge-small | 2,779 | 1,681 | 1,749 | 1,427, 1,532 | -18%, -12% | 673.7 | 2.60, 2.25 |
| into-map-small | 1,170 | 809.9 | 683.4 | 524.2, 525.9 | -23%, -23% | 279.6 | 2.44, 1.87 |
| into-set-100 | 7,024 | 6,664 | 6,506 | 5,397, 5,406 | -17%, -17% | 3,622 | 1.80, 1.48 |
| into-vec-small | 250.4 | 194.4 | 184.9 | 175.8, 179.4 | -5%, -3% | 124.8 | 1.48, 1.42 |
| into-vec-keys-small | 1,311 | 682.7 | 406.2 | 410.4, 414.9 | +1%, +2% | 314.8 | 1.29, 1.29 |
| into-xform-1000 | 40,222 | 29,709 | 30,348 | 30,059, 30,768 | -1%, +1% | 14,033 | 2.16, 2.16 |
| sorted-map-assoc-100 | 193.5 | 158.6 | 125.0 | 124.1, 129.0 | -1%, +3% | 202.8 | 0.62, 0.61 |
| str-sorted-map-20 | 39,453 | 32,660 | 30,660 | 30,077, 30,168 | -2%, -2% | 18,841 | 1.63, 1.58 |
| vec-conj-64 | 1,771 | 1,352 | 1,831 | 1,703, 1,714 | -7%, -6% | 1,675 | 1.09, 1.02 |
| group-by-100 | 7,564 | 6,195 | 6,141 | 6,119, 6,204 | -0%, +1% | 4,339 | 1.42, 1.41 |

- **The path's own operations:** `into` of a small vector onto `()` is now within 5% of shadow-cljs's (3.77 times before), and onto a sorted map 2.9 times (5.1): its remaining time is the tree map's own insertion of keyword keys.
- **`merge-small`** (-12 to -18%) conjoins a map onto a map, which reads each entry's items through `entryNth`.
- **`into-map-small` and `into-set-100`** (-17 to -24% here and in the recheck) take none of the changed code (`into` onto an editable target): alone in their process (`--ops into-map-small,into-set-100`, master then final, load 6.3-7.2) they measure 375.1 to 384.1 ns (+2%) and 4,892 to 4,862 (-1%), so their gain beside the other operations is the shared process's type feedback (the reductions' function no longer `conjProtocol`'s variadic dispatcher), as light-cljs-1067 found for `group-by-100`; it is not claimed. `sorted-map-assoc-100`'s +3% in one after run (-1% in the other) and `into-vec-keys-small`'s +1 to +2% are within their runs' spread.

**Recheck with the final binary** `light-cljs-1076-5111cbc9` against master `light-cljs-86dc4aa0`, the same operations, final, master, final (load 4.8-6.0, 5.9-10.4, 10.6-26.2), light-cljs ns per operation:

| op | master | final (two runs) | change | shadow-cljs (master run) | light/shadow master, final |
| --- | ---: | ---: | ---: | ---: | ---: |
| into-sorted-map-small | 3,957 | 2,269, 2,280 | -43%, -42% | 766.2 | 5.16, 2.96 |
| into-list-small | 435.1 | 123.7, 121.4 | -72%, -72% | 116.6 | 3.73, 1.06 |
| merge-small | 1,652 | 1,439, 1,382 | -13%, -16% | 666.9 | 2.48, 2.16 |
| into-map-small | 676.7 | 520.6, 516.5 | -23%, -24% | 275.5 | 2.46, 1.89 |
| into-set-100 | 6,519 | 5,343, 5,268 | -18%, -19% | 3,547 | 1.84, 1.51 |
| into-vec-small | 182.2 | 176.1, 176.1 | -3%, -3% | 122.8 | 1.48, 1.43 |
| into-vec-keys-small | 402.1 | 410.6, 407.8 | +2%, +1% | 310.6 | 1.29, 1.32 |
| into-xform-1000 | 30,203 | 29,804, 29,419 | -1%, -3% | 13,905 | 2.17, 2.14 |
| sorted-map-assoc-100 | 124.4 | 123.7, 124.7 | -1%, +0% | 199.1 | 0.62, 0.62 |
| str-sorted-map-20 | 30,424 | 30,086, 29,723 | -1%, -2% | 18,464 | 1.65, 1.63 |
| vec-conj-64 | 1,655 | 1,676, 1,672 | +1%, +1% | 1,600 | 1.03, 1.05 |
| group-by-100 | 6,260 | 6,081, 6,057 | -3%, -3% | 4,313 | 1.45, 1.41 |

No touched operation is more than 2% slower (`into-vec-keys-small`, +1 to +2%, as in pass 2).

**Workload 200k** (`compare.bb PROJECT --runs 3 --runtime-only --runtime 1,40`, two copies of `workload-200k-seed42`, `-1076b` built by the before binary and `-1076` by the after, run before, after, before, after), wall time at 40 iterations, median (range), and light-cljs's steady state:

| round | light before | light after | shadow-cljs (before / after runs) | steady state, ms per iteration | load before / after |
| --- | ---: | ---: | ---: | ---: | ---: |
| 1 | 1.799 s (1.797-1.807) | 1.810 s (1.805-1.930) | 1.673 / 1.511 s | 36.2 to 36.6 (shadow-cljs 33.4 / 29.5) | 14.0-15.5 / 14.9-15.9 |
| 2 | 2.058 s (1.748-2.081) | 1.818 s (1.719-1.868) | 1.528 / 1.562 s | 42.9 to 36.5 (shadow-cljs 30.1 / 30.2) | 15.3 / 16.1-16.3 |

The steady state is unchanged within its spread (round 2's before run had one fast and two slow runs); the workload's `into` calls are onto vectors and maps.

**Size** (`bb benchmarks/re-frame.bb size simple todomvc flow --light-only --light BIN`, fresh ports from `shadow-port-tool-983` with the before binary, built by each binary), the whole bundle with React, gzip -9:

| App | master `86dc4aa0` | first runtime (`86e2b4b3`) | final (`5111cbc9`) | change |
| --- | ---: | ---: | ---: | ---: |
| re-frame simple | 114,271 | 114,321 | 114,418 | +147 (+0.13%) |
| TodoMVC | 145,538 | 145,613 | 145,692 | +154 (+0.11%) |
| flow | 333,239 | 333,248 | 333,356 | +117 (+0.04%) |

- **Raw (final):** 374,578 to 375,235 (+657), 482,313 to 482,970 (+657), 1,287,891 to 1,288,565 (+674): `conjOwn` and `conjRead` (347 bytes at advanced in `mapv-count`), `entryNth` and `entryNthRead`, the review's fix adding the last and moving `conjRead`'s finish to every value.
- **Rebased final** (`light-cljs-1076-26d51570`, with the second and third reviews' fixes, against master `light-cljs-d87d84a1`, light-cljs-1075's, fresh ports with the master binary): re-frame simple 114,496 to 114,678 (+182, +0.16%), TodoMVC 145,780 to 145,934 (+154, +0.11%), flow 333,444 to 333,743 (+299, +0.09%) after gzip; raw +720, +720 and +737.
- **Size ratchet** (rebased onto light-cljs-1075's `d87d84a1`): the readable runtime grows 4,028 bytes with the second review's fixes (every none and prepare ceiling moves to the next 1,000 above); `mapv-count` grows 375 bytes (ceiling 54,400), `features` and `features-direct` 811 (ceilings 120,100 and 119,700), and the features example's shared chunk 814 (ceiling 96,200) (tests/support/ratchet_fixtures.rs).

## 2026-11-21: the fallbacks' shared steps (light-cljs-1078)

The upstream-shaped paths behind the operations' slot-first fronts (`countOf`, `nthOf` and the others) share their cold steps: one function reads a runtime class's slot and then the protocol lookup (`fallbackMethod`), one throws their errors (`unsupported`), and the own paths that a compiler session sets are put in front of them by effects that an `Advanced` slice does not keep, where sixteen of them tested those paths themselves (docs/value-layout.md, "What light-cljs-1078 landed"). The fronts and the call-site entries are unchanged. The sizes are in benchmarks/re-frame-vs-shadow.md.

**Binaries:** before `light-cljs-688b1f03` (master; its compiler sources are master `15fc27ba`'s), after `light-cljs-1078-e8ed2ca0` (this pellet on `15fc27ba`) and the final `light-cljs-1078-6b2eabef` (with the Sol review's fix: `reduceOf` reads the kind once again), V8 builds; shadow-cljs 3.5.3, Node v26.12.1. No run waited for the machine; all unreserved.

**Gate** (`bb benchmarks/core-ops/run.bb --runs 5 --ops ...`, the operations whose cold paths it reaches: nil and host values, lazy seqs and ranges, `conj` and `assoc` onto nil, printing, before then after with `--baseline`; load 5.3-9.2 before, 4.3-5.7 after), light-cljs ns per operation, median (range):

| op | light before | light after | change | shadow-cljs after | light/shadow before, after |
| --- | ---: | ---: | ---: | ---: | ---: |
| count-small | 2.12 | 2.11 (2.11-2.12) | +0% | 2.12 | 1.01, 1.00 |
| dissoc-small | 45.7 | 45.5 (45.1-47.4) | +0% | 82.8 | 0.56, 0.55 |
| get-in-3 | 50.8 | 50.9 (50.6-50.9) | +0% | 48.1 | 1.03, 1.06 |
| assoc-in-3 | 94.6 | 94.6 (93.8-95.9) | +0% | 90.1 | 1.06, 1.05 |
| update-in-3 | 115.3 | 114.9 (114.4-116.9) | +0% | 100.5 | 1.15, 1.14 |
| destructure-map-3 | 31.8 | 31.8 (31.6-32.4) | +0% | 34.5 | 0.92, 0.92 |
| filter-count-1000 | 32,860 | 32,595 (32,005-32,766) | -1% | 30,390 | 1.07, 1.07 |
| keys-small | 298.4 | 297.8 (294.4-303.2) | +0% | 74.4 | 4.00, 4.00 |
| lazy-walk-1000 | 43,254 | 42,894 (42,799-43,343) | -1% | 40,964 | 1.05, 1.05 |
| seq-walk-vec-1000 | 27,382 | 27,059 (26,956-27,538) | -1% | 10,378 | 2.63, 2.61 |
| reduce-range-1000 | 4,901 | 4,837 (4,790-4,939) | -1% | 5,253 | 0.94, 0.92 |
| str-nested-map | 4,108 | 4,118 (4,087-4,156) | +0% | 2,367 | 1.75, 1.74 |
| eq-map-7 | 70.7 | 70.7 (70.2-71.2) | +0% | 100.2 | 0.70, 0.71 |
| hash-fresh-map-7 | 77.3 | 76.6 (76.2-76.9) | -1% | 651.8 | 0.12, 0.12 |
| multimethod | 67.2 | 67.3 (66.3-67.4) | +0% | 38.9 | 1.75, 1.73 |
| variadic-call | 10.3 | 10.3 (10.2-10.3) | +0% | 9.48 | 1.08, 1.09 |
| apply-vec | 178.3 | 176.4 (175.2-178.4) | -1% | 74.6 | 2.37, 2.37 |
| merge-small | 1,493 | 1,487 (1,470-1,507) | +0% | 778.1 | 1.92, 1.91 |
| select-keys-3 | 210.0 | 208.7 (190.8-210.9) | -1% | 232.2 | 0.89, 0.90 |
| group-by-100 | 6,040 | 6,028 (5,792-6,094) | +0% | 4,283 | 1.40, 1.41 |
| sort-by-100 | 6,374 | 6,364 (6,339-6,464) | +0% | 4,766 | 1.35, 1.34 |
| str-sorted-map-20 | 31,297 | 31,626 (31,547-31,872) | +1% | 20,855 | 1.51, 1.52 |
| vec-pop | 23.7 | 23.8 (23.7-23.8) | +0% | 24.5 | 0.97, 0.97 |
| list-pop | 28.5 | 28.6 (28.4-30.0) | +0% | 12.9 | 2.19, 2.22 |
| disj-set-7 | 116.0 | 112.8 (111.0-116.9) | -3% | 151.4 | 0.77, 0.74 |
| into-list-small | 152.0 | 152.6 (151.5-157.2) | +0% | 109.1 | 1.39, 1.40 |

No touched operation is slower than +1% (`str-sorted-map-20`, 31,297 to 31,626, whose runs overlap: 31,233-31,760 before) or faster than -3% (`disj-set-7`, 111.9-116.9 before, 111.0-116.9 after): the steps that moved are on the cold paths, and the fronts and call-site entries, which the gate's operations mostly take, are unchanged.

**Workload 200k** (`compare.bb workload-200k-seed42 --runs 5 --runtime-only --runtime 1,40`, the project's light-cljs output rebuilt with each binary just before its run, every checksum equal to the oracle's; load 6.99-7.29 before, 7.90-8.22 after), median wall time (range):

| iterations | light before | light after | shadow-cljs (before run, after run) |
| --- | ---: | ---: | ---: |
| 1 | 0.361 s (0.358-0.404) | 0.363 s (0.356-0.369) | 0.343 s, 0.342 s |
| 40 | 1.689 s (1.676-1.696) | 1.684 s (1.674-1.705) | 1.405 s, 1.401 s |

Steady state, from the medians at 1 and 40 iterations: light-cljs 34.0 to 33.9 ms per iteration, shadow-cljs 27.2 in both runs (light/shadow 1.25, 1.25): unchanged.

**Gate pass 2** with the final binary `light-cljs-1078-6b2eabef` (the Sol review asked for the operations whose fallbacks call the shared step, `nth`, `peek` and `reduce-kv`, with ranges; and the reductions, whose `reduceOf` the fix changed), before then after with `--baseline` (load 7.0-7.4 before, 7.5-8.1 after), light-cljs ns per operation, median (range):

| op | light before | light after | change | shadow-cljs after | light/shadow before, after |
| --- | ---: | ---: | ---: | ---: | ---: |
| destructure-vec-3 | 5.46 (5.45-5.49) | 5.44 (5.41-5.46) | +0% | 9.84 | 0.56, 0.55 |
| disj-set-7 | 62.2 (61.9-63.0) | 62.4 (61.9-62.7) | +0% | 114.2 | 0.54, 0.55 |
| filter-count-1000 | 26,373 (26,218-26,585) | 26,217 (25,785-26,471) | -1% | 28,067 | 0.93, 0.93 |
| reduce-entries-small | 83.8 (83.3-86.2) | 83.2 (83.0-84.4) | -1% | 85.7 | 0.97, 0.97 |
| reduce-kv-small | 6.48 (6.44-6.57) | 6.43 (6.36-6.51) | -1% | 38.0 | 0.17, 0.17 |
| reduce-map-inc-1000 | 34,410 (33,763-34,530) | 34,044 (33,810-34,603) | -1% | 42,672 | 0.81, 0.80 |
| reduce-range-1000 | 4,876 (4,840-4,881) | 4,853 (4,785-4,995) | +0% | 5,472 | 0.90, 0.89 |
| reduce-vec-1000 | 5,197 (5,147-5,317) | 5,211 (5,142-5,254) | +0% | 2,509 | 2.07, 2.08 |
| str-sorted-map-20 | 30,851 (30,718-31,074) | 30,800 (30,596-31,219) | +0% | 20,169 | 1.52, 1.53 |
| vec-nth-1024 | 4.54 (4.48-4.67) | 4.56 (4.55-4.59) | +0% | 5.15 | 0.87, 0.89 |
| vec-peek | 3.65 (3.64-3.66) | 3.64 (3.61-3.65) | +0% | 3.49 | 1.05, 1.04 |

No operation moves more than 1% either way. Pass 1's operations ran with `light-cljs-1078-e8ed2ca0`; the fix after it touches only `reduceOf`, whose operations pass 2 measures. Workload 200k (above) also ran with `e8ed2ca0`, before the fix, which changes only `reduceOf`'s cold path back to master's text.

## 2026-11-21: the store tests by kind (light-cljs-1079)

Whether a value is one of the runtime's own (`ownedKind`) and the store inventories that were store tests (`seqableValue`, `collectionValue`, `metaValue`, `withMetaValue`) and `fnValue` test the value's keyed kind against fixed bits, where a table of stores by kind (`kindTable`, `storeHolds`, `storesPredicate`, `ownedStores`) asked the class's store; the extended lookup answers a runtime class's mask bit for the protocols it caches by kind (docs/value-layout.md, "What light-cljs-1079 landed"). The sizes are in benchmarks/re-frame-vs-shadow.md.

**Binaries:** before `light-cljs-1078-6b2eabef` (its compiler sources are master `3f9c2446`'s), after `light-cljs-1079-12301e59`, V8 builds; shadow-cljs 3.5.3, Node v26.12.1. No run waited for the machine; all unreserved. Raw results: `research/output-speed/store-tests-1079`.

**Gate** (`bb benchmarks/core-ops/run.bb --runs 5 --ops ...`; the gate's program extends no type, so its operations reach the change through printing, `meta` and the predicates' native tests: printing, seq walks, records, `apply`, `merge`, `select-keys`, `group-by`, `sort-by`, `into`; before then after with `--baseline`; load 5.2-6.4 before, 6.0-7.1 after), light-cljs ns per operation, median (range):

| op | light before | light after | change | shadow-cljs after | light/shadow before, after |
| --- | ---: | ---: | ---: | ---: | ---: |
| count-small | 2.13 | 2.11 (2.11-2.14) | -1% | 2.10 | 1.00, 1.00 |
| vec-conj-64 | 1,636 | 1,449 (1,444-1,657) | -11% | 1,469 | 1.08, 0.99 |
| destructure-map-3 | 34.6 | 34.3 (34.1-34.5) | -1% | 34.5 | 1.00, 0.99 |
| filter-count-1000 | 30,178 | 29,610 (28,633-30,398) | -2% | 30,872 | 0.98, 0.96 |
| mapv-1000 | 14,091 | 14,126 (13,960-14,188) | +0% | 12,288 | 1.14, 1.15 |
| keys-small | 299.4 | 301.0 (296.0-304.7) | +1% | 75.4 | 3.95, 3.99 |
| lazy-walk-1000 | 39,097 | 39,627 (39,156-41,261) | +1% | 41,127 | 0.93, 0.96 |
| seq-walk-vec-1000 | 27,319 | 27,507 (27,060-27,887) | +1% | 10,061 | 2.66, 2.73 |
| str-nested-map | 4,078 | 4,109 (4,076-4,143) | +1% | 2,362 | 1.71, 1.74 |
| str-concat | 168.1 | 169.3 (166.2-171.4) | +1% | 14.9 | 11.21, 11.39 |
| protocol-on-record | 2.40 | 2.39 (2.37-2.46) | +0% | 2.39 | 1.00, 1.00 |
| record-assoc | 50.4 | 49.9 (49.8-50.2) | -1% | 10.4 | 4.80, 4.82 |
| multimethod | 71.0 | 71.3 (70.7-71.4) | +1% | 39.8 | 1.79, 1.79 |
| variadic-call | 14.6 | 14.7 (14.5-14.7) | +0% | 9.74 | 1.52, 1.51 |
| apply-vec | 179.1 | 179.4 (177.2-181.0) | +0% | 75.6 | 2.37, 2.37 |
| swap-inc | 4.83 | 4.74 (4.71-4.87) | -2% | 5.67 | 0.85, 0.84 |
| merge-small | 1,496 | 1,472 (1,415-1,543) | -2% | 726.9 | 2.06, 2.03 |
| select-keys-3 | 211.0 | 215.2 (211.9-216.5) | +2% | 227.1 | 0.93, 0.95 |
| group-by-100 | 6,062 | 6,049 (5,962-6,106) | +0% | 4,273 | 1.40, 1.42 |
| sort-by-100 | 6,350 | 6,424 (6,300-6,558) | +1% | 4,676 | 1.35, 1.37 |
| str-sorted-map-20 | 32,032 | 31,931 (31,598-33,134) | +0% | 20,501 | 1.55, 1.56 |
| into-vec-small | 148.3 | 147.3 (144.8-149.7) | -1% | 129.1 | 1.15, 1.14 |
| into-map-small | 421.8 | 427.6 (419.5-434.7) | +1% | 277.6 | 1.52, 1.54 |
| into-list-small | 157.6 | 158.5 (156.4-159.0) | +1% | 111.2 | 1.40, 1.43 |

No touched operation is more than 2% slower (`select-keys-3`, 211.0 to 215.2, whose runs overlap: 209.2-215.8 before). `vec-conj-64`'s -11% is noise: its runs overlap too (1,440-1,679 before, 1,444-1,657 after).

**Predicates in a program that extends types** (the gate's program extends none, so the hooks' and the extended lookup's paths are timed by a small program of this pellet's, `research/output-speed/store-tests-1079/predbench`: an `extend-type` of a program type and of `object`, then each operation over 18 values (vectors, maps, lists, sets, lazy seqs, ranges, a keyword, an atom, a host object, a string, nil, a program type's value, a seq, a cons, a volatile, a delay and a uuid) in turn; built by each binary at `:advanced`, 5 runs of each alternated, every checksum equal; load 5.4-5.7), ns per call, median (range):

| op | before | after | change |
| --- | ---: | ---: | ---: |
| `(satisfies? ISeqable x)` | 66.2 (64.8-67.1) | 61.3 (59.8-64.6) | -7% |
| `(satisfies? IMeta x)` | 69.4 (66.4-72.6) | 63.7 (63.1-68.1) | -8% |
| `seqable?` | 40.1 (38.8-40.5) | 38.4 (38.1-44.4) | -4% |
| `coll?` | 29.7 (28.8-29.8) | 27.2 (26.8-27.4) | -8% |
| `counted?` | 75.8 (74.5-80.2) | 71.6 (67.5-73.7) | -6% |
| `fn?` | 36.8 (36.0-38.9) | 34.5 (33.6-35.1) | -6% |
| `meta` | 88.1 (86.9-91.8) | 87.3 (86.5-89.7) | -1% |
| `pr-str` (of the first seven) | 1,110 (1,086-1,139) | 1,104 (1,091-1,122) | -1% |

The store test was a call to the class's store's `has`; the kind test is arithmetic on the kind the hooks have already read.

**Workload 200k** (`compare.bb workload-200k-seed42-1079 --runs 5 --runtime-only --runtime 1,40`, a copy of the project whose light-cljs output was rebuilt with each binary just before its run, every checksum equal to the oracle's; load 5.8-6.0 before, 6.0-6.6 after), median wall time (range):

| iterations | light before | light after | shadow-cljs (before run, after run) |
| --- | ---: | ---: | ---: |
| 1 | 0.353 s (0.343-0.357) | 0.350 s (0.346-0.353) | 0.330 s, 0.331 s |
| 40 | 1.585 s (1.557-1.597) | 1.552 s (1.543-1.560) | 1.346 s, 1.331 s |

Steady state, from the medians at 1 and 40 iterations: light-cljs 31.6 to 30.8 ms per iteration, shadow-cljs 26.1 and 25.6 (light/shadow 1.21, 1.20).

**Pass 2 with the final binary** `light-cljs-1079-53b509ee` (the Sol review's fixes: six native tests read their class's mask bit, `copyWithMeta`'s probes test the kind), the same operations, before then after with `--baseline` (load 4.1-4.7 before, 3.8-4.5 after), light-cljs ns per operation, median (range):

| op | light before | light after | change | shadow-cljs after | light/shadow before, after |
| --- | ---: | ---: | ---: | ---: | ---: |
| count-small | 2.09 | 2.10 (2.08-2.11) | +0% | 2.09 | 0.99, 1.00 |
| vec-conj-64 | 1,659 | 1,440 (1,425-1,641) | -13% | 1,470 | 1.12, 0.98 |
| destructure-map-3 | 34.2 | 34.3 (33.9-34.3) | +0% | 34.5 | 0.99, 0.99 |
| filter-count-1000 | 29,970 | 29,926 (29,600-30,347) | +0% | 30,507 | 0.98, 0.98 |
| mapv-1000 | 13,924 | 14,104 (14,014-14,176) | +1% | 12,161 | 1.13, 1.16 |
| keys-small | 296.2 | 297.6 (293.8-302.3) | +0% | 74.4 | 3.96, 4.00 |
| lazy-walk-1000 | 38,726 | 38,733 (38,453-39,186) | +0% | 40,892 | 0.95, 0.95 |
| seq-walk-vec-1000 | 26,987 | 26,929 (26,816-27,126) | +0% | 10,056 | 2.66, 2.68 |
| str-nested-map | 4,051 | 4,063 (4,048-4,118) | +0% | 2,346 | 1.73, 1.73 |
| str-concat | 166.7 | 166.8 (165.7-168.1) | +0% | 14.9 | 11.26, 11.23 |
| protocol-on-record | 2.35 | 2.34 (2.32-2.41) | +0% | 2.36 | 1.00, 0.99 |
| record-assoc | 49.1 | 49.0 (49.0-50.4) | +0% | 10.0 | 4.89, 4.89 |
| multimethod | 66.4 | 70.6 (66.4-71.3) | +6% | 39.5 | 1.69, 1.79 |
| variadic-call | 14.4 | 14.4 (14.3-14.5) | +0% | 9.70 | 1.52, 1.49 |
| apply-vec | 179.6 | 178.9 (175.9-183.1) | +0% | 74.8 | 2.42, 2.39 |
| swap-inc | 4.76 | 4.70 (4.69-4.76) | -1% | 5.62 | 0.86, 0.84 |
| merge-small | 1,416 | 1,407 (1,392-1,438) | -1% | 716.5 | 1.97, 1.96 |
| select-keys-3 | 213.4 | 212.4 (194.0-214.3) | +0% | 223.8 | 0.95, 0.95 |
| group-by-100 | 5,941 | 5,946 (5,851-6,113) | +0% | 4,235 | 1.39, 1.40 |
| sort-by-100 | 6,317 | 6,177 (6,140-6,305) | -2% | 4,679 | 1.36, 1.32 |
| str-sorted-map-20 | 31,808 | 31,754 (31,387-32,330) | +0% | 20,471 | 1.55, 1.55 |
| into-vec-small | 146.7 | 144.6 (142.7-147.7) | -1% | 127.6 | 1.15, 1.13 |
| into-map-small | 429.8 | 426.4 (394.3-430.6) | -1% | 274.4 | 1.57, 1.55 |
| into-list-small | 156.4 | 156.5 (155.2-159.9) | +0% | 110.5 | 1.41, 1.42 |

`multimethod`'s +6% is noise: its runs overlap (65.1-70.4 before, 66.4-71.3 after), and timed alone with `protocol-on-record` in two more alternated pairs (`mm-before1`, `mm-after1`, `mm-before2`, `mm-after2`) it was 4% and 8% faster (46.1 to 44.2 and 48.7 to 44.7 ns; that program's smaller set of operations times it lower). `vec-conj-64`'s -13% overlaps too (1,432-1,674 before, 1,425-1,641 after). Every other operation moves -2% to +1%.

**Predicates, pass 2** (the final binary, the same program; load 3.7-4.1), ns per call, median (range): `(satisfies? ISeqable x)` 66.5 (65.9-72.3) to 60.6 (58.3-63.5), -9%; `(satisfies? IMeta x)` 67.6 to 62.7, -7%; `seqable?` 39.8 to 38.0, -4%; `coll?` 29.4 to 26.6, -9%; `counted?` 76.1 to 72.0, -5%; `fn?` 36.8 to 34.3, -7%; `meta` 87.8 to 85.8, -2%; `pr-str` 1,090 to 1,083, -1%.

**Workload 200k, pass 2** (the final binary, its own copy of the project rebuilt by each binary before its run; load 3.7-4.1 before, 3.8-3.9 after): 40 iterations 1.524 s (1.515-1.548) before, 1.530 s (1.523-1.537) after; 1 iteration 0.345 s and 0.336 s. Steady state 30.6 to 30.9 ms per iteration (+1%, within the runs' spread; shadow-cljs 25.1 and 25.5 in the two runs, light/shadow 1.22 and 1.21). With pass 1's 31.6 to 30.8, the change is within the workload's run-to-run spread both ways.

## 2026-11-21: each upstream member by its own name (light-cljs-1077)

An `Advanced` program keeps each upstream member of the runtime's types (`cnt`, `first`, `meta`, ...) and each static but `EMPTY` only where it names that member, where naming any one kept every type's fields (docs/value-layout.md, "What light-cljs-1077 landed"). No operation's code changes: what a program without the members it does not name loses is the accessors on the classes' prototypes. The sizes are in benchmarks/re-frame-vs-shadow.md.

**Binaries:** before `light-cljs-1079-53b509ee` (its compiler sources are master `4af741fb`'s), after `light-cljs-1077-0a0b5a28`, V8 builds; shadow-cljs 3.5.3, Node v26.12.1. No run waited for the machine; all unreserved. Raw results: `research/output-speed/class-code-1077`.

**Gate** (`bb benchmarks/core-ops/run.bb --runs 5`, all 80 operations, the after run with `--baseline` the before's; load 3.48-4.36 before, 3.49-4.81 after). The gate's program names one member, `all`, so before it kept every class's fields and now none: every operation on a runtime value reaches classes whose prototypes lost their accessors. No operation is more than 3% slower but one:
- **`keys-hash100` +12%** (3,396 to 3,803 ns), whose runs overlap (3,356-3,793 before, 3,617-3,851 after) and which earlier pellets' runs show as two-valued (3,761-7,634 in light-cljs-1070's). Timed alone in two alternated pairs (`--ops keys-hash100,vals-hash100,keys-small`, load 3.72-4.16) it is 2,923 to 2,926 and 2,911 to 2,916 ns, +0% both times; `vals-hash100` +3% and -4%, `keys-small` +0% and +1%.
- The rest are within -3% (`vec-nth-1024`) and +2% (`destructure-vec-3`, `multi-arity-call`, `str-sorted-map-20`).

**Workload 200k** (`compare.bb workload-200k-seed42-1077 --runs 5 --runtime-only --runtime 1,40`, a copy of the project whose light-cljs output was rebuilt with each binary just before its run, every checksum equal to the oracle's; two rounds, before then after; load 3.51-4.62), steady state from the medians at 1 and 40 iterations: light-cljs 30.1 to 30.2 and 29.9 to 30.2 ms per iteration, shadow-cljs 24.5 to 25.5 (light/shadow 1.17 to 1.23), within the spread.

## 2026-11-21: the gate after the class conversions (light-cljs-1023)

The review that the conversion pellets' per-pellet thresholds moved to: the full core-ops gate on master once every conversion and size pellet had landed, every operation above shadow-cljs's median profiled, two small causes fixed here (`str`, vector literals), one operation improved (`group-by`), and the causes left filed, one pellet per cause, as light-cljs-1080 to light-cljs-1087 (more causes than eight were found; the merged ones are named in light-cljs-1085, light-cljs-1086 and light-cljs-1087).

**Binaries:** master `light-cljs-ed53a283` (light-cljs-1077 landed), `light-cljs-1023-1d906703` (this pellet's fixes but the last `group-by` step, for the second full pass), `light-cljs-1023-24d518ff` (all of them), and `light-cljs-f533efb1` (master before the conversions, whose compiler is `6ceeb23b`'s, light-cljs-1007's table's); V8 builds; shadow-cljs 3.5.3, Node v26.12.1. No run waited for the machine; every run is unreserved, and peers' builds and runs loaded it throughout. Raw results, the profiles and the map-literal prototype: `research/output-speed/gate-review-1023`.

### The full gate on master

`bb benchmarks/core-ops/run.bb --runs 5 --baseline` light-cljs-1007's `master-6ceeb23b.edn`, all 80 operations on `light-cljs-ed53a283`, 5 runs per compiler alternated, load 5.3-13.8. Medians over the runs with their ranges, ns per operation. Status: "at or below" shadow-cljs's median; "parity" above it but within the run-to-run ranges (light-cljs's lowest run at or under shadow-cljs's highest); otherwise the fix here or the pellet filed for it. 41 operations are at or below, 9 at parity, and 30 above it: the 28 whose ranges are apart and `record-kw-field` and `merge-small`, whose ranges meet only through one slow shadow-cljs run.

| op | light-cljs ns/op | range | shadow-cljs ns/op | range | light/shadow | status |
| --- | ---: | ---: | ---: | ---: | ---: | --- |
| kw-small | 7.69 | 7.56-7.77 | 9.87 | 9.75-9.93 | 0.78 | at or below |
| get-small | 11.0 | 11.0-11.2 | 14.9 | 14.9-14.9 | 0.74 | at or below |
| get-small-miss | 15.6 | 15.3-15.7 | 19.3 | 19.2-19.3 | 0.81 | at or below |
| contains-small | 9.39 | 9.26-9.44 | 12.1 | 12.1-12.2 | 0.78 | at or below |
| count-small | 2.12 | 2.11-2.14 | 2.13 | 2.10-2.13 | 1.00 | at or below |
| assoc-small-existing | 17.1 | 16.9-17.4 | 29.4 | 29.2-30.0 | 0.58 | at or below |
| assoc-small-new | 40.4 | 40.0-41.4 | 39.6 | 39.1-40.1 | 1.02 | parity (ranges overlap) |
| dissoc-small | 46.8 | 45.9-48.3 | 57.7 | 57.0-58.8 | 0.81 | at or below |
| update-small | 27.1 | 26.5-27.7 | 44.1 | 44.0-44.6 | 0.61 | at or below |
| map-literal-3 | 21.0 | 20.5-24.9 | 10.4 | 10.0-10.5 | 2.02 | light-cljs-1080 |
| get-hash100 | 11.7 | 11.5-11.7 | 12.0 | 11.9-12.3 | 0.97 | at or below |
| kw-hash100 | 12.9 | 12.9-14.5 | 13.5 | 13.5-13.7 | 0.96 | at or below |
| assoc-hash100 | 74.3 | 73.7-76.0 | 80.1 | 79.2-81.0 | 0.93 | at or below |
| dissoc-hash100 | 79.2 | 77.6-79.9 | 77.9 | 77.3-78.7 | 1.02 | parity (ranges overlap); just above them in the second pass: light-cljs-1087 |
| update-hash100 | 85.0 | 84.5-86.1 | 97.4 | 95.9-98.5 | 0.87 | at or below |
| get-in-3 | 64.3 | 63.7-66.7 | 58.5 | 57.3-59.4 | 1.10 | light-cljs-1087 |
| assoc-in-3 | 136.1 | 135.5-137.7 | 134.0 | 129.8-137.6 | 1.02 | parity (ranges overlap) |
| update-in-3 | 162.0 | 148.2-166.7 | 139.5 | 137.8-143.1 | 1.16 | light-cljs-1087 |
| vec-conj-64 | 1,330 | 1,310-1,540 | 1,330 | 1,288-1,343 | 1.00 | parity (ranges overlap) |
| vec-literal-4 | 21.1 | 20.5-21.6 | 6.87 | 6.77-7.15 | 3.07 | fixed here |
| vec-nth-1024 | 4.56 | 4.49-4.65 | 5.17 | 5.15-5.26 | 0.88 | at or below |
| vec-invoke-1024 | 5.12 | 5.06-5.13 | 4.92 | 4.80-5.13 | 1.04 | parity (ranges overlap); just above them in the second pass: light-cljs-1087 |
| vec-peek | 3.66 | 3.58-3.68 | 3.48 | 3.46-3.50 | 1.05 | light-cljs-1087 |
| vec-assoc-1024 | 53.9 | 53.5-55.1 | 74.8 | 74.3-75.6 | 0.72 | at or below |
| destructure-vec-3 | 5.45 | 5.43-5.49 | 9.84 | 9.83-9.97 | 0.55 | at or below |
| destructure-map-3 | 50.9 | 50.8-54.0 | 49.4 | 49.1-49.5 | 1.03 | light-cljs-1087 |
| reduce-vec-1000 | 5,224 | 5,205-5,280 | 2,508 | 2,491-2,515 | 2.08 | light-cljs-1084 |
| reduce-map-inc-1000 | 38,754 | 38,293-41,417 | 41,543 | 40,375-42,714 | 0.93 | at or below |
| filter-count-1000 | 31,788 | 31,392-32,226 | 29,022 | 28,836-30,401 | 1.10 | light-cljs-1087 |
| into-xform-1000 | 29,247 | 28,789-30,380 | 18,755 | 18,466-18,785 | 1.56 | light-cljs-1084 |
| mapv-1000 | 14,065 | 13,967-14,511 | 14,516 | 14,350-14,671 | 0.97 | at or below |
| keys-small | 284.5 | 279.4-288.1 | 77.0 | 76.1-80.3 | 3.69 | light-cljs-1081 |
| vals-small | 292.2 | 289.3-295.5 | 79.2 | 78.5-79.3 | 3.69 | light-cljs-1081 |
| keys-hash100 | 3,743 | 3,485-3,934 | 5,065 | 5,053-5,172 | 0.74 | at or below |
| vals-hash100 | 3,964 | 3,817-4,079 | 5,083 | 5,045-5,147 | 0.78 | at or below |
| lazy-walk-1000 | 36,294 | 36,050-36,915 | 50,802 | 50,337-51,835 | 0.71 | at or below |
| seq-walk-vec-1000 | 25,143 | 24,959-25,901 | 19,675 | 19,534-20,120 | 1.28 | light-cljs-1087 |
| reduce-range-1000 | 4,844 | 4,759-4,891 | 5,476 | 5,474-5,548 | 0.88 | at or below |
| reduce-kv-small | 6.46 | 6.44-6.53 | 37.9 | 37.6-38.2 | 0.17 | at or below |
| reduce-entries-small | 82.0 | 81.4-84.3 | 78.9 | 76.4-81.0 | 1.04 | light-cljs-1087 |
| str-nested-map | 4,242 | 4,224-4,251 | 2,376 | 2,332-2,390 | 1.79 | light-cljs-1086 |
| str-concat | 167.7 | 166.0-169.8 | 14.6 | 14.4-14.7 | 11.49 | fixed here |
| eq-map-7 | 134.5 | 133.3-135.8 | 151.2 | 151.0-152.9 | 0.89 | at or below |
| eq-vec-10 | 39.0 | 38.8-39.8 | 83.1 | 81.2-83.4 | 0.47 | at or below |
| hash-fresh-vec-4 | 47.0 | 46.6-47.3 | 116.9 | 115.8-228.2 | 0.40 | at or below |
| hash-fresh-map-7 | 107.0 | 106.0-107.9 | 656.6 | 652.9-1,722 | 0.16 | at or below |
| protocol-on-record | 2.38 | 2.37-2.46 | 2.40 | 2.39-3.64 | 0.99 | at or below |
| record-kw-field | 5.78 | 5.76-5.81 | 4.21 | 4.18-9.54 | 1.37 | light-cljs-1082 (shadow-cljs's range holds one slow run) |
| record-dot-field | 0.88 | 0.87-0.88 | 0.88 | 0.87-2.99 | 1.00 | at or below |
| record-assoc | 54.2 | 53.1-54.6 | 14.1 | 13.8-33.8 | 3.86 | light-cljs-1082 |
| multimethod | 111.3 | 110.2-111.5 | 48.4 | 47.5-82.0 | 2.30 | light-cljs-1083 |
| multi-arity-call | 2.02 | 1.99-2.03 | 2.00 | 1.98-8.87 | 1.01 | parity (ranges overlap) |
| variadic-call | 14.6 | 14.6-15.0 | 14.5 | 14.2-19.5 | 1.01 | parity (ranges overlap) |
| apply-vec | 181.2 | 179.7-185.5 | 93.5 | 90.1-103.9 | 1.94 | light-cljs-1084 |
| unknown-fn-call | 4.50 | 4.46-4.56 | 6.50 | 6.49-17.9 | 0.69 | at or below |
| closure-alloc-call | 9.28 | 9.17-9.53 | 8.48 | 8.39-20.5 | 1.09 | parity (shadow-cljs's range holds one slow run); above in the second pass: light-cljs-1087 |
| swap-inc | 4.77 | 4.73-4.93 | 5.64 | 5.60-12.5 | 0.85 | at or below |
| swap-assoc | 27.4 | 25.3-28.2 | 37.8 | 37.5-193.7 | 0.73 | at or below |
| merge-small | 1,653 | 1,606-1,726 | 906.8 | 896.4-2,520 | 1.82 | light-cljs-1085 (shadow-cljs's range holds one slow run) |
| select-keys-3 | 242.9 | 231.6-245.9 | 255.8 | 251.5-280.2 | 0.95 | at or below |
| group-by-100 | 6,081 | 6,051-6,365 | 5,115 | 5,085-5,699 | 1.19 | improved here (1.11x to 1.19x after); light-cljs-1085 |
| sort-by-100 | 7,326 | 7,012-7,446 | 4,796 | 4,723-5,448 | 1.53 | light-cljs-1084 |
| transient-vec-100 | 1,152 | 1,146-1,165 | 1,161 | 1,151-1,299 | 0.99 | at or below |
| transient-map-100 | 4,705 | 4,641-5,027 | 4,559 | 4,186-4,833 | 1.03 | parity (ranges overlap); just above them in the second pass: light-cljs-1087 |
| sorted-map-assoc-100 | 157.7 | 155.9-162.3 | 211.0 | 209.5-236.5 | 0.75 | at or below |
| str-sorted-map-20 | 33,052 | 32,493-34,469 | 20,811 | 20,663-22,477 | 1.59 | light-cljs-1086 |
| into-vec-keys-small | 684.3 | 675.4-704.6 | 337.1 | 329.0-339.4 | 2.03 | light-cljs-1081 |
| into-set-100 | 6,650 | 6,638-6,770 | 4,385 | 4,288-4,490 | 1.52 | light-cljs-1085 |
| vec-pop | 24.1 | 23.8-24.3 | 24.6 | 24.3-24.9 | 0.98 | at or below |
| list-pop | 28.9 | 28.6-29.1 | 14.9 | 14.9-15.0 | 1.93 | light-cljs-1087 |
| disj-set-7 | 118.0 | 111.6-118.5 | 157.7 | 157.2-159.8 | 0.75 | at or below |
| disj-sorted-set-100 | 205.1 | 203.7-213.9 | 227.5 | 223.6-228.5 | 0.90 | at or below |
| disj-variadic-set-7 | 205.0 | 203.7-205.9 | 302.4 | 299.7-308.8 | 0.68 | at or below |
| vec-assoc-variadic-1024 | 173.0 | 170.0-173.3 | 223.4 | 221.4-223.9 | 0.77 | at or below |
| into-vec-small | 194.2 | 192.2-195.6 | 158.5 | 157.5-162.1 | 1.23 | light-cljs-1085 |
| into-map-small | 797.5 | 788.6-807.7 | 335.7 | 329.7-336.7 | 2.38 | light-cljs-1085 |
| into-sorted-map-small | 2,785 | 2,766-2,803 | 843.9 | 832.2-845.7 | 3.30 | light-cljs-1086 |
| into-list-small | 193.7 | 193.1-194.2 | 123.9 | 122.2-125.4 | 1.56 | light-cljs-1084 |
| bit-and | 0.59 | 0.58-0.60 | 0.59 | 0.58-0.60 | 1.00 | at or below |
| aget-bit-and | 0.60 | 0.59-0.60 | 0.60 | 0.59-0.61 | 1.00 | at or below |

### What this pellet fixed and improved

- **`str`** at `:advanced` is its operands' texts joined with `+`, as upstream's `str` macro writes it: a string as it is, a number or boolean as a template (`` `${i}` ``, which V8 converts faster than `+` of a string and a number), any other operand through `rt$strOne`, the runtime's one-value `str`. An operand evaluated after one whose text may run code (a `toString`, a printing) must be a pure operand, or the call stays the runtime's `str`, which evaluates every operand first; that `str` is now a loop over `strOne` where it mapped a closure over a rest array and joined. `str-concat` was 15% slower on master than on `6ceeb23b` (145.9 to 167.7 ns, the only operation of light-cljs-1007's table that got slower by more than 1%): `str` asked `sets.has` and `maps.has` of every number, which read its keyed kind.
- **Vector literals** of at most 32 items built at their site are `rt$vectorOfTail([...])`, a vector of the new array as its tail, as upstream's `PersistentVector` of a tail, where `vectorFromArray` copied the array and `vectorTrie` walked it with a default-parameter closure. Pooled constant vectors keep `vectorFromArray`.
- **`group-by`** gathers its groups in a transient map, as upstream's does, where each item made a new persistent map, and makes an empty vector only for a new group: 1.19x to 1.11x-1.19x; the rest, the transient `assoc!`'s layers, is light-cljs-1085's.

Before and after for the operations these reach, light-cljs ns per operation, from the two full passes (master, then `light-cljs-1023-1d906703`, load 8.8-14.1, `gate-1d906703.md`):

| op | light-cljs master | light-cljs after (range) | change | shadow-cljs master, after | light/shadow master, after |
| --- | ---: | ---: | ---: | ---: | ---: |
| str-concat | 167.7 | 13.3 (13.1-13.4) | -92% | 14.6, 15.1 | 11.49, 0.88 |
| str-nested-map | 4,242 | 3,990 (3,952-4,028) | -6% | 2,376, 2,395 | 1.79, 1.67 |
| str-sorted-map-20 | 33,052 | 31,858 (31,565-33,753) | -4% | 20,811, 20,837 | 1.59, 1.53 |
| vec-literal-4 | 21.1 | 6.32 (6.26-6.43) | -70% | 6.87, 6.86 | 3.07, 0.92 |
| hash-fresh-vec-4 | 47.0 | 30.1 (29.8-30.3) | -36% | 116.9, 118.1 | 0.40, 0.26 |
| group-by-100 | 6,081 | 5,745 (5,548-6,155) | -6% | 5,115, 5,173 | 1.19, 1.11 |

The same operations with the final binary, in a selection of their own (`--ops str-nested-map,str-concat,str-sorted-map-20,vec-literal-4,hash-fresh-vec-4,group-by-100,merge-small,multimethod --runs 5`, master then `light-cljs-1023-24d518ff` with `--baseline`, two alternated pairs, load 10.1-22.8; `touched-before1.md` to `touched-after2.md`), light-cljs ns per operation, before and after, and light/shadow:

| op | pair 1 | pair 2 | light/shadow before, after |
| --- | ---: | ---: | ---: |
| str-concat | 140.6 to 14.2 (-90%) | 145.5 to 14.0 (-90%) | 8.88-9.25, 0.89-0.90 |
| vec-literal-4 | 25.4 to 10.1 (-60%) | 30.6 to 10.2 (-67%) | 2.37-2.91, 0.96-0.98 |
| hash-fresh-vec-4 | 48.6 to 25.8 (-47%) | 49.2 to 26.1 (-47%) | 0.93, 0.50-0.51 |
| group-by-100 | 6,206 to 4,887 (-21%) | 6,290 to 5,212 (-17%) | 1.40, 1.11-1.19 |
| str-nested-map | 3,914 to 3,813 (-3%) | 3,914 to 3,801 (-3%) | 1.60-1.61, 1.57 |
| str-sorted-map-20 | 31,027 to 30,307 (-2%) | 32,260 to 30,029 (-7%) | 1.46-1.49, 1.45-1.47 |
| multimethod | 70.3 to 53.9 (-23%) | 72.1 to 53.9 (-25%) | 1.89-1.95, 1.42-1.45 |
| merge-small | 1,529 to 1,578 (+3%) | 1,496 to 1,597 (+7%) | 2.04-2.10, 2.19-2.24 |

`str-concat` and `vec-literal-4` are below shadow-cljs's medians. `group-by-100` is 1.11x and 1.19x after (its after ranges 4,730-4,969 and 4,687-6,267 against shadow-cljs's 4,306-4,461 and 4,352-4,518): its remaining cause, the transient `assoc!`'s layers, is light-cljs-1085's, where it is filed with its profile on this binary (`profile-group-by-24d518ff.txt`). `str` of a collection gains only the call's overhead; printing itself is light-cljs-1086's. `multimethod` gains 23-25% here and 20% in the full pass, though no change reaches its path; `merge-small` loses 3-7% in both selections but, timed alone in two more alternated pairs (`--ops merge-small`), it is 1,385 to 1,398 and 1,374 to 1,390 ns (+1%, within the ranges): both move with the type feedback the changed operations leave, as `--ops` selections show (README, "Order matters").

The other operations of the second full pass are within -15% and +6% of master's: `destructure-map-3` -15%, `filter-count-1000` -8% and `multimethod` -20%, which none of the fixes reaches, run after `str-concat` and the vector literals in `benches`, whose type feedback changed; `merge-small` +6% (1,653 to 1,746 ns), +1% when timed alone (below).

### The filed causes

Each pellet holds its operations' gate numbers, its profile (`--pseudo-names`, `light-cljs-1023-b58edd5f`, `profiles-b58edd5f.txt`) and its "Do":

- **light-cljs-1080, map literals** (`map-literal-3` 2.02x): `mapFromShape` copies the site's values array and the shape's keys into a third array; one interleaved array at the site, prototyped here (`map-literal-prototype.diff`, held back because it moves eight pinned tests of light-cljs-220's key shapes).
- **light-cljs-1081, key and value seqs** (`keys-small` and `vals-small` 3.69x, `into-vec-keys-small` 2.03x): a map's `seq` makes every entry up front, and `KeySeqCell` reads through getters with the slot-first tests per step.
- **light-cljs-1082, records** (`record-assoc` 3.86x, `record-kw-field` 1.37x): the shared `recordMethodTable` methods find a field with a `Map` lookup and build the new record with `members.map` and a spread; upstream's `defrecord` writes both per type.
- **light-cljs-1083, multimethods** (`multimethod` 2.30x): keyword dispatch values are never cached, so every call walks the table with `isa?` and `dominates`; `call` takes and spreads rest arrays.
- **light-cljs-1084, function values** (`reduce-vec-1000` 2.08x, `apply-vec` 1.94x, `into-xform-1000` 1.56x, `into-list-small` 1.56x, `sort-by-100` 1.53x): `+` and `-` as values branch on `arguments`, a transducer's step is `callable`'s rest-and-spread wrapper, `apply` conses and walks its arguments as seqs, `sort-by` builds its comparator per comparison.
- **light-cljs-1085, building collections** (`into-map-small` 2.38x, `merge-small` 1.82x, `into-set-100` 1.52x, `into-vec-small` 1.23x, `group-by-100` 1.19x, 1.11x-1.19x after this pellet): a `WeakSet` per transient vector, `nth` reads of map entries in a map's `into`, and an entry object per hash-trie insert; three causes merged.
- **light-cljs-1086, printing and compare** (`into-sorted-map-small` 3.30x, `str-nested-map` 1.79x, `str-sorted-map-20` 1.59x): the printer writes each piece through the generic `coreMethod` dispatcher and probes each value with `ownMethod` and `keyMarker`; `compare` probes `IComparable` through `protocolHooks.builtin` before comparing two keywords.
- **light-cljs-1087, small gaps in the gate's order** (`list-pop` 1.93x, `seq-walk-vec-1000` 1.28x, `update-in-3` 1.16x, `get-in-3` 1.10x, `filter-count-1000` 1.10x, `vec-peek`, `reduce-entries-small`, `destructure-map-3`, and the parity operations just above their ranges in the second pass: `closure-alloc-call`, `vec-invoke-1024`, `dissoc-hash100`, `transient-map-100`): most are at parity when run alone, so their causes are the type feedback of the operations before them; `list-pop`'s `-pop` calls the generic `count` and `rest`.

### Against master before the conversions

No operation ended slower than on master before the conversions:
- **The 64 operations of light-cljs-1007's table**, against its `master-6ceeb23b.edn` (`6ceeb23b`'s compiler is `f533efb1`'s; the master pass above ran with `--baseline` it): 61 are faster on `ed53a283` (-99% to -1%), `multi-arity-call` (+0%) and `closure-alloc-call` (+1%) are within their ranges, and one is slower, `str-concat`, 145.9 to 167.7 ns (+15%), which this pellet's `str` change takes to 13.3. The conversion pellets' sections above report each of these steps; none left an operation slower than this.
- **The 16 operations added since** (the key and value walks of light-cljs-1064, the `into`s of light-cljs-1065, 1067 and 1076, the `pop`s and `disj`s of light-cljs-1073), which light-cljs-1007's table lacks: timed in a selection of their own on `light-cljs-f533efb1` and on `light-cljs-ed53a283` with `--baseline` the first (`--runs 3`, load 6.5-18.5; `f533-new-ops.md`, `master-new-ops.md`), every one is 63% to 98% faster on master: `vec-assoc-variadic-1024` 434.3 to 159.4 ns (-63%), `into-set-100` 21,963 to 5,151 (-77%), `disj-variadic-set-7` -81%, `into-sorted-map-small` -83%, the others -89% to -98%. A full pass on `light-cljs-f533efb1` was started but stopped at the tool's time limit after its second run (its outputs take 10 to 15 minutes a run), so these come from the selection.

### Workload 200k and sizes

**Workload 200k** (`compare.bb workload-200k-seed42-1023 --runs 5 --runtime-only --runtime 1,40`, a copy of the project whose light-cljs output was rebuilt with each binary just before its run, every checksum equal to the oracle's; two rounds, master then `light-cljs-1023-1d906703`; load 9.2-11.2), steady state from the medians at 1 and 40 iterations: light-cljs 34.3 and 34.0 ms per iteration on master, 32.6 and 32.6 after; shadow-cljs 28.2, 28.0, 27.3, 27.2 in the same rounds; light/shadow 1.22 and 1.21 to 1.19 and 1.20.

**Sizes** (`bb benchmarks/re-frame.bb size simple todomvc flow --light-only`, fresh ports with `shadow-port-tool-983`), gzip -9 bytes of light-cljs's output as written, without React, master to `light-cljs-1023-1d906703`: simple 67,474 to 67,565 (+91, +0.13%), todomvc 97,154 to 97,267 (+113, +0.12%), flow 280,884 to 280,475 (-409, -0.15%); the whole esbuild bundles 113,598 to 113,612, 145,158 to 145,235 and 333,137 to 333,290 (+0.01% to +0.05%). The growth is the `str` sites written out and `strOne` and `vectorOfTail` beside `str` and `vectorFromArray`. The size ratchet's notes are in tests/support/ratchet_fixtures.rs: nine fixtures grow 10 to 149 bytes at `:advanced`, and the readable runtime 1,111.

## 2026-11-21: small map literals as one array (light-cljs-1080)

At `:advanced` a map literal of at most 8 distinct constant keys whose values are not all constants is `rt$arrayMapOfLiteral([k0, v0, k1, v1, ...])`: an array map of the one array its site builds, which the map keeps, as upstream's literal is `new PersistentArrayMap(null, 3, [k1, v1, k2, v2, k3, v3], null)`. Before, the site built an array of the values, and `mapFromShape` read the keys from a pooled key shape (`rt$mapShape`) and pushed keys and values into a second array. The keys are constants, so the values are still evaluated in order, once each. Past 8 keys the hash-map shapes (`rt$hashMapShape`, `rt$hashMapFromShape`) stay. This is light-cljs-1023's prototype (`research/output-speed/gate-review-1023/map-literal-prototype.diff`), whose vector half landed there.

**Binaries:** before `light-cljs-cb4c3d94` (master, light-cljs-1023 landed), after `light-cljs-1080-55250c16` (a work-in-progress commit whose compiler sources are this pellet's commit's); V8 builds; shadow-cljs 3.5.3, Node v26.12.1. No run waited for the machine; every run is unreserved, with peers' builds and runs on it. Raw results: `research/output-speed/map-literals-1080`.

**Gate** (`bb benchmarks/core-ops/run.bb --runs 5`, all 80 operations, before then after with `--baseline` the before's; load 7.44-10.23 before, 4.68-6.50 after; `gate-before.md`, `gate-after.md`). The operations this reaches: `map-literal-3` builds its map in the loop; the small-map operations build theirs with a literal before it. Light-cljs ns per operation, medians with ranges:

| op | light-cljs before | light-cljs after | change | shadow-cljs before, after | light/shadow before, after |
| --- | ---: | ---: | ---: | ---: | ---: |
| map-literal-3 | 22.9 (22.6-23.4) | 9.85 (9.06-10.1) | -57% | 10.9, 10.8 | 2.10, 0.91 |
| kw-small | 8.06 (7.89-8.11) | 8.08 (7.70-8.31) | +0% | 10.3, 9.98 | 0.79, 0.81 |
| get-small | 11.5 (11.1-11.7) | 11.2 (11.0-11.8) | -2% | 15.8, 15.5 | 0.72, 0.72 |
| get-small-miss | 16.2 (15.9-16.4) | 16.2 (15.4-16.4) | +0% | 20.4, 20.2 | 0.80, 0.80 |
| contains-small | 9.66 (9.43-10.1) | 9.69 (9.33-10.0) | +0% | 12.8, 12.5 | 0.75, 0.77 |
| count-small | 2.15 (2.14-2.22) | 2.15 (2.10-2.26) | +0% | 2.16, 2.13 | 1.00, 1.01 |
| assoc-small-existing | 17.5 (17.3-17.8) | 17.3 (16.8-17.9) | -1% | 30.4, 31.6 | 0.58, 0.55 |
| assoc-small-new | 41.6 (40.4-42.4) | 41.6 (40.5-42.3) | +0% | 41.5, 42.2 | 1.00, 0.99 |
| dissoc-small | 49.9 (47.4-50.1) | 48.9 (46.3-49.5) | -2% | 61.3, 60.3 | 0.81, 0.81 |
| update-small | 28.2 (26.4-28.8) | 27.8 (26.6-27.9) | -1% | 46.4, 46.3 | 0.61, 0.60 |

`map-literal-3` is below shadow-cljs's median, its after range (9.06-10.1) below shadow-cljs's (10.0-11.0). The other 70 operations are within -10% (`reduce-kv-small`) and +6%: only `dissoc-hash100` is over +5% (83.2 to 87.8 ns, ranges 82.5-85.4 and 82.6-88.1), whose path builds no map literal; timed alone in two alternated pairs (`--ops dissoc-hash100`) it is 82.9 to 82.3 and 82.8 to 82.3 ns (-1% both times; shadow-cljs 74.8 and 75.6, light-cljs-1087's gap).

The touched operations in a selection of their own (`--ops map-literal-3,kw-small,...,update-small --runs 5`, master then after with `--baseline`, two alternated pairs; load 4.90-8.62 over these and the `dissoc-hash100` pairs; `touched-before1.md` to `touched-after2.md`): `map-literal-3` 20.7 to 9.37 and 20.9 to 9.41 ns (-55% both; shadow-cljs 10.1 and 10.2; light/shadow 2.06 to 0.93), the small-map operations -2% to +1%.

**Workload 200k** (`compare.bb workload-200k-seed42-1080 --runs 5 --runtime-only --runtime 1,40`, a copy of the project whose light-cljs output was rebuilt with each binary just before its run, every checksum equal to the oracle's; four rounds, before then after in the first two and after then before in the last two; load 6.36-7.85), steady state from the medians at 1 and 40 iterations, light-cljs ms per iteration and light/shadow:

| round | before | after |
| --- | ---: | ---: |
| 1 | 31.1 (1.20) | 32.9 (1.22) |
| 2 | 32.7 (1.16) | 34.0 (1.23) |
| 3 | 33.0 (1.22) | 33.4 (1.22) |
| 4 | 33.6 (1.19) | 32.5 (1.22) |

Within the spread: light-cljs's medians are 31.1-33.6 before and 32.5-34.0 after, drifting with run order in the first two rounds (31.1, 32.9, 32.7, 34.0), and light/shadow 1.16-1.22 before and 1.22-1.23 after. The workload's 1,024 sites now build their maps this way, and a CPU profile of each output (`--pseudo-names`, 40 iterations, `workload-profile.txt`) shows `mapFromShape` at 0.42% of samples before and `arrayMapOfLiteral` at 0.14% after, which bounds what this change can move there.

**Sizes** (`bb benchmarks/re-frame.bb size simple todomvc flow --light-only`, fresh ports with `shadow-port-tool-983`; `size-cb4c3d94.md`, `size-1080-55250c16.md`), gzip -9 bytes of light-cljs's output as written, without React: simple 67,569 to 67,028 (-541, -0.80%), todomvc 97,268 to 96,219 (-1,049, -1.08%); the whole esbuild bundles 113,620 to 113,095 and 145,233 to 144,083. Writing each site's keys costs fewer bytes than the shapes and their keys' hashes it removes from the pool. Flow's output is byte-identical (280,483): its rewrite-clj extends `IPrintWithWriter` to a type it is given at run time (`make-printable-cljs!`), so its program extends a core protocol beyond its own types and no map literal of it was ever shaped. The size ratchet's notes are in tests/support/ratchet_fixtures.rs: three fixtures shrink 156 to 188 bytes at `:advanced`, the others do not move, and the readable runtime grows 354 bytes.

## 2026-11-21: multimethod calls (light-cljs-1083)

A multimethod call was 1.88x shadow-cljs's in the full gate on master `cb4c3d94`. Where a program's multimethods keep lean array tables (at `:advanced`, `multi_tables` in src/rta.rs), `getMethod` cached only a function's answers, so a keyword dispatch value walked the methods on every call with `isa?` and `dominates`; `MultiFn`'s `call` took and spread rest arrays. Now:

- **The lean cache.** `getMethod` scans the cache by identity first, as upstream's `array-index-of` compares keywords, so an interned keyword literal finds its method, or its cached answer, with no walk. A walk's answer for a plain dispatch value (`leanPlain`: nil, a boolean, a string, a number equal to itself, a keyword, a symbol, or a vector of them) is cached, as upstream caches every answer: in the cache while it holds fewer than 16 entries, past them in a `Map` by a text that equal plain values share (`leanCode`), so the cache, a scan, stays short; a new method clears both, as before.
- **Calls.** `MultiFn`'s `call` passes 0 to 3 arguments to the dispatch function and the method through a step per count (`multiCall1`), with no rest array or spread, and more through `multiApply`.
- **Tables in atoms.** Where a program keeps them there (it can give a hierarchy ancestors, prefers methods, has more than eight methods, or reads the tables; workload 200k is such a program), a call's `-get-method` takes the protocol dispatcher's steps for two arguments (`getMethodProtocol`), where it went through the dispatcher's rest array, slice and spread behind a function that called it. Without this, the per-count steps alone made these calls 5% slower (65.2 to 68.5 ns, medians of six alternated runs), one call level past what V8 inlined.

**Binaries:** master `light-cljs-cb4c3d94` and `light-cljs-1083-4127d2a9` (this change before its rebase onto light-cljs-1080, which touches no multimethod unit, and before the review's two fixes: the `Map` past 16 entries and a key resolved at boot for the shared-heap profile), and for the last rows and the sizes `light-cljs-1083-46bf535d` (the final change on master `1fac567c`) and `light-cljs-1fac567c`; V8 builds; shadow-cljs 3.5.3, Node v26.12.1. No run waited for the machine; every run is unreserved. Raw records and profiles: `research/output-speed/multimethods-1083`.

| measurement | light-cljs before | light-cljs after | change | shadow-cljs before, after | light/shadow before, after |
| --- | ---: | ---: | ---: | ---: | ---: |
| gate `multimethod`, full pass (load 4.4-5.9, then 10.3-14.0; `gate-before`, `gate-after2`) | 88.1 (87.1-93.1) | 42.0 (41.5-43.4) | -52% | 46.9, 46.7 | 1.88, 0.90 |
| gate `multimethod` alone, pair 1 (`--ops multimethod`, load 11.6-12.4; `mm2-*`) | 50.7 (50.3-51.7) | 20.3 (19.7-21.6) | -60% | 38.2, 38.7 | 1.33, 0.52 |
| gate `multimethod` alone, pair 2 (load 10.4-11.0) | 51.1 (50.9-51.7) | 19.9 (19.6-20.4) | -61% | 38.1, 38.0 | 1.34, 0.52 |
| tables in atoms, pair 1 (the gate's source with one `derive`, `atoms-main.cljs`, `--ops multimethod`, load 8.8-8.9; `atoms2-*`) | 65.6 (64.9-66.0) | 45.5 (45.0-46.3) | -31% | 38.3, 38.2 | 1.71, 1.19 |
| tables in atoms, pair 2 (load 8.5-8.7) | 65.1 (64.7-69.1) | 46.2 (45.8-46.3) | -29% | 38.2, 38.2 | 1.70, 1.21 |
| final binary: `multimethod` alone, two pairs (`mm3-*`) | 51.1, 50.8 | 20.0, 19.9 | -61% | 38.0, 38.5 | 1.34-1.35, 0.52-0.53 |
| final binary: tables in atoms, two pairs (`atoms3-*`) | 64.6, 64.5 | 45.4, 44.4 | -30%, -31% | 38.4, 37.7 | 1.70-1.71, 1.18 |

ns per call, medians of 5 runs per compiler with their ranges. `multimethod` is below shadow-cljs's median in every pass. With the tables in atoms it is still 1.20x: each call asks the protocol lookup for a program's `-get-method` and reads the atoms through `deref` (profile `profile-atoms-4127d2a9.txt`), filed as light-cljs-1088.

**The other operations** of the full pass are within -6% and +5% of master's. Four are +4% to +5%, none of which reaches a multimethod step; the after pass ran at twice the load. Timed alone in two alternated pairs (`--ops map-literal-3,merge-small,into-set-100,into-xform-1000`, load 4.8-7.7; `moved-*`): `map-literal-3` +0% and +0%, `into-xform-1000` +1% and -1%, `into-set-100` -1% and +0%, `merge-small` +5% and +0% (and +1% and -1% in two pairs of its own with the first binary of this change, `merge-*`), as light-cljs-1023 found it moving with the type feedback of the operations before it.

**Workload 200k** (`compare.bb workload-200k-seed42-1083 --runs 5 --runtime-only --runtime 1,40`, the project's light-cljs output rebuilt with each binary just before its run, every checksum equal to the oracle's; two rounds, master then after; load 6.3-9.9; `workload3-*`): steady state 33.3 and 31.6 ms per iteration on master, 32.5 and 32.5 after; shadow-cljs 26.6, 26.1, 27.1 and 25.6 in the same runs; light/shadow 1.25 and 1.17 before, 1.24 and 1.27 after, no change beyond the rounds' spread. Its multimethods keep their tables in atoms; multimethod calls are about 2% of its profile.

**TodoMVC** (`bb benchmarks/re-frame.bb runtime todomvc --runs 5`, a fresh port with `shadow-port-tool-983`, load 3.4-4.7; `todomvc-*.md`): adding 1,000 todos takes 2,117 ms (2,093-2,155) on master and 2,138 (2,103-2,149) after, shadow-cljs 956 and 960 (light/shadow 2.21 and 2.23); no multimethod is on that step's path, and the other steps are within their ranges.

**Sizes** (`bb benchmarks/re-frame.bb size simple todomvc flow --light-only`, fresh ports with `shadow-port-tool-983`; `size-1fac567c.md`, `size-1083-46bf535d.md`), gzip -9 bytes of light-cljs's output as written, without React, master `1fac567c` to the final change: simple 67,028 to 67,028 (it keeps no multimethod), todomvc 96,219 to 96,724 (+505, +0.52%), flow 280,479 to 280,948 (+469, +0.17%); the whole esbuild bundles 113,095 to 113,095, 144,083 to 144,602 (+0.36%) and 333,314 to 333,869 (+0.17%). The growth is the steps above in the apps that keep a multimethod (re-frame.alpha's `reg`, and todomvc's router): the per-count calls, `leanMethod` with the `Map` past 16 entries and `leanCode`, `leanPlain` and `symbolOf`; before the review's fixes todomvc grew 348 bytes (+0.36%). The size ratchet's note is in tests/support/ratchet_fixtures.rs: `defmulti` grows about 1,500 bytes at `:advanced` (19,782 bytes after single-pass nested-vector `leanCode`), and the readable runtime 5,846.

## 2026-11-22: array-map and hash-map index seqs and getter-free key/value seqs (light-cljs-1081)

In the gate on master `ba3efd525`, `keys-small` (3.66x) and `vals-small` (3.66x) remained far above shadow-cljs's medians because an array map's `seq` allocated every `MapEntry` up front into a fresh array, and `KeySeqCell` read every step through prototype getters and generic `first`/`next`/`key`/`val` dispatch; hash-map seqs (`NodeSeq`, `ArrayNodeSeq`) similarly built and froze every `MapEntry` up front on `seq`, and `into-vec-keys-small` (0.94x on `ba3efd525` after `keySeqReduce`) still built the whole array of `MapEntry` objects before reducing. Now:

- **Lazy array-map and hash-map index seqs (`ArrayMapIndexCell`, `NodeSeqCell`, `ArrayNodeSeqCell`).** `(seq m)` on an array map creates an `ArrayMapIndexCell(arr, i, root)` (snapshotting a public `PersistentArrayMap`'s array so later edits to the caller's array are not observed), and `(seq hm)` on a hash map creates a `NodeSeqCell(nodes, i, root)` or `ArrayNodeSeqCell(nodes, i, root)` indexing its cached `mapTraversal` array of internal `{key, value}` leaf objects — each with keyed `-first`, `-rest`, `-next`, `-seq`, `-count`, and `-reduce` slots (plus `-drop` on array-map seqs preserving master's view-offset semantics for nonpositive offsets). Realized `MapEntry` objects are built lazily only when `-first` or `-reduce` demands an entry and cached once per index in a prototype-free `Object.create(null)` map on `root` (immune to `Array.prototype`/`Object.prototype` pollution) so repeated `(first s)` / `(second s)` / tail traversals share entry identity and preserve `specify!` on entries, while `(seq m)`, `(count (seq m))`, `(keys m)`, and `(vals m)` allocate zero entry cache or `MapEntry` objects. Moving `makeMap.seq` to `when: seqPath` keeps `makeMap.seq` out of `:advanced` builds without `seqPath`.
- **Getter-free `KeySeqCell` with slim slots and direct key/value reads.** `KeySeqCell` uses plain `first`/`rest`/`next`/`seq`/`reduce` methods behind thin 1-line keyed slot wrappers (keeping V8 TurboFan feedback vectors compact so polymorphic `firstSite`/`nextSite` in `lazy-walk-1000` still inline `ChunkedCons`). When walking an unextended `ArrayMapIndexCell`, `NodeSeqCell`, or `ArrayNodeSeqCell` with no realized entry cache (`root.entries === null`), `first` and `reduce` read keys and values straight from the flat pairs array or traversal nodes without building any `MapEntry`; `seqReduceFrom` and post-callback revalidation in `ArrayMapIndexCell`, `NodeSeq`/`ArrayNodeSeq`, `KeySeqCell`, and `keySeqReduce` ensure mid-reduction protocol mutations (`extend-type`, `specify!`) take effect on the very next step.

**Binaries:** before `light-cljs-ba3efd525` (master `ba3efd525`, SHA-256 `91d2970f3008019c...`), after `light-cljs-1081-after` (this change's runtime, SHA-256 `94d5230bf1b153d8...`), V8 builds; shadow-cljs 3.5.3, Node v26.12.1. No run waited for the machine; all runs are unreserved.

**Gate — touched operations** (`bb benchmarks/core-ops/run.bb --runs 5 --ops keys-small,vals-small,keys-hash100,vals-hash100,into-vec-keys-small,merge-small,destructure-map-3,str-nested-map`, load 2.68-3.20 before, 2.93-3.51 after) and **full 80-op pass** (`--runs 5`, load 2.74-5.89 before, 3.04-4.09 after), light-cljs ns per operation, medians of 5 alternated runs with ranges:

| op (touched selection) | light-cljs before | light-cljs after | change | shadow-cljs before, after | light/shadow before, after |
| --- | ---: | ---: | ---: | ---: | ---: |
| keys-small | 244.2 (240.4-246.4) | 57.8 (56.7-60.0) | -76% | 65.8, 64.9 | 3.71, 0.89 |
| vals-small | 245.3 (238.0-247.1) | 46.5 (46.3-46.7) | -81% | 87.7, 87.1 | 2.80, 0.53 |
| into-vec-keys-small | 255.1 (253.5-258.1) | 135.8 (134.7-138.0) | -47% | 336.1, 335.8 | 0.76, 0.40 |
| keys-hash100 | 2,902 (2,877-2,952) | 954.7 (944.4-971.2) | -67% | 5,380, 5,324 | 0.54, 0.18 |
| vals-hash100 | 3,321 (3,057-3,361) | 1,141 (1,127-1,143) | -66% | 5,137, 5,164 | 0.65, 0.22 |
| merge-small | 1,463 (1,446-1,560) | 1,396 (1,387-1,432) | -5% | 782.1, 784.1 | 1.87, 1.78 |
| str-nested-map | 907.9 (882.7-916.9) | 915.9 (893.5-928.2) | +1% | 2,328, 2,327 | 0.39, 0.39 |
| destructure-map-3 | 31.8 (31.8-32.4) | 31.6 (29.9-32.8) | -1% | 34.5, 34.1 | 0.92, 0.93 |

| op (full 80-op pass) | light-cljs before | light-cljs after | change | shadow-cljs before, after | light/shadow before, after |
| --- | ---: | ---: | ---: | ---: | ---: |
| keys-small | 267.2 (266.4-275.2) | 59.2 (58.0-59.5) | -78% | 73.1, 73.2 | 3.66, 0.81 |
| vals-small | 278.1 (275.5-284.4) | 46.1 (45.7-46.7) | -83% | 75.9, 76.3 | 3.66, 0.60 |
| into-vec-keys-small | 302.8 (298.7-307.9) | 175.8 (174.1-178.4) | -42% | 322.0, 323.6 | 0.94, 0.54 |
| keys-hash100 | 3,331 (3,223-3,622) | 816.6 (801.6-826.8) | -75% | 4,907, 4,931 | 0.68, 0.17 |
| vals-hash100 | 3,747 (3,596-3,841) | 996.4 (991.5-1,019) | -73% | 4,902, 4,886 | 0.76, 0.20 |
| lazy-walk-1000 | 36,279 (35,260-36,453) | 36,867 (36,261-37,077) | +2% | 48,779, 48,982 | 0.74, 0.75 |
| seq-walk-vec-1000 | 26,166 (26,037-27,441) | 24,005 (23,605-24,499) | -8% | 19,196, 19,173 | 1.36, 1.25 |
| merge-small | 1,434 (1,431-1,497) | 1,470 (1,457-1,545) | +3% | 875.6, 877.2 | 1.64, 1.68 |
| str-nested-map | 898.2 (878.0-931.4) | 873.1 (859.0-894.2) | -3% | 2,282, 2,296 | 0.39, 0.38 |
| destructure-map-3 | 34.3 (34.0-34.5) | 34.1 (33.8-34.6) | +0% | 49.0, 49.0 | 0.70, 0.70 |

`keys-small`, `vals-small`, and `into-vec-keys-small` are all well below shadow-cljs's medians in both selections (0.40-0.89x), and lazy hash-map seqs cut `keys-hash100` and `vals-hash100` by 66-75% (to 0.17-0.22x shadow-cljs). In the full 80-op pass, every single one of the other 70 operations is within -11% (`apply-vec`) and -8% (`seq-walk-vec-1000`) to +3% (`merge-small` and `select-keys-3`, both with overlapping ranges; `merge-small` is -5% in the touched selection); zero operations regress beyond their ranges.

**Workload 200k** (`compare.bb --runs 5 --runtime-only --runtime 1,40`, private copy rebuilt with each binary just before its run, every checksum equal to the oracle's; load 3.05-3.21 before, 3.05-3.58 after): 1 iteration wall 0.313 s (0.308-0.319) to 0.318 s (0.315-0.330), 40 iterations wall 1.371 s (1.349-1.428) to 1.382 s (1.342-1.416), steady state 27.1 to 27.3 ms per iteration (shadow-cljs 23.0 and 22.5 ms; light/shadow 1.18 before and 1.21 after), peak memory at 40 iterations 0.253 to 0.251 GB (light/shadow 0.95 both).

**Sizes** (light-cljs `:advanced` output as written, without React, compiled with `--no-cache` from `research/shadow-credibility/{simple-port-973,todomvc-port-980,flow-port-980}`):
- `simple`: raw 253,507 to 260,287 (+6,780), gzip -9 68,731 to 70,315 (+1,584, +2.30%), brotli 59,352 to 60,598 (+1,246).
- `todomvc`: raw 368,927 to 375,700 (+6,773), gzip -9 98,547 to 100,125 (+1,578, +1.60%), brotli 84,623 to 85,960 (+1,337).
- `flow`: raw 1,210,576 to 1,217,362 (+6,786), gzip -9 282,579 to 284,342 (+1,763, +0.62%), brotli 227,331 to 228,594 (+1,263).

The fixed ~6.78 KB raw (~1.58-1.76 KB gzip) across all three apps is the lazy `ArrayMapIndexCell` and `NodeSeqCell`/`ArrayNodeSeqCell` classes (with shared prototype-free entry caches and `seqReduceFrom` protocol-mutation fallbacks), `KeySeqCell`'s plain methods and slim slot wrappers for both map representations, and `ownEntryItems`, trading ~1.6 KB gzip for the 42-83% reduction across all five map key/value walk benchmarks with zero regressions across the 80-op gate. The size ratchet's note is in tests/support/ratchet_fixtures.rs (readable runtime +22,002 bytes; three record-only fixtures shrink 939-2,124 bytes at `:advanced`, nine seq-walking fixtures grow).

## 2026-11-23: per-type record lookup and assoc (light-cljs-1082)

In the full gate on master `96f4c97a`, `record-kw-field` (`(:w r)`) was 1.36x shadow-cljs's median (5.65 vs 4.14 ns) and `record-assoc` (`(:w (assoc r :w i))`) was 3.79x (52.1 vs 13.7 ns), because both ran the runtime's shared `recordMethods.lookup` and `recordMethods.assoc`: a per-type `Map` lookup (`recordKeywordFieldIndex`), a megamorphic keyed load (`this[Type.members[index]]`), and for `-assoc` an array allocation, closure, and spread (`...Type.members.map(...)`) per update. Now:

- **Per-type `ILookup/-lookup` and `IAssociative/-assoc` kits.** At `:advanced` (single-file, serial and parallel), `splice_record_kits` emits each record type's own `ILookup/-lookup` (both arities) and `IAssociative/-assoc` alongside `-equiv` and `-hash` in `defineType`'s kit argument. Arity 2 of `-lookup` delegates through the receiver's 3-arity slot (`this.cljs$core$ILookup$_lookup$arity$3$LKEY(this, a1, null)`) so `extend-type`/`specify!` replacing only 3-arity `-lookup` still governs 2-arity lookups. Arity 3 of `-lookup` and `-assoc` compare the key against each field's interned keyword constant by identity (`===`), fall back to `switch (rt$keywordFqn(a1))` for non-interned keywords (such as `(keyword "w")`), and delegate other keys to `rt$get`/`rt$assoc` on `this["__extmap"]`. Field `-assoc` copies the fields into locals and calls the type's current constructor (`new g<n>(v0, ..., this["__meta"], e, null)`) at a single site (`O(N)` code size per record type).
- **Selective emission and generic fallback.** `RuntimeRoots::record_kits` emits `-lookup` only when `recordMethods.lookup` is kept alongside a real lookup use (`RECORD_LOOKUP_USES` or a direct `get` root, excluding record-only programs that pull `get` solely via `recordEntries` -> `keywordPrototype`) and emits `-assoc` whenever `recordMethods.assoc` is kept. The runtime's shared `recordMethods.lookup` and `recordMethods.assoc` remain for raw `defrecord*`, lower optimization levels, chunked builds, the evaluator, and the macro host.

Measured on Apple M3 Ultra (Virtual), 24 CPUs, 32 GiB, macOS 15.7.3; light-cljs master `96f4c97a` (`light-cljs-96f4c97a`, SHA-256 `2f249e3718d17c02...`) against after (`light-cljs`, SHA-256 `9e437d6a099f4cb8...`), V8 builds; shadow-cljs 3.5.3, Node v26.12.1. No run waited for the machine; all runs are unreserved.

**Gate — record operations** (`bb benchmarks/core-ops/run.bb --runs 5 --ops protocol-on-record,record-kw-field,record-dot-field,record-assoc`, load 4.19-4.29 before, 4.26-4.32 after) and **full 80-op pass** (`--runs 5`, load 2.87-4.10 before on `96f4c97a`, 2.91-5.19 after), light-cljs ns per operation, medians of 5 alternated runs with ranges:

| op (record selection) | light-cljs before | light-cljs after | change | shadow-cljs before, after | light/shadow before, after |
| --- | ---: | ---: | ---: | ---: | ---: |
| protocol-on-record | 2.37 (2.37-2.60) | 2.33 (2.32-2.38) | -2% | 2.37, 2.35 | 1.00, 0.99 |
| record-kw-field | 5.53 (5.53-5.54) | 2.39 (2.34-2.40) | -57% | 2.78, 2.74 | 1.99, 0.87 |
| record-dot-field | 0.87 (0.87-0.88) | 0.86 (0.82-0.87) | -1% | 0.87, 0.86 | 1.00, 1.00 |
| record-assoc | 54.6 (54.3-55.0) | 6.85 (6.72-7.19) | -87% | 8.23, 8.20 | 6.64, 0.84 |

| op (full 80-op pass) | light-cljs before | light-cljs after | change | shadow-cljs before, after | light/shadow before, after |
| --- | ---: | ---: | ---: | ---: | ---: |
| protocol-on-record | 2.32 (2.20-2.33) | 2.32 (2.32-2.40) | +0% | 2.32, 2.38 | 1.00, 0.97 |
| record-kw-field | 5.65 (5.63-5.75) | 2.99 (2.97-3.09) | -47% | 4.14, 4.20 | 1.36, 0.71 |
| record-dot-field | 0.86 (0.81-0.88) | 0.86 (0.81-0.89) | +0% | 0.87, 0.87 | 0.99, 0.99 |
| record-assoc | 52.1 (51.4-53.1) | 9.21 (9.02-9.34) | -82% | 13.7, 13.8 | 3.79, 0.67 |

Both `record-kw-field` and `record-assoc` are well below shadow-cljs's medians with non-overlapping ranges in both the isolated record selection (0.87x and 0.84x) and the full 80-op pass (0.71x and 0.67x). Across all 76 other operations in the full pass, `light/shadow` ratios remain within -0.05 to +0.05 of `96f4c97a` (both compilers saw the same ~3-7% load-driven shift on loop baselines `bit-and`/`aget-bit-and`); zero operations regressed.

**Workload 200k** (`compare.bb --runs 5 --runtime-only --runtime 1,40`, private copy rebuilt with each binary just before its run, every checksum equal to the oracle's; load 4.54-4.98 before, 4.32-4.51 after): 1 iteration wall 0.330 s (0.324-0.335) to 0.328 s (0.316-0.343), 40 iterations wall 1.365 s (1.338-1.429) to 1.371 s (1.348-1.390), steady state 26.5 to 26.7 ms per iteration (shadow-cljs 22.1 and 23.3 ms; light/shadow 1.20 before and 1.15 after), peak memory at 40 iterations 0.254 to 0.259 GB (light/shadow 0.94 to 0.97).

**Sizes** (light-cljs `:advanced` output as written, without React, compiled with `--no-cache` from `research/shadow-credibility/{simple-port-973,todomvc-port-980,flow-port-980}`):
- `simple` (0 `defrecord`s): raw 260,287 to 260,287 (+0), gzip -9 70,315 to 70,315 (+0, 0.00%), brotli 60,598 to 60,598 (+0).
- `todomvc` (0 `defrecord`s): raw 375,700 to 375,700 (+0), gzip -9 100,125 to 100,125 (+0, 0.00%), brotli 85,960 to 85,960 (+0).
- `flow` (29 `defrecord`s): raw 1,218,298 to 1,235,503 (+17,205, +1.41%), gzip -9 284,449 to 287,474 (+3,025, +1.06%), brotli 228,561 to 230,439 (+1,878, +0.82%).

`flow` grows +3,025 bytes gzip (+1.06%) because its `rewrite-clj` dependency defines 29 `defrecord` types (about 104 gzip bytes per record type for the per-type `ILookup/-lookup` and `IAssociative/-assoc` methods); `simple` and `todomvc` define no records and do not move. The size ratchet's note is in tests/support/ratchet_fixtures.rs (readable runtime +0 bytes; zero ratchet fixtures or `features` chunks move).
