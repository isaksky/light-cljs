(require '[babashka.fs :as fs] '[clojure.edn :as edn])
(def repo (fs/absolutize "."))
(def port (fs/path repo "research/penpot-light"))
(def files (sort-by str (mapcat #(filter fs/regular-file? (fs/glob (fs/path port %) "**")) ["frontend/src" "common/src" "vendor" "generated" "shims"])))
(def digest (java.security.MessageDigest/getInstance "SHA-256"))
(doseq [file files]
  ;; Sorted repository-relative UTF-8 path, then exact content, no separator.
  (.update digest (.getBytes (str (fs/relativize repo file)) "UTF-8"))
  (.update digest (java.nio.file.Files/readAllBytes file)))
(def result {:files (count files) :sha256 (format "%064x" (java.math.BigInteger. 1 (.digest digest)))})
(prn result)
(def before (edn/read-string (slurp (fs/file (fs/parent *file*) "inputs.edn"))))
(assert (= result (select-keys before [:files :sha256])) "Port path/content hash changed")
