(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.edn :as edn] '[clojure.string :as str])
(def here (fs/parent *file*))
(def output (fs/file "/Users/admin/src/light-cljs/benchmarks/results/compiler-2026-10-16/penpot-phase52"))
(fs/create-dirs output)
(doseq [pattern ["*.bb" "*.c" "*.sh" "*.log" "*.edn" "*.time" "*.patch" "*.txt" "*.json"] file (fs/glob here pattern)]
  (fs/copy file (fs/file output (fs/file-name file)) {:replace-existing true}))
(fs/copy-tree (fs/file here "linux-results") (fs/file output "linux-results") {:replace-existing true})
(def connection (edn/read-string (slurp (fs/file here "linux-connection.edn"))))
(fs/copy-tree (:logs connection) (fs/file output "linux-readiness") {:replace-existing true})
(spit (fs/file output "candidate-hashes.txt")
  (:out (apply p/shell {:out :string} "shasum" "-a" "256"
          (map #(str (fs/file here %)) ["light-cljs-quickjs-mimalloc" "main-quickjs-mimalloc.js" "quickjs-allocator-quotes.patch"
                                        "project/Cargo.lock" "allocator-probe.c" "linux-run.sh"]))))
(def finalized
  (-> (slurp "/private/tmp/light-cljs-phase47/finalize.bb")
      (str/replace "penpot-phase47" "penpot-phase52")
      (str/replace "[\"finalize.bb\" \"prepare-finalize.bb\" \"validate-production.bb\"]" "[\"finalize.bb\" \"promote.bb\" \"validate.bb\"]")))
(spit (fs/file here "finalize.bb") finalized)
(p/shell "bb" (str (fs/file here "finalize.bb")))
(fs/copy (fs/file here "production/candidate-identity.txt") (fs/file output "production/candidate-identity.txt") {:replace-existing true})
(println "Archived candidate validation, Linux allocator probe and production rerun.")
