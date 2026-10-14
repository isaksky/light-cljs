// The examples the playground offers, smallest first. Each is a complete
// program: an entry namespace and its files. `expect` is the printed value of
// the entry's last form, which check.mjs verifies at every optimization level.
//
// The small examples show one feature each, log through js/console.log and
// end in a number or string, so that the emitted JavaScript holds little
// beyond the feature; only "Printing" and the calculator call
// (enable-console-print!). At --optimize advanced a program that never prints
// embeds no printer, so a collection as its final value has no toString and
// the page shows [object Object]; check.mjs allows that at advanced only.

export const EXAMPLES = [
  {
    id: "hello",
    title: "Hello, world",
    entry: "hello.core",
    expect: "Hello, playground!",
    files: [
      {
        path: "hello/core.cljs",
        source: `(ns hello.core)

(defn greet [name]
  (str "Hello, " name "!"))

(js/console.log (greet "world"))

;; The entry namespace's last form is the program's value.
(greet "playground")
`,
      },
    ],
  },
  {
    id: "macro",
    title: "A macro",
    entry: "hello.macro",
    expect: "the body ran",
    files: [
      {
        path: "hello/macro.cljs",
        source: `(ns hello.macro)

;; defmacro works in an ordinary .cljs file and the macro is usable right
;; below it: the compiler runs macros in its embedded JavaScript engine
;; while compiling. Open the JavaScript tab to see the expansion.
(defmacro unless [test & body]
  \`(if ~test nil (do ~@body)))

(unless (> 1 2)
  (js/console.log "one is not greater than two"))

(unless (< 1 2)
  (js/console.log "this never prints"))

;; The last form's value is the program's value, here the macro's body.
(unless (> 1 2) "the body ran")
`,
      },
    ],
  },
  {
    id: "destructuring",
    title: "Destructuring",
    entry: "feature.destructuring",
    expect: "6",
    files: [
      {
        path: "feature/destructuring.cljs",
        source: `(ns feature.destructuring)

(defn sum-first-two [[x y & more]]
  (js/console.log "ignoring" (count more) "more")
  (+ x y))

(defn city [{:keys [city] :or {city "unknown"}}]
  city)

(js/console.log (city {:street "Main" :city "Oslo"}))
(js/console.log (city {}))

(let [{:keys [x y]} {:x 10 :y 20}]
  (js/console.log "x + y =" (+ x y)))

(sum-first-two [1 5 3 4 5])
`,
      },
    ],
  },
  {
    id: "lazy",
    title: "Lazy sequences",
    entry: "feature.lazy",
    expect: "12586269025",
    files: [
      {
        path: "feature/lazy.cljs",
        source: `(ns feature.lazy)

;; An infinite sequence: only what is consumed gets computed.
(def fibs
  (map first (iterate (fn [[a b]] [b (+ a b)]) [0 1])))

(js/console.log "the tenth fibonacci number is" (nth fibs 10))
(js/console.log (count (take-while #(< % 1000) fibs)) "of them are below 1000")

(nth fibs 50)
`,
      },
    ],
  },
  {
    id: "atoms",
    title: "Atoms",
    entry: "feature.atoms",
    expect: "101",
    files: [
      {
        path: "feature/atoms.cljs",
        source: `(ns feature.atoms)

(def counter (atom 0))

(js/console.log "start:" @counter)
(swap! counter inc)
(swap! counter #(+ % 10))
(js/console.log "after two swaps:" @counter)
(reset! counter 100)
(js/console.log "after reset:" @counter)

;; swap! returns the new value.
(swap! counter inc)
`,
      },
    ],
  },
  {
    id: "protocols",
    title: "Protocols and records",
    entry: "feature.protocols",
    expect: "12",
    files: [
      {
        path: "feature/protocols.cljs",
        source: `(ns feature.protocols)

(defprotocol Shape
  (area [this]))

(defrecord Rect [w h]
  Shape
  (area [_] (* w h)))

(defrecord Square [side]
  Shape
  (area [_] (* side side)))

(js/console.log "square:" (area (->Square 5)))

;; Records are maps too: fields answer keyword lookup.
(def rect (->Rect 3 4))
(js/console.log "width:" (:w rect))

(area rect)
`,
      },
    ],
  },
  {
    id: "multimethods",
    title: "Multimethods",
    entry: "feature.multimethods",
    expect: "Meow",
    files: [
      {
        path: "feature/multimethods.cljs",
        source: `(ns feature.multimethods)

(defmulti speak :kind)

(defmethod speak :dog [_] "Woof")
(defmethod speak :cat [_] "Meow")
(defmethod speak :default [_] "...")

(js/console.log (speak {:kind :dog :name "Rex"}))
(js/console.log (speak {:kind :fish}))

(speak {:kind :cat})
`,
      },
    ],
  },
  {
    id: "printing",
    title: "Printing with println",
    entry: "feature.printing",
    expect: '{:a 1, :b [2 3], :c "text"}',
    files: [
      {
        path: "feature/printing.cljs",
        source: `(ns feature.printing)

;; As in ClojureScript, println needs a print function: this one is
;; console.log. It brings the printer into the program, so the emitted
;; JavaScript grows; compare the sizes with the other examples.
(enable-console-print!)

(def data {:a 1 :b [2 3] :c "text"})

(println "println:" data)
(prn data)
(println (pr-str "strings keep their quotes with pr"))
(print "print")
(println " and println")

data
`,
      },
    ],
  },
  {
    id: "namespaces",
    title: "Two namespaces",
    entry: "demo.main",
    expect: "55",
    files: [
      {
        path: "demo/main.cljs",
        source: `(ns demo.main
  (:require [demo.math :as math]))

;; demo.math lives in demo/math.cljs: namespace dots become directories.
(js/console.log "5 squared is" (math/square 5))

(math/sum-of-squares 5)
`,
      },
      {
        path: "demo/math.cljs",
        source: `(ns demo.math)

(defn square [x] (* x x))

(defn sum-of-squares [n]
  (loop [i 1, total 0]
    (if (> i n)
      total
      (recur (inc i) (+ total (square i))))))
`,
      },
    ],
  },
  {
    id: "calculator",
    title: "Larger: a calculator in several namespaces",
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
