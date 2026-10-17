(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.string :as str])
(doseq [phase [63 64]]
  (let [here (fs/file (str "/private/tmp/light-cljs-phase" phase))
        out (fs/file (str "/Users/admin/src/light-cljs/benchmarks/results/compiler-2026-10-16/penpot-phase" phase))
        scratch (fs/file here "patch-inputs")]
    (fs/create-dirs out)
    (doseq [pattern ["*.bb" "*.log" "*.time" "*.edn"] file (fs/glob here pattern)]
      (fs/copy file (fs/file out (fs/file-name file)) {:replace-existing true}))
    (doseq [[side root] [["before" "/private/tmp/light-cljs-phase58/project"] ["after" (fs/file here "project")]]]
      (fs/create-dirs (fs/file scratch side "src"))
      (fs/copy (fs/file root "src/js_runtime.js") (fs/file scratch side "src/js_runtime.js") {:replace-existing true}))
    (let [delta (p/shell {:dir (str scratch) :out :string :continue true} "git" "diff" "--no-index" "--" "before" "after")]
      (assert (= 1 (:exit delta)))
      (spit (fs/file out "active-ancestor-stack.patch") (-> (:out delta) (str/replace "a/before/" "a/") (str/replace "b/after/" "b/"))))
    (spit (fs/file out "equivalence.txt")
      (:out (p/shell {:out :string} "bb" "/private/tmp/light-cljs-phase61/compare-runtime.bb" (str phase))))
    (spit (fs/file out "source-hashes.txt")
      (:out (apply p/shell {:out :string} "shasum" "-a" "256"
              (map #(str (fs/file here %)) [(if (= phase 63) "light-cljs-active-stack" "light-cljs-quickjs-active-stack") "main-none.js" "project/src/js_runtime.js" "project/Cargo.lock"])))))
  )
