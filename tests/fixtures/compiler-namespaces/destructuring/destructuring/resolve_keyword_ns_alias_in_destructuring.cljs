(ns destructuring.resolve-keyword-ns-alias-in-destructuring (:require [cljs.core :as c]))
(defn run [observe]
  (do (do (let [{:keys [::c/x ::c/y ::c/z] :or {z 3}} {:cljs.core/x 1 :cljs.core/y 2}] (assert (= [1 2 3] [x y z]))) true)))
run
