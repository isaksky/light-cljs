(ns nested.b (:require [nested.helpers :refer [snap]] [nested.macros :refer [next-value]]))
(def s (snap))
(def y (next-value))
