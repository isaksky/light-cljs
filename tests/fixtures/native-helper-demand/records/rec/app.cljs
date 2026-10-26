(ns rec.app (:require [rec.macros :refer [endpoint]]))
(defn plain [] (endpoint false))
(defn secured [] (endpoint true))
(js/console.log (pr-str (plain)) (pr-str (secured)))
