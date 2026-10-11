;; Compatibility profile preserves the previously supported strict EDN subset.
(ns clojure.edn)
(defn read-string ([s] (compiler-edn-read-string s)) ([opts s] (compiler-edn-read-string opts s)))
(defn read
  ([] (compiler-edn-read))
  ([rdr] (compiler-edn-read rdr))
  ([opts rdr] (compiler-edn-read opts rdr))
  ([rdr eof-error? eof opts] (compiler-edn-read rdr eof-error? eof opts)))
