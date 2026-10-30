#!/usr/bin/env bb
;; Inclusive and self sample counts per function on one thread of a
;; `/usr/bin/sample` call tree. A function is counted once per stack, so
;; recursion does not inflate it. Names are demangled with c++filt.
;;
;;   bb benchmarks/sample-inclusive.bb SAMPLE.txt [--thread N] [--top N] [--match TEXT]
;;                                     [--buckets "NAME=TEXT;NAME=TEXT..."]
;;
;; --thread picks the Nth thread section (0, the main thread, by default);
;; --match keeps only functions whose demangled name contains TEXT.
;; --buckets instead splits the thread's samples into disjoint buckets: each
;; sample goes to the bucket of the innermost function on its stack whose
;; demangled name contains that bucket's TEXT (the first such bucket listed),
;; or to "other".
(require '[clojure.string :as str]
         '[babashka.process :as p])

(let [[file & opts] *command-line-args*
      opts (apply hash-map opts)
      thread (parse-long (get opts "--thread" "0"))
      top (parse-long (get opts "--top" "40"))
      needle (get opts "--match")
      buckets (some->> (get opts "--buckets") (re-seq #"([^=;]+)=([^;]+)") (map rest))
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
      nodes (keep #(when-let [m (re-find #"^([ +!:|]*)(\d+) (.*?)  \(in " %)]
                     [(count (nth m 1)) (parse-long (nth m 2)) (nth m 3)])
                  (rest section))
      raw (vec (distinct (map #(nth % 2) nodes)))
      demangled (zipmap raw (-> (p/shell {:in (str/join "\n" raw) :out :string} "c++filt")
                                :out str/split-lines))
      bucket-of (memoize (fn [name] (some (fn [[b text]] (when (str/includes? (demangled name) text) b))
                                          buckets)))
      inclusive (java.util.HashMap.)
      self (java.util.HashMap.)
      add (fn [^java.util.HashMap m k n] (.put m k (+ n (or (.get m k) 0))))]
  (loop [nodes nodes
         stack []                       ; [{:depth :name :n :children :bucket}]
         open {}]                       ; name -> times open on the stack
    (let [close (fn [stack open depth]
                  (loop [stack stack open open]
                    (if (and (seq stack) (>= (:depth (peek stack)) depth))
                      (let [{:keys [name n children bucket]} (peek stack)]
                        (add self (if buckets bucket name) (- n children))
                        (recur (pop stack) (update open name dec)))
                      [stack open])))]
      (if-let [[depth n name] (first nodes)]
        (let [[stack open] (close stack open depth)
              bucket (or (bucket-of name) (:bucket (peek stack)) "other")
              stack (if (seq stack)
                      (update-in stack [(dec (count stack)) :children] + n)
                      stack)]
          (when-not (pos? (get open name 0))
            (add inclusive name n))
          (recur (rest nodes)
                 (conj stack {:depth depth :name name :n n :children 0 :bucket bucket})
                 (update open name (fnil inc 0))))
        (close stack open -1))))
  (println (format "thread %d: %d samples" thread total))
  (if buckets
    (do (println "share%\tsamples\tbucket")
        (doseq [[b n] (sort-by val > (into {} self))]
          (println (format "%.1f\t%d\t%s" (* 100.0 (/ n total)) n b))))
    (let [rows (->> raw
                    (map (fn [r] [(demangled r) (get inclusive r) (get self r 0)]))
                    (filter (fn [[nice]] (or (nil? needle) (str/includes? nice needle))))
                    (sort-by second >)
                    (take top))]
      (println "inclusive%\tself%\tinclusive\tself\tfunction")
      (doseq [[nice incl slf] rows]
        (println (format "%.1f\t%.1f\t%d\t%d\t%s" (* 100.0 (/ incl total)) (* 100.0 (/ slf total))
                         incl slf (subs nice 0 (min 160 (count nice)))))))))
