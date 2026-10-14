;; Copyright (c) Rich Hickey. Eclipse Public License 1.0.
;; See epl-v10.html at the repository root; retain this notice.
(ns cljs.ns-test (:refer-clojure :exclude [+ for] :rename {mapv core-mapv}) (:require-macros [clojure.core :as lang :refer [when when-let] :rename {when always when-let always-let}]) (:require [cljs.ns-test.foo :refer [baz]] [clojure.set :as s :refer [intersection] :rename {intersection itsc}] [fake.ns :as-alias fake]) (:use [cljs.ns-test.bar :only [quux]]))
(def + -)
(defn run [] (do (do (assert (= false (exists? mapv))) (assert (= true (exists? core-mapv))) true)))
