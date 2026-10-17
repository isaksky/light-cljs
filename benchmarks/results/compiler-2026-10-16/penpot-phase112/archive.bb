(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.edn :as edn])
(def here (fs/parent *file*))
(def root (fs/file "/Users/admin/src/light-cljs"))
(def output (fs/file root "benchmarks/results/compiler-2026-10-16/penpot-phase112"))
(def validation (edn/read-string (slurp (fs/file here "validation.edn"))))
(assert (= [1442 0 106] (:tests validation)))
(assert (= [1406 0 106] (:quickjs-tests validation)))
(fs/create-dirs output)
(doseq [pattern ["*.bb" "*.log" "*.edn"] file (fs/glob here pattern)]
  (fs/copy file (fs/file output (fs/file-name file)) {:replace-existing true}))
(fs/create-dirs (fs/file output "bench"))
(doseq [pattern ["*.log" "*.txt" "*.edn"] file (fs/glob (fs/file here "bench") pattern)]
  (fs/copy file (fs/file output "bench" (fs/file-name file)) {:replace-existing true}))
(def before (edn/read-string (slurp (fs/file here "production-before-import.edn"))))
(def new-files (concat ["src/compiler_evaluator.rs"]
                      (map #(str "src/" (fs/file-name %)) (fs/glob (fs/file here "project/src") "v8*"))))
(def files (concat (keys before) new-files))
(spit (fs/file output "integration.patch")
  (apply str (for [file files]
    (let [old (if (contains? before file) (str (fs/file here "before-native-import" file)) "/dev/null")
          result (p/shell {:out :string :continue true} "diff" "-u" "--label" (str "a/" file) "--label" (str "b/" file)
                         old (str (fs/file here "project" file)))]
      (assert (#{0 1} (:exit result))) (:out result)))))
(doseq [file ["build.rs" "prepare.bb" "engine-budget.patch" "README.md"]]
  (fs/copy (fs/file here "project/vendor/v8" file) (fs/file output (str "vendor-" file)) {:replace-existing true}))
(defn sha [file]
  (format "%064x" (java.math.BigInteger. 1 (.digest (java.security.MessageDigest/getInstance "SHA-256") (fs/read-all-bytes file)))))
(spit (fs/file output "source-hashes.edn")
  (pr-str (into (sorted-map)
    (for [file (concat (map #(fs/file here "project" %) files)
                 (filter fs/regular-file? (fs/glob (fs/file here "project/vendor/v8") "**" {:hidden true}))
                 (filter fs/regular-file? (fs/glob (fs/file here "project/vendor/v8-headers") "**"))) ]
      [(str (fs/relativize (fs/file here "project") file)) (sha file)]))))
(spit (fs/file output "artifact-hashes.edn")
  (pr-str (into (sorted-map) (for [file ["light-cljs-v8-integrated" "frozen-engine/librusty_v8.a" "frozen-engine/src_binding.rs" "frozen-engine/args.gn"
                                       "bench/render.js" "bench/main-none.js" "bench/main-advanced.js"]]
    [file (sha (fs/file here file))]))))
(doseq [label ["render" "main-none" "main-advanced"]]
  (p/shell "cmp" (str (fs/file here "bench" (str label ".js"))) (str "/private/tmp/light-cljs-phase111/" label ".js")))
(let [sources (sort-by str (filter fs/regular-file?
                (mapcat #(fs/glob (fs/file root "research/penpot-light" %) "**")
                  ["frontend/src" "common/src" "vendor" "generated" "shims"])))
      digest (java.security.MessageDigest/getInstance "SHA-256")]
  (doseq [file sources]
    (.update digest (.getBytes (str (fs/relativize root file)) "UTF-8"))
    (.update digest (fs/read-all-bytes file)))
  (let [result (str (count sources) " files\n" (format "%064x" (java.math.BigInteger. 1 (.digest digest))) "\n")]
    (assert (= result (slurp (fs/file root "benchmarks/results/compiler-2026-10-16/penpot-phase111/port-hash.txt"))))
    (spit (fs/file output "port-hash.txt") result)))
(println "Integrated backend evidence archived")
