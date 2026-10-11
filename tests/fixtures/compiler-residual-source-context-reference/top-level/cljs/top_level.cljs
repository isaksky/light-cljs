;; Copyright (c) Rich Hickey. Eclipse Public License 1.0.
;; See research/clojurescript/epl-v10.html; retain this notice.
(ns cljs.top-level (:refer-clojure :exclude [test]))
(let [foo 1]
  (defn bar []
    foo))
(let [foo 2]
  (defn baz []
    foo))
(defn run [] (do (do (assert (= (bar) 1)) (assert (= (baz) 2)) true)))
