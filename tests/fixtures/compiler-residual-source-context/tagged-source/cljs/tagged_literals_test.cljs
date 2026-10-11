;; Copyright (c) Rich Hickey. Eclipse Public License 1.0.
;; See research/clojurescript/epl-v10.html; retain this notice.
(ns cljs.tagged-literals-test (:require [cljs.reader :as reader]))

(defn run [] (do (do (assert (= #cljs/tag [1 2 3] [1 2 3])) true)))
