;; First-party applicable cljs.reader public contracts, also run unchanged on both references.
(require '[cljs.reader :as r] '[clojure.edn :as edn])
(let [original-table @r/*tag-table*
      original-default @r/*default-data-reader-fn*]
  [(r/parse-and-validate-timestamp "2012-02-29T23:59:60.123456-05:30")
   (r/parse-and-validate-timestamp "0007Z")
   (= (inst-ms (r/parse-timestamp "2010-11-12T13:14:15.666-05:00")) 1289585655666)
   (= (.getUTCFullYear (r/parse-timestamp "0007")) 1907)
   (mapv (fn [s] (try (r/parse-and-validate-timestamp s) false (catch js/Error e true)))
         ["bad" "2019-02-29" "2010-13" "2010-11-12T24:00" "2010-11-12T12:00:60"])
   (try
     (reset! r/*tag-table* {'direct inc})
     (reset! r/*default-data-reader-fn* (fn [tag value] [tag value]))
     [(r/read-string "#direct 4")
      (r/read-string "#unknown 8")
      (r/read-string {:readers {'direct dec}} "#direct 4")
      (r/read-string {:default (fn [tag value] value)} "#unknown 8")
      (= (r/register-tag-parser! 'direct dec) inc)
      (= (r/deregister-tag-parser! 'direct) dec)
      (fn? (r/register-default-tag-parser! (fn [tag value] value)))
      (fn? (r/deregister-default-tag-parser!))
      (binding [r/*tag-table* (atom {'bound inc})
                r/*default-data-reader-fn* (atom (fn [tag value] [tag value]))]
        [(r/read-string "#bound 9") (r/read-string "#other 7")])
      (= @r/*tag-table* {})
      (nil? @r/*default-data-reader-fn*)
      (try (edn/read-string "#bound 9") false (catch js/Error e true))]
     (finally (reset! r/*tag-table* original-table)
              (reset! r/*default-data-reader-fn* original-default)))
   (= @r/*tag-table* original-table)
   (= @r/*default-data-reader-fn* original-default)])
