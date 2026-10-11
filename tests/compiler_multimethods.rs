//! Canonical hierarchy/multimethod behavior and phase-owned declaration demand.
use light_cljs::evaluator::{Evaluator, EvaluatorOptions, RUNTIME_SOURCE};
use light_cljs::{
    EvalOptions, EvalSession, MacroHost, analyze_with_macros, emitter::emit_js_unit, parse,
};
use std::process::Command;

fn both(source: &str, expected: &str) {
    let mut host = MacroHost::new();
    let p = analyze_with_macros(source, &parse(source).unwrap(), &mut host).unwrap();
    let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
    assert_eq!(
        evaluator.print(evaluator.evaluate(&p).unwrap()).unwrap(),
        expected,
        "QuickJS: {source}"
    );
    let script = format!(
        "const rt = {RUNTIME_SOURCE}; console.log(rt.core.prStr(({})(rt)));",
        emit_js_unit(&p)
    );
    let out = Command::new("node").args(["-e", &script]).output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8(out.stdout).unwrap().trim(),
        expected,
        "Node: {source}"
    );
}
fn printed(s: &mut EvalSession, source: &str) -> String {
    let value = s.eval_string(source).unwrap().value;
    let result = s.print(value).unwrap();
    s.release(value).unwrap();
    result
}

