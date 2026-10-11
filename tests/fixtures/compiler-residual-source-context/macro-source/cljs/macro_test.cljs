;; Copyright (c) Rich Hickey. Eclipse Public License 1.0.
;; See research/clojurescript/epl-v10.html; retain this notice.
(ns cljs.macro-test (:refer-clojure :exclude [==]) (:require [cljs.macro-test.macros :refer [== sm-cljs-3027]] [cljs.macro-test.cljs2852] [single-seg-macros]))

(defn run [] (do (do (assert (= (== 1 1) 2)) true) (do (assert (= '([x])) (cljs.macro-test.cljs2852/beta)) (assert (= '([x] [x y])) (cljs.macro-test.cljs2852/delta)) (assert (= '([x] [x & xs])) (cljs.macro-test.cljs2852/zeta)) true) (do (assert (= {"a" "b"} (sm-cljs-3027))) true) (do (assert (= 5 (single-seg-macros/test-macro 2 3))) true)))
