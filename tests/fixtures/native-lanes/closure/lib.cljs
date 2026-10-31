(ns closure.lib) (def log (atom [])) (defmacro note [x] (count (swap! log conj x)))
