(ns definitions.macros)
(defmacro scoped [target value & body] `(binding [~target ~value] ~@body))
(defmacro define-once [target value] `(defonce ~target ~value))
(defmacro root-var [target] `(var ~target))
