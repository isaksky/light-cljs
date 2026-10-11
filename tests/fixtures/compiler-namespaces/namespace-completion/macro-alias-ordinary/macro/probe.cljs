;; Ordinary .cljs macro imports; original JVM phase-map contract is pinned in the reference tree.
(ns macro.probe (:require [namespace-observer.macros :refer [imports]]))
(require '[cljs.test :as test])
(defn run [observe]
(assert (= '#{cljs.test} (set (vals (:requires (imports))))))
  true)
run
