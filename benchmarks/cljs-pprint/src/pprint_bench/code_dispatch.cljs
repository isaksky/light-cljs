(ns pprint-bench.code-dispatch
  (:require [cljs.pprint :as pp :refer [pprint with-pprint-dispatch]]))

(enable-console-print!)

(defn main []
  (with-pprint-dispatch pp/code-dispatch
    (pprint '(defn square [x] (let [y (* x x)] (if (pos? y) y 0))))))

main
