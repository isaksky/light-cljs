(ns integrated.macros)

(defmacro traced [observe label expression]
  `(do (~observe ~label) ~expression))
