(ns definitions.main
  (:require [definitions.values :as v :refer [*value*]]
            [cljs.core :as c :refer [binding with-redefs declare resolve defonce]
             :rename {binding scope with-redefs replacing declare forward resolve lookup defonce once}])
  (:require-macros [definitions.macros :as m]
                  [cljs.core :refer [binding] :rename {binding macro-scope}]))
(forward definitions.main/later)
(defn call-later [] (later))
(defn later [] 7)
(once data nil)
(once data (throw 0))
(m/define-once from-macro false)
(m/define-once from-macro (throw 0))
(def saved v/function)
(def live (fn [] (v/function)))
(def root (m/root-var v/function))
(js/console.log (pr-str (call-later) data from-macro))
(js/console.log (pr-str (scope [*value* 2] [(v/read-value) v/*value*]) (v/read-value)))
(js/console.log (pr-str (replacing [v/ordinary true] [(v/read-ordinary) v/ordinary]) (v/read-ordinary)))
(js/console.log (pr-str (m/scoped v/*value* 3 (v/read-value)) (macro-scope [v/*value* 4] (v/read-value))))
(js/console.log (pr-str (c/with-redefs [v/function (fn [] 9)] [(saved) (live) (root)]) [(saved) (live) (root)]))
(js/console.log (pr-str (identical? @root v/function) (= (lookup 'v/function) root) (:public-note (meta #'v/ordinary))))
(js/console.log (pr-str (try (scope [v/*value* 8] (throw (v/read-value))) (catch :default e e)) (v/read-value)))
(js/console.log (pr-str (set! v/*value* 11) (v/read-value)))
(def ^:dynamic *promise-value* 1)
(c/binding [*promise-value* 2]
  (let [snapshot *promise-value*]
    (.then (.resolve js/Promise) (fn [] (js/console.log snapshot *promise-value*)))))
