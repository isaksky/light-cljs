(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.string :as str])
(def here (fs/parent *file*))
(def out (fs/file "/Users/admin/src/light-cljs/benchmarks/results/compiler-2026-10-16/penpot-phase85"))
(fs/create-dirs out)
(def counts (reduce #(mapv + %1 %2) [0 0 0]
  (map #(mapv parse-long (rest %)) (re-seq #"test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored" (slurp (fs/file here "tests-v8-4threads.log"))))))
(assert (= counts [1409 0 106]) counts)
(spit (fs/file here "validation.edn") (pr-str {:tests counts :clippy :passed :format :passed :node-workflows 3}))
(doseq [pattern ["*.bb" "*.log" "*.time" "*.edn"] file (fs/glob here pattern)]
  (fs/copy file (fs/file out (fs/file-name file)) {:replace-existing true}))
(def files ["src/analyzer.rs" "src/arena.rs" "src/compilation_session.rs" "src/layered.rs" "src/dev_build.rs"])
(def scratch (fs/file here "validated-patch-inputs"))
(doseq [[side root] [["before" "/private/tmp/light-cljs-phase76/project"] ["after" (fs/file here "project")]] file files]
  (fs/create-dirs (fs/parent (fs/file scratch side file)))
  (fs/copy (fs/file root file) (fs/file scratch side file) {:replace-existing true}))
(let [delta (p/shell {:dir (str scratch) :out :string :continue true} "git" "diff" "--no-index" "--" "before" "after")]
  (assert (= 1 (:exit delta)))
  (spit (fs/file out "combined-changes.patch") (-> (:out delta) (str/replace "a/before/" "a/") (str/replace "b/after/" "b/"))))
(p/shell "cmp" (str (fs/file here "main-none.js")) "/private/tmp/light-cljs-phase42/production/main-none.js")
(spit (fs/file out "equivalence.txt") "Complete output byte-identical to phase42, including the known main ESM syntax defect.\n")
(fs/create-dirs (fs/file out "clean"))
(doseq [pattern ["*.bb" "*.time" "*.edn"] file (fs/glob (fs/file here "clean") pattern)]
  (fs/copy file (fs/file out "clean" (fs/file-name file)) {:replace-existing true}))
(doseq [label ["render" "main-none" "main-advanced"]]
  (p/shell "cmp" (str (fs/file here "clean" (str label ".js"))) (str "/private/tmp/light-cljs-phase42/production/" label ".js")))
(spit (fs/file out "source-hashes.txt") (:out (apply p/shell {:out :string} "shasum" "-a" "256"
  (concat (map #(str (fs/file here %)) ["light-cljs-compact-program-capacity" "main-none.js" "clean/render.js" "clean/main-none.js" "clean/main-advanced.js" "project/Cargo.lock"])
          (map #(str (fs/file here "project" %)) files)))))
