(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.string :as str]
         '[clojure.edn :as edn] '[clojure.java.io :as io])
(def here (fs/parent *file*))
(def out (fs/file "/Users/admin/src/light-cljs/benchmarks/results/compiler-2026-10-16/penpot-phase99"))
(assert (= [1418 0 106] (:tests (edn/read-string (slurp (fs/file here "validation.edn"))))))
(fs/create-dirs out)
(doseq [pattern ["*.bb" "*.log" "*.time" "*.edn"] file (fs/glob here pattern)]
  (fs/copy file (fs/file out (fs/file-name file)) {:replace-existing true}))
(fs/create-dirs (fs/file out "clean"))
(doseq [pattern ["*.bb" "*.time" "*.edn"] file (fs/glob (fs/file here "clean") pattern)]
  (fs/copy file (fs/file out "clean" (fs/file-name file)) {:replace-existing true}))
(def files ["src/analyzer.rs" "src/emitter.rs" "src/compilation_session.rs"])
(def inputs (fs/file here "archive-patch-inputs"))
(doseq [[side root] [["before" "/private/tmp/light-cljs-phase97/project"] ["after" (fs/file here "project")]] file files]
  (fs/create-dirs (fs/parent (fs/file inputs side file)))
  (fs/copy (fs/file root file) (fs/file inputs side file) {:replace-existing true}))
(let [result (p/shell {:dir (str inputs) :out :string :continue true} "git" "diff" "--no-index" "--" "before" "after")]
  (assert (= 1 (:exit result)))
  (spit (fs/file out "changes.patch") (-> (:out result) (str/replace "a/before/" "a/") (str/replace "b/after/" "b/"))))
(doseq [label ["render" "main-none" "main-advanced"]]
  (let [output (fs/file here "clean" (str label ".js"))]
    (p/shell "cmp" (str output) (str "/private/tmp/light-cljs-phase42/production/" label ".js"))
    (let [result (with-open [in (io/input-stream output)]
                   (p/shell {:in in :out :string :err :string :continue true} "node" "--check" "--input-type=module"))]
      (assert (= (:exit result) (if (= label "render") 0 1)))
      (spit (fs/file out "clean" (str label "-syntax.txt"))
        (str "Byte-identical to phase42. Explicit ESM syntax exit: " (:exit result) "\n" (:err result))))))
(spit (fs/file out "source-hashes.txt")
  (:out (apply p/shell {:out :string} "shasum" "-a" "256"
    (concat (map #(str (fs/file here %)) ["light-cljs-prepared-deferred-units" "main-none.js" "clean/render.js" "clean/main-none.js" "clean/main-advanced.js"])
            (map #(str (fs/file here "project" %)) files)))))
(println "Archived validated V8 prepared-unit candidate")
