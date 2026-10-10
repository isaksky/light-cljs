(ns namespace-observer.macros)
(defmacro import-maps []
  (list 'quote {:imports (:imports (:ns &env))
                :requires (dissoc (:requires (:ns &env)) 'cljs.core)}))
