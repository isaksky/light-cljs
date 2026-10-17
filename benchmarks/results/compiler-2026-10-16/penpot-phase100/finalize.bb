(require '[babashka.fs :as fs] '[babashka.process :as p]
         '[clojure.java.io :as io])
(def here (fs/parent *file*))
(def repo (fs/file "/Users/admin/src/light-cljs"))
(def output (fs/file repo "benchmarks/results/compiler-2026-10-16/penpot-phase100"))
(def production (fs/file here "production"))
(fs/create-dirs (fs/file output "production"))
(doseq [file (concat (fs/glob production "*.log") (fs/glob production "*.time.txt")
                    (map #(fs/file production %) ["results.edn" "implementation.patch" "name_index.rs"]))]
  (fs/copy file (fs/file output "production" (fs/file-name file)) {:replace-existing true}))
(doseq [file ["finalize.bb" "promote.bb" "validate.bb"]]
  (fs/copy (fs/file here file) (fs/file output file) {:replace-existing true}))
(doseq [job ["render" "main-none" "main-advanced"]]
  (let [file (fs/file production (str job ".js"))]
    (p/shell "cmp" (str file) (str "/private/tmp/light-cljs-phase42/production/" job ".js"))
    (let [result (with-open [in (io/input-stream file)]
                   (p/shell {:in in :out :string :err :string :continue true}
                     "node" "--check" "--input-type=module"))]
      (assert (= (if (= job "render") 0 1) (:exit result)))
      (spit (fs/file output "production" (str job "-syntax.txt"))
        (str "Byte-identical to phase42.\nExplicit ESM syntax exit: " (:exit result) "\n"
             (:out result) (:err result))))))
(spit (fs/file output "production-hashes.txt")
  (:out (apply p/shell {:out :string} "shasum" "-a" "256"
          (map str (concat
            (map #(fs/file production %) ["light-cljs" "implementation.patch" "name_index.rs"
                                          "render.js" "main-none.js" "main-advanced.js"])
            (map #(fs/file repo %) ["src/emitter.rs" "src/evaluator.rs" "src/compiler_bootstrap.rs" "src/analyzer.rs" "src/arena.rs" "src/compilation_session.rs" "src/compiler_reader.rs" "src/dev_build.rs" "src/eval_compiler.rs" "src/layered.rs" "src/macro_host.rs" "docs/compiler.md" "docs/evaluator.md"]))))))
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
(println "Production evidence archived; outputs unchanged; main ESM defect persists.")
