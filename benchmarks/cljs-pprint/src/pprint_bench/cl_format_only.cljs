(ns pprint-bench.cl-format-only
  (:require [cljs.pprint :refer [cl-format]]))

(enable-console-print!)

(defn main []
  (cl-format true "~a is ~:r: ~{~d~^, ~}~%" "Bob" 3 [10 20 30]))

main
