;; Copyright (c) Rich Hickey. Eclipse Public License 1.0.
;; See research/clojurescript/epl-v10.html; retain this notice.
(ns cljs.keyword-test (:require [cljs.keyword-other :as other] [cljs.keyword-macros :as macros]))

(defn run [] (do (do (assert (= ::bar :cljs.keyword-test/bar)) (assert (= ::other/foo :cljs.keyword-other/foo)) (assert (= ::macros/foo :cljs.keyword-macros/foo)) true)))
