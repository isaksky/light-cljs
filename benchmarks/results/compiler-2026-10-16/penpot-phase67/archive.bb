(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.string :as str])
(doseq [[phase baseline files binary]
        [[67 58 ["src/emitter.rs"] "light-cljs-deferred-stats"]
         [68 58 ["src/emitter.rs" "src/evaluator.rs" "src/compiler_bootstrap.rs" "src/compilation_session.rs" "src/v8_evaluator.rs" "src/v8_services.js"] "light-cljs-namespace-cache"]
         [69 68 ["src/v8_evaluator.rs" "src/v8_services.js"] "light-cljs-namespace-cache"]]]
  (let [here (fs/file (str "/private/tmp/light-cljs-phase" phase))
        out (fs/file (str "/Users/admin/src/light-cljs/benchmarks/results/compiler-2026-10-16/penpot-phase" phase))
        scratch (fs/file here "patch-inputs")]
    (fs/create-dirs out)
    (doseq [pattern ["*.bb" "*.log" "*.time" "*.edn"] file (fs/glob here pattern)]
      (fs/copy file (fs/file out (fs/file-name file)) {:replace-existing true}))
    (doseq [[side root] [["before" (str "/private/tmp/light-cljs-phase" baseline "/project")] ["after" (fs/file here "project")]] file files]
      (fs/create-dirs (fs/parent (fs/file scratch side file)))
      (fs/copy (fs/file root file) (fs/file scratch side file) {:replace-existing true}))
    (let [delta (p/shell {:dir (str scratch) :out :string :continue true} "git" "diff" "--no-index" "--" "before" "after")]
      (assert (= 1 (:exit delta)))
      (spit (fs/file out "changes.patch") (-> (:out delta) (str/replace "a/before/" "a/") (str/replace "b/after/" "b/"))))
    (p/shell "cmp" (str (fs/file here "main-none.js")) "/private/tmp/light-cljs-phase42/production/main-none.js")
    (spit (fs/file out "equivalence.txt") "Complete output byte-identical to phase42, including the known main ESM syntax defect.\n")
    (spit (fs/file out "source-hashes.txt") (:out (apply p/shell {:out :string} "shasum" "-a" "256"
      (concat (map #(str (fs/file here %)) [binary "main-none.js" "project/Cargo.lock"])
              (map #(str (fs/file here "project" %)) files)))))))
