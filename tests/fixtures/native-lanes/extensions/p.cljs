(ns extensions.p)
(defprotocol P (value [x]))
(deftype T [] P (value [_] "base") Object (toString [_] "base"))
(defmulti choose identity)
(defmethod choose :default [_] "base")
(defmacro inspect []
  (let [x (T.)]
    [(value x) (value x) (.toString x) (choose :x) (choose :x)]))
