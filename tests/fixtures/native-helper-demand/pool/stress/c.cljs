(ns stress.c (:require [stress.macros :refer [double tagged pair]]
                       [stress.a :as a]))
(defn f1 [] (double (a/f1)))
(defn f2 [] (tagged "c" 3))
(defmacro local [x] (a/grow x))
(defn f3 [] (local (pair 5 6)))
(defn f4 [] (+ (f1) (double 7)))
(defn f5 [] (str (f2) (f4) (a/f5)))
