(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.string :as str])
(def here (fs/parent *file*))
(def out (fs/file "/Users/admin/src/light-cljs/benchmarks/results/compiler-2026-10-16/penpot-phase70"))
(fs/create-dirs out)
(doseq [dir ["" "clean" "heaps"]]
  (fs/create-dirs (fs/file out dir))
  (doseq [pattern ["*.bb" "*.log" "*.time" "*.edn"] file (fs/glob (fs/file here dir) pattern)]
    (fs/copy file (fs/file out dir (fs/file-name file)) {:replace-existing true})))
(def files ["src/emitter.rs" "src/terse.rs" "tests/evaluation/evaluator.rs"])
(def scratch (fs/file here "patch-inputs"))
(doseq [[side root] [["before" "/private/tmp/light-cljs-phase69/project"] ["after" (fs/file here "project")]] file files]
  (fs/create-dirs (fs/parent (fs/file scratch side file)))
  (fs/copy (fs/file root file) (fs/file scratch side file) {:replace-existing true}))
(let [delta (p/shell {:dir (str scratch) :out :string :continue true} "git" "diff" "--no-index" "--" "before" "after")]
  (assert (= 1 (:exit delta)))
  (spit (fs/file out "changes.patch") (-> (:out delta) (str/replace "a/before/" "a/") (str/replace "b/after/" "b/"))))
(doseq [[actual baseline] [["main-none.js" "main-none"] ["clean/render.js" "render"] ["clean/main-none.js" "main-none"] ["clean/main-advanced.js" "main-advanced"] ["heaps/main-128.js" "main-none"]]]
  (p/shell "cmp" (str (fs/file here actual)) (str "/private/tmp/light-cljs-phase42/production/" baseline ".js")))
(spit (fs/file out "equivalence.txt") "Every complete output is byte-identical to phase42. Both main outputs retain the known ESM syntax defect; no application execution is claimed.\n")
(spit (fs/file out "source-hashes.txt") (:out (apply p/shell {:out :string} "shasum" "-a" "256"
  (concat (map #(str (fs/file here %)) ["light-cljs-unit-hygiene" "main-none.js" "clean/render.js" "clean/main-none.js" "clean/main-advanced.js" "heaps/main-128.js" "project/Cargo.lock"])
          (map #(str (fs/file here "project" %)) files)))))
