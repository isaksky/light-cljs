(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.string :as str])
(def root (or (first *command-line-args*) (fs/parent *file*)))
(def prior "/private/tmp/light-cljs-penpot-fixed-20261016")
(defn sha [text] (format "%064x" (java.math.BigInteger. 1 (.digest (java.security.MessageDigest/getInstance "SHA-256") (.getBytes text "UTF-8")))))
(defn split-runtime [text]
  (let [start (str/index-of text "// Original bounded runtime for emitted JavaScript.")
        end (some-> (str/index-of text "\n})();" start) (+ 6))]
    (assert (and start end) "Embedded runtime missing")
    [(str (subs text 0 start) "<embedded-runtime>" (subs text end)) (subs text start end)]))
(doseq [[job before] [["render" "render.js"] ["main-none" "clean/main-none.js"] ["main-advanced" "main-advanced.js"]]]
  (let [old (slurp (fs/file prior before)) file (fs/file root (str job ".js")) new (slurp file)
        [a old-runtime] (split-runtime old) [b new-runtime] (split-runtime new)
        syntax (p/shell {:out :string :err :string :continue true} "node" "--check" (str file))
        result {:job job :body-equal (= a b) :old-body-sha256 (sha a) :new-body-sha256 (sha b)
                :output-sha256 (sha new) :output-bytes (fs/size file)
                :runtime-byte-delta (- (alength (.getBytes new-runtime "UTF-8")) (alength (.getBytes old-runtime "UTF-8")))
                :syntax-exit (:exit syntax) :syntax-stderr (:err syntax)}]
    (prn result)
    (assert (= a b) (str job " application output changed"))
    (assert (zero? (:exit syntax)) (:err syntax))))
