(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.edn :as edn]
         '[clojure.string :as str])
(def here (fs/parent *file*))
(def repo (fs/file "/Users/admin/src/light-cljs"))
(def output (fs/file repo "benchmarks/results/compiler-2026-10-16/penpot-phase107"))
(assert (= [1426 0 106] (:tests (edn/read-string (slurp (fs/file here "validation.edn"))))))
(fs/create-dirs output)
(def files ["src/v8_evaluator.rs" "src/v8_watchdog.rs"])
(spit (fs/file here "changes.patch")
  (apply str (for [file files]
    (let [before (str "/private/tmp/light-cljs-phase102/project/" file)
          after (str (fs/file here "project" file))
          result (p/shell {:out :string :continue true} "git" "diff" "--no-index" "--" before after)]
      (assert (= 1 (:exit result)))
      (-> (:out result)
        (str/replace "a/private/tmp/light-cljs-phase102/project/" "a/")
        (str/replace "b/private/tmp/light-cljs-phase107/project/" "b/"))))))
(doseq [pattern ["*.bb" "*.log" "*.time" "*.time.txt" "*.edn" "*.patch"] file (fs/glob here pattern)]
  (fs/copy file (fs/file output (fs/file-name file)) {:replace-existing true}))
(doseq [label ["render" "main-none" "main-advanced"]]
  (p/shell "cmp" (str (fs/file here (str label ".js")))
    (str "/private/tmp/light-cljs-phase101/production/" label ".js")))
(spit (fs/file output "source-hashes.txt")
  (:out (apply p/shell {:out :string} "shasum" "-a" "256"
    (concat (map #(str (fs/file here %)) ["light-cljs-v8-guarded-startup" "render.js"
                                         "main-none.js" "main-advanced.js"])
            (map #(str (fs/file here "project" %)) files)))))
(let [files (sort-by str
              (filter fs/regular-file?
                (mapcat #(fs/glob (fs/file repo "research/penpot-light" %) "**")
                  ["frontend/src" "common/src" "vendor" "generated" "shims"])))
      digest (java.security.MessageDigest/getInstance "SHA-256")]
  (doseq [file files]
    (.update digest (.getBytes (str (fs/relativize repo file)) "UTF-8"))
    (.update digest (fs/read-all-bytes file)))
  (let [result (str (count files) " files\n" (format "%064x" (java.math.BigInteger. 1 (.digest digest))) "\n")]
    (assert (= result (slurp (fs/file repo "benchmarks/results/compiler-2026-10-16/penpot-phase42/port-hash.txt"))))
    (spit (fs/file output "port-hash.txt") result)))
(println "Guarded startup evidence archived; all outputs identical and port unchanged")
