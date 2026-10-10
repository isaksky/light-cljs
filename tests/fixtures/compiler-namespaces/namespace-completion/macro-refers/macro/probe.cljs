(ns macro.probe (:require-macros [namespace-observer.macros :refer [imports]]))
(require-macros '[cljs.test :as test :refer [deftest is]])
(defn run [observe]
(assert (= '#{cljs.test} (set (vals (:require-macros (imports)))))) (assert (= '#{deftest is} (set (keys (:use-macros (imports))))))
  true)
run
