(ns nested.main (:require [nested.a :as a] [nested.b :as b] [nested.macros :refer [next-value]]))
(js/console.log (pr-str [a/x a/s b/s b/y (next-value)]))
