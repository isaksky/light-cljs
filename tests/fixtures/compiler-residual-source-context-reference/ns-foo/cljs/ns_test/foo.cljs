;; Copyright (c) Rich Hickey. Eclipse Public License 1.0.
;; See epl-v10.html at the repository root; retain this notice.
(ns cljs.ns-test.foo (:require [made.up.lib :as-alias lib]))
(defn baz [] 123)
(def kw ::foo)
(def qkw '::foo)
(defn run [] (do (do (assert (= (str kw) ":cljs.ns-test.foo/foo")) (assert (= (str qkw) ":cljs.ns-test.foo/foo")) true) (do (assert (keyword-identical? ::lib/foo :made.up.lib/foo)) true)))