#[test]
fn hierarchy_closure_diamonds_vectors_and_failure_categories() {
    both(
        r#"
      (let [h (-> (make-hierarchy) (derive :dog :pet) (derive :dog :mammal)
                  (derive :pet :animal) (derive :mammal :animal))
            cut (underive h :dog :pet)
            removed (underive cut :dog :mammal)]
        (assert (= #{:pet :mammal} (parents h :dog)))
        (assert (= #{:pet :mammal :animal} (ancestors h :dog)))
        (assert (= #{:dog :pet :mammal} (descendants h :animal)))
        (assert (= #{:dog} (descendants cut :mammal)))
        (assert (nil? (parents removed :dog)))
        (assert (nil? (ancestors removed :dog)))
        (assert (isa? h [:dog [:dog :pet]] [:animal [:mammal :animal]]))
        (assert (not (isa? h [:dog] [:animal :animal])))
        (assert (identical? h (derive h :dog :pet)))
        (assert (identical? h (underive h :other :pet)))
        (assert (= :cycle (try (derive h :animal :dog) (catch js/Error e
                            (if (.includes (.-message e) "Cyclic derivation") :cycle :wrong)))))
        (assert (= :redundant (try (derive h :dog :animal) (catch js/Error e
                            (if (.includes (.-message e) "as ancestor") :redundant :wrong)))))
        (assert (= :invalid (try (derive h :dog :dog) (catch js/Error e :invalid))))
        (assert (= :invalid (try (derive :dog :animal) (catch js/Error e :invalid))))
        (derive ::dog ::animal)
        (assert (isa? ::dog ::animal))
        (underive ::dog ::animal)
        (assert (not (isa? ::dog ::animal)))
        true)"#,
        "true",
    );
}

#[test]
fn dispatch_cache_mutations_defaults_ambiguity_and_preferences() {
    both(
        r#"
      (def h (atom (-> (make-hierarchy) (derive :dog :animal) (derive :dog :pet))))
      (defmulti speak identity :hierarchy h :default :fallback)
      (defmethod speak :animal [x] [:animal x])
      (defmethod speak :fallback [x] [:fallback x])
      (assert (= [:animal :dog] (speak :dog)))
      (assert (= [:fallback :unknown] (speak :unknown)))
      (defmethod speak :pet [x] [:pet x])
      (assert (= :ambiguous (try (speak :dog) (catch js/Error e
                        (if (.includes (.-message e) "Multiple methods") :ambiguous :wrong)))))
      (prefer-method speak :pet :animal)
      (assert (= [:pet :dog] (speak :dog)))
      (assert (= {:pet #{:animal}} (prefers speak)))
      (assert (= :conflict (try (prefer-method speak :animal :pet) (catch js/Error e
                        (if (.includes (.-message e) "Preference conflict") :conflict :wrong)))))
      (remove-method speak :pet)
      (assert (= [:animal :dog] (speak :dog)))
      (swap! h underive :dog :animal)
      (assert (= [:fallback :dog] (speak :dog)))
      (swap! h derive :dog :animal)
      (assert (= [:animal :dog] (speak :dog)))
      (remove-all-methods speak)
      (assert (empty? (methods speak)))
      (assert (empty? (prefers speak)))
      (assert (nil? (get-method speak :dog)))
      (assert (= :missing (try (speak :dog) (catch js/Error e
                         (if (.includes (.-message e) "No method in multimethod 'cljs.user/speak'") :missing :wrong)))))
      true"#,
        "true",
    );
}

#[test]
fn functions_protocols_constructor_metadata_and_apply_preserve_arguments() {
    both(
        r#"
      (def effects (atom []))
      (defmulti collect "collect docs" {:private true} (fn [& xs] (swap! effects conj [:dispatch (vec xs)]) (count xs)))
      (defmethod collect 0 [] (swap! effects conj :zero) :zero)
      (defmethod collect 2 ([a b] (swap! effects conj :two) (+ a b)) ([a b c] (+ a b c)))
      (defmethod collect :default [a & xs] (swap! effects conj :many) (into [a] xs))
      (assert (= [:zero 3 [1 2 3]] [(collect) (apply collect [1 2]) ((identity collect) 1 2 3)]))
      (assert (= [[:dispatch []] :zero [:dispatch [1 2]] :two [:dispatch [1 2 3]] :many] @effects))
      (assert (= (vec (range 30)) (apply collect (range 30))))
      (assert (satisfies? IMultiFn collect))
      (assert (ifn? collect))
      (assert (not (fn? collect)))
      (assert (= "collect" (name collect)))
      (assert (= "cljs.user" (namespace collect)))
      (assert (= "collect docs" (:doc (meta #'collect))))
      (assert (= :default (default-dispatch-val collect)))
      (assert (= 2 ((dispatch-fn collect) 4 5)))
      (assert (= 11 ((get-method collect 2) 5 6)))
      (assert (= :arity (try ((get-method collect 2) 5) (catch js/Error e :arity))))
      (let [m (MultiFn. 'custom identity :default (atom (make-hierarchy)) (atom {}) (atom {}) (atom {}) (atom nil))]
        (-add-method m :x (fn [x] [:yes x]))
        (assert (= [:yes :x] (m :x)))
        (assert (= (-methods m) (methods m)))
        (assert (= (-prefers m) {}))
        (assert (= (-default-dispatch-val m) :default))
        (assert (= ((-dispatch-fn m) :x) :x))
        (assert (= ((-get-method m :x) :x) [:yes :x]))
        (-remove-method m :x)
        (-prefer-method m :a :b)
        (assert (= (-prefers m) {:a #{:b}}))
        (assert (identical? m (-reset m))))
      (let [m (reify IMultiFn
                (-reset [this] :reset)
                (-add-method [this d f] [d (f d)])
                (-remove-method [this d] d)
                (-prefer-method [this x y] [x y])
                (-get-method [this d] identity)
                (-methods [this] {:custom true})
                (-prefers [this] {})
                (-default-dispatch-val [this] :custom)
                (-dispatch-fn [this] identity))]
        (assert (= [:reset [:x :x] :x [:a :b] :x {:custom true} {} :custom :x]
                   [(remove-all-methods m) (-add-method m :x identity) (remove-method m :x)
                    (prefer-method m :a :b) ((get-method m :x) :x) (methods m) (prefers m)
                    (default-dispatch-val m) ((dispatch-fn m) :x)])))
      true"#,
        "true",
    );
}

#[test]
fn macro_demands_methods_without_unrelated_initializers_and_keeps_phase_state() {
    both(
        r#"
      (def hits (atom 0))
      (def h (atom (derive (make-hierarchy) :child :parent)))
      (defmulti f (fn [x] (swap! hits inc) x) :hierarchy h)
      (defmethod f :parent [x] [:parent x])
      (def unrelated (do (assert (= 0 @hits)) :runtime))
      (defmacro m [] (list 'quote [(f :child) @hits]))
      (remove-method f :parent)
      [(m) (methods f) @hits unrelated]"#,
        "[[[:parent :child] 1] {} 0 :runtime]",
    );
}

#[test]
fn macro_method_replacements_follow_declaration_frontiers() {
    both(
        r#"
      (defmulti f identity)
      (defmethod f :x [_] :old)
      (defmacro before [] (f :x))
      (def saved (before))
      (defmacro force-saved [] saved)
      (force-saved)
      (defmethod f :x [_] :new)
      (defmacro after [] (f :x))
      [saved (after) (f :x)]"#,
        "[:old :new :new]",
    );
}

#[test]
fn sessions_and_dynamic_hierarchy_roots_invalidate_independently() {
    let mut first = EvalSession::new(EvalOptions::default()).unwrap();
    let mut second = first.fresh_session().unwrap();
    let setup = "(def ^:dynamic h (derive (make-hierarchy) :child :parent)) (defmulti f identity :hierarchy #'h) (defmethod f :parent [_] :parent) (defmethod f :default [_] :default) (f :child)";
    assert_eq!(printed(&mut first, setup), ":parent");
    assert_eq!(printed(&mut second, setup), ":parent");
    assert_eq!(
        printed(
            &mut first,
            "[(binding [h (make-hierarchy)] (f :child)) (f :child)]"
        ),
        "[:default :parent]"
    );
    assert_eq!(
        printed(&mut first, "(remove-method f :parent) (f :child)"),
        ":default"
    );
    assert_eq!(printed(&mut second, "(f :child)"), ":parent");
    assert_eq!(
        printed(
            &mut first,
            "(derive ::child ::parent) (isa? ::child ::parent)"
        ),
        "true"
    );
    assert_eq!(printed(&mut second, "(isa? ::child ::parent)"), "false");
}

#[test]
fn javascript_prototypes_and_custom_hierarchy_queries_preserve_cljs_behavior() {
    both(
        r#"
      (let [parent (js* "function Parent() {}")
            child (js* "function Child() {}")
            _ (js/Object.setPrototypeOf child parent)
            h (derive (make-hierarchy) parent ::parent)]
        (assert (identical? parent (first (bases child))))
        (assert (contains? (supers child) parent))
        (assert (contains? (parents h child) parent))
        (assert (contains? (ancestors h child) ::parent))
        (assert (isa? h child ::parent))
        (assert (nil? (bases nil)))
        (assert (nil? (supers nil)))
        (assert (= :constructors (try (descendants h parent) (catch js/Error e
                                  (if (= "Can't get descendants of constructors" (.-message e)) :constructors :wrong)))))
        (assert (= :arity (try (apply isa? [h]) (catch js/Error e :arity))))
        (assert (= :arity (try (apply make-hierarchy [1]) (catch js/Error e :arity))))
        true)"#,
        "true",
    );
}

#[test]
fn skipped_defmulti_declarations_do_not_shift_later_method_frontiers() {
    both(
        r#"
      (defmulti f identity)
      (defmethod f :x [_] :old)
      (defmulti f (constantly :wrong))
      (defmacro before [] (f :x))
      (def saved (before))
      (defmethod f :x [_] :new)
      (defmacro after [] (f :x))
      [(after) saved (f :x)]"#,
        "[:new :old :new]",
    );
}

#[test]
fn method_body_macros_expand_once_at_the_declaration_site() {
    both(
        r#"
      (def hits (atom 0))
      (defmacro value [] (swap! hits inc) 7)
      (defmulti f identity)
      (defmethod f :x [_] (value))
      (defmacro value [] 99)
      (defmacro run [] (assert (= 7 (f :x))) (assert (= 1 @hits)) (f :x))
      [(run) (f :x) @hits]"#,
        "[7 7 0]",
    );
}

#[test]
fn alias_and_computed_method_targets_are_demanded_in_source_order() {
    both(
        r#"
      (def hits (atom 0))
      (defmulti f identity)
      (def alias f)
      (defmethod alias :x [_] :alias)
      (defmethod (do (swap! hits inc) f) :x [_] :computed)
      (defmethod f :x [_] :last)
      (defmulti unrelated identity)
      (defmethod unrelated :x [_] (throw (js/Error. "unrelated")))
      (defmacro m [] (list 'quote [(f :x) @hits]))
      [(m) (f :x) @hits]"#,
        "[[:last 1] :last 1]",
    );
}

#[test]
fn public_api_arities_execute_in_macro_phase() {
    both(
        r#"
      (defmulti f identity)
      (defmethod f :default [x] [:default x])
      (defmacro check []
        (let [h (derive (make-hierarchy) :child :parent)
              m (MultiFn. 'local identity :default (atom h) (atom {}) (atom {}) (atom {}) (atom nil))]
          (assert (satisfies? IMultiFn m))
          (assert (isa? h :child :parent))
          (assert (= #{:parent} (parents h :child)))
          (assert (= #{:parent} (ancestors h :child)))
          (assert (= #{:child} (descendants h :parent)))
          (assert (nil? (parents (underive h :child :parent) :child)))
          (derive ::child ::parent)
          (assert (isa? ::child ::parent))
          (assert (= #{::parent} (parents ::child)))
          (assert (= #{::parent} (ancestors ::child)))
          (assert (= #{::child} (descendants ::parent)))
          (underive ::child ::parent)
          (assert (not (isa? ::child ::parent)))
          (assert (seq (bases js/String)))
          (assert (seq (supers js/String)))
          (-add-method m :parent (fn [x] [:parent x]))
          (assert (= [:parent :child] (m :child)))
          (assert (= [:parent :child] ((get-method m :child) :child)))
          (assert (= #{:parent} (set (keys (methods m)))))
          (prefer-method m :a :b)
          (assert (= {:a #{:b}} (prefers m)))
          (assert (= :default (default-dispatch-val m)))
          (assert (= :child ((dispatch-fn m) :child)))
          (remove-method m :parent)
          (assert (nil? (get-method m :child)))
          (remove-all-methods m)
          (assert (empty? (prefers m)))
          (assert (= [:default :x] (f :x)))
          true))
      (check)"#,
        "true",
    );
}
