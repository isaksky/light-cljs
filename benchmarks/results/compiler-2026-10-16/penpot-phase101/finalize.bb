(require '[babashka.fs :as fs] '[babashka.process :as p])
(def here (fs/parent *file*))
(def repo (fs/file "/Users/admin/src/light-cljs"))
(def production (fs/file here "production"))
(def output (fs/file repo "benchmarks/results/compiler-2026-10-16/penpot-phase101"))
(doseq [label ["render" "main-none" "main-advanced"]]
  (p/shell "bb" (str (fs/file here "audit-output.bb")) label (str production)))
(p/shell "bb" (str (fs/file here "archive.bb")))
(fs/create-dirs (fs/file output "production"))
(doseq [pattern ["*.log" "*.time.txt" "*.edn" "*.patch" "*.rs" "*.txt"] file (fs/glob production pattern)]
  (fs/copy file (fs/file output "production" (fs/file-name file)) {:replace-existing true}))
(spit (fs/file output "production-hashes.txt")
  (:out (apply p/shell {:out :string} "shasum" "-a" "256"
          (map str (concat
            (map #(fs/file production %) ["light-cljs" "implementation.patch" "name_index.rs"
                                          "render.js" "main-none.js" "main-advanced.js"])
            (map #(fs/file repo %) ["src/analyzer.rs" "src/js_syntax.rs" "src/emitter.rs"
                                    "src/emitter/chunked.rs" "tests/compiler/macros.rs"
                                    "tests/compiler/optimization.rs" "docs/compiler.md"]))))))
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
(println "Production evidence archived; all changes audited; port unchanged")
