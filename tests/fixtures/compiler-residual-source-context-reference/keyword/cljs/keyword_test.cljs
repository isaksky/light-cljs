;; Copyright (c) Rich Hickey. Eclipse Public License 1.0.
;; See research/clojurescript/epl-v10.html; retain this notice.
(ns cljs.keyword-test (:require-macros [cljs.keyword-macros :as macros]) (:require [cljs.keyword-other :as other]))

(defn run [] (do (do (assert (= ::bar :cljs.keyword-test/bar)) (assert (= ::other/foo :cljs.keyword-other/foo)) (assert (= ::macros/foo :cljs.keyword-macros/foo)) true)))
