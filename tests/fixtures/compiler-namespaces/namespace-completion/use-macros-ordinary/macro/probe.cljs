;; Ordinary .cljs macro imports; original JVM phase-map contract is pinned in the reference tree.
(ns macro.probe (:require [namespace-observer.macros :refer [imports]]))
(require '[cljs.test :refer [deftest is]])
(defn run [observe]
(assert (= '#{deftest is} (set (keys (:uses (imports)))))) (assert (= '{cljs.test cljs.test} (:requires (imports)))) (assert (empty? (:require-macros (imports)))) (assert (empty? (:use-macros (imports))))
  true)
run
