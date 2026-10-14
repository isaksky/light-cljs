;; Copyright (c) Rich Hickey. Eclipse Public License 1.0.
;; See epl-v10.html at the repository root; retain this notice.
(ns residualcomplete.readstring (:require [clojure.edn] [cljs.reader]))
(defn run [] (do (do (do (assert (= (clojure.edn/read-string "(+ 1 2)")            (cljs.reader/read-string "(+ 1 2)"))) (assert (= (clojure.edn/read-string "{:a #{[1]}}")            (cljs.reader/read-string "{:a #{[1]}}")))) true)))
