(defn helper [x] (list '+ x 2))
(defmacro demand [x]
  (js* "globalThis.beforeDemand = (globalThis.beforeDemand || 0) + 1")
  (let [result (helper x)]
    (if (= 1 (js* "globalThis.beforeDemand")) result
      (throw (js/Error. "macro replayed")))))
(js/console.log (demand 40))
