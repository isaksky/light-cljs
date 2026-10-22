# Generated projects: light-cljs vs shadow-cljs, advanced builds (October 22, 2026)

Projects from [the generator](generated-project/README.md), seed 42, at 5k, 200k
and 800k nonblank lines. Both compilers build at advanced: `light-cljs release
bench` (default `:advanced`) and `clojure -M:shadow release bench` (Closure
advanced). Every output printed the oracle's checksum for input 42 and 3
iterations, and `check-waste.bb --strict` passed for both.

Reproduce, one project at a time and with no other heavy work running:

```sh
bb benchmarks/generated-project/generate.bb /tmp/g200k --seed 42 --lines 200k
bb benchmarks/generated-project/compare.bb /tmp/g200k --light target/release/light-cljs --runs 3 --warm-jvm 4
```

## Setup

- 24-core Mac, macOS 15.7.3, 32 GB, OpenJDK 25.0.2. One job at a time.
- light-cljs built from `ac88cb2`. shadow-cljs 3.5.3 with the generated `-Xmx8g`.
- Uncached: light `--no-cache` with `.light-cljs` removed; shadow with
  `.shadow-cljs/` removed. Cached: one warm-up build, then rebuilds with no source
  change. Light reuses its whole output then; shadow still runs Closure.
- Shadow command-line times include JVM startup. The in-JVM rows repeat
  `shadow.cljs.devtools.api/release` in one JVM, as a running shadow server does.
- Medians of 5 runs (5k), 3 runs (200k) and 1 run (800k). Peak RSS from `/usr/bin/time -l`.

## Build time and peak memory

| Size | light uncached | light cached | shadow uncached | shadow cached | shadow in one JVM (1st / steady) |
|---|---|---|---|---|---|
| 5k (5,046 lines, 11 workers) | 0.43 s, 0.15 GB | 0.09 s, 0.05 GB | 10.2 s, 1.53 GB | 8.1 s, 1.00 GB | 4.6 s / 1.7 s |
| 200k (199,129 lines, 450 workers) | 29.9 s, 1.18 GB | 4.8 s, 1.02 GB | 43.6 s, 4.02 GB | 35.4 s, 3.30 GB | 29.5 s / 24.4 s, 5.3 GB |
| 800k (795,693 lines, 1,808 workers) | 310 s, 6.37 GB | 30.2 s, 4.05 GB | 181 s, 7.59 GB | 162 s, 7.93 GB | not run |

At 800k shadow's first uncached build took 422 s at 8.6 GB, close to its 8 GB
heap; the 181 s row is the second, ordinary run.

## Output size

| Size | light bytes / gzip -9 | shadow bytes / gzip -9 | light gzip vs shadow |
|---|---|---|---|
| 5k | 214,145 / 66,471 | 263,193 / 57,701 | +15% |
| 200k | 4,097,518 / 1,261,793 | 4,067,637 / 1,002,909 | +26% |
| 800k | 16,149,220 / 4,930,779 | 15,710,764 / 3,894,315 | +27% |

Waste markers kept at 200k (all `:removable` markers were removed by both):

| Category | light | shadow |
|---|---|---|
| unused arities (best-effort) | 86 / 204 | 51 / 204 |
| unreachable defmethods (retained) | 406 / 406 | 406 / 406 |

At 800k: unused arities 329 / 779 (light) and 215 / 779 (shadow).

## Findings

1. **Uncached compile time is superlinear in light-cljs.** 200k to 800k is 4x the
   code; light takes 10.4x longer (29.9 s to 310 s) and shadow 4.2x. Light wins at
   200k and loses at 800k.
2. **A cached rebuild with no change also grows superlinearly**: 0.09 s, 4.8 s,
   30.2 s (6.3x from 200k to 800k) and 4 GB at 800k, though nothing is compiled.
3. **gzip is 26-27% worse than shadow at the same raw size.** At 200k light's
   output has 98,232 distinct identifiers against shadow's 26,279: locals get
   program-unique names (`tVY`, `tVZ`, ...) where Closure reuses short names in
   each function scope. Light also emits 9,410 `(function(){...})()` expression
   wrappers and 3,177 calls to the truthiness helper.
4. **Light keeps more unused arities** (86 vs 51 at 200k).
