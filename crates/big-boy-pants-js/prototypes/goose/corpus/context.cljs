(defmacro context [x]
  (list 'quote [x &form (get (get &env :ns) :name) (meta &form)]))
(js/console.log
  (count ^{:trace "call"} (context ^{:tag "node"} [foo :key {:k [1 1]}]))
  (count (context [bar :other {:z [2 2]}])))
