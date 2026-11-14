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
  closure. For a keyword key without an `IEquiv` of its own, it makes the
  key's protocol lookup once per search, not once per entry, then compares
  spellings as `equalPair` does. Any other key still goes through
  `equalPair` for each entry, in order.
- A map entry is made with `Object.create` of its type's prototype instead
  of `Object.setPrototypeOf` on a fresh vector.

Nothing observable changes. `builtin_types::small_maps_keep_their_key_tests_entries_and_hashes`,
written and passing before the change, checks:
- key comparison, including equal keywords that are not identical;
- the order of a key's `-equiv` and `-hash` calls;
- that `assoc` keeps the existing key, and the order of keys;
- `map-entry?`, metadata and identity of the entries seqs give.

**Setup.** The kit and machine of the 2026-11-14 section above, with these
differences:
- before is `light-cljs-32cb0373` (988's binary, whose runtime equals
  master's `1aee1a66`) and after is `light-cljs-5467ea43`;
- each output was built with its binary just before its runs;
- before and after ran back to back on each workload, each alternating with
  shadow-cljs.

10 runs of each. **Unreserved:** peer workers ran builds and test suites
throughout. The 1-minute load average before a run was 2.7 to 24.6, and it
differed between before and after (workload 200k: 11 to 16 before, 5 to 10
after). The light/shadow ratios, whose two sides ran alternately, are the
fairer comparison. Raw results are in `research/shadow-credibility/runtime-989`.

Only the rows that moved are shown. Ranges are in brackets.

| Workload | light-cljs before | light-cljs after | change | light/shadow before | light/shadow after |
| --- | ---: | ---: | ---: | ---: | ---: |
| workload 200k, steady state per iteration | 103.8 ms | 91.6 ms | -12% | 4.02 | 3.75 |
| workload 200k, wall time at 40 iterations | 4.546 s (4.211-4.640) | 4.020 s (3.811-4.319) | -12% | 3.37 | 3.16 |
| workload 200k, peak memory at 40 iterations | 0.428 GB (0.407-0.436) | 0.321 GB (0.314-0.339) | -25% | 1.53 | 1.21 |
| workload 800k, steady state per iteration | 428.8 ms | 355.3 ms | -17% | 3.70 | 3.32 |
| workload 800k, wall time at 16 iterations | 7.883 s (7.553-8.289) | 6.818 s (6.652-9.485) | -14% | 2.87 | 2.56 |
| workload 800k, peak memory at 16 iterations | 0.794 GB (0.753-0.809) | 0.664 GB (0.655-0.746) | -16% | 1.42 | 1.19 |
| re-frame-shaped 200k app, steady state per iteration | 1,335 ms | 1,027 ms | -23% | 4.94 | 3.91 |
| re-frame-shaped 200k app, wall time at 10 iterations | 15.753 s (15.029-16.063) | 12.641 s (12.228-16.329) | -20% | 4.15 | 3.30 |
| re-frame-shaped 200k app, first output | 3.719 s (3.539-3.954) | 3.383 s (3.257-3.689) | -9% | 2.76 | 2.32 |
| re-frame-shaped 200k app, peak memory at 10 iterations | 1.273 GB (1.243-1.320) | 0.804 GB (0.759-0.889) | -37% | 1.75 | 1.11 |
| TodoMVC, add 1,000 todos | 9,581 ms (9,232-9,956) | 8,939 ms (8,574-9,092) | -7% | 9.78 | 9.18 |
| TodoMVC, toggle all | 159.6 ms (140.3-201.8) | 135.9 ms (130.8-161.5) | -15% | 3.01 | 2.58 |
| TodoMVC, clear completed | 40.6 ms (33.2-51.4) | 34.7 ms (32.0-40.0) | -15% | 1.55 | 1.39 |
| TodoMVC, whole run wall time | 10.52 s (9.97-11.87) | 9.86 s (9.36-10.50) | -6% | 6.24 | 6.11 |
| TodoMVC, whole run user CPU | 11.14 s (10.75-11.37) | 10.49 s (10.09-10.61) | -6% | 5.97 | 5.70 |

- **Unchanged within their ranges:** startup at one iteration of the
  workloads, TodoMVC's page load and edit step, its JavaScript heap
  (14.06 to 13.71 MB with 1,000 todos) and its peak memory.
- **Peak memory** falls most where many small maps live at once: each entry
  no longer holds a closure and its context.
- **Output size:** a program building array maps keeps the entry class and
  the longer search, about 170 bytes at advanced (`map-only` 8,066 to 8,257
  bytes); the size ratchet notes it in tests/support/ratchet_fixtures.rs.

**What is left of the small-map costs.** A sampling profile of TodoMVC's add
step (`--pseudo-names`, as above) puts `entryIndex` at 1.8% inclusive, from
5.8%. `mapEntry` is still 12% self:
- The WeakSet registration that brands each map entry (`mapEntries`) is most
  of it: in a micro-benchmark, building an entry costs about 36 ns without
  it and 220 to 350 ns with it. It is a value-kind brand, left to
  light-cljs-990.
- The add step builds so many entries because the map's `seq` builds all of
  them at once. TodoMVC checks its app database with cljs.spec after every
  event, and spec's map conform seqs the sorted map of todos and every todo
  in it. Upstream's array-map and hash-map seqs build an entry only when it
  is read, and a sorted map's tree nodes are its entries. Building entries
  lazily would change what a map's seq holds, so it is recorded here and
  not done.
