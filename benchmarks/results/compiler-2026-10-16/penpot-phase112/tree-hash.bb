(require '[babashka.fs :as fs] '[babashka.process :as p])
(defn tree-sha [root]
  (let [digest (java.security.MessageDigest/getInstance "SHA-256")]
    (doseq [file (sort-by #(str (fs/relativize root %)) (filter fs/regular-file? (fs/glob root "**" {:hidden true})))]
      (.update digest (.getBytes (str (fs/relativize root file)) "UTF-8"))
      (.update digest (byte-array [0]))
      (.update digest (.digest (java.security.MessageDigest/getInstance "SHA-256") (fs/read-all-bytes file))))
    (format "%064x" (java.math.BigInteger. 1 (.digest digest)))))
(doseq [[label archive] [["previous" "/private/tmp/light-cljs-phase108/rust-dependencies.tar.gz"] ["fresh" "/private/tmp/light-cljs-phase112/prepare-check/rust-dependencies.tar.gz.partial"]]]
  (let [root (fs/file (fs/parent *file*) (str "rust-" label))]
    (fs/create-dirs root)
    (p/shell "tar" "-xzf" archive "-C" (str root))
    (println label (tree-sha root))))
