(require '[clojure.java.io :as io] '[clojure.string :as str])
(def fixture-root (or (first *command-line-args*) (throw (ex-info "Usage: bb generate.bb OUTPUT-DIR" {}))))
(defn write! [dir ns s] (let [f (io/file dir (str (str/replace ns "." "/") ".cljs"))] (io/make-parents f) (spit f s)))
(write! (str fixture-root "/synthetic-4000") "demo.main"
 (str "(ns demo.main)\n"
  (apply str (for [i (range 4000)]
   (format "(defn f%d [x] (when x (cond (= x 1) 1 :else (+ x %d))))\n" i i)))))
(doseq [n [1000 2000 4000 8000]]
 (write! (str fixture-root "/calls-" n) "probe.main"
  (str "(ns probe.main)\n(defn f0 [] 1)\n" (apply str (for [i (range 1 n)] (str "(defn f" i " [] (f0))\n"))))))
(doseq [n [25 50 100]]
 (doseq [i (range n)]
  (write! (str fixture-root "/namespaces-" n) (str "probe.n" i)
    (str "(ns probe.n" i (when (pos? i) (str " (:require " (str/join " " (for [a (range 10)] (str "[probe.n" (dec i) " :as a" a "]"))) ")")) ")\n"
       (apply str (for [j (range 40)] (str "(defn f" j " [x] x)\n")))))))
(doseq [m [0 50 100]]
 (write! (str fixture-root "/macros-" m) "probe.main"
  (str "(ns probe.main)\n"
    (apply str (for [i (range m)] (str "(defmacro m" i " [x] x)\n")))
    (apply str (for [i (range 1000)] (str "(defn f" i " [x] (when x 1))\n"))))))
