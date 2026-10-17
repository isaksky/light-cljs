(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.java.io :as io])
(def here (fs/parent *file*))
(def project "/private/tmp/light-cljs-phase21/project")
(def target "/private/tmp/light-cljs-phase13-storage-profile/target")
(defn build [name flags]
  (println "Building" name) (flush)
  (with-open [out (io/output-stream (fs/file here (str name "-build.log")))]
    (p/shell {:dir project :extra-env (merge {"CARGO_TARGET_DIR" target} flags) :out out :err :out}
      "cargo" "build" "--release" "--example" "replay_probe" "--bin" "light-cljs"))
  (doseq [[src dst] [["release/examples/replay_probe" (str "probe-" name)] ["release/light-cljs" (str "light-cljs-" name)]]]
    (fs/copy (fs/file target src) (fs/file here dst) {:replace-existing true})))
(build "generate" {"CFLAGS" "-fprofile-instr-generate" "RUSTFLAGS" "-C link-arg=-fprofile-instr-generate"})
(println "Training on captured grammar") (flush)
(with-open [out (io/output-stream (fs/file here "training.log"))]
  (p/shell {:extra-env {"LLVM_PROFILE_FILE" (str (fs/file here "training-%p.profraw"))} :out out :err :out}
    (str (fs/file here "probe-generate")) (str (fs/file here "runtime.js")) "/private/tmp/light-cljs-phase31/replay.js"))
(apply p/shell (concat ["xcrun" "llvm-profdata" "merge" "-output" (str (fs/file here "training.profdata"))]
                      (map str (fs/glob here "training-*.profraw"))))
(build "pgo" {"CFLAGS" (str "-fprofile-instr-use=" (fs/file here "training.profdata"))})
(doseq [[name executable] [["pgo-first" (str (fs/file here "probe-pgo"))]
                          ["baseline" "/private/tmp/light-cljs-phase38/probe-before"]
                          ["pgo-second" (str (fs/file here "probe-pgo"))]]]
  (println "Replay" name) (flush)
  (with-open [out (io/output-stream (fs/file here (str name ".log")))]
    (p/shell {:out out :err :out} "/usr/bin/time" "-l" executable (str (fs/file here "runtime.js")) "/private/tmp/light-cljs-phase31/replay.js")))
