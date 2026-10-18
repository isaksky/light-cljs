#!/usr/bin/env bb
;; Attribute self samples of a macOS `sample` call tree (one thread) to an
;; outer phase (outermost marker on the stack) and an inner engine (innermost
;; light-cljs module or JS/V8 frame).
(require '[clojure.string :as str])
(def file (first *command-line-args*))
(def outer-markers
  [["deferred units (drain_deferred)" #"14drain_deferred"]
   ["on-demand unit analysis (macro demand)" #"18CompilationSession7analyze"]
   ["ns top-level forms" #"17NamespaceAnalysis7analyze"]
   ["parse+load" #"6Loader5visit|6Loader10visit_lazy"]
   ["prepare_source" #"14prepare_source"]
   ["finish" #"17NamespaceAnalysis6finish"]
   ["prepare_application" #"prepare_application"]
   ["emit" #"7emitter|emit_esm|chunked"]])
(defn engine [sym]
  (cond
    (re-find #"^\?\?\?|^Builtins_|v8::|cppgc|^Runtime_|V8|EphemeronHash|absl::|std::__Cr" sym) "JS engine (V8)"
    (re-find #"10light_cljs" sym)
    (let [m (re-find #"10light_cljs(\d+)" sym)
          n (some-> m second parse-long)
          start (when m (+ (str/index-of sym (first m)) (count (first m))))]
      (if (and n start (<= (+ start n) (count sym)))
        (subs sym start (+ start n))
        "light_cljs?"))
    :else nil))
(def phase-self (atom {}))
(def engine-self (atom {}))
(def cross (atom {}))
(def nested (atom 0))
(def total (atom 0))
(defn attribute [stack self]
  (when (pos? self)
    (swap! total + self)
    (let [syms (map :sym stack)
          outer (or (some (fn [[k re]] (when (some #(re-find re %) syms) k)) outer-markers)
                    ;; innermost-first search would be wrong for outer; we want outermost: syms is root-first
                    "other")
          inner (or (some engine (reverse syms)) "other")
          js-depth (first (keep-indexed (fn [i s] (when (re-find #"Execution::Call" s) i)) syms))
          rust-after-js (and js-depth
                             (some #(re-find #"8analyzer|19compilation_session" %) (drop js-depth syms)))]
      (when rust-after-js (swap! nested + self))
      (swap! phase-self update outer (fnil + 0) self)
      (swap! engine-self update inner (fnil + 0) self)
      (swap! cross update [outer inner] (fnil + 0) self))))
(let [stack (atom [])]
  (doseq [line (line-seq (clojure.java.io/reader file))]
    (when-let [[_ prefix cnt sym] (re-find #"^([ +!:|]*)(\d+) (.*)$" line)]
      (let [depth (count prefix) cnt (parse-long cnt)]
        (loop []
          (let [top (peek @stack)]
            (when (and top (>= (:depth top) depth))
              (swap! stack pop)
              (attribute (conj @stack top) (- (:count top) (:children top)))
              (when-let [parent (peek @stack)]
                (swap! stack #(conj (pop %) (update parent :children + 0))))
              (recur))))
        (when-let [parent (peek @stack)]
          (swap! stack #(conj (pop %) (update parent :children + cnt))))
        (swap! stack conj {:depth depth :count cnt :children 0 :sym (str/trim sym)}))))
  (loop []
    (when-let [top (peek @stack)]
      (swap! stack pop)
      (attribute (conj @stack top) (- (:count top) (:children top)))
      (recur))))
(defn pct [n] (format "%6d %5.1f%%" n (* 100.0 (/ n (max 1 @total)))))
(println "total self samples" @total)
(println "\n== outer phase (outermost marker) ==")
(doseq [[k v] (sort-by (comp - val) @phase-self)] (println (pct v) k))
(println "\n== inner engine/module (innermost light-cljs module or V8) ==")
(doseq [[k v] (take 30 (sort-by (comp - val) @engine-self))] (println (pct v) k))
(println "\n== Rust analysis nested inside a JS call ==" (pct @nested))
(println "\n== phase x engine (top) ==")
(doseq [[k v] (take 40 (sort-by (comp - val) @cross))] (println (pct v) k))
