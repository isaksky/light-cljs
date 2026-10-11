(ns functions.macros)
(defmacro overload [base]
  `(fn ([] ~base) ([x# & rest#] (+ ~base x# (count rest#)))))
