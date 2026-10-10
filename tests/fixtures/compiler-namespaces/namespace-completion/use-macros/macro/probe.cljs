(ns macro.probe (:require-macros [namespace-observer.macros :refer [imports]]))
(use-macros '[cljs.test :only [deftest is]])
(defn run [observe]
(assert (= '#{deftest is} (set (keys (:use-macros (imports)))))) (assert (= '{cljs.test cljs.test} (:require-macros (imports)))) (assert (empty? (:requires (imports))))
  true)
run
