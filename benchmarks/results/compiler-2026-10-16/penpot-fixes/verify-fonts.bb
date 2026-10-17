(require '[babashka.fs :as fs] '[clojure.string :as str])
(def root (fs/parent *file*))
(def font_source (slurp "research/penpot-light/generated/app/common/fonts_data.cljs"))
(def old (slurp (fs/file root "main-none-old.body.js")))
(def new (slurp (fs/file root "main-none-new.body.js")))
(defn matches [pattern text]
  (into {} (map (fn [[_ id uuid]] [id uuid]) (re-seq pattern text))))
(def current (matches #":id \"(gfont-[^\"]+)\", :uuid #uuid \"([0-9a-f-]+)\"" font_source))
(def emitted-pattern #"keyword\(null, \"id\"\), \"(gfont-[^\"]+)\", __light0_rt\.keyword\(null, \"uuid\"\), __light0_rt\.core\.uuid\(\"([0-9a-f-]+)\"\)")
(def previous (matches emitted-pattern old))
(def emitted (matches emitted-pattern new))
(assert (= current emitted) "Newly emitted font IDs/UUIDs must equal current port input")
(assert (= (set (keys current)) (set (keys previous))) "Catalog font IDs changed")
(assert (pos? (count current)))
(assert (= (count current) (count (set (vals current))) (count (set (vals previous)))) "Font UUIDs must be unique")
(def replacements (into {} (for [[id uuid] current] [uuid (previous id)])))
(defn sha [text]
  (format "%064x" (java.math.BigInteger. 1 (.digest (java.security.MessageDigest/getInstance "SHA-256") (.getBytes text "UTF-8")))))
(doseq [job ["main-none" "main-advanced"]]
  (let [old (slurp (fs/file root (str job "-old.body.js"))) new (slurp (fs/file root (str job "-new.body.js")))
        n (atom 0)
        normalized (str/replace new #"(__light0_rt(?:\.core\.uuid|\$uuid)\(\")([0-9a-f-]+)(\"\))"
                               (fn [[whole before uuid after]]
                                 (if-let [prior (replacements uuid)]
                                   (do (swap! n inc) (str before prior after)) whole)))
        result {:job job :font-count (count current) :replacements @n
                :current-emitted-fonts-match-input (= current emitted)
                :body-equal-after-font-uuid-mapping (= old normalized)
                :old-body-sha256 (sha old) :normalized-body-sha256 (sha normalized)}]
    (prn result)
    (spit (fs/file root "font-verification.edn") (str (pr-str result) "\n") :append true)
    (assert (= (count current) @n) "Every catalog UUID should occur once")
    (assert (= old normalized) "Differences remain beyond current font UUIDs")))
