#!/usr/bin/env bb
;; For self samples, report the Rust function that made the outermost call into
;; V8 (the light-cljs frame just above the first Execution::Call), and the
;; innermost Rust function that invoked the leaf JS (to see nested reentry).
(require '[clojure.string :as str])
(def file (first *command-line-args*))
(defn ident [sym]
  ;; last two v0 identifiers of a mangled light_cljs symbol
  (let [ids (map second (re-seq #"\d+([A-Za-z_][A-Za-z0-9_]*)" (first (str/split sym #"\s")))) ]
    (str/join "::" (take-last 3 (remove #(re-find #"^(Cs|Nt|Nv|Ms|B)" %) ids)))))
(def outer-caller (atom {}))
(def leaf-caller (atom {}))
(def total (atom 0))
(defn attribute [stack self]
  (when (pos? self)
    (let [syms (mapv :sym stack)
          js? (fn [s] (re-find #"^\?\?\?|^Builtins_|v8::|cppgc|^Runtime_|EphemeronHash|absl::|std::__Cr" s))
          leaf (peek syms)]
      (when (some js? [leaf])
        (swap! total + self)
        (let [first-js (first (keep-indexed (fn [i s] (when (re-find #"v8_evaluator6invoke" s) i)) syms))
              before (when first-js (subvec syms 0 first-js))
              rust (fn [v] (last (filter #(re-find #"10light_cljs" %) v)))
              oc (when first-js (str (ident (nth syms (- first-js 1))) " <- " (ident (nth syms (- first-js 2))) " <- " (ident (nth syms (- first-js 3)))))
              lc (some-> syms rust ident)]
          (swap! outer-caller update (or oc "?") (fnil + 0) self)
          (swap! leaf-caller update (or lc "?") (fnil + 0) self))))))
(let [stack (atom [])]
  (doseq [line (line-seq (clojure.java.io/reader file))]
    (when-let [[_ prefix cnt sym] (re-find #"^([ +!:|]*)(\d+) (.*)$" line)]
      (let [depth (count prefix) cnt (parse-long cnt)]
        (loop []
          (let [top (peek @stack)]
            (when (and top (>= (:depth top) depth))
              (swap! stack pop)
              (attribute (conj @stack top) (- (:count top) (:children top)))
              (recur))))
        (when-let [parent (peek @stack)]
          (swap! stack #(conj (pop %) (update parent :children + cnt))))
        (swap! stack conj {:depth depth :count cnt :children 0 :sym (str/trim sym)}))))
  (loop []
    (when-let [top (peek @stack)]
      (swap! stack pop)
      (attribute (conj @stack top) (- (:count top) (:children top)))
      (recur))))
(println "JS self samples" @total)
(println "\n== Rust caller of the outermost JS entry ==")
(doseq [[k v] (take 25 (sort-by (comp - val) @outer-caller))] (println (format "%6d" v) k))
(println "\n== innermost Rust frame above the leaf JS ==")
(doseq [[k v] (take 25 (sort-by (comp - val) @leaf-caller))] (println (format "%6d" v) k))
