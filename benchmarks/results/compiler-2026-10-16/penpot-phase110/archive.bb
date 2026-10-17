(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.edn :as edn])
(def here (fs/parent *file*))
(def output (fs/file "/Users/admin/src/light-cljs/benchmarks/results/compiler-2026-10-16/penpot-phase110"))
(assert (= [1440 0 106] (:tests (edn/read-string (slurp (fs/file here "validation.edn"))))))
(fs/create-dirs output)
(def files ["src/v8_evaluator.rs" "src/v8_buffers.rs" "src/v8_buffer_tests.rs"])
(spit (fs/file here "changes.patch")
  (apply str (for [file files]
    (let [result (p/shell {:out :string :continue true} "diff" "-u"
                   "--label" (str "a/" file) "--label" (str "b/" file)
                   (str "/private/tmp/light-cljs-phase109/project/" file)
                   (str (fs/file here "project" file)))]
      (assert (= 1 (:exit result))) (:out result)))))
(doseq [pattern ["*.bb" "*.log" "*.time" "*.edn" "*.patch"] file (fs/glob here pattern)]
  (fs/copy file (fs/file output (fs/file-name file)) {:replace-existing true}))
(doseq [label ["render" "main-none" "main-advanced"]]
  (p/shell "cmp" (str (fs/file here (str label ".js")))
    (str "/private/tmp/light-cljs-phase109/" label ".js")))
(doseq [label ["baseline-main" "default-heap-main"]]
  (p/shell "cmp" (str (fs/file here (str label ".js"))) (str (fs/file here "main-none.js"))))
(spit (fs/file output "source-hashes.txt")
  (:out (apply p/shell {:out :string} "shasum" "-a" "256"
    (concat (map #(str (fs/file here %)) ["light-cljs-v8-aggregate-budget" "render.js" "main-none.js" "main-advanced.js" "baseline-main.js" "default-heap-main.js"])
            (map #(str (fs/file here "project" %)) files)))))
(fs/copy "/private/tmp/light-cljs-phase109/engine-hashes.txt" (fs/file output "engine-hashes.txt") {:replace-existing true})
(let [repo (fs/file "/Users/admin/src/light-cljs")
      sources (sort-by str (filter fs/regular-file?
                (mapcat #(fs/glob (fs/file repo "research/penpot-light" %) "**")
                  ["frontend/src" "common/src" "vendor" "generated" "shims"])))
      digest (java.security.MessageDigest/getInstance "SHA-256")]
  (doseq [file sources]
    (.update digest (.getBytes (str (fs/relativize repo file)) "UTF-8"))
    (.update digest (fs/read-all-bytes file)))
  (let [result (str (count sources) " files\n" (format "%064x" (java.math.BigInteger. 1 (.digest digest))) "\n")]
    (assert (= result (slurp (fs/file repo "benchmarks/results/compiler-2026-10-16/penpot-phase42/port-hash.txt"))))
    (spit (fs/file output "port-hash.txt") result)))
(println "Aggregate budget evidence archived")
