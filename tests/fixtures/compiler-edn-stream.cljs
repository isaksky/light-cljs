(ns edn-stream.main (:require [clojure.edn :as edn] [cljs.reader :as reader]))
(defn stream-contract []
  (let [stream (reader/string-push-back-reader "nil false [1 2]\nlast" "shared.edn")]
    (assert (= [(edn/read stream) (edn/read {} stream)
                (edn/read stream true :unused {}) (reader/read-line stream)
                (edn/read stream) (edn/read stream)]
               [nil false [1 2] "" 'last nil])))
  (doseq [eof [nil false :done "EOF"]]
    (let [stream (reader/string-push-back-reader "")]
      (assert (= eof (edn/read {:eof eof} stream)))
      (assert (= eof (edn/read stream false eof {:eof :ignored})))
      (assert (= eof (edn/read stream nil eof {})))))
  (doseq [flag [true 0 ""]]
    (assert (= ["reader" "empty.edn" 1 1 0]
      (try (edn/read (reader/string-push-back-reader "" "empty.edn") flag :unused {:eof :ignored})
           (catch js/Error e [(.-category e) (.-source e) (.-line e) (.-column e) (.-offset e)])))))
  (assert (= ["reader" "two.edn" 1 1]
    (try (edn/read {} (reader/string-push-back-reader "" "two.edn"))
         (catch js/Error e [(.-category e) (.-source e) (.-line e) (.-column e)]))))
  (let [events (atom [])
        stream (reader/string-push-back-reader "#tag 1 #other 2 #_#tag 3 4")
        opts {:readers {'tag (fn [v] (swap! events conj [:tag v]) (inc v))}
              :default (fn [tag v] (swap! events conj [tag v]) [tag v])}]
    (assert (= [(edn/read stream true nil opts) (edn/read opts stream)
                (edn/read stream false :done opts) (edn/read stream false :done opts)
                @events]
               [2 ['other 2] 4 :done [[:tag 1] ['other 2] [:tag 3]]])))
  (binding [reader/*tag-table* (atom {'tag inc})
            reader/*default-data-reader-fn* (atom (fn [tag v] :ambient))]
    (assert (= ['tag 1] (edn/read (reader/string-push-back-reader "#tag 1") true nil
                                {:readers {'tag nil} :default vector})))
    (doseq [text ["#tag 1" "#uuid \"550e8400-e29b-41d4-a716-446655440000\"" "#queue [1]"
                  "#js [1]" "#inst \"2020\"" "@x" "`x" "~x" "#(inc %)" "#'x" "#=1"
                  "#\"x\"" "#?(:cljs 1)" "::x" "[1" "{:a}" "#{1 1}"]]
      (assert (= "reader" (try (edn/read (reader/string-push-back-reader text) false :done {})
                              (catch js/Error e (.-category e)))))))
  (assert (= ["reader" "malformed.edn" 2 1 1]
    (try (edn/read (reader/string-push-back-reader "\n]" "malformed.edn") false :done {})
         (catch js/Error e [(.-category e) (.-source e) (.-line e) (.-column e) (.-offset e)]))))
  (let [error (js/Error. "handler identity")]
    (assert (identical? error
      (try (edn/read (reader/string-push-back-reader "#tag 1") true nil
                     {:readers {'tag (fn [v] (throw error))}})
           (catch js/Error e e)))))
  (let [stream (reader/string-push-back-reader "#tag 1 2" "reentry.edn")]
    (assert (= ["host" "reentry.edn" 1 7]
      (try (edn/read stream true nil {:readers {'tag (fn [v] (edn/read stream false :done {}))}})
           (catch js/Error e [(.-category e) (.-source e) (.-line e) (.-column e)])))))
  (let [stream (reader/string-push-back-reader "[1" "fused.edn")]
    (assert (= "reader" (try (edn/read stream false :done {}) (catch js/Error e (.-category e)))))
    (assert (= :done (edn/read stream false :done {}))))
  (let [chunks (atom ["#tag " "41\n" "false"])
        seen (atom [])
        stream (reader/input-provider "chunks.edn" (fn [] (let [chunk (first @chunks)]
                 (swap! chunks rest) (swap! seen conj chunk) chunk)))]
    (assert (= [(edn/read stream true nil {:readers {'tag inc}}) (reader/read-line stream)
                (edn/read stream) (edn/read stream false :done {}) @seen]
               [42 "" false :done ["#tag " "41\n" "false" nil]])))
  (assert (= [1 2 nil :done]
    (with-in-str "1 2" [(edn/read) (edn/read) (edn/read *in*) (edn/read {:eof :done} *in*)])))
  (assert (= "reader" (with-in-str "" (try (edn/read) (catch js/Error e (.-category e))))))
  (assert (= "Expected input stream" (try (edn/read) (catch js/Error e (.-message e)))))
  (assert (= "Expected input stream" (with-in-str "1"
    (try (edn/read {}) (catch js/Error e (.-message e))))))
  (doseq [args [[1 2 3] [1 2 3 4 5]]]
    (assert (= (str "Invalid arity: " (count args))
      (try (apply edn/read args) (catch js/Error e (.-message e))))))
  (doseq [stream ["1" {} (js-obj)]]
    (assert (= "Expected input stream"
      (try (edn/read stream false :done {}) (catch js/Error e (.-message e))))))
  (let [stream (reader/string-push-back-reader "1")]
    (reader/close stream)
    (assert (= "host" (try (edn/read stream false :done {}) (catch js/Error e (.-category e))))))
  true)
