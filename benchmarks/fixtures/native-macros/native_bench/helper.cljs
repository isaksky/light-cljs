(ns native-bench.helper)
(defn demanded [x] (list '+ x 2))
(defmacro via-helper [x]
  (js* "(globalThis.__nativeBenchEffects = (globalThis.__nativeBenchEffects || 0) + 1)")
  (let [form (demanded x)]
    (list 'vector form (js* "globalThis.__nativeBenchEffects"))))
(js/console.log (pr-str (via-helper 40)))
