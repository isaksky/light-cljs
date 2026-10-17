(require '[babashka.fs :as fs] '[babashka.process :as p] '[clojure.string :as str])
(def here (fs/parent *file*))
(defn body [path]
  (let [text (slurp path) a (str/index-of text "// Original bounded runtime for emitted JavaScript.")
        b (when a (some-> (str/index-of text "\n})();" a) (+ 6)))]
    (assert (and a b))
    (str (subs text 0 a) (subs text b))))
(def text
  (apply str
    (for [[job prior] [["render" "render"] ["main" "main-none"]]]
      (let [file (fs/file here (str job ".js"))]
        (assert (= (body file) (body (str "/private/tmp/light-cljs-phase32/production/" prior ".js"))))
        (p/shell {:out :string :err :string} "node" "--check" (str file))
        (str job ": application body matches phase32 outside runtime; complete output passes Node syntax checking.\n")))))
(spit (fs/file here "prototype-equivalence.txt") text)
(print text)
