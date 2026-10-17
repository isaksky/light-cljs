(require '[babashka.fs :as fs] '[clojure.string :as str])
(def here (fs/parent *file*))
(def destination (fs/file "benchmarks/results/compiler-2026-10-16/penpot-phase40"))
(fs/create-dirs destination)
(doseq [file (concat (fs/glob here "*.bb") (fs/glob here "*.log"))]
  (fs/copy file (fs/file destination (fs/file-name file)) {:replace-existing true}))
(spit (fs/file destination "README.md")
"# Native QuickJS profile-guided compilation diagnostic

Production remains phase33. Clean isolated sources are built with Clang
instrumentation (-fprofile-instr-generate), trained on the two captured
grammar invocations, merged with llvm-profdata, then rebuilt with
-fprofile-instr-use. Rust optimization settings remain unchanged. The
PGO executable runs twice around a frozen ordinary baseline; jobs are serial.

| Replay | Invocation milliseconds |
| --- | --- |
| Instrumented training | 12799, 12895 |
| PGO first process | 10471, 10508 |
| Ordinary baseline | 9304, 9322 |
| PGO second process | 10510, 10496 |

Every encoded result matches the capture, but this trained build is about
13% slower than the ordinary build. It is rejected; no PGO flags, profile
data or native code changes are added to production. This single training
experiment does not rule out every compiler/PGO configuration.

Frozen executables, runtime, and training profile remain under
/private/tmp/light-cljs-phase40. Scripts and logs are archived here.
")
(def report "benchmarks/penpot-investigation.md")
(let [text (slurp report) anchor "The previous phase32 comparison was"]
  (when-not (str/includes? text "Phase40 rejects")
    (spit report (str/replace-first text anchor
      (str "Phase40 rejects a native Clang PGO build trained on the captured grammar:\nits matching replays are about 13% slower than the ordinary build. No compiler\nflags or profile data are retained. See `penpot-phase40/` for reproduction.\n\n" anchor)))))
