(require '[babashka.fs :as fs] '[clojure.string :as str])
(def here (fs/parent *file*))
(def destination (fs/file "benchmarks/results/compiler-2026-10-16/penpot-phase39"))
(fs/create-dirs destination)
(doseq [file (concat (fs/glob here "*.bb") (fs/glob here "*.log")
                    [(fs/file here "time-gated-profile.c") (fs/file here "replay-summary.edn")])]
  (fs/copy file (fs/file destination (fs/file-name file)) {:replace-existing true}))
(spit (fs/file destination "README.md")
"# Time-gated JavaScript sampling and diagnostic upper bounds

Production remains phase33. An isolated QuickJS build samples the JavaScript
stack at interrupt checks at least 1 ms apart, using CLOCK_MONOTONIC. The
captured grammar replay returns the exact expected descriptor in both trials
(9344/9366 ms), with 16,429 eligible samples and zero dropped table entries.

This is time-gated cooperative sampling, not an asynchronous CPU profiler.
Native functions and long gaps between polls can be charged to the next
JavaScript endpoint; no percentage here establishes exact wall time.
Anonymous generated functions with identical source coordinates also merge.
The leading runtime endpoints are publicMethod, private brand predicates,
externalMethod and the declaration-demand callback, consistent with the
earlier native profile. The callback is not a compiler-global root read.

Separate non-instrumented replay experiments:

| Variant | First / second invocation ms |
| --- | ---: |
| Baseline | 9258 / 9263 |
| Normal first call, then demand entirely disabled | 9308 / 7918 |
| WeakMap memo of native protocol membership | 9157 / 9257 |

All encoded results match. Disabling demand is deliberately restricted to
the already initialized diagnostic replay; it is not valid for general
compilation. Its ~15% saving is an upper bound for removing this machinery
from this invocation. The memo variant shows no useful gain and its general
mutation semantics were not validated. Neither variant is retained.

The large replay, source/runtime snapshots and executables remain in the
phase31, phase38 and phase39 temporary directories referenced by the scripts.
")
(def report "benchmarks/penpot-investigation.md")
(let [text (slurp report) anchor "The previous phase32 comparison was"]
  (when-not (str/includes? text "Phase39 time-gated")
    (spit report (str/replace-first text anchor
      (str "Phase39 time-gated interrupt sampling again points to protocol dispatch and\ndeclaration demand. Disabling demand after the replay's first call saves about\n15%, but is not valid for general compilation. Native membership memoization\nshows no useful gain. These diagnostic variants are not retained; the sampling\nlimitations and exact-result checks are recorded in `penpot-phase39/`.\n\n" anchor)))))
