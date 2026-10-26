(ns coll.macros)
(def kinds #{:get :count})
(defn lookup-form [k]
  (if (contains? kinds k)
    (list 'quote (assoc {} :op k))
    (list 'quote (dissoc {:op k} :op))))
(defn arity [args] (count (seq args)))
(defmacro describe [k & args] (vector (lookup-form k) (arity args)))
