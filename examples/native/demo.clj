(ns demo (:require [math-helpers :as math]))

(println math/greeting)
{:squares (mapv math/square (range 1 6))
 :sum (reduce + (range 1 6))}
