;; Copyright (c) Rich Hickey. Eclipse Public License 1.0.
;; See epl-v10.html at the repository root; retain this notice.
(ns cljs.tagged-literals-test (:require [cljs.reader :as reader] [clojure.set :as set]))

(defn run [] (do (reader/register-tag-parser! (quote cljs/tag) identity) (reader/register-tag-parser! (quote cljs/inc) inc) (reader/register-tag-parser! (quote cljs/union) set/union) (do (do (assert (object? (cljs.reader/read-string "#js {}"))) (assert (= {} (cljs.reader/read-string "#cljs/tag {}"))) (assert (= (cljs.reader/read-string "#cljs/inc 0") 1)) (assert (= (cljs.reader/read-string "#cljs/union #{1}") #{1})) true))))
