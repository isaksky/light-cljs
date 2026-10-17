(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.string :as str]
         '[clojure.java.io :as io])
(def here (fs/parent *file*))
(def output (fs/file "/Users/admin/src/light-cljs/benchmarks/results/compiler-2026-10-16/penpot-phase104"))
(fs/create-dirs output)
(p/shell "cmp" (str (fs/file here "main.js")) "/private/tmp/light-cljs-phase102/main-none.js")
(doseq [pattern ["*.bb" "*.log" "*.edn" "*.patch" "*.txt"] file (fs/glob here pattern)
        :when (not= "sample.txt" (str (fs/file-name file)))]
  (fs/copy file (fs/file output (fs/file-name file)) {:replace-existing true}))
(doseq [name ["sample.txt" "main.cpuprofile"]]
  (with-open [out (io/output-stream (fs/file output (str name ".gz")))]
    (p/shell {:out out} "gzip" "-c" (str (fs/file here name)))))
(let [result (p/shell {:out :string :continue true} "git" "diff" "--no-index" "--"
               "/private/tmp/light-cljs-phase102/project/src/v8_evaluator.rs"
               (str (fs/file here "project/src/v8_evaluator.rs")))]
  (assert (= 1 (:exit result)))
  (spit (fs/file output "changes.patch")
    (-> (:out result)
      (str/replace "a/private/tmp/light-cljs-phase102/project/" "a/")
      (str/replace "b/private/tmp/light-cljs-phase104/project/" "b/"))))
(spit (fs/file output "source-hashes.txt")
  (:out (p/shell {:out :string} "shasum" "-a" "256"
    (str (fs/file here "light-cljs-current-v8-profile")) (str (fs/file here "main.js"))
    (str (fs/file here "project/src/v8_evaluator.rs"))
    (str (fs/file here "project/src/js_runtime.js"))
    (str (fs/file here "profile-runtime.js"))
    (str (fs/file here "main.cpuprofile")) (str (fs/file here "sample.txt")))))
