(ns bootstrap.app)

(defmacro threaded [input]
  (as-> input n
    (list '+ n n)
    (list '* 2 n)))

(let [seen (js/Array.)
      n 99
      value (as-> (do (.push seen "start") 2) n
              (do (.push seen "double") (+ n n))
              (let [n 10] (.push seen "shadow") (inc n))
              (do (.push seen "nested") (as-> n n (inc n) (* n 2)))
              [n n])
      empty (as-> (do (.push seen "zero") 7) n)]
  (js/console.log
    (pr-str [value n empty (threaded 3)
             (as-> [2 3] [a b] (+ a b)) (vec seen)])))
