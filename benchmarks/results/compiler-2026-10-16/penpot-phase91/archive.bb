(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.java.io :as io] '[clojure.string :as str])
(def repo "/Users/admin/src/light-cljs")
(doseq [[phase binary changed] [[91 "light-cljs-macro-timing" ["src/macro_host.rs"]]
                              [92 "light-cljs-quickjs-sampling" ["vendor/rquickjs-sys/quickjs/quickjs.c"]]]]
  (let [here (fs/file (str "/private/tmp/light-cljs-phase" phase))
        out (fs/file repo "benchmarks/results/compiler-2026-10-16" (str "penpot-phase" phase))]
    (fs/create-dirs out)
    (doseq [file (mapcat #(fs/glob here %) ["*.bb" "*.log" "*.edn" "*.txt"])]
      (fs/copy file (fs/file out (fs/file-name file)) {:replace-existing true}))
    (if (= phase 92)
      (with-open [compressed (io/output-stream (fs/file out "main-none.time.gz"))]
        (p/shell {:out compressed} "gzip" "-c" (str (fs/file here "main-none.time"))))
      (fs/copy (fs/file here "main-none.time") (fs/file out "main-none.time") {:replace-existing true}))
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
    (p/shell "cmp" (str (fs/file here "main-none.js")) "/private/tmp/light-cljs-phase42/production/main-none.js")
    (spit (fs/file out "source-hashes.txt")
      (:out (apply p/shell {:out :string} "shasum" "-a" "256"
              (concat [(str (fs/file here binary)) (str (fs/file here "main-none.js"))]
                      (map #(str (fs/file here "project" %)) changed)))))))
