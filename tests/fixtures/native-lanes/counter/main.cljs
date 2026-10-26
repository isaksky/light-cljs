(ns counter.main (:require [counter.dep :as d] [counter.macros :refer [next-value]])) (js/console.log d/value (next-value))
