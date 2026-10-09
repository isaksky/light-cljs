(ns demo.macros)
(defmacro twice [x]
  `(let [value# ~x]
     (js/Math.imul value# 2)))
