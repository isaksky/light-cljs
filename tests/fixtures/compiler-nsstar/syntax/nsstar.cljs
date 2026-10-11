(ns syntax.nsstar (:require [clojure.string]))
(ns* (:require '[clojure.string :as text]))
(defn run [observe] (= ["A" "1-2"] [(text/upper-case "a") (text/join "-" [1 2])]))
(pr-str (run nil))
