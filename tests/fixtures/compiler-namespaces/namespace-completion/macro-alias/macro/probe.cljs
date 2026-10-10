(ns macro.probe (:require-macros [namespace-observer.macros :refer [imports]]))
(require-macros '[cljs.test :as test])
(defn run [observe]
(assert (= '#{cljs.test} (set (vals (:require-macros (imports))))))
  true)
run
