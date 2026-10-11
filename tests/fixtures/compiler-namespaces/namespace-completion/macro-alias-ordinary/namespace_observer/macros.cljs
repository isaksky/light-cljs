(ns namespace-observer.macros)
(defmacro imports []
  (let [clean (fn [m] (into {} (remove (fn [[k v]]
                          (or (= v 'namespace-observer.macros) (= v 'cljs.core))) m)))]
    (list 'quote {:requires (clean (:requires (:ns &env)))
                  :require-macros (clean (:require-macros (:ns &env)))
                  :uses (clean (:uses (:ns &env)))
                  :use-macros (clean (:use-macros (:ns &env)))})))
