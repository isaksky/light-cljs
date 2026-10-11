//! Core macros must expand through compiled source with native evaluation disabled.
use light_cljs::evaluator::{Evaluator, EvaluatorOptions};
use light_cljs::{MacroHost, analyze_with_macros, compile_with_macros, parse};

fn result(source: &str) -> String {
    let mut host = MacroHost::new();
    host.disable_native_evaluation();
    let syntax = parse(source).unwrap();
    let program = analyze_with_macros(source, &syntax, &mut host)
        .unwrap_or_else(|error| panic!("{source}\n{error}"));
    let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
    let value = evaluator
        .evaluate(&program)
        .unwrap_or_else(|error| panic!("{source}\n{error}"));
    evaluator.print(value).unwrap()
}

fn invalid(source: &str, message: &str) {
    let mut host = MacroHost::new();
    host.disable_native_evaluation();
    let error = compile_with_macros(source, &mut host)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains(message),
        "{source}\nexpected {message:?} in {error}"
    );
}

#[test]
fn logical_and_conditional_macros_preserve_short_circuit_values_and_effects() {
    assert_eq!(
        result(
            r#"
      (let [calls (atom [])]
        [(and) (or) (and 1 nil (swap! calls conj :bad))
         (or false 7 (swap! calls conj :bad))
         (if-not false :yes :no) (if-not true :no)
         (when true (swap! calls conj :when) :value)
         (when-not false (swap! calls conj :when-not) :other)
         (cond false :bad (= 2 2) :match :else :bad)
         @calls])"#
        ),
        "[true nil nil 7 :yes nil :value :other :match [:when :when-not]]"
    );
}

#[test]
fn conditional_bindings_distinguish_false_from_nil_and_destructure() {
    assert_eq!(
        result(
            r#"
      [(if-let [[x] [3]] x :bad)
       (if-let [x false] :bad :false)
       (if-some [x false] x :bad)
       (if-some [x nil] :bad :nil)
       (when-let [{:keys [x]} {:x 4}] x)
       (when-some [x false] [x])
       (when-first [[x] [[5]]] x)
       (when-first [x []] :bad)]"#
        ),
        "[3 :false false :nil 4 [false] 5 nil]"
    );
    for source in [
        "(if-let (x 1) x)",
        "(when-some [x] x)",
        "(when-first [x [] y []] x)",
    ] {
        invalid(source, "conditional binding");
    }
}

#[test]
fn threading_families_preserve_order_and_nil_semantics() {
    assert_eq!(
        result(
            r#"
      [(-> 1 inc (* 3))
       (->> [1 2 3] (map inc) (reduce +))
       (as-> 2 value (+ value 1) (* 4 value))
       (as-> 8 value)
       (cond-> 1 true inc false (* 100) true (+ 2))
       (cond->> [1 2] true (map inc) false (filter odd?) true vec)
       (some-> nil inc)
       (some-> false (vector :kept))
       (some->> [1 2] (map inc) vec)]"#
        ),
        "[6 9 12 8 4 [2 3] nil [false :kept] [2 3]]"
    );
    invalid("(cond-> 1 true)", "test/step pairs");
    invalid("(cond->> 1 true)", "test/step pairs");
}

