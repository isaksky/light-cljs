(transduce (comp (filter odd?) (map inc) (take 1000)) + 0 (range))
