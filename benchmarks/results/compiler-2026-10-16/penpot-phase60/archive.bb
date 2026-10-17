(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.string :as str])
(def here (fs/parent *file*))
(def out (fs/file "/Users/admin/src/light-cljs/benchmarks/results/compiler-2026-10-16/penpot-phase60"))
(fs/create-dirs out)
(doseq [pattern ["*.bb" "*.log" "*.time" "*.edn" "*.cpuprofile"] file (fs/glob here pattern)]
  (fs/copy file (fs/file out (fs/file-name file)) {:replace-existing true}))
(def scratch (fs/file here "patch-inputs"))
(doseq [[side root] [["before" "/private/tmp/light-cljs-phase58/project"] ["after" (fs/file here "project")]]]
  (fs/create-dirs (fs/file scratch side "src"))
  (fs/copy (fs/file root "src/v8_evaluator.rs") (fs/file scratch side "src/v8_evaluator.rs") {:replace-existing true}))
(let [delta (p/shell {:dir (str scratch) :out :string :continue true} "git" "diff" "--no-index" "--" "before" "after")]
  (assert (= 1 (:exit delta)))
  (spit (fs/file out "cpu-profile-instrumentation.patch") (-> (:out delta) (str/replace "a/before/" "a/") (str/replace "b/after/" "b/"))))
(spit (fs/file out "source-hashes.txt")
  (:out (apply p/shell {:out :string} "shasum" "-a" "256"
          (map #(str (fs/file here %)) ["light-cljs-profile" "main-none.js" "project/src/v8_evaluator.rs" "project/Cargo.lock" "main.cpuprofile"]))))
