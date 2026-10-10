(ns boundary.main (:require [boundary.lib :as x] [boundary.lib :as x])
 (:require-macros [boundary.macros :as m :refer [twice]] [boundary.macros :as m :refer [twice]]))
 (defn run [observe]
   (and (identical? x/add boundary.lib/add) (= 42 (x/add 1)) (= 6 (m/twice 3)) (= 8 (twice 4))))
 run