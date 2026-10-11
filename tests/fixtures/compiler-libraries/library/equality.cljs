(ns library.equality (:require [library.upstream :as upstream]))
(defn run [observe] (upstream/test-preserves-meta) true)
run
