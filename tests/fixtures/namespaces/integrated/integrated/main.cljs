(ns integrated.main
  (:require [integrated.shared :as shared] [integrated.macros :refer [traced]]))

(defn run [observe]
  (js/console.log "integrated")
  (let [state (atom 0)
        changes (atom [])
        realized (atom [])
        [first-value & tail :as values] [1 2 3]
        {:keys [step] :or {step 2}} {}
        entry (with-meta (shared/->Entry 7 values) {:origin :shared})
        window (shared/->Window values)
        callback (with-meta (reify IFn (-invoke [_ x] (+ x step))) {:kind :callback})]
    (add-watch state :trace
      (fn [_ _ old-value new-value]
        (swap! changes conj [old-value new-value])
        (observe [old-value new-value])))
    (binding [shared/*offset* 10]
      (swap! state + shared/*offset*))
    (let [sum (apply shared/total @state [first-value] [tail [4]])
          calculated (traced observe "macro" (callback sum))
          lazy-values (map (fn [x] (swap! realized conj x) (callback x)) (seq window))
          pending @realized
          collected (into [] lazy-values)
          failure (try
                    (traced observe "throw" (throw (ex-info "integrated failure" {:value @state})))
                    (catch js/Error error
                      (observe (ex-message error))
                      (:value (ex-data error)))
                    (finally (js/console.log "cleanup")))
          recursive (letfn [(even-step [n] (if (zero? n) true (odd-step (dec n))))
                            (odd-step [n] (if (zero? n) false (even-step (dec n))))]
                      (even-step 12))]
      (remove-watch state :trace)
      (reset! state 99)
      [(name shared/platform) sum calculated (shared/countdown 100000)
       (shared/scale entry 3) (record? (assoc entry :extra 4))
       (name (:origin (meta entry))) (name (:kind (meta callback)))
       (count window) (get window 1) (reduce + window)
       pending collected @realized failure recursive
       @changes @state shared/*offset*])))

run
