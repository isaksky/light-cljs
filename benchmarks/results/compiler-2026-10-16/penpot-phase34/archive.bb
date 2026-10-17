(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.string :as str])
(def here (fs/parent *file*))
(def evidence "benchmarks/results/compiler-2026-10-16/penpot-phase34")
(fs/create-dirs evidence)
(doseq [pattern ["*.bb" "*.log"]
        file (fs/glob here pattern)]
  (fs/copy file (fs/file evidence (fs/file-name file)) {:replace-existing true}))
(spit (fs/file evidence "source-hashes.txt")
  (:out (apply p/shell {:out :string} "shasum" "-a" "256"
    (map str (concat (fs/glob here "runtime-*.js") [(fs/file here "replay.js")])))))
(spit (fs/file evidence "README.md")
"# Native dispatch and private-store experiments

These isolated runtime variants replay the captured Instaparse grammar macro
from phase31 against a phase33 baseline. Every replay's encoded result matches
the capture. None of these variants is retained in production, and these are
not full-build speedup measurements.

| Variant | First invocation ms | Second invocation ms |
| --- | ---: | ---: |
| Phase33 baseline | 9,522 | 9,716 |
| Direct List/EmptyList/Cons methods and masks | 9,124 | 9,154 |
| Also direct IndexedSeq/ChunkedSeq/NodeSeq methods and masks | 8,537 | 8,710 |
| Private seq stores changed to WeakMaps | 9,122 | 9,067 |
| Also vector store changed to WeakMap | 8,944 | 9,071 |
| Avoid Object boxing in two private-brand checks | 9,203 | 9,282 |
| Direct collection methods without masks | 8,962 | 9,090 |

The mask variants are incorrect: an object forged from List.prototype changes
from `[false,false]` to `[true,true]` for seq?/counted?. The preliminary expanded
variant also adds ICounted to NodeSeq, which upstream does not declare. The
final slots-only probe removes masks and excludes Cons and NodeSeq from the
count method; its forged-object result returns to `[false,false]`. It has not
received a complete protocol/receiver/slicing audit or full-suite validation.

WeakMap variants conflict with the runtime's existing no-WeakMap-write seq
pipeline regression and its measured V8 allocation strategy. Their roughly
5–6% replay gain does not establish a worthwhile cross-engine tradeoff. The
boxing probe is smaller and representation-preserving, but its roughly 4%
single-macro improvement has not been tested on a whole build. Production
remains the fully validated phase33 implementation.

Runs were sequential, after the clean phase33 benchmarks, with no overlapping
CPU-heavy build, test or profile. Scripts and raw timing logs are retained
here; generated runtime variants, replay and fingerprints remain under
`/private/tmp/light-cljs-phase34/`. Reproduction uses the phase31 capture and
the existing standalone QuickJS probe.

The next diagnostic measures current per-macro transport and execution, with
nested deferred analysis/emission separated, to choose a larger target.
")
