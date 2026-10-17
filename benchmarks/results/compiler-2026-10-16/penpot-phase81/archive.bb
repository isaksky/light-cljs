(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.string :as str])
(def here (fs/parent *file*))
(def out (fs/file "/Users/admin/src/light-cljs/benchmarks/results/compiler-2026-10-16/penpot-phase81"))
(fs/create-dirs out)
(doseq [pattern ["*.bb" "*.log" "*.time" "*.edn"] file (fs/glob here pattern)]
  (fs/copy file (fs/file out (fs/file-name file)) {:replace-existing true}))
(def files ["src/main.rs" "src/lib.rs" "src/compilation_session.rs"])
(def scratch (fs/file here "patch-inputs"))
(doseq [[side root] [["before" "/private/tmp/light-cljs-phase79/project"] ["after" (fs/file here "project")]] file files]
  (fs/create-dirs (fs/parent (fs/file scratch side file)))
  (when (fs/exists? (fs/file root file))
    (fs/copy (fs/file root file) (fs/file scratch side file) {:replace-existing true})))
(let [delta (p/shell {:dir (str scratch) :out :string :continue true} "git" "diff" "--no-index" "--" "before" "after")]
  (assert (= 1 (:exit delta)))
  (spit (fs/file out "changes.patch") (-> (:out delta) (str/replace "a/before/" "a/") (str/replace "b/after/" "b/"))))
(p/shell "cmp" (str (fs/file here "main-none.js")) "/private/tmp/light-cljs-phase42/production/main-none.js")
(spit (fs/file out "equivalence.txt") "Complete output byte-identical to phase42, including the known main ESM syntax defect.\n")
(spit (fs/file out "source-hashes.txt") (:out (apply p/shell {:out :string} "shasum" "-a" "256"
  (concat (map #(str (fs/file here %)) ["light-cljs-session-allocation-probe" "main-none.js" "project/Cargo.lock"])
          (map #(str (fs/file here "project" %)) files)))))
