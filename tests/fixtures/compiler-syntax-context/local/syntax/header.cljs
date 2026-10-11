(ns syntax.header (:require [clojure.string :as text] [syntax.observer :refer [caller]]))
(def value 4)
(defn run [observe] (= ["4-A" "syntax.header" true] [(text/join "-" [value (text/upper-case "a")]) (caller) (= syntax.header/value value)]))
(pr-str (run nil))