#[test]
fn case_and_condp_preserve_grouping_defaults_and_predicate_results() {
    assert_eq!(
        result(
            r#"
      [(case 2 (1 2) :small 3 :three :other)
       (case 'x x :symbol :other)
       (case :x :x :keyword :other)
       (condp = 2 1 :one 2 :two :other)
       (condp (fn [test value] (if (= test value) [value] nil))
         2 1 :one 2 :>> first :other)
       (condp = 2 2 :>>)
       (try (case 7 1 :one) (catch :default e :missing))
       (try (condp = 7 1 :one) (catch :default e :missing))]"#
        ),
        "[:small :symbol :keyword :two 2 :>> :missing :missing]"
    );
    invalid("(case 1 (1 2) :a 2 :b)", "duplicate case test constant");
    invalid("(case [1] [1] :a [1] :b)", "duplicate case test constant");
    // Macro arguments use the same canonical JavaScript Number contract as
    // compiled runtime values: integer-looking and decimal syntax coincide.
    invalid(
        "(case 1 1 :integer 1.0 :float)",
        "duplicate case test constant",
    );
    invalid("(cond true)", "even number");
}

#[test]
fn iteration_and_volatile_macros_preserve_input_evaluation_counts() {
    assert_eq!(
        result(
            r#"
      (let [calls (atom []) value (volatile! 0)]
        (dotimes [i (do (swap! calls conj :limit) 3)]
          (swap! calls conj i))
        (while (< @value 2) (vswap! value inc))
        (vswap! (do (swap! calls conj :volatile) value) + 4)
        [@calls @value])"#
        ),
        "[[:limit 0 1 2 :volatile :volatile] 6]"
    );
    invalid("(dotimes (i 2) i)", "vector");
    invalid("(dotimes [i 2 j 3] i)", "exactly two");
}

#[test]
fn volatile_swap_rechecks_target_after_evaluating_the_function() {
    assert_eq!(
        result(
            r#"
      (let [calls (atom []) v (volatile! 1) w (volatile! 10)
            targets (atom 0) functions (atom 0)
            target (fn []
                     (swap! calls conj :target)
                     (if (= 1 (swap! targets inc)) v w))]
        [(vswap! (target)
           (do (swap! calls conj :fn) (swap! functions inc) +)
           (do (swap! calls conj 3) 3)
           (do (swap! calls conj 4) 4))
         @v @w @targets @functions @calls])"#
        ),
        "[17 17 10 2 1 [:target :fn :target 3 4]]"
    );
}

#[test]
fn interop_macros_execute_in_quickjs() {
    assert_eq!(
        result(
            r#"
      (let [o (doto (js-obj "x" 1) (aset "y" 2))
            a (array 1 2 3)
            mapped (amap a i ret (* 2 (aget a i)))
            sum (areduce a i ret 0 (+ ret (aget a i)))
            upper (memfn toUpperCase)]
        [(aget o "x") (aget o "y") (vec mapped) sum
         (upper "abc") (.. "abc" (toUpperCase) (charAt 1))
         (vec (js-keys (js-obj "b" 1 "a" 2 "b" 3)))])"#
        ),
        "[1 2 [2 4 6] 6 \"ABC\" \"B\" [\"b\" \"a\"]]"
    );
}

#[test]
fn comprehensions_lazy_cat_and_selector_use_source_macros() {
    assert_eq!(
        result(
            r#"
      (let [seen (atom [])
            select-a (selector {:keys [a] :select true})]
        (doseq [x [1 2 3] :when (odd? x)] (swap! seen conj x))
        [(vec (for [x [1 2] y [10 20] :let [z (+ x y)] :when (odd? z)] z))
         @seen (vec (lazy-cat [1] [2 3])) (select-a {:a 1 :b 2})])"#
        ),
        "[[11 21] [1 3] [1 2 3] {:a 1}]"
    );
    invalid("(selector [])", "expected a map");
    invalid("(selector {})", "at least one");
}

#[test]
fn source_declarations_are_compile_time_only_and_can_use_ecmascript() {
    assert_eq!(
        result(
            r#"(defmacro upper [s] (.toUpperCase s))
        (defmacro selection ([x] x) ([x y & more] (last more)))
        [(upper "hello") (selection 1) (selection 1 2 3 4)]"#
        ),
        "[\"HELLO\" 1 4]"
    );
    assert_eq!(result("(defmacro nothing [] 1)"), "nil");
    invalid("(defmacro m [] 1) m", "compile-time macro");
    invalid("(if false (defmacro m [] 1))", "top level");
}

#[test]
fn bootstrap_failure_and_expansion_limits_leave_reused_host_clean() {
    let mut host = MacroHost::new();
    host.disable_native_evaluation();
    host.evaluator_options.poll_budget = 0;
    assert!(compile_with_macros("(when true 7)", &mut host).is_err());
    host.evaluator_options = EvaluatorOptions::default();
    assert!(compile_with_macros("(when true 7)", &mut host).is_ok());
    host.limits.expansions = 1;
    let error = compile_with_macros("(when true (when true 7))", &mut host)
        .unwrap_err()
        .to_string();
    assert!(error.contains("expansion limit"), "{error}");
    host.limits.expansions = 1024;
    assert!(compile_with_macros("(cond true)", &mut host).is_err());
    assert!(compile_with_macros("(when true 7)", &mut host).is_ok());
}

#[test]
fn seed_bindings_assertions_and_reflection_remain_native_free() {
    assert_eq!(
        result(
            r#"
      (defn checked [[x]] {:pre [(pos? x)] :post [(pos? %)]} x)
      (def visible 7)
      [(let [{:keys [x] :or {x 4}} {}] x)
       (checked [3])
       (try (checked [-1]) (catch :default e :failed))
       (try (assert false "bootstrap assertion") (catch :default e :asserted))
       (contains? (ns-publics 'cljs.user) 'visible)
       (comment unresolved (js/process.exit 1))]
    "#
        ),
        "[4 3 :failed :asserted true nil]"
    );
}

#[test]
fn default_compile_and_analyze_emit_have_identical_literal_results() {
    let source = "(reduce + (for [x [1 2 3]] (if-some [y x] (-> y inc) 0)))";
    let js = light_cljs::compile(source).unwrap();
    let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
    assert_eq!(
        evaluator
            .number(evaluator.evaluate_js(&js).unwrap())
            .unwrap(),
        9.0
    );
    assert_eq!(result(source), "9");
}
