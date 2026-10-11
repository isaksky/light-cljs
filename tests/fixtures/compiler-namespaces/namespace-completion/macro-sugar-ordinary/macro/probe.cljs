;; Ordinary .cljs macro imports; original JVM phase-map contract is pinned in the reference tree.
(ns macro.probe (:require [namespace-observer.macros :refer [imports]]))
(require '[cljs.test :as test :refer [deftest is]])
(defn run [observe]
(assert (= '#{cljs.test} (set (vals (:requires (imports)))))) (assert (empty? (:require-macros (imports)))) (assert (= '#{deftest is} (set (keys (:uses (imports))))))
  true)
run
