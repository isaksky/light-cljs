#!/usr/bin/env bb
;; Reconstruct the exact published source package plus its omitted pinned data.
;; The cache is private build output; changing this recipe or patch invalidates it.
(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.java.io :as io]
         '[clojure.string :as str])
(def here (fs/parent (fs/absolutize *file*)))
(def cache (fs/absolutize (first *command-line-args*)))
(def engine (fs/file cache "v8-152.2.0"))
(def patch-file (fs/file here "engine-budget.patch"))
(defn sha [bytes]
  (format "%064x" (java.math.BigInteger. 1
    (.digest (java.security.MessageDigest/getInstance "SHA-256") bytes))))
(defn tree-sha [root]
  (let [digest (java.security.MessageDigest/getInstance "SHA-256")
        relative #(str/replace (str (fs/relativize root %)) "\\" "/")]
    (doseq [file (sort-by relative (filter fs/regular-file? (fs/glob root "**" {:hidden true})))]
      (.update digest (.getBytes (relative file) "UTF-8"))
      (.update digest (byte-array [0]))
      (.update digest (.digest (java.security.MessageDigest/getInstance "SHA-256") (fs/read-all-bytes file))))
    (format "%064x" (java.math.BigInteger. 1 (.digest digest)))))
(def recipe (sha (.getBytes (str (slurp *file*) (slurp patch-file)) "UTF-8")))
(def marker (fs/file cache "prepared.sha256"))
(fs/create-dirs cache)
;; Serializes manual preparation too; closing the channel releases its lock.
(with-open [lock-file (java.io.RandomAccessFile. (str (fs/file cache "prepare.lock")) "rw")
            channel (.getChannel lock-file)]
  (.lock channel)
  (when-not (and (fs/exists? marker) (= recipe (slurp marker)) (fs/exists? engine))
    (defn fetch [name url expected]
      (let [target (fs/file cache name)]
        (when-not (and (fs/exists? target) (or (nil? expected) (= expected (sha (fs/read-all-bytes target)))))
          (let [partial (str target ".partial")]
            (p/shell "curl" "--fail" "--location" "--retry" "3" "--silent" "--show-error"
                     "--output" partial url)
            (when expected
              (assert (= expected (sha (fs/read-all-bytes partial))) (str "SHA-256 mismatch: " url)))
            (fs/move partial target {:replace-existing true})))
        target))
    (let [crate (fetch "v8-152.2.0.crate"
                       "https://static.crates.io/crates/v8/v8-152.2.0.crate"
                       "a10fe1a92da5c32c7c7f838ce36c0ccfcfd5edf0865b58bdde820aa64cea9888")
          rust (fetch "rust-dependencies.tar.gz"
                      "https://chromium.googlesource.com/chromium/src/third_party/rust/+archive/afbc96607d0e659715d803cc099607dd1737fc41.tar.gz"
                      nil)
          icu (fs/file cache "icudtl.dat")]
      (when-not (and (fs/exists? icu)
                    (= "9f48c7f9c7c94d516a14870707e910ab94d75ae640ff6842c4af53276cd26ebe"
                       (sha (fs/read-all-bytes icu))))
        (let [encoded (:out (p/shell {:out :string} "curl" "--fail" "--location" "--retry" "3" "--silent" "--show-error"
                      "https://chromium.googlesource.com/chromium/deps/icu/+/d578f2e8b7bd5938e21cfb6bf15c079e0aa5b738/common/icudtl.dat?format=TEXT"))
              bytes (.decode (java.util.Base64/getMimeDecoder) encoded)]
          (assert (= "9f48c7f9c7c94d516a14870707e910ab94d75ae640ff6842c4af53276cd26ebe" (sha bytes))
                  "ICU data SHA-256 mismatch")
          (with-open [out (io/output-stream icu)] (.write out bytes))))
      (when (fs/exists? marker) (fs/delete marker))
      (when (fs/exists? engine) (fs/delete-tree engine))
      (p/shell "tar" "-xzf" (str crate) "-C" (str cache))
      (p/shell "tar" "-xzf" (str rust) "-C" (str (fs/file engine "third_party/rust")))
      ;; Gitiles regenerates tar metadata; verify the canonical path/content
      ;; tree, independently reproduced from two downloads of this exact pin.
      (assert (= "88ed763562a2d9ad761aaaecdc7ac538bdcc78280bad17ae7542bb340f2dca45"
                 (tree-sha (fs/file engine "third_party/rust")))
              "Pinned Rust dependency contents differ; remove the cached rust-dependencies.tar.gz to retry")
      (fs/create-dirs (fs/file engine "third_party/icu/common"))
      (fs/copy icu (fs/file engine "third_party/icu/common/icudtl.dat") {:replace-existing true})
      (p/shell {:dir (str engine)} "git" "apply" "--check" (str patch-file))
      (p/shell {:dir (str engine)} "git" "apply" (str patch-file))
      (spit marker recipe))))
