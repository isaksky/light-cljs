(ns namespace-observer.macros)

;; The namespace symbol is the public macro-environment observation of the
;; metadata asserted through *cljs-ns* and the namespace AST name upstream.
(defmacro namespace-symbol-metadata []
  (list 'quote (apply dissoc (meta (:name (:ns &env)))
                     [:file :line :column :end-line :end-column])))
