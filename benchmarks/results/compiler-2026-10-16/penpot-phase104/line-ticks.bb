(require '[cheshire.core :as json] '[babashka.fs :as fs] '[clojure.string :as str])
(def here (fs/parent *file*))
(def profile (get-in (json/parse-string (slurp (fs/file here "cpu-profile.json")) true) [:result :profile]))
(def runtime-lines (->> (str/split-lines (slurp "/private/tmp/light-cljs-phase104/project/src/js_runtime.js")) (remove #(str/starts-with? (str/triml %) "//@")) (str/join "\n")))
(def source-lines (str/split-lines (str/replace (slurp "/private/tmp/light-cljs-phase104/project/src/v8_services.js") "__LIGHT_RUNTIME_SOURCE__" runtime-lines)))
(spit (fs/file here "profile-runtime.js") (str/join "\n" source-lines))
(doseq [function ["visit" "macroArguments" "decodeNode" "decode" "publicMethod"]]
  (let [ticks (reduce (fn [out n] (reduce (fn [m {:keys [line ticks]}] (update m line (fnil + 0) ticks)) out (:positionTicks n))) {}
                (filter #(and (= "3" (get-in % [:callFrame :scriptId])) (= function (get-in % [:callFrame :functionName]))) (:nodes profile)))]
    (println function)
    (doseq [[line count] (take 18 (sort-by val > ticks))]
      (println count line (get source-lines (dec line))))))
