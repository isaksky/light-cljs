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
in this step.

Raw results (the core-ops EDN and Markdown of every run above, the first
gate and the back-to-back recheck included, and both workload runs):
research/output-speed/vector-fields-1026.
