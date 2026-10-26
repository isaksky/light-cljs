(ns tickets.macros)
(def tickets (atom 0))
(defmacro take-ticket [] (let [n (swap! tickets inc)] (when (= n 4) (throw (js/Error. (str "ticket " n)))) n))
