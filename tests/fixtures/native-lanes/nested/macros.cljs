(ns nested.macros) (def counter (atom 0)) (defmacro next-value [] (swap! counter inc))
