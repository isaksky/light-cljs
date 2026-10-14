;; Copyright (c) Rich Hickey. Eclipse Public License 1.0.
;; See epl-v10.html at the repository root; retain this notice.
(ns data-readers-test.records)
(defrecord Foo [a b])
(defn run [] (do (assert (= (Foo. 1 2) #data_readers_test.records.Foo{:a 1 :b 2})) (assert (= (Foo. 1 2)
           '#data_readers_test.records.Foo{:a 1 :b 2})) (assert (= (Foo. 1 2)
           (second ''#data_readers_test.records.Foo{:a 1 :b 2}))) (assert (= (Foo. 'a 'b)
           (let [a 1
                 b 2]
             #data_readers_test.records.Foo{:a a :b b}))
        (pr-str
          (let [a 1
                b 2]
            #data_readers_test.records.Foo{:a a :b b}))) (assert (= (Foo. 'a 'b)
           (let [a 1
                 b 2]
             '#data_readers_test.records.Foo{:a a :b b}))
        (pr-str
          (let [a 1
                b 2]
            '#data_readers_test.records.Foo{:a a :b b}))) true))
