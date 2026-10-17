(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.string :as str])
(doseq [[phase baseline binary] [[105 102 "light-cljs-lazy-error-paths"]
                                [106 101 "light-cljs-lazy-error-paths-quickjs"]]]
  (let [here (fs/file (str "/private/tmp/light-cljs-phase" phase))
        output (fs/file (str "/Users/admin/src/light-cljs/benchmarks/results/compiler-2026-10-16/penpot-phase" phase))
        before (str "/private/tmp/light-cljs-phase" baseline "/project/")
        after (str here "/project/")]
    (fs/create-dirs output)
    (doseq [pattern ["*.bb" "*.log" "*.edn" "*.time" "*.txt"]
            file (fs/glob here pattern)]
      (fs/copy file (fs/file output (fs/file-name file)) {:replace-existing true}))
    (when (= phase 105)
      (doseq [name ["replay-differential.js" "paths-differential.js"]]
        (fs/copy (fs/file here name) (fs/file output name) {:replace-existing true})))
    (let [result (p/shell {:out :string :continue true} "git" "diff" "--no-index" "--"
                   (str before "src/js_runtime.js") (str after "src/js_runtime.js"))]
      (assert (= 1 (:exit result)))
      (spit (fs/file output "changes.patch")
        (-> (:out result) (str/replace (str "a" before) "a/") (str/replace (str "b" after) "b/"))))
    (spit (fs/file output "hashes.txt")
      (:out (p/shell {:out :string} "shasum" "-a" "256"
              (str (fs/file here binary)) (str (fs/file here "main-none.js"))
              (str (fs/file here "baseline-main.js")) (str (fs/file here "project/src/js_runtime.js")))))))
