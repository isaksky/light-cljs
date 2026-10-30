#!/usr/bin/env bb
;; Inclusive and self sample counts per function on one thread of a
;; `/usr/bin/sample` call tree. A function is counted once per stack, so
;; recursion does not inflate it. Names are demangled with c++filt.
;;
;;   bb benchmarks/sample-inclusive.bb SAMPLE.txt [--thread N] [--top N] [--match TEXT]
;;
;; --thread picks the Nth thread section (0, the main thread, by default);
;; --match keeps only functions whose demangled name contains TEXT.
(require '[clojure.string :as str]
         '[babashka.process :as p])

(let [[file & opts] *command-line-args*
      opts (apply hash-map opts)
      thread (parse-long (get opts "--thread" "0"))
      top (parse-long (get opts "--top" "40"))
      needle (get opts "--match")
      lines (->> (str/split-lines (slurp file))
                 (drop-while #(not= "Call graph:" %))
                 rest
                 (take-while #(not (str/starts-with? % "Total number in stack"))))
      sections (->> lines
                    (partition-by #(boolean (re-find #"^    \d+ Thread_" %)))
                    (partition-all 2)
                    (map #(apply concat %)))
      section (nth sections thread)
      total (parse-long (second (re-find #"^    (\d+)" (first section))))
      inclusive (java.util.HashMap.)
      self (java.util.HashMap.)
      add (fn [^java.util.HashMap m k n] (.put m k (+ n (or (.get m k) 0))))]
  (loop [lines (rest section)
         stack []                       ; [{:depth :name :n :children}]
         open {}]                       ; name -> times open on the stack
    (let [close (fn [stack open depth]
                  (loop [stack stack open open]
                    (if (and (seq stack) (>= (:depth (peek stack)) depth))
                      (let [{:keys [name n children]} (peek stack)]
                        (add self name (- n children))
                        (recur (pop stack) (update open name dec)))
                      [stack open])))]
      (if-let [line (first lines)]
        (if-let [m (re-find #"^([ +!:|]*)(\d+) (.*?)  \(in " line)]
          (let [depth (count (nth m 1))
                n (parse-long (nth m 2))
                name (nth m 3)
                [stack open] (close stack open depth)
                stack (if (seq stack)
                        (update-in stack [(dec (count stack)) :children] + n)
                        stack)]
            (when-not (pos? (get open name 0))
              (add inclusive name n))
            (recur (rest lines)
                   (conj stack {:depth depth :name name :n n :children 0})
                   (update open name (fnil inc 0))))
          (recur (rest lines) stack open))
        (close stack open -1))))
  (let [names (vec (keys inclusive))
        demangled (-> (p/shell {:in (str/join "\n" names) :out :string} "c++filt")
                      :out str/split-lines)
        rows (->> (map vector names demangled)
                  (map (fn [[raw nice]] [nice (get inclusive raw) (get self raw 0)]))
                  (filter (fn [[nice]] (or (nil? needle) (str/includes? nice needle))))
                  (sort-by second >)
                  (take top))]
    (println (format "thread %d: %d samples" thread total))
    (println "inclusive%\tself%\tinclusive\tself\tfunction")
    (doseq [[nice incl slf] rows]
      (println (format "%.1f\t%.1f\t%d\t%d\t%s" (* 100.0 (/ incl total)) (* 100.0 (/ slf total))
                       incl slf (subs nice 0 (min 160 (count nice))))))))
