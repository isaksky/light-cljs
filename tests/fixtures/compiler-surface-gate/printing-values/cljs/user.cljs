(ns cljs.user)
(defrecord PrintingRecord [a b])
      (defn printing-witness []
        (assert (= "" (pr-str)))
        (assert (= "#" (binding [*print-readably* false *print-length* 1 *print-level* 0 *print-meta* true]
                           (str (with-meta ["x" [1 2]] {:a true})))))
        (assert (= "\n" (prn-str)))
        (assert (= "-0.0 ##NaN ##Inf ##-Inf" (pr-str -0.0 js/NaN js/Infinity (- js/Infinity))))
        (assert (= "\"escape chars \\t \\r \\n \\\\ \\\" \\b \\f\"" (pr-str "escape chars \t \r \n \\ \" \b \f")))
        (assert (= "[\"üñîçó∂£\" :ทดสอบ/你好 こんにちは]" (pr-str ["üñîçó∂£" :ทดสอบ/你好 'こんにちは])))
        (assert (= "[1 ...] {...} #{...} #js [1 ...] #js {...}"
                   (str (binding [*print-length* 1] (pr-str [1 2])) " "
                        (binding [*print-length* 0] (pr-str {:a 1 :b 2} #{1 2})) " "
                        (binding [*print-length* 1] (pr-str #js [1 2])) " "
                        (binding [*print-length* 0] (pr-str #js {:a 1})))))
        (assert (= "[[1 #] {:a #}]" (binding [*print-level* 2] (pr-str [[1 [2]] {:a [3]}]))))
        (assert (= "# # # # #" (binding [*print-level* 0] (pr-str [1] {:a 1} #{1} #js [1] #js {:a 1}))))
        (assert (= "[1 2]" (binding [*print-length* 0] (pr-str-with-opts [[1 2]] {:readably true}))))
        (assert (= "[1 …] {:a 1, …} #{1 …} #js [1 …]"
                   (pr-str-with-opts [[1 2] (array-map :a 1 :b 2) (sorted-set 1 2) #js [1 2]]
                                     {:readably true :print-length 1 :more-marker "…"})))
        (assert (= "(1 …)" (pr-str-with-opts [(eduction (map identity) [1 2])] {:print-length 1 :more-marker "…"})))
        (assert (= "#cljs.user.PrintingRecord{:a ..., ...}" (binding [*print-length* 1] (pr-str (->PrintingRecord 1 2)))))
        (assert (= "#cljs.user.PrintingRecord{#, #}" (binding [*print-level* 1] (pr-str (->PrintingRecord [1] 2)))))
        (assert (= "#cljs.user.PrintingRecord{:a #, :b 2}" (binding [*print-level* 2] (pr-str (->PrintingRecord [1] 2)))))
        (assert (= "#" (binding [*print-level* 0] (pr-str (->PrintingRecord [1] 2)))))
        (assert (= "#:sample{:a 1, :b 2}" (binding [*print-namespace-maps* true] (pr-str {:sample/a 1 :sample/b 2}))))
        (assert (= "^{:tag \"v\"} [\"x\"]" (binding [*print-meta* true] (pr-str (with-meta ["x"] {:tag "v"})))))
        (assert (= "[x]" (binding [*print-meta* true *print-readably* true]
                              (pr-str-with-opts [(with-meta ["x"] {:tag "v"})] {}))))
        (assert (= "x [y]" (print-str "x" ["y"])))
        (assert (= "x\n" (println-str "x")))
        (assert (= "\"x\"\n" (prn-str "x")))
        (assert (nil? (flush)))
        true)
(defn oracle-run [observe] (printing-witness))
oracle-run
