(js/benchExport
 vector list hash-map hash-set nth assoc conj get dissoc disj
 seq first next count = contains? keyword reduce +
 (fn* [v i] (v i))
 (fn* [k m] (k m))
 (fn* [m k] (m k))
 (fn* [s k] (s k))
 (fn* [m k v] (assoc m k (assoc (get m k) :leaf v)))
 (fn* [m k] (get (get m k) :leaf))
 (fn* [xs] (.reduce xs (fn* [a b] (js/benchAdd a b)) 0)))
