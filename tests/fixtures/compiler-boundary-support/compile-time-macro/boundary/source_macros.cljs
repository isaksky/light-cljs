(ns boundary.source-macros)
(defmacro capture [x] (list 'quote [&form (:context &env) x]))
(defmacro choose ([x] x) ([x y & more] (last more)))
(defn run [observe]
  (and (= ['(capture 9) :expr 9] (capture 9))
       (= [1 4] [(choose 1) (choose 1 2 3 4)])
       (var? #'capture)
       (= #'capture (resolve 'capture))
       (:macro (meta #'capture))
       (try @#'capture
            (catch :default error
              (boolean (re-find #"compile-time macro" (.-message error)))))))
run
