(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.string :as str] '[clojure.java.io :as io])
(def here (fs/parent *file*))
(def out (fs/file "/Users/admin/src/light-cljs/benchmarks/results/compiler-2026-10-16/penpot-phase98"))
(fs/create-dirs out)
(doseq [pattern ["*.bb" "*.rs" "*.log" "*.time" "*.edn"] file (fs/glob here pattern)]
  (fs/copy file (fs/file out (fs/file-name file)) {:replace-existing true}))
(def files ["src/allocation_probe.rs" "src/lib.rs" "src/main.rs" "src/compilation_session.rs"])
(def inputs (fs/file here "patch-inputs"))
(doseq [[side root] [["before" "/private/tmp/light-cljs-phase97/project"] ["after" (fs/file here "project")]] file files
        :when (fs/exists? (fs/file root file))]
  (fs/create-dirs (fs/parent (fs/file inputs side file)))
  (fs/copy (fs/file root file) (fs/file inputs side file) {:replace-existing true}))
(let [result (p/shell {:dir (str inputs) :out :string :continue true} "git" "diff" "--no-index" "--" "before" "after")]
  (assert (= 1 (:exit result)))
  (spit (fs/file out "changes.patch") (-> (:out result) (str/replace "a/before/" "a/") (str/replace "b/after/" "b/"))))
(p/shell "cmp" (str (fs/file here "main-none.js")) "/private/tmp/light-cljs-phase42/production/main-none.js")
(spit (fs/file out "source-hashes.txt")
  (:out (apply p/shell {:out :string} "shasum" "-a" "256"
    (concat (map #(str (fs/file here %)) ["light-cljs-retained-program-probe" "main-none.js"])
            (map #(str (fs/file here "project" %)) files)))))
(let [result (with-open [in (io/input-stream (fs/file here "main-none.js"))]
               (p/shell {:in in :out :string :err :string :continue true} "node" "--check" "--input-type=module"))]
  (assert (= 1 (:exit result)))
  (spit (fs/file out "main-syntax.txt") (str "Byte-identical to phase42. Explicit ESM syntax exit: 1\n" (:err result))))
