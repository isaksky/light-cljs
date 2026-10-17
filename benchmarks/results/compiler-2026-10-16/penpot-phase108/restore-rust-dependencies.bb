(require '[babashka.fs :as fs] '[babashka.process :as p] '[cheshire.core :as json])
(def here (fs/parent *file*))
(def pin "afbc96607d0e659715d803cc099607dd1737fc41")
(def provenance (json/parse-string (slurp (fs/file here "rust-source.json")) true))
(assert (= pin (:sha provenance)))
(assert (= "https://chromium.googlesource.com/chromium/src/third_party/rust" (:submodule_git_url provenance)))
(def source (str "https://chromium.googlesource.com/chromium/src/third_party/rust/+archive/" pin ".tar.gz"))
(when-not (fs/exists? (fs/file here "rust-dependencies.tar.gz"))
  (p/shell "curl" "--fail" "--silent" "--show-error" "--location" "--output"
    (str (fs/file here "rust-dependencies.tar.gz")) source))
(def target (fs/file here "v8/third_party/rust"))
(defn sha [file]
  (format "%064x" (java.math.BigInteger. 1
    (.digest (java.security.MessageDigest/getInstance "SHA-256") (fs/read-all-bytes file)))))
(def before (into {} (for [file (fs/glob target "**") :when (fs/regular-file? file)]
                       [(str (fs/relativize target file)) (sha file)])))
(p/shell "tar" "-xzf" (str (fs/file here "rust-dependencies.tar.gz")) "-C" (str target))
(doseq [[file expected] before] (assert (= expected (sha (fs/file target file))) file))
(spit (fs/file here "rust-restoration.edn")
  (pr-str {:commit pin :preserved-packaged-files (count before)
           :archive-sha256 (sha (fs/file here "rust-dependencies.tar.gz"))
           :source source}))
(println "Restored pinned vendored Rust crates; existing packaged files match" (count before))
