(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.java.io :as io] '[clojure.string :as str])
(def repo "/Users/admin/src/light-cljs")
(doseq [[phase binary changed] [[93 "light-cljs-quickjs-ready-roots" ["src/compiler_roots.js"]]
                              [94 "light-cljs-quickjs-demand-path" ["src/compiler_roots.js" "src/js_runtime.js" "src/evaluator.rs"]]
                              [95 "light-cljs-quickjs-native-stores" ["src/js_runtime.js"]]]]
  (let [here (fs/file (str "/private/tmp/light-cljs-phase" phase))
        out (fs/file repo "benchmarks/results/compiler-2026-10-16" (str "penpot-phase" phase))]
    (fs/create-dirs out)
    (doseq [file (mapcat #(fs/glob here %) ["*.bb" "*.log" "*.edn" "*.time"])]
      (fs/copy file (fs/file out (fs/file-name file)) {:replace-existing true}))
    (spit (fs/file out "changes.patch")
      (apply str (for [file changed]
        (let [result @(p/process ["git" "diff" "--no-index" "--"
                                  (str (fs/file "/private/tmp/light-cljs-phase87/project" file))
                                  (str (fs/file here "project" file))]
                                 {:out :string :err :string})]
          (assert (= 1 (:exit result)))
          (-> (:out result)
              (str/replace "a/private/tmp/light-cljs-phase87/project/" "a/")
              (str/replace (str "b/private/tmp/light-cljs-phase" phase "/project/") "b/"))))))
    (assert (= 1 (:exit @(p/process ["rg" "-a" "-q" "QJS_SAMPLE" (str (fs/file here binary))]))))
    (spit (fs/file out "source-hashes.txt")
      (:out (apply p/shell {:out :string} "shasum" "-a" "256"
              (concat [(str (fs/file here binary)) (str (fs/file here "main-none.js"))
                       (str (fs/file here "project/vendor/rquickjs-sys/quickjs/quickjs.c"))]
                      (map #(str (fs/file here "project" %)) changed)))))
    (when (= phase 93)
      (fs/create-dirs (fs/file out "stale-engine"))
      (fs/copy (fs/file here "stale-engine/build.log") (fs/file out "stale-engine/build.log") {:replace-existing true})
      (with-open [compressed (io/output-stream (fs/file out "stale-engine/main-none.time.gz"))]
        (p/shell {:out compressed} "gzip" "-c" (str (fs/file here "stale-engine/main-none.time")))))))
