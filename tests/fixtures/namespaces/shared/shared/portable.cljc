(ns shared.portable)
(defmacro number [] 13)
(def values [0 #?@(:cljs [1 2]) #?(:other 9 :default 3)])
