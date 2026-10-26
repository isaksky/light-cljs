(ns counter.dep (:require [counter.macros :refer [next-value]])) (def value (next-value))
