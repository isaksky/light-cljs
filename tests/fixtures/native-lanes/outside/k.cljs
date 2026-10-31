(ns outside.k (:require [outside.reg :as r])) (def state (atom 0)) (defmacro register [] (swap! r/table assoc :k state) nil)
