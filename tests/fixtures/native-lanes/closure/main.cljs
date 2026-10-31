(ns closure.main (:require [closure.early :as e] [closure.slow :as s] [closure.a :as a] [closure.b :as b]))
(js/console.log (pr-str [e/e s/x a/a b/b]))
