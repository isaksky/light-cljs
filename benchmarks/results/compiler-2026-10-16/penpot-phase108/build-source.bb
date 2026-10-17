(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.java.io :as io]
         '[clojure.string :as str])
(def here (fs/parent *file*))
(def env {"CARGO_TARGET_DIR" (str (fs/file here "target"))
          "V8_FROM_SOURCE" "1"
          "LIBCLANG_PATH" "/opt/homebrew/opt/llvm@23/lib"
          "CARGO_PROFILE_RELEASE_LTO" "thin"
          "CARGO_PROFILE_RELEASE_CODEGEN_UNITS" "1"})
(def selected (:out (p/shell {:dir (str (fs/file here "project")) :out :string}
                     "cargo" "tree" "--locked" "-p" "v8" "--depth" "0")))
(assert (str/includes? selected (str (fs/file here "v8"))) selected)
(spit (fs/file here "selected-engine.txt") selected)
(spit (fs/file here "build-command.edn")
  (pr-str {:dir (str (fs/file here "project")) :environment env
           :command ["cargo" "build" "--release" "--locked" "-j" "20"]}))
(with-open [out (io/output-stream (fs/file here "build-source.log"))]
  (let [result (p/shell {:dir (str (fs/file here "project")) :out out :err :out
                         :extra-env env :continue true}
                 "cargo" "build" "--release" "--locked" "-j" "20")]
    (spit (fs/file here "build-result.edn") (pr-str (select-keys result [:exit])))
    (assert (zero? (:exit result)) "Pinned V8 source build failed; see build-source.log")))
(fs/copy (fs/file here "target/release/light-cljs")
  (fs/file here "light-cljs-v8-source-baseline") {:replace-existing true})
(println "Pinned unmodified V8 source build completed")
