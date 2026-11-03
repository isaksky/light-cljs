;; Declared effect: one fresh compiler gensym; never safe for automatic replay.
(defmacro announce [x]
  (let [fresh (gensym "goose_effect_")]
    (list 'quote [x (symbol? fresh)])))
(js/console.log (pr-str (announce 7)))
