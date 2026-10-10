(ns shared.portable #?(:cljs (:require-macros [shared.portable])))
#?(:clj (defmacro number [] 13)
   :cljs (def values [0 #?@(:cljs [1 2]) #?(:other 9 :default 3)]))
