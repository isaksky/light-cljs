(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.string :as str] '[clojure.java.io :as io])
(doseq [[phase baseline binary] [[65 58 "light-cljs-unused-form"] [66 52 "light-cljs-quickjs-unused-form"]]]
  (let [here (fs/file (str "/private/tmp/light-cljs-phase" phase))
        out (fs/file (str "/Users/admin/src/light-cljs/benchmarks/results/compiler-2026-10-16/penpot-phase" phase))
        scratch (fs/file here "archive-inputs")]
    (fs/create-dirs out)
    (doseq [pattern ["*.bb" "*.log" "*.time" "*.edn"] file (fs/glob here pattern)]
      (fs/copy file (fs/file out (fs/file-name file)) {:replace-existing true}))
    (doseq [[side root] [["before" (str "/private/tmp/light-cljs-phase" baseline "/project")] ["after" (fs/file here "project")]]
            file ["compiler_bootstrap.rs" "macro_host.rs"]]
      (fs/create-dirs (fs/file scratch side "src"))
      (fs/copy (fs/file root "src" file) (fs/file scratch side "src" file) {:replace-existing true}))
    (let [delta (p/shell {:dir (str scratch) :out :string :continue true} "git" "diff" "--no-index" "--" "before" "after")]
      (assert (= 1 (:exit delta)))
      (spit (fs/file out "unused-core-form.patch") (-> (:out delta) (str/replace "a/before/" "a/") (str/replace "b/after/" "b/"))))
    (p/shell "cmp" (str (fs/file here "main-none.js")) "/private/tmp/light-cljs-phase42/production/main-none.js")
    (spit (fs/file out "equivalence.txt") "Complete output byte-identical to phase42, including the unchanged known main ESM syntax defect.\n")
    (spit (fs/file out "source-hashes.txt")
      (:out (apply p/shell {:out :string} "shasum" "-a" "256"
              (map #(str (fs/file here %)) [binary "main-none.js" "project/src/compiler_bootstrap.rs" "project/src/macro_host.rs" "project/src/js_runtime.js" "project/Cargo.lock"]))))))
