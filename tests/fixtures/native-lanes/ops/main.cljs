(ns ops.main (:require [ops.a :as a] [ops.b :as b] [ops.macros :refer [step]]))
(js/console.log (pr-str [a/x a/y b/x (step :main)]))
