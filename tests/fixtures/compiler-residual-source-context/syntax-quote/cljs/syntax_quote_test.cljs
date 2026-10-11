;; Copyright (c) Rich Hickey. Eclipse Public License 1.0.
;; See research/clojurescript/epl-v10.html; retain this notice.
(ns cljs.syntax-quote-test (:require [cljs.test :as test]))

(defn run [] (do (do (assert (= 'cljs.syntax-quote-test/foo `foo)) (assert (= 'cljs.test/test-vars `test/test-vars)) (assert (= 'cljs.test/deftest `test/deftest)) (assert (= 'cljs.test/foo `test/foo)) true)))
