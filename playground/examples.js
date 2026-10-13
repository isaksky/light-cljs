// The examples the playground offers, smallest first. Each is a complete
// program: an entry namespace and its files. `expect` is the printed value of
// the entry's last form, which check.mjs verifies at every optimization level.

export const EXAMPLES = [
  {
    id: "hello",
    title: "Hello, world",
    entry: "hello.core",
    expect: '{:count 5, :longest "world", :sum 15}',
    files: [
      {
        path: "hello/core.cljs",
        source: `(ns hello.core)

;; As in ClojureScript, println needs a print function: this one is console.log.
(enable-console-print!)

(defn greet [name]
  (str "Hello, " name "!"))

(println (greet "world"))

(doseq [n (range 1 6)]
  (println n "squared is" (* n n)))

;; The entry namespace's last form is the program's value.
{:count   (count (range 1 6))
 :longest (apply max-key count ["hi" "hello" "world"])
 :sum     (reduce + (range 1 6))}
`,
      },
    ],
  },
  {
    id: "macro",
    title: "A macro",
    entry: "hello.macro",
    expect: "[0 1 1 2 3 5 8 13 21 34]",
    files: [
      {
        path: "hello/macro.cljs",
        source: `(ns hello.macro)

(enable-console-print!)

;; defmacro works in an ordinary .cljs file and the macro is usable right
;; below it: the compiler runs macros in its embedded JavaScript engine while
;; compiling. Try the JavaScript tab to see what the expansion became.
(defmacro unless [test & body]
  \`(if ~test nil (do ~@body)))

(defmacro timed [label & body]
  \`(let [start# (js/performance.now)
         value# (do ~@body)]
     (println ~label "took" (- (js/performance.now) start#) "ms")
     value#))

(defn fib-seq []
  (map first (iterate (fn [[a b]] [b (+ a b)]) [0 1])))

(unless (> 1 2)
  (println "one is not greater than two"))

(timed "ten fibonacci numbers"
  (vec (take 10 (fib-seq))))
`,
      },
    ],
  },
  {
    id: "sequences",
    title: "Sequences and destructuring",
    entry: "seqs.core",
    expect: '{:fizzbuzz "1 2 Fizz 4 Buzz Fizz 7 8 Fizz Buzz 11 Fizz 13 14 FizzBuzz", :primes [2 3 5 7 11 13 17 19 23 29], :by-length {3 ["the" "fox"], 5 ["quick" "brown" "jumps"]}}',
    files: [
      {
        path: "seqs/core.cljs",
        source: `(ns seqs.core
  (:require [clojure.string :as str]))

(enable-console-print!)

(defn fizzbuzz [n]
  (cond (zero? (mod n 15)) "FizzBuzz"
        (zero? (mod n 3))  "Fizz"
        (zero? (mod n 5))  "Buzz"
        :else              n))

;; Lazy, infinite sequences are fine; only what is consumed gets computed.
(def naturals (iterate inc 1))

(defn prime? [n]
  (and (> n 1)
       (not-any? #(zero? (rem n %))
                 (range 2 (inc (js/Math.floor (js/Math.sqrt n)))))))

;; Destructuring in let, fn parameters and loop.
(let [[first-word & more :as all] (str/split "the quick brown fox jumps" #" ")
      {:keys [x y] :or {y 0}} {:x 10}]
  (println "first:" first-word "rest:" more "all:" (count all))
  (println "x + y =" (+ x y)))

(loop [[n & more] (range 1 16), acc []]
  (if n
    (recur more (conj acc (fizzbuzz n)))
    (println "fizzbuzz:" acc)))

{:fizzbuzz  (str/join " " (map fizzbuzz (range 1 16)))
 :primes    (vec (take 10 (filter prime? naturals)))
 :by-length (into (sorted-map)
                  (group-by count (str/split "the quick brown fox jumps" #" ")))}
`,
      },
    ],
  },
  {
    id: "state",
    title: "Atoms and watches",
    entry: "state.core",
    expect: "{:balance 120, :history [100 70 120], :overdraft-rejected 1}",
    files: [
      {
        path: "state/core.cljs",
        source: `(ns state.core)

(enable-console-print!)

(def account (atom {:balance 0 :history []}))
(def rejected (atom 0))

(add-watch account :log
  (fn [_ _ old new]
    (println "balance:" (:balance old) "->" (:balance new))))

(defn deposit! [amount]
  (swap! account (fn [{:keys [balance] :as acct}]
                   (-> acct
                       (assoc :balance (+ balance amount))
                       (update :history conj (+ balance amount))))))

(defn withdraw! [amount]
  (if (< (:balance @account) amount)
    (do (swap! rejected inc)
        (println "rejected: cannot withdraw" amount))
    (deposit! (- amount))))

(deposit! 100)
(withdraw! 30)
(withdraw! 500)
(deposit! 50)

;; Reading an atom yields a plain, immutable value.
(assoc @account :overdraft-rejected @rejected)
`,
      },
    ],
  },
  {
    id: "protocols",
    title: "Protocols, records and multimethods",
    entry: "shapes.core",
    expect: '{:areas [78.54 12 4.5], :largest "circle", :described ("a circle of radius 5" "a 3 by 4 rectangle" "a triangle")}',
    files: [
      {
        path: "shapes/core.cljs",
        source: `(ns shapes.core)

(enable-console-print!)

(defprotocol Shape
  (area [this])
  (perimeter [this]))

(defrecord Circle [r]
  Shape
  (area [_] (* js/Math.PI r r))
  (perimeter [_] (* 2 js/Math.PI r)))

(defrecord Rect [w h]
  Shape
  (area [_] (* w h))
  (perimeter [_] (* 2 (+ w h))))

;; A plain map can take part too: extend the protocol to maps by shape kind.
(extend-protocol Shape
  cljs.core/PersistentArrayMap
  (area [{:keys [base height]}] (/ (* base height) 2))
  (perimeter [{:keys [a b c]}] (+ a b c)))

(defmulti describe (fn [shape] (if (record? shape) (type shape) :map)))
(defmethod describe Circle [{:keys [r]}] (str "a circle of radius " r))
(defmethod describe Rect [{:keys [w h]}] (str "a " w " by " h " rectangle"))
(defmethod describe :default [_] "a triangle")

(def shapes [(->Circle 5) (map->Rect {:w 3 :h 4}) {:base 3 :height 3 :a 3 :b 3 :c 3}])

(defn round2 [x] (/ (js/Math.round (* 100 x)) 100))

(doseq [shape shapes]
  (println (describe shape) "has area" (round2 (area shape))
           "and perimeter" (round2 (perimeter shape))))

(println "records are maps too:" (assoc (->Circle 1) :colour "red"))

{:areas     (mapv (comp round2 area) shapes)
 :largest   (if (instance? Circle (apply max-key area shapes)) "circle" "other")
 :described (map describe shapes)}
`,
      },
    ],
  },
  {
    id: "namespaces",
    title: "Several namespaces and macros",
    entry: "demo.main",
    expect: '{:total 55, :words ["the" "quick" "brown" "fox"], :sorted {:a 1, :b 2, :c 3}}',
    files: [
      {
        path: "demo/main.cljs",
        source: `(ns demo.main
  (:require [demo.math :as math]
            [demo.macros :refer [twice unless]]
            [clojure.string :as str]))

;; As in ClojureScript, println needs a print function: this one is console.log.
(enable-console-print!)

(defn describe [x]
  (str x " doubled is " (twice x)))

(println "squares:" (mapv math/square (range 1 6)))
(println (str/join ", " (map describe [1 2 3])))
(unless (empty? math/primes)
  (println "primes below 30:" math/primes))

;; The entry's last form is the program's value.
{:total (reduce + (map math/square (range 1 6)))
 :words (str/split "the quick brown fox" #" ")
 :sorted (into (sorted-map) {:b 2 :a 1 :c 3})}
`,
      },
      {
        path: "demo/math.cljs",
        source: `(ns demo.math)

(defn square [x] (* x x))

(defn prime? [n]
  (and (> n 1)
       (not-any? #(zero? (rem n %)) (range 2 (inc (js/Math.floor (js/Math.sqrt n)))))))

(def primes (vec (filter prime? (range 30))))
`,
      },
      {
        path: "demo/macros.cljs",
        source: `(ns demo.macros)

;; Macros are ordinary defmacros in .cljs files; the compiler runs them in
;; its embedded JavaScript engine while compiling.
(defmacro twice [x]
  \`(let [v# ~x] (+ v# v#)))

(defmacro unless [test & body]
  \`(if ~test nil (do ~@body)))
`,
      },
    ],
  },
  {
    id: "calculator",
    title: "Larger: a calculator in three namespaces",
    entry: "calc.main",
    expect: '{"1 + 2 * 3" 7, "(1 + 2) * 3" 9, "2 ^ 3 ^ 2" 512, "10 / 4 - 1" 1.5, "-(3 + 4) * 2" -14, "x * (x + 1) / 2" 55}',
    files: [
      {
        path: "calc/main.cljs",
        source: `(ns calc.main
  (:require [calc.lexer :as lexer]
            [calc.parser :as parser]
            [calc.eval :refer [evaluate]]
            [calc.macros :refer [report]]))

;; A small arithmetic language: the lexer turns text into tokens, the parser
;; builds a tree by precedence climbing and eval walks the tree. Each lives
;; in its own namespace; the reporting macro lives in a fourth.
(enable-console-print!)

(def inputs
  ["1 + 2 * 3"
   "(1 + 2) * 3"
   "2 ^ 3 ^ 2"
   "10 / 4 - 1"
   "-(3 + 4) * 2"
   "x * (x + 1) / 2"])

(defn calculate [text env]
  (-> text lexer/tokenize parser/parse (evaluate env)))

(doseq [text inputs]
  (report text (calculate text {'x 10})))

(println "tokens:" (lexer/tokenize "-(3 + 4) * 2"))
(println "tree:  " (parser/parse (lexer/tokenize "1 + 2 * 3")))

(println "errors are values too:")
(doseq [bad ["1 +" "2 * (3" "y + 1"]]
  (println " " (pr-str bad) "->" (try (calculate bad {})
                                   (catch :default e (ex-message e)))))

(into {} (map (fn [text] [text (calculate text {'x 10})]) inputs))
`,
      },
      {
        path: "calc/lexer.cljs",
        source: `(ns calc.lexer
  (:require [clojure.string :as str]))

(defn- digit? [c] (and c (re-matches #"[0-9]" c)))
(defn- letter? [c] (and c (re-matches #"[a-zA-Z_]" c)))

(def ^:private operators
  {"+" :plus, "-" :minus, "*" :times, "/" :divide, "^" :power, "(" :open, ")" :close})

(defn tokenize
  "Turns text into a vector of tokens: numbers, symbols and operator keywords."
  [text]
  (loop [chars (seq text), tokens []]
    (let [c (first chars)]
      (cond
        (nil? c) tokens

        (str/blank? c) (recur (rest chars) tokens)

        (digit? c)
        (let [[number more] (split-with #(or (digit? %) (= % ".")) chars)]
          (recur more (conj tokens (js/parseFloat (apply str number)))))

        (letter? c)
        (let [[name more] (split-with #(or (letter? %) (digit? %)) chars)]
          (recur more (conj tokens (symbol (apply str name)))))

        (operators c)
        (recur (rest chars) (conj tokens (operators c)))

        :else (throw (ex-info (str "unexpected character " (pr-str c)) {:at (count tokens)}))))))
`,
      },
      {
        path: "calc/parser.cljs",
        source: `(ns calc.parser)

;; Precedence climbing over the token vector. The result is a tree of
;; [:op left right], [:neg x], numbers and symbols.

(def ^:private precedence {:plus 1, :minus 1, :times 2, :divide 2, :power 3})
(def ^:private right-assoc #{:power})

(declare parse-expr)

(defn- expect [tokens token]
  (if (= (first tokens) token)
    (rest tokens)
    (throw (ex-info (str "expected " token ", got " (pr-str (or (first tokens) "end of input"))) {}))))

(defn- parse-atom [tokens]
  (let [t (first tokens)]
    (cond
      (number? t) [t (rest tokens)]
      (symbol? t) [t (rest tokens)]
      (= t :minus) (let [[x more] (parse-atom (rest tokens))] [[:neg x] more])
      (= t :open) (let [[x more] (parse-expr (rest tokens) 0)]
                    [x (expect more :close)])
      :else (throw (ex-info (str "unexpected " (pr-str (or t "end of input"))) {})))))

(defn- parse-expr [tokens min-prec]
  (loop [[left tokens] (parse-atom tokens)]
    (let [op (first tokens)
          prec (precedence op)]
      (if (and prec (>= prec min-prec))
        (let [next-prec (if (right-assoc op) prec (inc prec))
              [right more] (parse-expr (rest tokens) next-prec)]
          (recur [[op left right] more]))
        [left tokens]))))

(defn parse [tokens]
  (let [[tree rest-tokens] (parse-expr tokens 0)]
    (when (seq rest-tokens)
      (throw (ex-info (str "trailing tokens " (pr-str (vec rest-tokens))) {})))
    tree))
`,
      },
      {
        path: "calc/eval.cljs",
        source: `(ns calc.eval)

(def ^:private operators
  {:plus +, :minus -, :times *, :divide /, :power (fn [a b] (js/Math.pow a b))})

(defn evaluate
  "Walks a parse tree; symbols look themselves up in env."
  [tree env]
  (cond
    (number? tree) tree
    (symbol? tree) (if (contains? env tree)
                     (get env tree)
                     (throw (ex-info (str "unbound variable " tree) {:env env})))
    (= (first tree) :neg) (- (evaluate (second tree) env))
    :else (let [[op left right] tree]
            ((operators op) (evaluate left env) (evaluate right env)))))
`,
      },
      {
        path: "calc/macros.cljs",
        source: `(ns calc.macros)

;; Prints the source text next to its value, padded so the values line up.
(defmacro report [text value]
  \`(let [text# ~text]
     (println (.padEnd text# 16) "=" ~value)))
`,
      },
    ],
  },
];
