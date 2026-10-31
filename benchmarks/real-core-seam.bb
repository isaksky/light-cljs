#!/usr/bin/env bb
;; Counts the seam between the real cljs.core (src/js_runtime.js) and the
;; Pants build's engine-owned values (docs/real-core-design.md, "Spike",
;; light-cljs-645):
;;
;; 1. per representation store, how many manifest units read its payload
;;    (`vectors.get(`), write it (`.set(`/`.reset(`) or only test the brand
;;    (`.has(`);
;; 2. how many units capture the hub `coreCells`, the registries
;;    `kindTables`, `methodKeys`, `protocolHooks`, a module-level `let`, or a
;;    binding the runtime reassigns at load time (`seq = behindProtocol(seq,
;;    ...)`), which the engine keeps in a binding cell;
;; 3. unless --no-walk, the publication walk of a naive boot of the whole
;;    COMPILER_RUNTIME and of the units `map` reaches, by running the ignored
;;    tests in tests/bbp_macros/real_core_spike.rs on the Pants build.
;;
;; Usage: bb benchmarks/real-core-seam.bb [--no-walk]
;; The static counts read the annotated source textually: a unit "captures"
;; a binding when its body names it as an identifier (not after a `.`)
;; outside the unit declaring it. The manifest's own analysis
;; (src/runtime_manifest.rs) is exact; these counts are for sizing.
(require '[babashka.process :refer [shell]]
         '[clojure.string :as str])

(def root (-> *file* java.io.File. .getAbsoluteFile .getParentFile .getParentFile))
(def runtime-text (slurp (java.io.File. root "src/js_runtime.js")))

(defn units
  "The manifest units as {:name :body}, in order. A header is a `//@unit`
  line and its `//@ ` continuation lines; the body runs to the next header
  or `//@exports`."
  [text]
  (loop [lines (str/split-lines text) current nil out []]
    (if-let [line (first lines)]
      (let [t (str/trim line)]
        (cond
          (str/starts-with? t "//@unit ")
          (recur (rest lines)
                 {:name (second (str/split t #"\s+")) :body []}
                 (cond-> out current (conj current)))
          (str/starts-with? t "//@exports")
          (cond-> out current (conj current))
          (str/starts-with? t "//@") (recur (rest lines) current out)
          current (recur (rest lines) (update current :body conj line) out)
          :else (recur (rest lines) current out)))
      (cond-> out current (conj current)))))

(def all-units
  (mapv #(update % :body (fn [ls] (str/join "\n" ls))) (units runtime-text)))

(defn code
  "A body without line comments and string literal contents, so names in
  prose and messages are not counted."
  [body]
  (-> body
      (str/replace #"(?m)//.*$" "")
      (str/replace #"\"(?:[^\"\\\n]|\\.)*\"" "\"\"")
      (str/replace #"'(?:[^'\\\n]|\\.)*'" "''")))

(def bodies (mapv (fn [u] (assoc u :code (code (:body u)))) all-units))

(defn count-units [pred] (count (filter pred bodies)))

(defn calls? [store method c]
  (re-find (re-pattern (str "(?<![\\w$.])" store "\\." method "\\(")) c))

(defn names? [binding c]
  (re-find (re-pattern (str "(?<![\\w$.])" (java.util.regex.Pattern/quote binding) "(?![\\w$])")) c))

(defn declared-by
  "The unit declaring top-level `binding` (its first `const`, `let`,
  `function` or `class` at the factory's top level)."
  [binding]
  (some (fn [u]
          (when (re-find (re-pattern (str "(?m)^  (?:const|let|var|function\\*?|class) (?:[^;=]*[,\\s])?"
                                          binding "(?![\\w$])"))
                         (:code u))
            (:name u)))
        bodies))

(defn capturing
  "Units other than `binding`'s declaring unit that name it."
  [binding]
  (let [owner (declared-by binding)]
    (filter #(and (not= (:name %) owner) (names? binding (:code %))) bodies)))

(def stores ["vectors" "cells" "maps" "sets" "views" "lazyValues"])

;; Writers of a store's payload. A unit writes one when it calls the store's
;; `set`/`reset`, or mutates a payload it retrieved: through the `get` call
;; itself (`lazyValues.get(this)[key] = v`), through a local holding it
;; (`const cell = cells.get(x); cell.chunkedState.hash = h`), or by passing
;; it to a helper that mutates its first parameter (`deferredValue(lazy)`),
;; in which case the helper's unit writes it too. Textual and
;; over-inclusive: a property write on a payload's nested object counts.

(def mutation-suffix
  "After an expression: a property or index step and an assignment, an
  update, or a mutating method call."
  (str "(?:\\??\\.[\\w$]+|\\[[^\\]]*\\])*"
       "(?:(?:\\??\\.[\\w$]+|\\[[^\\]]*\\])\\s*(?:\\*\\*|<<|>>>?|&&|\\|\\||\\?\\?|[-+*/%&|^])?=(?!=)"
       "|(?:\\.[\\w$]+|\\[[^\\]]*\\])\\s*(?:\\+\\+|--)"
       "|\\.(?:push|pop|shift|unshift|splice|sort|reverse|fill|copyWithin|set|delete|clear|add)\\()"))

(defn store-get [store]
  (str "(?<![\\w$.])" store "\\.get\\([^()]*(?:\\([^()]*\\)[^()]*)*\\)"))

(defn quote-re [s] (java.util.regex.Pattern/quote s))

(defn aliases
  "Locals a unit binds to a payload of `store`."
  [store c]
  (set (map second (re-seq (re-pattern (str "(?:\\b(?:const|let|var)\\s+|,\\s*)([\\w$]+)\\s*=\\s*"
                                             (store-get store)))
                           c))))

(defn mutates? [expr c]
  (re-find (re-pattern (str expr mutation-suffix)) c))

(defn mutating-helpers
  "`[name unit]` of functions that mutate their first parameter:
  `function deferredValue(state, ...)` or `kindHooks.realizeLazy = lazy =>`."
  [units]
  (for [u units
        [_ fname param] (concat
                         (re-seq #"function\s+([\w$]+)\s*\(\s*([\w$]+)" (:code u))
                         (re-seq #"([\w$][\w$.]*)\s*=\s*\(?\s*([\w$]+)\s*(?:,[^)=]*)?\)?\s*=>" (:code u)))
        :when (mutates? (str "(?<![\\w$.])" (quote-re param)) (:code u))]
    [fname (:name u)]))

(defn payload-writers
  "Names of the units that write `store`'s payload."
  [units store]
  (let [helpers (mutating-helpers units)
        passes? (fn [c expr]
                  (some (fn [[h _]] (re-find (re-pattern (str "(?<![\\w$.])" (quote-re h) "\\(\\s*" expr)) c))
                        helpers))
        writes (for [u units
                     :let [c (:code u)
                           locals (aliases store c)
                           alias-exprs (map #(str "(?<![\\w$.])" (quote-re %) "(?![\\w$])") locals)]
                     :when (or (calls? store "set" c) (calls? store "reset" c)
                               (mutates? (store-get store) c)
                               (some #(mutates? % c) alias-exprs)
                               (some #(passes? c %) (cons (store-get store) alias-exprs)))]
                 (:name u))
        via-helpers (for [u units
                          :let [c (:code u)
                                exprs (cons (store-get store)
                                            (map #(str "(?<![\\w$.])" (quote-re %) "(?![\\w$])")
                                                 (aliases store c)))]
                          [h owner] helpers
                          :when (some #(re-find (re-pattern (str "(?<![\\w$.])" (quote-re h) "\\(\\s*" %)) c)
                                      exprs)]
                      owner)]
    (set (concat writes via-helpers))))

(when (some #{"--self-test"} *command-line-args*)
  (let [units [{:name "set" :code "vectors.set(v, store);"}
               {:name "direct" :code "lazyValues.get(this)[key] = value;"}
               {:name "alias" :code "const cell = cells.get(x); cell.chunkedState.hash = h;"}
               {:name "push" :code "const items = vectors.get(v); items.push(1);"}
               {:name "helper" :code "function deferredValue(state, lazy) { state.value = 1; return state.value; }"}
               {:name "caller" :code "const lazy = lazyValues.get(x); return deferredValue(lazy, true);"}
               {:name "read" :code "const cell = cells.get(x); return cell.head === y;"}
               {:name "rebind" :code "let cell = cells.get(x); cell = other; return cell;"}]
        expect {"vectors" #{"set" "push"} "lazyValues" #{"direct" "helper" "caller"} "cells" #{"alias"}}]
    (doseq [[store want] expect]
      (let [got (payload-writers units store)]
        (when (not= got want)
          (binding [*out* *err*] (println "self-test failed:" store "got" got "want" want))
          (System/exit 1))))
    (println "self-test passed")
    (System/exit 0)))

(def module-lets
  (->> (re-seq #"(?m)^  let ([^;]*);" runtime-text)
       (mapcat (fn [[_ decls]]
                 (map second (re-seq #"(?:^|,\s*)([A-Za-z_$][\w$]*)\s*(?:=(?![=>])|$)" decls))))
       distinct
       vec))

(def load-time-reassigned
  "Top-level bindings a unit assigns at load time (`seq = ...;`), other than
  the module-level lets: wrapper effects."
  (->> bodies
       (mapcat (fn [u] (map second (re-seq #"(?m)^  ([A-Za-z_$][\w$]*) = " (:code u)))))
       (remove (set module-lets))
       distinct
       sort
       vec))

(println "## Static seam counts")
(println)
(println (format "Units: %d (src/js_runtime.js)." (count bodies)))
(println)
(println "| Store | Units reading the payload (`get`) | Units calling `set`/`reset` | Units writing the payload (any way) | Units testing the brand only (`has`) |")
(println "| --- | ---: | ---: | ---: | ---: |")
(doseq [s stores]
  (let [reads (count-units #(calls? s "get" (:code %)))
        setters (count-units #(or (calls? s "set" (:code %)) (calls? s "reset" (:code %))))
        writers (payload-writers bodies s)
        has-only (count-units #(and (calls? s "has" (:code %))
                                    (not (calls? s "get" (:code %)))
                                    (not (calls? s "set" (:code %)))))]
    (println (format "| `%s` | %d | %d | %d | %d |" s reads setters (count writers) has-only))
    (when (some #{"--writers"} *command-line-args*)
      (println (format "|   %s writers: %s | | | | |" s (str/join " " (sort writers)))))))
(println)
(println "| Captured state | Units naming it |")
(println "| --- | ---: |")
(doseq [b ["coreCells" "kindTables" "methodKeys" "protocolHooks" "kindHooks"]]
  (println (format "| `%s` | %d |" b (count (capturing b)))))
(let [by-let (set (mapcat (fn [b] (map :name (capturing b))) module-lets))
      by-reassigned (set (mapcat (fn [b] (map :name (capturing b))) load-time-reassigned))
      any (set (concat by-let by-reassigned
                       (mapcat (fn [b] (map :name (capturing b)))
                               ["coreCells" "kindTables" "methodKeys" "protocolHooks" "kindHooks"])))]
  (println (format "| a module-level `let` (%d: %s) | %d |"
                   (count module-lets) (str/join ", " module-lets) (count by-let)))
  (println (format "| a binding reassigned at load time (%d, e.g. %s) | %d |"
                   (count load-time-reassigned)
                   (str/join ", " (take 8 load-time-reassigned))
                   (count by-reassigned)))
  (println (format "| any of the above | %d |" (count any))))

(when-not (some #{"--no-walk"} *command-line-args*)
  (println)
  (println "## Publication walk (Pants build, ignored tests in tests/bbp_macros/real_core_spike.rs)")
  (println)
  (let [out (:out (shell {:dir (str root) :out :string :err :string
                          :extra-env {"CARGO_BUILD_JOBS" (or (System/getenv "CARGO_BUILD_JOBS") "4")}}
                         "cargo" "test" "--locked" "--no-default-features" "--features" "bbp-macros"
                         "--test" "bbp_macros" "publication_walk" "--" "--ignored" "--nocapture"
                         "--test-threads=1"))]
    (doseq [line (str/split-lines out)
            :when (str/starts-with? line "SEAM ")]
      (println (str "    " (subs line 5))))))
