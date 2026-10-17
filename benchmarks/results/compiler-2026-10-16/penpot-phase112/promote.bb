(require '[babashka.fs :as fs] '[clojure.edn :as edn])
(def here (fs/parent *file*))
(def root (fs/file "/Users/admin/src/light-cljs"))
(def project (fs/file here "project"))
(def validation (edn/read-string (slurp (fs/file here "validation.edn"))))
(assert (= [1442 0 106] (:tests validation)))
(assert (= [1406 0 106] (:quickjs-tests validation)))
(assert (fs/exists? (fs/file root "benchmarks/results/compiler-2026-10-16/penpot-phase112/artifact-hashes.edn")))
(def before (edn/read-string (slurp (fs/file here "production-before-import.edn"))))
(defn sha [file]
  (format "%064x" (java.math.BigInteger. 1
    (.digest (java.security.MessageDigest/getInstance "SHA-256") (fs/read-all-bytes file)))))
(doseq [[file expected] before]
  (assert (= expected (sha (fs/file root file))) (str "Production file changed during validation: " file)))
(def new-files (concat ["src/compiler_evaluator.rs"]
                      (map #(str "src/" (fs/file-name %)) (fs/glob (fs/file project "src") "v8*"))))
(doseq [file new-files] (assert (not (fs/exists? (fs/file root file))) file))
(doseq [name ["v8" "v8-headers"]]
  (assert (not (fs/exists? (fs/file root "vendor" name))) name))
(doseq [file (concat (keys before) new-files)]
  (fs/create-dirs (fs/parent (fs/file root file)))
  (fs/copy (fs/file project file) (fs/file root file) {:replace-existing true}))
(doseq [name ["v8" "v8-headers"]]
  (fs/copy-tree (fs/file project "vendor" name) (fs/file root "vendor" name)))
(spit (fs/file here "promoted.edn") (pr-str {:source :phase112 :files (vec (concat (keys before) new-files)) :vendor ["v8" "v8-headers"]}))
(println "Validated native backend promoted; unrelated production edits preserved")
