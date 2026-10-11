(ns edn-stream.main (:require [clojure.edn :as edn] [cljs.reader :as reader-types]))
(defn stream-contract []
  (let [s (reader-types/string-push-back-reader "nil false 42")]
    (assert (= [(edn/read s) (edn/read {} s) (edn/read s true nil {}) (edn/read s)]
               [nil false 42 nil])))
  (doseq [eof [nil false :done "EOF"]]
    (let [s (reader-types/string-push-back-reader "")]
      (assert (= eof (edn/read {:eof eof} s)))
      (assert (= eof (edn/read s false eof {:eof :ignored})))
      (assert (= eof (edn/read s nil eof {})))))
  (doseq [flag [true 0 ""]]
    (assert (try (edn/read (reader-types/string-push-back-reader "") flag :unused {:eof :ignored})
                 false (catch :default e true))))
  (assert (try (edn/read {} (reader-types/string-push-back-reader "")) false (catch :default e true)))
  (let [s (reader-types/string-push-back-reader "#tag 1 #tag 2") events (atom [])
        opts {:readers {'tag (fn [x] (swap! events conj x) (inc x))}}]
    (assert (= [(edn/read opts s) (edn/read s false :done opts) (edn/read s false :done opts) @events]
               [2 3 :done [1 2]])))
  true)
