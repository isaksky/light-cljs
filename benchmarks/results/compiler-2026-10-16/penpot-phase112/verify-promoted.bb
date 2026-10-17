(require '[babashka.fs :as fs] '[clojure.edn :as edn])
(def root (fs/file "/Users/admin/src/light-cljs"))
(def hashes (edn/read-string (slurp (fs/file root "benchmarks/results/compiler-2026-10-16/penpot-phase112/source-hashes.edn"))))
(defn sha [file]
  (format "%064x" (java.math.BigInteger. 1
    (.digest (java.security.MessageDigest/getInstance "SHA-256") (fs/read-all-bytes file)))))
(doseq [[path expected] hashes]
  (assert (= expected (sha (fs/file root path))) path))
(println "Promoted source matches the archived candidate:" (count hashes) "files")
