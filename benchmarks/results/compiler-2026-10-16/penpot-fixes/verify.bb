(require '[babashka.fs :as fs] '[babashka.process :as process]
         '[clojure.string :as str])
(def root (fs/absolutize (fs/parent *file*)))
(def prior (or (first *command-line-args*)
               "/private/tmp/claude-501/-Users-admin-src-light-cljs/ef390548-9edc-4e71-ba3a-a2837e664e83/scratchpad/bench"))
(defn sha [text]
  (format "%064x" (java.math.BigInteger. 1 (.digest (java.security.MessageDigest/getInstance "SHA-256") (.getBytes text "UTF-8")))))
(defn without-runtime [text]
  (let [start (str/index-of text "// Original bounded runtime for emitted JavaScript.")
        end (when start (str/index-of text "\n})();" start))]
    (assert (and start end) "Expected one embedded runtime factory")
    (str (subs text 0 start) "<embedded-runtime>" (subs text (+ end 6)))))
(def comparisons
  (for [[job baseline] [["render" "sub_render5.js"] ["main-none" "main_none.js"] ["main-advanced" "main_advanced.js"]]]
    (let [old (slurp (fs/file prior baseline)) current (fs/file root (if (= job "main-none") "clean/main-none.js" (str job ".js"))) new (slurp current)
          old-body (without-runtime old) new-body (without-runtime new)
          checked (process/shell {:out :string :err :string :continue true}
                                 "node" "--check" (str current))]
      {:job job :body-equal (= old-body new-body)
       :old-sha256 (sha old) :new-sha256 (sha new)
       :old-body-sha256 (sha old-body) :new-body-sha256 (sha new-body)
       :syntax-exit (:exit checked) :syntax-stderr (:err checked)})))
(doseq [record comparisons]
  (spit (fs/file root "verification.edn") (str (pr-str record) "\n") :append true)
  (prn record))
(assert (every? #(zero? (:syntax-exit %)) comparisons) "Penpot output syntax failed")
