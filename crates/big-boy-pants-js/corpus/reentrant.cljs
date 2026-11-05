(defmacro seed [] [11 22])
(def shared (seed))
(defmacro consume []
  (let [counter (js* "({n:0})")]
    (js* "(~{}.n = ~{}.n + 1)" counter counter)
    (let [value shared]
      (list 'quote [value (js* "~{}.n" counter)]))))
(js/console.log (pr-str (consume)))
