# Native protocol and immutable record costs, October 9, 2026

All seven resident dispatch programs returned **499500** from 1000 prebuilt values. These measurements separate ordinary saved function calls, construction, analyzed reduction, definition work, reclamation and fresh native processes. Records and reify do not have an assumed performance advantage. No production code or dependency changed for this report.

Record protocol dispatch made 13000 allocation requests per 1000 calls versus 9000 for direct map-field calls. Reify construction made 18000 requests per 1000 objects versus 3000 for captured closures. Several host-invocation timings varied widely in two separate runs, so their median speed ratios are not stable conclusions. Both full distributions and negative findings are retained below.

## Environment, identities and reproduction

Apple M3 Ultra virtual machine, 24 visible CPUs, 32 GiB RAM, macOS 15.7.3 arm64; Rust/Cargo 1.99.0, LLVM 23.1.2, default release optimization level 3. No affinity or frequency control. [Environment](environment.json) records exact toolchain, UTC time, build overrides and dirty Git state. The complete [methodology and commands](../../../interpreter.md#protocol-and-immutable-record-cost-report) define every boundary.

Nine raw samples × 32 operations per resident phase, after two equal-sized warmups. One explicit GC follows each batch. An `invoke_1000`, `analyzed_sum_1000` or `construct_1000` operation contains **1000 items**: there are 32000 items in each such sample. Other operations are single calls/definitions. Timed scalar extraction/checksums and `black_box` handling are included; fixture setup and full value/type assertions are excluded. Both members of every pair use retained inputs or fresh inputs consistently.

Builds, full tests and lint checks finished before this final run. An earlier attempt overlapped lint compilation and is excluded; it remains in the sibling `types/` cache directory with `EXCLUDED.txt`. The measured run was sequential, in fixed resident scenario/phase order; fresh process order rotated/reversed. VM scheduling, warm code/data and uncontrolled OS caches limit generalization. MAD/ranges describe sample variation, not confidence intervals.

The [manifest](manifest.json) hashes sources, fixtures, binaries and the exact source archive. Saved binaries/archive are outside `target`:

```text
/Users/admin/.cache/light-cljs-baselines/light-cljs-65/verified-types
```

The archive includes all preceding uncommitted native feature work; HEAD alone does not identify the executable. This report was written after measurements. [Runner log](runner.log) and [build log](build.log) record executed commands.

## Saved function dispatch

Time is µs per **1000 calls**, median ± MAD (min–max). Allocation counts, reallocations and requested bytes are per 1000 calls from the separate instrumented executable. The method value is saved once and invoked through ordinary `Context::invoke`. The mixed case alternates R/S with equal method bodies and includes their two nominal types.

| Scenario | µs / 1000 calls | Allocations | Reallocations | Requested bytes |
| --- | --- | --- | --- | --- |
| map | 2291.75 ± 1258.76 (542.44–3590.48) | 9,000 | 0 | 352,000 |
| direct-record | 565.41 ± 11.06 (528.09–655.84) | 9,000 | 0 | 352,000 |
| inline | 10647.64 ± 4777.18 (1579.24–15847.23) | 13,000 | 0 | 656,000 |
| external | 10396.83 ± 1611.22 (4801.24–15245.98) | 13,000 | 0 | 656,000 |
| reify | 321.10 ± 2.53 (315.76–342.24) | 5,000 | 0 | 344,000 |
| closure | 246.55 ± 3.35 (239.82–260.96) | 2,000 | 0 | 80,000 |
| mixed | 11482.98 ± 295.87 (2129.65–11863.70) | 13,000 | 0 | 656,000 |

`map` and `direct-record` invoke `(fn [r] (:n r))`. Inline record methods read the receiver field; external methods read `:n` through lookup. Reify objects capture each `n`; closure objects capture it and a saved wrapper calls the captured function. These pairs establish equal scalar outcomes, not identical object semantics. Saved constructors and method values stay rooted, and the mixed factory branches only during construction, outside dispatch timing.


A second full run of the exact saved executable reproduced unusually wide
host-invocation distributions for map and record protocol calls. Its nine raw
samples are preserved in [timings-confirmation.csv](timings-confirmation.csv),
with complete [distributions](confirmation-summary.json) and the
[command/binary identity](confirmation-identity.json). No samples from either
clean run were discarded. The cause is not isolated by this benchmark, and
these timings do not establish a stable record-versus-map speed ratio. The
analyzed reduction and allocator counts provide separate, more repeatable
observations; they must not be substituted for the host-call boundary.

| Scenario | Confirmation µs / 1000 host calls |
| --- | --- |
| map | 6601.55 ± 138.41 (485.47–6966.73) |
| direct-record | 524.18 ± 5.19 (514.00–530.26) |
| inline | 4555.61 ± 3502.65 (892.68–15372.10) |
| external | 7427.03 ± 4453.79 (1886.04–15180.25) |
| reify | 317.05 ± 6.50 (297.75–328.31) |
| closure | 244.54 ± 2.93 (241.48–255.70) |
| mixed | 10362.72 ± 4330.29 (2322.14–16619.40) |

## Analyzed reduction and repeated construction

Both columns are µs per 1000 values, median ± MAD (min–max). Analyzed reduction executes retained code and traverses the same retained finite vector in the language. Construction invokes the saved factory with each scalar input and drops every new value; it includes repeated reify/closure make/drop and excludes method invocation. The 1000 prebuilt values used by the earlier phases remain intentionally live during construction.

| Scenario | Analyzed reduction µs | Construction µs | Construction allocations | Construction requested bytes |
| --- | --- | --- | --- | --- |
| map | 929.25 ± 6.33 (910.56–941.20) | 671.24 ± 6.67 (645.58–683.34) | 16,000 | 844,000 |
| direct-record | 926.52 ± 9.23 (913.16–936.24) | 710.32 ± 4.50 (702.95–723.58) | 16,000 | 896,000 |
| inline | 1162.92 ± 14.55 (1133.12–1182.11) | 725.45 ± 7.77 (707.05–773.77) | 16,000 | 896,000 |
| external | 1116.01 ± 4.88 (1095.24–1127.64) | 724.72 ± 4.67 (712.66–732.09) | 16,000 | 896,000 |
| reify | 728.33 ± 10.59 (715.75–752.41) | 846.94 ± 3.52 (842.72–860.47) | 18,000 | 988,000 |
| closure | 640.77 ± 4.78 (634.73–727.99) | 174.47 ± 2.85 (171.08–181.41) | 3,000 | 104,000 |
| mixed | 1172.15 ± 10.93 (1148.17–1305.32) | 988.43 ± 5.30 (978.73–1005.54) | 18,000 | 976,000 |

## Record/map operations

Time is µs per single operation, median ± MAD (min–max). All full projected contents and result types are validated before timing. Key lookup uses a separately constructed equal key. Nominal record/map inequality, distinct record-type keys and map-key separation are correctness checks outside measurement.

| Operation | Map µs | Record µs | Allocations map / record | Requested bytes map / record |
| --- | --- | --- | --- | --- |
| lookup | 0.57 ± 0.06 (0.49–0.63) | 0.55 ± 0.06 (0.48–0.61) | 9 / 9 | 352 / 352 |
| assoc_basis | 0.92 ± 0.01 (0.85–1.11) | 0.92 ± 0.01 (0.84–0.93) | 19 / 19 | 984 / 984 |
| assoc_extra | 0.92 ± 0.01 (0.86–0.94) | 0.92 ± 0.01 (0.86–1.19) | 19 / 19 | 984 / 984 |
| dissoc_extra | 1.15 ± 0.04 (1.02–1.36) | 1.26 ± 0.01 (1.17–1.51) | 23 / 27 | 1,144 / 1,304 |
| dissoc_basis | 1.17 ± 0.01 (1.11–1.21) | 1.30 ± 0.01 (1.24–1.32) | 23 / 27 | 1,144 / 1,304 |
| from_map | 2.64 ± 0.03 (2.49–2.76) | 1.10 ± 0.03 (1.02–1.30) | 59 / 20 | 3,376 / 824 |
| key_lookup | 2.92 ± 0.01 (2.88–3.21) | 3.31 ± 0.21 (2.76–3.52) | 36 / 36 | 5,536 / 5,536 |

Removing `:extra` preserves record type. **Removing basis field `:y` returns an ordinary map** with the expected remaining contents. `dissoc_basis` therefore includes a type transition, not semantic interchangeability. `from_map` compares `map->R` with `(into {} m)` copying the same three-entry map; no identity-return shortcut is timed. Scalars and updated `:n` fields provide checksums, while exact full contents/types are asserted separately.

## Macro, analysis and definition publication

Median µs per definition. Every field also has all nine samples, MAD and range in [summary.json](summary.json). Macro expansion acts on retained quoted data. Analysis includes macro expansion/code generation and drops analyzed handles; effectful definitions do not reuse analysis. Publication executes retained analyzed code. Full evaluation includes reading/analysis/publication from the retained named source. These boundaries overlap and must not be summed.

| Definition | Macroexpand | Analyze | Publish analyzed code | Read/analyze/evaluate |
| --- | --- | --- | --- | --- |
| protocol | 10.26 | 49.17 | 9.12 | 60.29 |
| record-data | 14.21 | 77.95 | 11.35 | 92.14 |
| record-inline | 19.86 | 126.00 | 11.24 | 149.77 |
| record-extension | 8.55 | 49.69 | 1.17 | 49.41 |

## Reclamation and retained generations

Every warmed post-batch collection returned to exactly equal heap/name/source/code/namespace/Var/root/**slot** counts for its scenario. This covers fresh reify/closure objects, successful and failed definitions and replacement captures. Every such allocator-instrumented GC row recorded **zero allocations, reallocations and requested bytes**. Old-generation retention deliberately keeps a record plus its type descriptor and captured vector; releasing those handles restores exact baseline live counts. The current definition and its code/source/metadata remain intentionally live.

Times below are µs, median ± MAD (min–max). Redefinition time is per operation; GC is per 32-definition batch. H/S/C/R/T means live heap values / sources / executable units / roots / arena slots. Vacant slots and scratch capacity remain after collection; they are not retained language values.

| Scenario | Redefine µs | GC µs / batch | Warmed H / S / C / R / T |
| --- | --- | --- | --- |
| released-generation | 280.37 ± 5.43 (274.94–296.17) | 2264.21 ± 44.00 (2176.71–2363.08) | 33 / 1 / 1 / 6 / 18,398 |
| retained-generation | 279.73 ± 3.69 (266.71–290.44) | 2254.71 ± 61.21 (2185.21–2671.00) | 46 / 2 / 2 / 8 / 18,414 |
| failed-generation | 10.70 ± 0.03 (10.04–10.79) | 62.75 ± 0.58 (58.88–67.00) | 28 / 1 / 3 / 8 / 922 |
| replaced-capture | 189.57 ± 3.03 (180.00–197.86) | 1601.92 ± 45.08 (1519.04–1688.75) | 34 / 2 / 3 / 8 / 15,477 |

Failed definitions deliberately repeat a duplicate basis field; the old record and method still return 7 afterward. Successful captured definitions and external replacements build a new 128-item capture vector each time. Stable live code/source counts reject permanent registry/cache retention of replaced definitions or captures. Separate `release_final` rows perform nine independent retain/redefine/release transitions with definition and preliminary collection outside timing:

| Release case | Release + GC µs | Heap values reclaimed | Final H / S / C / R / T |
| --- | --- | --- | --- |
| released-generation | 17.92 ± 0.21 (16.83–18.12) | 0 | 33 / 1 / 1 / 6 / 18,398 |
| retained-generation | 23.29 ± 1.17 (21.92–36.04) | 13 | 33 / 1 / 1 / 6 / 18,414 |

Method-callback collection stays inside the operation timer: one explicit collection for each of 32 invocations per sample, then one separately recorded post-batch GC. All three methods return 7 through collection with their receiver/captures alive.

| Method | Invoke including GC µs | Internal collections/sample | Post-batch GC µs |
| --- | --- | --- | --- |
| inline | 2.34 ± 0.01 (2.20–2.65) | 32 | 2.00 ± 0.04 (1.71–2.12) |
| external | 2.63 ± 0.23 (2.17–2.85) | 32 | 2.04 ± 0.21 (1.79–2.29) |
| reify | 1.57 ± 0.05 (1.33–2.10) | 32 | 1.38 ± 0.12 (1.08–1.54) |

## Allocator live/peak bytes and ordinary GC

The separate counting allocator reports requested Rust layout bytes for the **whole process**, not RSS, usable allocator size, reserved pages or interpreter allocation-limit units. Allocation counts exclude reallocations; requested bytes include the entire new realloc size. Instrumented timing is zero. Live/peak bytes below are absolute median KiB, not divided by operations. They include retained input/factory/method/code/harness storage, and phase order can retain earlier arena high-water capacity.

| Scenario | Live / peak KiB during 1000-call batches | Post-batch GC µs | Warmed H / S / C / R / T |
| --- | --- | --- | --- |
| map | 4157.0 / 4157.1 | 2323.88 ± 10.96 (2229.50–2886.17) | 1,008 / 2 / 2 / 1,009 / 1,133 |
| direct-record | 4419.1 / 4419.2 | 2451.38 ± 138.58 (2229.46–2822.88) | 1,022 / 2 / 4 / 1,009 / 2,239 |
| inline | 5726.2 / 5726.6 | 3294.67 ± 94.54 (3196.04–4133.58) | 1,030 / 2 / 4 / 1,009 / 2,358 |
| external | 5726.8 / 5727.1 | 3698.92 ± 122.42 (3149.67–4104.29) | 1,030 / 2 / 4 / 1,009 / 2,369 |
| reify | 4937.3 / 4937.6 | 1947.25 ± 70.83 (1876.42–2256.00) | 3,016 / 2 / 2 / 1,009 / 4,265 |
| closure | 3985.2 / 3985.2 | 2029.33 ± 22.21 (1959.83–2161.79) | 1,008 / 2 / 2 / 1,009 / 1,154 |
| mixed | 5746.1 / 5746.4 | 3402.75 ± 68.00 (2819.33–3791.12) | 1,048 / 2 / 8 / 1,009 / 2,558 |

Construction, definitions and every map operation also retain all live/peak fields in [allocations.csv](allocations.csv) and distributions in [summary.json](summary.json). GC medians are per collection, amortized by 32 operations only when comparing the same cadence. These measurements do not identify individual allocation sites.

## Fresh native CLI

Each sample is a new native process/context: source file read, definition/setup, construction of 1000 values, reduction, result printing, pipe drain and exit. All 54 executions printed exactly 499500. Times are milliseconds, median ± MAD (min–max); OS caches were uncontrolled. These are native interpreter processes, not generated JavaScript, JVM/SCI or browser startup measurements.

| Program | Fresh CLI ms |
| --- | --- |
| closure | 4.49 ± 0.05 (4.38–4.76) |
| external | 6.41 ± 0.06 (6.20–6.85) |
| map | 5.53 ± 0.11 (5.15–5.78) |
| mixed | 6.98 ± 0.30 (6.38–7.66) |
| record | 6.41 ± 0.20 (5.90–7.99) |
| reify | 6.18 ± 0.12 (5.59–6.54) |

## Unchanged-fixture regression comparison

The saved baseline is the validated transducer report at `light-cljs-58/transducers`, before the completed protocol/record batch. The runner verified its archive/binaries and each original fixture, then compiled the **unchanged archived interpreter harness** against the current library. New type fixtures remain separate; the general runner’s complete-fixture identity guard is unchanged. [Identity evidence](old-path-identity.json) records every original fixture hash and both library/harness scope. All old language outcomes matched in both reversed-order rounds.

| Old path | Round 1 change | Round 2 change | Allocations baseline / current |
| --- | --- | --- | --- |
| closures_loop/eval | -1.2% | -4.1% | 514 / 514 |
| closures_loop/read_analyze_eval | -0.4% | +0.9% | 2,970 / 2,970 |
| collections/eval | -0.7% | -0.2% | 16,121 / 16,121 |
| collections/read_analyze_eval | +1.8% | -1.3% | 19,176 / 19,176 |
| embedding/invoke | -9.4% | -11.4% | 1 / 1 |
| lazy/eval | -0.3% | +0.3% | 7,571 / 7,571 |
| lazy/read_analyze_eval | -0.1% | -0.5% | 9,277 / 9,277 |
| modules/cached_require | -6.1% | +0.9% | 388 / 388 |
| modules/first_load | -3.2% | +1.4% | 47,094 / 47,394 |

Negative changes are faster, positive changes slower. These comparisons span the whole preceding native feature batch and cannot attribute a change to this benchmark-only patch. Small differences and sign changes between rounds are retained as measured, without wall-clock thresholds or speculative optimization. Full raw distributions include all other old phases.

The expanded current interpreter harness also passed all ordinary/instrumented outcomes. Existing cyclic GC passed cadences 1, 16 and 256, including zero warmed allocator requests; [ordinary GC](interpreter_gc.log) is the valid timing run and [instrumented GC](interpreter_gc-allocations.log) provides counts. Its historical seven-sample cadence is unchanged and supplemental to the new nine-sample harness. Existing bounded-reuse measurements passed their nine samples × 256 analyses in both builds; [ordinary](interpreter_reuse.csv) and [instrumented](interpreter_reuse-allocations.csv) results retain cold-distinct-form versus repeated-retained-form boundaries.

## Verification

- Full `cargo test --locked` and `cargo test --locked --release`: **552 passed and 11 opt-in tests ignored in each**, including the doctest; [debug log](verification-debug.log), [release log](verification-release.log).
- Pinned SCI regeneration/check: **2475 reference cases matched; 419 native tests passed**, one opt-in wrapper ignored because the explicit reference job already ran; [SCI log](verification-sci.log).
- Strict all-target Clippy passed both ordinary and `count-allocations` builds; [ordinary log](verification-clippy.log). Formatting, Python compilation and whitespace checks passed.
- Nine-sample new, existing and matching-baseline runs passed all output/type/retention invariants; fresh CLI outcomes matched across every sample. No feature correctness work or unsupported runtime capability was deferred into this report.
