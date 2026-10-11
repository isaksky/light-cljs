(ns cljs.user)
(defn output-witness [] (let [events (atom []) sink #(swap! events conj [:out %]) err #(swap! events conj [:err %])]
        (binding [*print-fn* sink *print-err-fn* err]
          (assert (identical? sink (set-print-fn! sink)))
          (assert (identical? err (set-print-err-fn! err)))
          (print "a" "b") (pr "c") (*print-err-fn* "error") (string-print "direct")
          (assert (= [[:out "a b"] [:out "\"c\""] [:err "error"] [:out "direct"]] @events))
          (reset! events [])
          (binding [*print-newline* false] (println "quiet") (prn "quiet"))
          (assert (= [[:out "quiet"] [:out "\"quiet\""]] @events))
          (assert (= "outerinner\nend" (with-out-str
                     (print "outer") (print (with-out-str (println "inner"))) (print "end"))))
          (assert (= "yes\n" (binding [*print-newline* false] (with-out-str (println "yes")))))
          (try (with-out-str (print "hidden") (throw (js/Error. "capture"))) (catch js/Error _ nil))
          (assert (identical? sink *print-fn*))
          (assert (true? *print-newline*))
          (binding [*print-fn* (fn [_] (throw (js/Error. "sink"))) *print-readably* true]
            (try (print "oops") (catch js/Error _ nil))
            (assert (true? *print-readably*)))
          (reset! events [])
          (with-redefs [flush #(swap! events conj :flush)]
            (newline) (newline {:flush-on-newline false})
            (binding [*flush-on-newline* false] (println "line"))
            (prn :done))
          (assert (= [[:out "\n"] :flush [:out "\n"] [:out "line"] [:out "\n"]
                      [:out ":done"] [:out "\n"] :flush] @events)))
        true))
(defn oracle-run [observe] (output-witness))
oracle-run
