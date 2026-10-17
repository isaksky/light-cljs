# Phase11 cold Penpot comparison

Native sequence cursors avoid redispatching ISeqable during first/rest/next.
The regression covers explicit ISeqable, ISeq and INext overrides in emitted
applications at every optimization level and inside source macros. The complete
suite passes 1,378 tests, zero failures, 106 ignored. Strict Clippy, formatting
and all three direct Node workflows pass.

All three light-cljs jobs used fresh processes, one at a time with no overlapping
tests, builds or profiling. Render was measured before the size-ceiling-only
test adjustment; the two main builds used the subsequently validated executable.
The production code is the same. `results.edn` records exact commands and results.
The unchanged shadow comparison is the fresh phase10 run: release 62.09 s /
4,086,693,888 bytes RSS; compile 33.02 s / 2,430,648,320 bytes RSS. See that
directory for the shadow commands, logs and timings.

`equivalence.txt` compares application bodies with phase6b after excluding the
embedded runtime and records complete-output syntax checks. Penpot has not been
bundled and executed, so these checks do not establish end-to-end correctness.
The runtime grows by 767 readable bytes. Eight advanced fixture ceilings moved
to the next 1,000 bytes after overages of 11–362 bytes, justified by the semantic
fix and measured render speedup from 30.66 to 27.32 seconds. Retain this rationale
in the eventual commit message.

`source-hashes.txt` identifies the executable, implementation patch and untracked
name-index source saved in `/private/tmp/light-cljs-phase11/`. `port-hash.txt`
hashes each sorted repository-relative source path followed by its exact bytes
for the five unchanged port source roots. Shadow main also includes render,
rasterizer and code-highlight; light main includes its five lazy modules, with
render measured separately.
