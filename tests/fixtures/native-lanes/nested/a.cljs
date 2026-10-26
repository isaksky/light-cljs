(ns nested.a (:require [nested.helpers :refer [snap]] [nested.macros :refer [next-value]]))
(def x (next-value))
(def s (snap))
