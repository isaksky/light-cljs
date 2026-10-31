(ns closure.other) (defmacro double [x] (list (quote *) 2 x))
