(defmacro recover [] (try (nth [1] 10) (catch :default ignored 42)))
(js/console.log (recover))
