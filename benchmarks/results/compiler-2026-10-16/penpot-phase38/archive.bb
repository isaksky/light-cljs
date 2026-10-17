(require '[babashka.fs :as fs] '[clojure.string :as str] '[clojure.java.io :as io])
(def here (fs/parent *file*))
(def destination (fs/file "benchmarks/results/compiler-2026-10-16/penpot-phase38"))
(fs/create-dirs destination)
(doseq [file (concat (fs/glob here "*.bb") (fs/glob here "*.log") [(fs/file here "semantics.js")])]
  (fs/copy file (fs/file destination (fs/file-name file)) {:replace-existing true}))
(defn sha [path]
  (let [digest (java.security.MessageDigest/getInstance "SHA-256")]
    (.update digest (fs/read-all-bytes path))
    (format "%064x" (java.math.BigInteger. 1 (.digest digest)))))
(spit (fs/file destination "source-hashes.txt")
  (str/join "\n" (for [name ["before.c" "after.c" "upper-bound.c" "runtime.js" "probe-before" "probe-after" "probe-upper-bound"]]
    (str (sha (fs/file here name)) "  " name))))
(spit (fs/file destination "README.md")
"# Function.call ownership and frame diagnostics

Production remains at phase33. Neither experiment is retained.

The ownership variant preserves the native frame, realm, nested interrupt
checks and ordinary method lookup, but passes through interpreter-owned
arguments instead of forcing the child call to copy them. External calls
retain COPY_ARGV. The before/after semantic probes both pass 4,019 checks,
including escaped/mapped arguments, mutation, exceptions, bound functions,
Proxies, custom .call lookup and observable native stack frames.

The deliberately invalid upper-bound variant removes the native call frame.
It fails the stack-frame probe as expected and cannot be used in production.

| Replay | Invocation milliseconds |
| --- | --- |
| Baseline, alternating runs | 9221, 9247, 9248, 9301 |
| Ownership variant, alternating runs | 9235, 9319, 9309, 9334 |
| Invalid frame-removal upper bound | 9170, 9290 |
| Final baseline | 9376, 9402 |

Every replay reproduces the exact captured encoded macro result. The valid
variant shows no useful gain; even the unsafe variant improves only about
1–2% relative to the final baseline. Frequent Function.call frames in the
native profile do not establish their overhead as a major bottleneck.

Scripts and logs are archived here; frozen C/runtime sources, executables
and the large phase31 capture remain under /private/tmp/light-cljs-phase38
and /private/tmp/light-cljs-phase31. No production edits or new full-suite
claims follow from these rejected experiments.
")
(def report "benchmarks/penpot-investigation.md")
(def before "The previous phase32 comparison was")
(def note "Phase38 also rejects a QuickJS `Function.call` ownership optimization: repeated\nmacro replays show no useful gain. A deliberately invalid frame-removal variant\nfails the stack-frame semantic probe and saves only about 1–2%. Both diagnostics\nreproduce the captured result; neither changes production. Evidence is in\n`results/compiler-2026-10-16/penpot-phase38/`.\n\n")
(let [text (slurp report)]
  (when-not (str/includes? text "Phase38 also rejects")
    (spit report (str/replace-first text before (str note before)))))
