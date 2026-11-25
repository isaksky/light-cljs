//! Canonical transient collection ownership, public dispatch and macro parity.
use super::optimization::execute;
use crate::temp_dir::TempDir;
use light_cljs::evaluator::{Evaluator, EvaluatorOptions, RUNTIME_SOURCE};
use light_cljs::{
    ApplicationOptions, MacroHost, OptimizationLevel, analyze_with_macros, emitter::emit_js_unit,
    parse, prepare_application,
};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

fn both(source: &str, expected: &str) {
    let mut host = MacroHost::new();
    let p = analyze_with_macros(source, &parse(source).unwrap(), &mut host).unwrap();
    #[cfg(feature = "quickjs")]
    {
        let e = Evaluator::new(EvaluatorOptions::default()).unwrap();
        assert_eq!(
            e.print(e.evaluate(&p).unwrap()).unwrap(),
            expected,
            "QuickJS: {source}"
        );
    }
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

fn advanced_with_options(source: &str, expected: &str, options: ApplicationOptions) -> String {
    let wrapped = format!("(js/console.log (pr-str (do {source})))");
    let mut host = MacroHost::new();
    let program = analyze_with_macros(&wrapped, &parse(&wrapped).unwrap(), &mut host).unwrap();
    let js = prepare_application(&program, options).emit_js();
    assert_eq!(
        execute(&format!("{js};")).trim(),
        expected,
        "Advanced: {source}"
    );
    js
}

fn advanced(source: &str, expected: &str) {
    advanced_with_options(
        source,
        expected,
        ApplicationOptions::new(OptimizationLevel::Advanced),
    );
}

fn both_and_advanced(source: &str, expected: &str) {
    both(source, expected);
    advanced(source, expected);
}

const CONTRACT: &str = r#"
(let [v (with-meta [1] {:source true}) tv (transient v)
      _ (assert (satisfies? IEditableCollection v))
      _ (assert (satisfies? ITransientCollection tv))
      _ (assert (satisfies? ITransientAssociative tv))
      _ (assert (satisfies? ITransientVector tv))
      _ (assert (not (satisfies? ISeqable tv)))
      _ (assert (identical? tv (conj! tv)))
      _ (assert (= [] (persistent! (conj!))))
      _ (conj! tv 2 3)
      _ (assert (= 1 (-nth tv "0")))
      _ (assert (= 1 (-nth tv nil :missing)))
      _ (-assoc-n! tv 1 20)
      _ (pop! tv)
      _ (assert (= [1 20] [(tv 0) (nth tv 1)]))
      pv (persistent! tv)
      _ (assert (= v [1]))
      _ (assert (nil? (meta pv)))
      _ (assert (= (hash pv) (hash [1 20])))
      _ (assert (= :fail (try (conj! tv 99) (catch js/Error e :fail))))
      _ (assert (= :fail (try (persistent! tv) (catch js/Error e :fail))))
      _ (assert (= :fail (try (count tv) (catch js/Error e :fail))))
      _ (assert (= :fail (try (get tv :bad :missing) (catch js/Error e :fail))))
      _ (assert (= :missing (nth tv 99 :missing)))
      _ (assert (= :fail (try (nth tv 0 :missing) (catch js/Error e :fail))))
      _ (assert (= :fail (try (pop! (transient [])) (catch js/Error e :fail))))
      fresh (transient [1 2])
      _ (assert (= :fail (try (assoc! fresh 3 99) (catch js/Error e :fail))))
      _ (assert (= [1 2] (persistent! fresh)))
      k (with-meta [1] {:key true}) q [1]
      m (with-meta (hash-map k :first 0 :zero nil :nil) {:source true})
      tm (transient m)
      _ (assert (satisfies? ITransientMap tm))
      _ (apply assoc! tm (list q :last -0 :zero2 :remove 1))
      _ (apply dissoc! tm (list :remove :absent))
      pm (persistent! tm)
      _ (assert (= :last (get pm q)))
      _ (assert (identical? k (key (find pm q))))
      _ (assert (= :first (get m q)))
      _ (assert (= :zero2 (get tm 0)))
      _ (assert (= :fail (try (dissoc! tm 0) (catch js/Error e :fail))))
      _ (assert (nil? (meta pm)))
      _ (assert (= (hash pm) (hash (hash-map q :last 0 :zero2 nil :nil))))
      ta (transient (array-map :a 1))
      _ (persistent! ta)
      _ (assert (= :fail (try (ta :a) (catch js/Error e :fail))))
      ts (transient (hash-set k))
      _ (assert (satisfies? ITransientSet ts))
      _ (assert (identical? q (get ts q)))
      _ (apply conj! ts (list 0 -0 :remove))
      _ (apply disj! ts (list :remove :absent))
      ps (persistent! ts)
      _ (assert (identical? k (get ps q)))
      _ (assert (= 2 (count ps)))
      _ (assert (= (hash ps) (hash #{q 0})))
      saved [transient persistent! conj! assoc! dissoc! disj! pop!]
      _ (assert (= [2] ((nth saved 1) ((nth saved 6) ((nth saved 2) ((nth saved 0) [2]) 3)))))
      _ (assert (= {:b 2} ((nth saved 1) ((nth saved 4) ((nth saved 3) ((nth saved 0) {}) :a 1 :b 2) :a))))
      _ (assert (= #{2} ((nth saved 1) ((nth saved 5) ((nth saved 0) #{1 2}) 1))))
      events (atom [])
      custom (reify
               IEditableCollection (-as-transient [this] (swap! events conj :transient) this)
               ITransientCollection
               (-conj! [this x] (swap! events conj [:conj x]) this)
               (-persistent! [_] (swap! events conj :persistent) @events)
               ITransientAssociative (-assoc! [this k v] (swap! events conj [:assoc k v]) this)
               ITransientMap (-dissoc! [this k] (swap! events conj [:dissoc k]) this)
               ITransientVector
               (-assoc-n! [this k v] (swap! events conj [:assoc-n k v]) this)
               (-pop! [this] (swap! events conj :pop) this)
               ITransientSet (-disjoin! [this k] (swap! events conj [:disj k]) this))
      _ (-as-transient custom)
      _ (-conj! custom 1)
      _ (-assoc! custom :a 2)
      _ (-dissoc! custom :a)
      _ (-assoc-n! custom 0 4)
      _ (-pop! custom)
      _ (-disjoin! custom 1)
      result (-persistent! custom)
      _ (assert (= result [:transient [:conj 1] [:assoc :a 2] [:dissoc :a] [:assoc-n 0 4] :pop [:disj 1] :persistent]))]
  [pv (get pm q) (count ps) result])
"#;

#[test]
fn transient_api_arities_protocols_and_macro_phase_execute() {
    let expected = "[[1 20] :last 2 [:transient [:conj 1] [:assoc :a 2] [:dissoc :a] [:assoc-n 0 4] :pop [:disj 1] :persistent]]";
    both(CONTRACT, expected);
    both(
        &format!("(defmacro build-transient [] {CONTRACT}) (build-transient)"),
        expected,
    );
}

#[test]
fn complete_upstream_transient_bodies_execute_in_both_engines() {
    for line in include_str!("../fixtures/compiler-transients.tsv")
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
    {
        let columns: Vec<_> = line.split('\t').collect();
        both(columns[1], "true");
    }
}

fn reference_node(script: &str) -> String {
    let mut child = Command::new(std::env::var_os("NODE").unwrap_or_else(|| "node".into()))
        .args(["--max-old-space-size=96", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(script.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
#[ignore = "requires Clojure CLI, Java, Node, and both pinned CLJS references"]
fn transient_fixtures_match_pinned_release_and_checkout() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let checkout = std::env::var_os("LIGHT_CLJS_REFERENCE_CHECKOUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("research/clojurescript"));
    let cases: Vec<_> = include_str!("../fixtures/compiler-transients.tsv")
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| line.split('\t').collect::<Vec<_>>())
        .collect();
    for (kind, identity, dependency, checkout_arg) in [
        (
            "release",
            "1.12.134",
            "{:mvn/version \"1.12.134\"}".to_string(),
            "-".to_string(),
        ),
        (
            "checkout",
            "a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e",
            format!("{{:local/root {:?}}}", checkout),
            checkout.to_string_lossy().into_owned(),
        ),
    ] {
        let output_dir = TempDir::new(&format!("cljs-transients-{kind}"));
        let mut child =
            Command::new(std::env::var_os("CLOJURE").unwrap_or_else(|| "clojure".into()))
                .current_dir(&root)
                .args([
                    "-Srepro",
                    "-Sdeps",
                    &format!("{{:deps {{org.clojure/clojurescript {dependency}}}}}"),
                    "-M",
                    "tests/reference/execution.clj",
                    kind,
                    identity,
                    &checkout_arg,
                ])
                .arg(output_dir.path())
                .arg("cases")
                .arg(root.join("tests/fixtures/compiler-transients.tsv"))
                .stdin(Stdio::null())
                .stdout(fs::File::create(output_dir.join("build.stdout")).unwrap())
                .stderr(fs::File::create(output_dir.join("build.stderr")).unwrap())
                .spawn()
                .unwrap();
        let start = Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if start.elapsed() > Duration::from_secs(240) {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!(
                    "{kind} reference timed out; artifacts at {}",
                    output_dir.display()
                );
            }
            std::thread::sleep(Duration::from_millis(25));
        };
        let settings = fs::read_to_string(output_dir.join("build.stdout")).unwrap();
        assert!(
            status.success(),
            "{kind}: {}\nartifacts: {}",
            fs::read_to_string(output_dir.join("build.stderr")).unwrap(),
            output_dir.display()
        );
        assert!(
            settings.contains(&format!("REFERENCE IDENTITY {kind} {identity}")),
            "{settings}"
        );
        assert!(
            settings.contains(
                "REFERENCE SETTINGS {:optimizations :none, :static-fns false, :def-emits-var false}"
            ),
            "{settings}"
        );
        let output = reference_node(&format!(
            "process.chdir({:?}); require({:?}); for (const f of globalThis.lightCases) console.log(JSON.stringify(f(() => {{ throw new Error('unexpected outer observer'); }})));",
            output_dir,
            output_dir.join("reference.js")
        ));
        let lines: Vec<_> = output.lines().collect();
        assert_eq!(lines.len(), cases.len(), "{kind}");
        for (case, actual) in cases.iter().zip(lines) {
            assert_eq!(actual, case[2], "{kind}: {}", case[0]);
        }
        eprintln!(
            "Matched {} transient workflows against {kind} {identity}",
            cases.len()
        );
    }
}

#[test]
fn into_uses_custom_editable_protocol_and_real_transient_completion() {
    both(
        r#"
      (let [events (atom [])
            builder (reify ITransientCollection
                      (-conj! [this x] (swap! events conj [:conj x]) this)
                      (-persistent! [_] (swap! events conj :persistent) @events))
            target (reify IEditableCollection
                     (-as-transient [_] (swap! events conj :transient) builder))]
        [(into target [1 2]) (do (reset! events []) (into target (map inc) [1 2]))])"#,
        "[[:transient [:conj 1] [:conj 2] :persistent] [:transient [:conj 2] [:conj 3] :persistent]]",
    );
    both(
        r#"
      (let [escaped (atom nil)
            xf (fn [rf] (fn ([a] (rf a)) ([a x] (reset! escaped a) (rf a x))))
            result (into (with-meta [0] {:source true}) xf [1 2])]
        [result (meta result) (satisfies? ITransientCollection @escaped)
         (try (conj! @escaped 3) (catch js/Error e :closed))])"#,
        "[[0 1 2] {:source true} true :closed]",
    );
    // Target metadata must be read after persisting two-argument `into`, in both the
    // reduction path and the flat-map fast path (Sol #4 / Sol #7).
    both_and_advanced(
        r#"
      (let [step (atom 0)
            target-vec (specify! [0] IMeta (-meta [_] {:step @step}))
            src-seq (reify IReduce
                      (-reduce [_ f init]
                        (reset! step 1)
                        (f init 1)))
            r-vec (into target-vec src-seq)
            ;; Failure during build must not evaluate target IMeta prematurely
            meta-calls (atom 0)
            bad-target (specify! {} IMeta (-meta [_] (swap! meta-calls inc) {:bad true}))
            bad-src (reify IReduce (-reduce [_ _ _] (throw (js/Error. "boom"))))
            outcome (try (into bad-target bad-src) :ok (catch js/Error _ :err))]
        [(meta r-vec) outcome @meta-calls])"#,
        "[{:step 1} :err 0]",
    );
    // Sol #7: Separate advanced program without `specify!` so `fieldComparisons.on` stays
    // true and `kindHooks.intoMap` actually runs. A reified target implements
    // IEditableCollection (returning a real transient hash map) and IMeta, verifying
    // that `-meta` sees both the source key's IHash side effect and an already-persisted
    // transient, and instrumentation proves the flat array-map walk executed.
    let flat_meta_source = r#"
      (let [step (atom 0)
            escaped (atom nil)
            target (reify
                     IEditableCollection
                     (-as-transient [_]
                       (let [t (transient (hash-map :base 0))]
                         (reset! escaped t)
                         t))
                     IMeta
                     (-meta [_]
                       {:step @step
                        :closed (try (conj! @escaped [:late 1]) :open (catch js/Error _ :closed))}))
            k (reify IHash (-hash [_] (reset! step 2) 7) IEquiv (-equiv [a b] (identical? a b)))
            r (into target (array-map k :v))]
        [(meta r) (get r k)])"#;
    let mut flat_meta_opts = ApplicationOptions::new(OptimizationLevel::Advanced);
    flat_meta_opts.pseudo_names = true;
    let flat_meta_js = advanced_with_options(
        flat_meta_source,
        "[{:step 2, :closed :closed} :v]",
        flat_meta_opts,
    );
    assert!(flat_meta_js.contains("if (arr === undefined || from.slots$"));
    let instrumented_meta = flat_meta_js.replacen(
        "if (arr === undefined || from.slots$",
        "globalThis.__intoMapHits = (globalThis.__intoMapHits || 0) + 1; if (arr === undefined || from.slots$",
        1,
    );
    let meta_out = execute(&format!(
        "{instrumented_meta};\nconsole.log(globalThis.__intoMapHits || 0);"
    ));
    let meta_lines: Vec<_> = meta_out.trim().lines().collect();
    assert_eq!(meta_lines, vec!["[{:step 2, :closed :closed} :v]", "1"]);
}

#[test]
fn into_map_merge_and_map_conj_semantics_at_all_levels() {
    both_and_advanced(
        r#"
      (defrecord Rec [a b])
      (deftype Collide [id]
        IHash (-hash [_] 42)
        IEquiv (-equiv [this other] (and (instance? Collide other) (= id (.-id other)))))
      (let [m7 (with-meta {:a 1 :b 2 :c 3 :d 4 :e 5 :f 6 :g 7} {:src :m7})
            m3 (with-meta {:b 20 :h 8 :i 9} {:src :m3})
            m-nil {:a 1 nil :n1 :b 2 :c 3 :d 4 :e 5 :f 6 :g 7}
            c1 (Collide. 1) c2 (Collide. 2)
            hm (into (hash-map c1 :c1 c2 :c2) (for [i (range 20)] [i (* i 10)]))
            hm-nil (assoc hm nil :nil-v)
            sm (sorted-map :x 10 :y 20)
            rec (->Rec 100 200)
            ;; Signed zero keys and values across into, merge, conj, and 8->9 promotion
            z-into (into (array-map 0 0) (array-map -0 -0))
            z-merge (merge (array-map 0 0) (array-map -0 -0))
            z-conj (conj (array-map 0 0) (array-map -0 -0))
            z-prom (assoc {:a 1 :b 2 :c 3 :d 4 :e 5 :f 6 :g 7 :h 8} -0 -0)
            z-prom-into (into {:a 1 :b 2 :c 3 :d 4 :e 5 :f 6 :g 7 :h 8} (array-map -0 -0))
            ;; into: array->array, array->hash promotion, hash->array, hash->hash, record, sorted-map, seq of entries, seq of 2-vectors
            r1 (into (with-meta {} {:empty true}) m7)
            r2 (into m7 m3)
            r3 (into {} hm-nil)
            r4 (into hm {:a :from-arr 0 :overwrite})
            r5 (into (with-meta hm {:hm true}) hm-nil)
            r-rec (into {} rec)
            r-sm (into {} sm)
            r-entries (into {} (seq {:p 1 :q 2}))
            r-vecs (into {} (list [:u 3] [:v 4]))
            ;; merge: nil/false, 0/1/many args, 8->9 promotion, nil keys/values, hash/record/sorted-map sources
            mg [(merge) (merge nil) (merge nil false) (merge {:a 1})
                (merge nil {:a 1}) (merge false {:a 1}) (merge {:a 1} nil)
                (merge nil {:a 1} nil {:b 2 :a 10})]
            mg9 (merge m7 m3)
            mg-nil (merge m-nil {:i 9 nil nil})
            mg-hm (merge {:x 1} hm-nil)
            mg-rec (merge {:z 0} rec)
            mg-sm (merge {:z 0} sm)
            ;; conj: array map, hash map, nil, vector, map-entry, record, sorted-map
            cj1 (conj m7 m3)
            cj2 (conj m7 nil)
            cj3 (conj {:a 1} [:b 2])
            cj4 (conj {:a 1} (first {:b 2}))
            cj5 (conj {:a 1} hm)
            cj6 (conj {:z 0} rec)
            cj7 (conj {:z 0} sm)
            ;; specify! on source array-map / hash-map bypasses flat shortcuts
            spec-arr (specify! {:a 1 :b 2}
                       ISeqable (-seq [_] (list [:override 99])))
            spec-hm (specify! (into {} (for [i (range 12)] [i i]))
                      IReduce (-reduce [_ f init] (f init [:reduced 77])))]
        [(= r1 {:a 1 :b 2 :c 3 :d 4 :e 5 :f 6 :g 7}) (= (meta r1) {:empty true})
         (= (count r2) 9) (= (:b r2) 20) (= (:i r2) 9) (= (meta r2) {:src :m7})
         (= (count r3) 23) (= (get r3 nil) :nil-v) (= (get r3 c1) :c1) (= (get r3 c2) :c2)
         (= (count r4) 23) (= (get r4 0) :overwrite) (= (:a r4) :from-arr)
         (= (count r5) 23) (= (get r5 nil) :nil-v) (= (meta r5) {:hm true})
         (= r-rec {:a 100 :b 200}) (= r-sm {:x 10 :y 20})
         (= r-entries {:p 1 :q 2}) (= r-vecs {:u 3 :v 4})
         (js/Object.is (get z-into 0) -0) (js/Object.is (key (first z-into)) 0)
         (js/Object.is (get z-merge 0) -0) (js/Object.is (key (first z-merge)) 0)
         (js/Object.is (get z-conj 0) -0) (js/Object.is (key (first z-conj)) 0)
         (js/Object.is (get z-prom 0) -0) (js/Object.is (get z-prom-into 0) -0)
         mg
         (= (count mg9) 9) (= (:b mg9) 20) (= (meta mg9) {:src :m7})
         (= (count mg-nil) 9) (contains? mg-nil nil) (nil? (get mg-nil nil :miss))
         (= (count mg-hm) 24) (= (get mg-hm nil) :nil-v) (= (:x mg-hm) 1)
         (= mg-rec {:z 0 :a 100 :b 200}) (= mg-sm {:z 0 :x 10 :y 20})
         (= cj1 mg9) (= cj2 m7) (= cj3 {:a 1 :b 2}) (= cj4 {:a 1 :b 2})
         (= (count cj5) 23) (= cj6 {:z 0 :a 100 :b 200}) (= cj7 {:z 0 :x 10 :y 20})
         (into {} spec-arr) (conj {:k 0} spec-arr) (merge {:k 0} spec-arr)
         (into {} spec-hm)])"#,
        "[true true true true true true true true true true true true true true true true true true true true true true true true true true true true [nil nil nil {:a 1} {:a 1} {:a 1} {:a 1} {:a 10, :b 2}] true true true true true true true true true true true true true true true true true true {:override 99} {:k 0, :override 99} {:k 0, :override 99} {:reduced 77}]",
    );
}

#[test]
fn merge_direct_and_apply_preserve_reduction_semantics() {
    // Sol #3: direct merge must stop on `reduced` results and honor IndexedSeq dispatch.
    both_and_advanced(
        r#"
      (let [calls (atom 0)
            stopper (reify ICollection
                      (-conj [_ x]
                        (swap! calls inc)
                        (reduced {:done true :saw x})))
            direct (merge stopper {:a 1} {:b 2})
            c1 @calls
            _ (reset! calls 0)
            applied (apply merge [stopper {:a 1} {:b 2}])
            c2 @calls]
        [direct c1 applied c2])"#,
        "[{:done true, :saw {:a 1}} 1 {:done true, :saw {:a 1}} 1]",
    );
    // Separate program so `extend-type IndexedSeq` does not pollute the fast-path test above.
    both_and_advanced(
        r#"
      (let [reduced-hits (atom 0)]
        (extend-type IndexedSeq
          IReduce
          (-reduce [this f]
            (swap! reduced-hits inc)
            {:via-indexed-seq (count this)}))
        [(merge {:a 1} {:b 2}) @reduced-hits])"#,
        "[{:via-indexed-seq 2} 1]",
    );
}

#[test]
fn advanced_map_builders_observe_mid_walk_protocol_extensions() {
    // Sol #2: `intoMap` array-map source -> TransientHashMap `-conj!` installed mid-walk.
    advanced(
        r#"
      (let [installed (atom false)
            k1 (reify
                 IHash (-hash [_]
                         (when (not @installed)
                           (reset! installed true)
                           (extend-type TransientHashMap
                             ITransientCollection
                             (-conj! [t _] t)))
                         7)
                 IEquiv (-equiv [a b] (identical? a b)))
            m (into (hash-map) (array-map k1 :v1 :k2 :v2))]
        [(count m) (get m k1) (contains? m :k2)])"#,
        "[1 :v1 false]",
    );
    // Sol #2: `intoMap` array-map source -> TransientArrayMap `-conj!` returning `(reduced nil)` mid-walk.
    advanced(
        r#"
      (let [installed (atom false)
            k1 (reify
                 IHash (-hash [_] 7)
                 IEquiv (-equiv [a b]
                          (when (not @installed)
                            (reset! installed true)
                            (extend-type TransientArrayMap
                              ITransientCollection
                              (-conj! [t _] (reduced t))))
                          (identical? a b)))
            m (into {:seed 0} (array-map k1 :v1 :k2 :v2))]
        [(count m) (get m :seed) (get m k1) (contains? m :k2)])"#,
        "[2 0 :v1 false]",
    );
    // Ordinary reduction path (hash-map sources stay outside `kindHooks.intoMap`, Sol #5):
    // hash-map source -> TransientHashMap `-conj!` installed mid-walk.
    advanced(
        r#"
      (let [armed (atom false)
            installed (atom false)
            k1 (reify
                 IHash (-hash [_]
                         (when (and @armed (not @installed))
                           (reset! installed true)
                           (extend-type TransientHashMap
                             ITransientCollection
                             (-conj! [t _] t)))
                         1)
                 IEquiv (-equiv [a b] (identical? a b)))
            src (hash-map k1 :v1 :b 2 :c 3)
            _ (reset! armed true)
            m (into (hash-map) src)]
        [(count m) (get m k1)])"#,
        "[1 :v1]",
    );
    // Sol #2: `mapConj` (via `conj` and `merge`) -> `MapEntry` `-nth` installed mid-walk.
    advanced(
        r#"
      (let [installed (atom false)
            k1 (reify
                 IHash (-hash [_] 7)
                 IEquiv (-equiv [a b]
                          (when (not @installed)
                            (reset! installed true)
                            (extend-type MapEntry
                              IIndexed
                              (-nth [_ idx] (if (= idx 0) :overridden-k :overridden-v))))
                          (identical? a b)))
            m (conj {:seed 0} (array-map k1 :v1 :k2 :v2))]
        [(get m :seed) (get m k1) (contains? m :k2) (get m :overridden-k)])"#,
        "[0 :v1 false :overridden-v]",
    );
    // Sol #2: `mapConj` -> `PersistentArrayMapSeq` `-next` installed mid-walk.
    advanced(
        r#"
      (let [installed (atom false)
            k1 (reify
                 IHash (-hash [_] 7)
                 IEquiv (-equiv [a b]
                          (when (not @installed)
                            (reset! installed true)
                            (extend-type PersistentArrayMapSeq
                              INext
                              (-next [_] nil)))
                          (identical? a b)))
            m (merge {:seed 0} (array-map k1 :v1 :k2 :v2))]
        [(count m) (get m :seed) (get m k1) (contains? m :k2)])"#,
        "[2 0 :v1 false]",
    );
}

#[test]
fn advanced_map_builders_walk_flat_pairs() {
    // Sol #5, #8, #12: Verify at `:advanced` that plain `into`, `conj`, and `merge` take
    // the flat-pair shortcuts without calling `mapEntry` or `mergeMaps`, that `merge` has
    // no rest parameter, that `intoMap` stays scoped to array-map sources, and that
    // protocol overrides fall back cleanly.
    let source = r#"
      (let [m7 {:a 1 :b 2 :c 3 :d 4 :e 5 :f 6 :g 7}
            m3 {:b 20 :h 8 :i 9}
            r-into (into {} m7)
            r-conj (conj m7 m3)
            r-merge (merge m7 m3)]
        [(count r-into) (count r-conj) (count r-merge) (:i r-merge)])"#;
    let mut opts = ApplicationOptions::new(OptimizationLevel::Advanced);
    opts.pseudo_names = true;
    let js = advanced_with_options(source, "[7 9 9 9]", opts);
    assert!(
        js.contains("return new EntryCell(key, value);"),
        "expected advanced output to retain EntryCell construction for instrumentation"
    );
    assert!(
        js.contains("kindHooks.intoMap"),
        "expected advanced output to retain kindHooks.intoMap"
    );
    assert!(
        !js.contains("intoHashNode"),
        "expected kindHooks.intoMap to stay limited to array-map sources"
    );
    assert!(
        js.contains("function arrayMapPromote(self, arr, key, item) {"),
        "expected advanced output to retain out-of-line arrayMapPromote"
    );
    assert!(
        js.contains("function merge() {"),
        "expected direct merge without rest parameter"
    );
    assert!(
        js.contains("function mergeMaps(values, combine) {"),
        "expected mergeMaps fallback to be retained for instrumentation"
    );
    // Instrument EntryCell construction, intoMap's flat array-map walk, mapConj's flat walk,
    // arrayMapPromote, and mergeMaps to prove the flat shortcuts run with zero map-entry
    // allocation and zero mergeMaps fallback calls.
    let instrumented = js
        .replace(
            "return new EntryCell(key, value);",
            "{ globalThis.__mapEntryCalls = (globalThis.__mapEntryCalls || 0) + 1; return new EntryCell(key, value); }",
        )
        .replacen(
            "function arrayMapPromote(self, arr, key, item) {",
            "function arrayMapPromote(self, arr, key, item) { globalThis.__promoteCalls = (globalThis.__promoteCalls || 0) + 1;",
            1,
        )
        .replacen(
            "if (arr === undefined || from.slots$",
            "globalThis.__intoMapArrayCalls = (globalThis.__intoMapArrayCalls || 0) + 1; if (arr === undefined || from.slots$",
            1,
        )
        .replacen(
            "const arr = member.arr$",
            "globalThis.__mapConjFlatCalls = (globalThis.__mapConjFlatCalls || 0) + 1; const arr = member.arr$",
            1,
        )
        .replacen(
            "function mergeMaps(values, combine) {",
            "function mergeMaps(values, combine) { globalThis.__mergeMapsCalls = (globalThis.__mergeMapsCalls || 0) + 1;",
            1,
        );
    assert_ne!(instrumented, js);
    let check_script = format!(
        "{instrumented};\nconsole.log(JSON.stringify([globalThis.__mapEntryCalls || 0, globalThis.__intoMapArrayCalls || 0, globalThis.__mapConjFlatCalls || 0, globalThis.__promoteCalls || 0, globalThis.__mergeMapsCalls || 0]));"
    );
    let out = execute(&check_script);
    let lines: Vec<_> = out.trim().lines().collect();
    assert_eq!(lines, vec!["[7 9 9 9]", "[0,1,2,2,0]"]);

    // Verify that extending `MapEntry` `-nth` disables the flat shortcut and allocates entries.
    let fallback_source = r#"
      (let [i (atom 0)]
        (extend-type MapEntry
          IIndexed
          (-nth [_ idx]
            (if (= idx 0)
              (keyword (str "k" (swap! i inc)))
              (* @i 10))))
        (let [m (merge {:a 1} {:b 2 :c 3})]
          [(:a m) (:k1 m) (:k2 m) (contains? m :b)]))"#;
    let mut opts2 = ApplicationOptions::new(OptimizationLevel::Advanced);
    opts2.pseudo_names = true;
    let js2 = advanced_with_options(fallback_source, "[1 10 20 false]", opts2);
    assert!(js2.contains("return new EntryCell(key, value);"));
    let instrumented2 = js2.replace(
        "return new EntryCell(key, value);",
        "{ globalThis.__mapEntryCalls = (globalThis.__mapEntryCalls || 0) + 1; return new EntryCell(key, value); }",
    );
    let out2 = execute(&format!(
        "{instrumented2};\nconsole.log(globalThis.__mapEntryCalls || 0);"
    ));
    let lines2: Vec<_> = out2.trim().lines().collect();
    assert_eq!(lines2.len(), 2);
    assert_eq!(lines2[0], "[1 10 20 false]");
    let entry_calls: usize = lines2[1].parse().unwrap();
    assert!(
        entry_calls >= 2,
        "expected fallback to construct MapEntry objects, got {entry_calls}"
    );

    // Sol #8: Verify that an IndexedSeq override routes direct `merge` through `mergeMaps`.
    let indexed_seq_source = r#"
      (extend-type IndexedSeq
        IReduce
        (-reduce [this _] {:via-indexed-seq (count this)}))
      (merge {:a 1} {:b 2})"#;
    let mut opts3 = ApplicationOptions::new(OptimizationLevel::Advanced);
    opts3.pseudo_names = true;
    let js3 = advanced_with_options(indexed_seq_source, "{:via-indexed-seq 2}", opts3);
    assert!(js3.contains("function mergeMaps(values, combine) {"));
    let instrumented3 = js3.replacen(
        "function mergeMaps(values, combine) {",
        "function mergeMaps(values, combine) { globalThis.__mergeMapsCalls = (globalThis.__mergeMapsCalls || 0) + 1;",
        1,
    );
    let out3 = execute(&format!(
        "{instrumented3};\nconsole.log(globalThis.__mergeMapsCalls || 0);"
    ));
    let lines3: Vec<_> = out3.trim().lines().collect();
    assert_eq!(lines3, vec!["{:via-indexed-seq 2}", "1"]);
}

#[test]
fn transient_extensions_with_frozen_field_comparisons() {
    let source = r#"
      (let [ta (transient {:a 1})
            th (transient (hash-map :a 1))]
        (specify! ta ITransientCollection (-conj! [t _] t))
        (specify! th ITransientCollection (-conj! [t _] t))
        [(count (persistent! (conj! ta [:b 2])))
         (count (persistent! (conj! th [:b 2])))])"#;
    let mut host = MacroHost::new();
    let program = analyze_with_macros(source, &parse(source).unwrap(), &mut host).unwrap();
    let sealed_runtime = RUNTIME_SOURCE.replacen(
        "const fieldComparisons = {on: true};",
        "const fieldComparisons = Object.freeze({on: true});",
        1,
    );
    assert_ne!(sealed_runtime, RUNTIME_SOURCE);
    let script = format!(
        "const rt = {sealed_runtime}; console.log(rt.core.prStr(({})(rt)));",
        emit_js_unit(&program)
    );
    let out = Command::new("node").args(["-e", &script]).output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8(out.stdout).unwrap().trim(), "[1 1]");
}

#[test]
fn transient_vector_edit_token_ownership_boundaries_and_isolation() {
    both(
        r#"
      (let [;; Grow across the 32 and 1024+32 (1056) boundaries
            n 1100
            v1 (loop [i 0 tv (transient [])]
                 (if (< i n) (recur (inc i) (conj! tv i)) (persistent! tv)))
            _ (assert (= n (count v1)))
            _ (assert (= [0 31 32 500 1023 1024 1055 1056 1099]
                         [(nth v1 0) (nth v1 31) (nth v1 32) (nth v1 500)
                          (nth v1 1023) (nth v1 1024) (nth v1 1055) (nth v1 1056) (nth v1 1099)]))
            ;; Two concurrent sibling transients over the same persistent vector v1
            s1 (transient v1)
            s2 (transient v1)
            ;; Interleaved assoc! on shared nodes (first write copies) and owned nodes (second write in place)
            _ (assoc! s1 0 :s1-0)
            _ (assoc! s2 0 :s2-0)
            _ (assoc! s1 1 :s1-1)
            _ (assoc! s2 31 :s2-31)
            _ (assoc! s1 32 :s1-32)
            _ (assoc! s2 32 :s2-32)
            _ (assoc! s1 500 :s1-500)
            _ (assoc! s2 500 :s2-500)
            _ (assoc! s1 1023 :s1-1023)
            _ (assoc! s1 1024 :s1-1024)
            _ (assoc! s2 1055 :s2-1055)
            _ (assoc! s1 1056 :s1-1056)
            _ (assoc! s2 1099 :s2-1099)
            _ (assoc! s1 1100 :s1-1100)
            _ (conj! s1 :s1-1101)
            ;; Pop s2 across the 1057->1056 root-shrink boundary (shift 10 -> 5) while s1 is still active
            _ (dotimes [_ 45] (pop! s2))
            _ (assoc! s2 500 :s2-500b)
            _ (conj! s2 :s2-tail)
            vs1 (persistent! s1)
            vs2 (persistent! s2)
            ;; v1 must remain completely untouched
            _ (assert (= n (count v1)))
            _ (assert (= [0 1 31 32 500 1023 1024 1055 1056 1099]
                         [(nth v1 0) (nth v1 1) (nth v1 31) (nth v1 32) (nth v1 500)
                          (nth v1 1023) (nth v1 1024) (nth v1 1055) (nth v1 1056) (nth v1 1099)]))
            ;; Stale transient s1 must throw on every operation and leave vs1 intact
            stale-errors
            [(try (conj! s1 :bad) :ok (catch js/Error _ :err))
             (try (assoc! s1 0 :bad) :ok (catch js/Error _ :err))
             (try (assoc! s1 1102 :bad) :ok (catch js/Error _ :err))
             (try (pop! s1) :ok (catch js/Error _ :err))
             (try (persistent! s1) :ok (catch js/Error _ :err))
             (try (count s1) :ok (catch js/Error _ :err))
             (try (nth s1 0) :ok (catch js/Error _ :err))
             (try (get s1 0) :ok (catch js/Error _ :err))
             (try (s1 0) :ok (catch js/Error _ :err))]
            _ (assert (= (vec (repeat 9 :err)) stale-errors))
            ;; New transient over vs1 (whose nodes carry s1's old edit token) must copy on write
            ;; and also exercise pop! across 1057->1056 and 33->32 boundaries on owned and shared nodes
            t3 (transient vs1)
            _ (assoc! t3 0 :t3-0)
            _ (assoc! t3 500 :t3-500)
            _ (assoc! t3 1056 :t3-1056)
            _ (dotimes [_ 80] (pop! t3))
            _ (assoc! t3 32 :t3-32)
            _ (conj! t3 :t3-tail)
            v3 (persistent! t3)
            ;; Pop from 1102 all the way down past 1056 and 32 to 30, then rebuild past 32
            v4 (loop [tv (transient vs1)]
                 (if (> (count tv) 30)
                   (recur (pop! tv))
                   (persistent! (conj! (conj! (conj! tv :p30) :p31) :p32))))
            ;; Empty transient and pop-to-empty trim tail cleanly
            empty-roundtrip [(persistent! (transient []))
                             (persistent! (pop! (conj! (transient []) :once)))]
            ;; VectorViewCell (from assoc on subvec) transient isolates both source vector and view
            sv (assoc (subvec v1 30 70) 0 30)
            v-sv (persistent! (conj! (assoc! (transient sv) 0 :sv0) :sv-end))
            ;; Persistent updates on vs1 also leave vs1 untouched
            v5 (assoc (conj vs1 :p) 500 :p500)]
        [(nth v1 500) (nth vs1 500) (nth vs2 500) (nth v3 500) (nth v5 500)
         (count v1) (count vs1) (count vs2) (count v3) (count v4) (count v5)
         (peek vs1) (peek vs2) (peek v3) (peek v4) (peek v5)
         (nth vs1 0) (nth vs1 1) (nth vs1 32) (nth vs1 1056)
         (nth vs2 0) (nth vs2 31) (nth vs2 32) (nth vs2 1055)
         (nth v3 0) (nth v3 32)
         empty-roundtrip
         (nth sv 0) (count sv) (nth v-sv 0) (peek v-sv) (count v-sv)])"#,
        "[500 :s1-500 :s2-500b :t3-500 :p500 1100 1102 1056 1023 33 1103 :s1-1101 :s2-tail :t3-tail :p32 :p :s1-0 :s1-1 :s1-32 :s1-1056 :s2-0 :s2-31 :s2-32 :s2-tail :t3-0 :t3-32 [[] []] 30 40 :sv0 :sv-end 41]",
    );
}

#[test]
fn indexed_assoc_cannot_mutate_a_reentrantly_persisted_vector() {
    both(
        r#"
      (let [t (transient [1])
            saved (atom nil)
            index (js* "({valueOf() { if (~{} == null) { ~{}(~{}(~{})); } return 0; }})"
                       @saved (fn [v] (reset! saved v)) persistent! t)
            outcome (try (-assoc-n! t index 99) :ok (catch js/Error _ :err))]
        [outcome @saved (nth @saved 0)])"#,
        "[:err [1] 1]",
    );
    // Same check when the reentrant persistence happens on a trie write (index < tailOffset)
    both(
        r#"
      (let [t (transient (vec (range 40)))
            saved (atom nil)
            index (js* "({valueOf() { if (~{} == null) { ~{}(~{}(~{})); } return 0; }})"
                       @saved (fn [v] (reset! saved v)) persistent! t)
            outcome (try (-assoc-n! t index 99) :ok (catch js/Error _ :err))]
        [outcome (nth @saved 0) (count @saved)])"#,
        "[:err 0 40]",
    );
}

#[test]
fn transient_sets_and_maps_match_persistent_builds_across_sizes_and_edge_keys() {
    both_and_advanced(
        r#"
      (deftype CustomKey [h id]
        IHash (-hash [_] h)
        IEquiv (-equiv [_ other] (and (instance? CustomKey other) (= id (.-id other)))))
      (let [check-size
            (fn [[n exp-sh exp-mh exp-order]]
              (let [xs (vec (range n))
                    ps (reduce conj #{} xs)
                    ts (into #{} xs)
                    ts2 (persistent! (reduce conj! (transient #{}) xs))
                    pm (reduce (fn [m x] (assoc m x (* x 3))) {} xs)
                    tm (persistent! (reduce (fn [m x] (assoc! m x (* x 3))) (transient {}) xs))
                    s-seq (vec (seq ts))
                    m-keys (mapv key (seq tm))]
                (assert (= (count ps) (count ts) (count ts2) n))
                (assert (= (hash ps) (hash ts) (hash ts2) exp-sh))
                (assert (and (= ps ts) (= ts ps) (= ps ts2)))
                (assert (= (vec (seq ps)) s-seq (vec (seq ts2))))
                (assert (= (count pm) (count tm) n))
                (assert (= (hash pm) (hash tm) exp-mh))
                (assert (and (= pm tm) (= tm pm)))
                (assert (= (vec (seq pm)) (vec (seq tm))))
                (if (<= n 33)
                  (do (assert (= s-seq exp-order))
                      (assert (= m-keys exp-order)))
                  (let [[exp-head exp-tail exp-oh] exp-order]
                    (assert (= (vec (take 8 s-seq)) exp-head))
                    (assert (= (vec (take-last 8 s-seq)) exp-tail))
                    (assert (= (hash s-seq) exp-oh))
                    (assert (= (vec (take 8 m-keys)) exp-head))
                    (assert (= (vec (take-last 8 m-keys)) exp-tail))
                    (assert (= (hash m-keys) exp-oh))))
                true))
            ;; Expected hashes and seq orders captured from master (b965180c).
            sizes-ok (every? check-size
                             [[1 -1684229222 -1730596701 [0]]
                              [8 -1311951085 1920606970 [0 1 2 3 4 5 6 7]]
                              [9 1185812329 -1524359123 [0 1 2 3 4 5 6 7 8]]
                              [16 1572630083 533651172 (vec (range 16))]
                              [17 -1820114363 -2090259213 (vec (range 17))]
                              [32 -1405296752 -396163729 (vec (range 32))]
                              [33 -950456575 1800378393 (into [0 32] (range 1 32))]
                              [1050 922458704 140217157
                               [[0 1024 32 64 96 128 160 192]
                                [799 831 863 895 927 959 991 1023]
                                217608528]]])
            ;; Colliding hashes (same full hash) and deep splits (same low 5/10 bits, distinct at shift 10)
            c0 (CustomKey. 7 :c0)
            c1 (CustomKey. 7 :c1)
            c2 (CustomKey. 7 :c2)
            c1-dup (CustomKey. 7 :c1)
            d0 (CustomKey. 1 :d0)
            d1 (CustomKey. 33 :d1)
            d2 (CustomKey. 1025 :d2)
            d3 (CustomKey. 2049 :d3)
            mixed (concat (range 20) [nil c0 c1 c2 d0 d1 d2 d3 c1-dup nil])
            ps-coll (reduce conj #{} mixed)
            ts-coll (into #{} mixed)
            pm-coll (reduce (fn [m [i k]] (assoc m k i)) {} (map-indexed vector mixed))
            tm-coll (persistent! (reduce (fn [m [i k]] (assoc! m k i)) (transient {}) (map-indexed vector mixed)))
            ;; Disj/dissoc down through array, bitmap, and collision nodes
            ts-disj (persistent! (reduce disj! (transient ts-coll) (concat [nil c1 d2] (range 0 18))))
            ps-disj (apply disj ps-coll nil c1 d2 (range 0 18))
            tm-dissoc (persistent! (reduce dissoc! (transient tm-coll) (concat [nil c1 d2] (range 0 18))))
            pm-dissoc (apply dissoc pm-coll nil c1 d2 (range 0 18))
            ;; -0/0 coalescing and signed-zero value replacement (Sol #13)
            z-set-arr (persistent! (conj! (conj! (transient #{}) 0) -0))
            z-set-hash (into #{} (concat (range 1 12) [0 -0]))
            z-map-hash (persistent!
                         (assoc! (reduce (fn [m i] (assoc! m i i)) (transient {}) (range 12)) 0 -0))
            z-map-nil (persistent!
                        (assoc! (assoc! (reduce (fn [m i] (assoc! m i i)) (transient {}) (range 12)) nil 0) nil -0))
            ;; NaN members remain distinct in <=8, promoted 8->9, and transient 9+
            nan-arr-p (conj (conj #{} js/NaN) js/NaN)
            nan-arr-t (persistent! (conj! (conj! (transient #{}) js/NaN) js/NaN))
            nan-big-p (reduce conj #{} (concat (range 9) [js/NaN js/NaN]))
            nan-big-t (into #{} (concat (range 9) [js/NaN js/NaN]))
            ;; nil in sizes 1, 8, 9, 17, 33
            nil-ok (every? (fn [n]
                             (let [xs (conj (vec (range (dec n))) nil)
                                   p (reduce conj #{} xs)
                                   t (into #{} xs)]
                               (and (= (count p) (count t) n)
                                    (= (hash p) (hash t))
                                    (= p t)
                                    (= (vec (seq p)) (vec (seq t)))
                                    (contains? t nil)
                                    (= (dec n) (count (persistent! (disj! (transient t) nil)))))))
                           [1 8 9 17 33])]
        [sizes-ok
         (= (count ps-coll) (count ts-coll))
         (= (hash ps-coll) (hash ts-coll))
         (= ps-coll ts-coll)
         (= (vec (seq ps-coll)) (vec (seq ts-coll)))
         (identical? c1 (get ts-coll c1-dup))
         (= (count pm-coll) (count tm-coll))
         (= (hash pm-coll) (hash tm-coll))
         (= pm-coll tm-coll)
         (= (vec (seq pm-coll)) (vec (seq tm-coll)))
         (identical? c1 (key (find tm-coll c1-dup)))
         (= ts-disj ps-disj)
         (= (hash ts-disj) (hash ps-disj))
         (= tm-dissoc pm-dissoc)
         (= (hash tm-dissoc) (hash pm-dissoc))
         (= (count z-set-arr) 1)
         (js/Object.is (first z-set-arr) 0)
         (= (count z-set-hash) 12)
         (js/Object.is (get z-set-hash -0) 0)
         (js/Object.is (get z-map-hash 0) -0)
         (js/Object.is (get z-map-nil nil) -0)
         (= (count nan-arr-p) (count nan-arr-t) 2)
         (= (count nan-big-p) (count nan-big-t) 11)
         nil-ok])"#,
        "[true true true true true true true true true true true true true true true true true true true true true true true true]",
    );
}

#[test]
fn transient_set_and_map_isolation_staleness_and_reentrancy() {
    both_and_advanced(
        r#"
      (let [;; Sibling transients over persistent sets of sizes 4, 8, 9, 20, 40
            base-sets (mapv #(into #{} (range %)) [4 8 9 20 40])
            siblings-ok
            (every?
              (fn [s]
                (let [n (count s)
                      t1 (transient s)
                      t2 (transient s)
                      _ (conj! t1 :a :b :c 100 101 102 103 104 105)
                      _ (conj! t2 :x :y :z 200 201 202 203 204 205)
                      _ (disj! t1 0 1)
                      _ (disj! t2 2 3)
                      s1 (persistent! t1)
                      s2 (persistent! t2)
                      ;; Stale transient throws on mutating/count/persistent!, lookup still reads
                      stale-errs
                      [(try (conj! t1 :bad) :ok (catch js/Error _ :err))
                       (try (disj! t1 0) :ok (catch js/Error _ :err))
                       (try (persistent! t1) :ok (catch js/Error _ :err))
                       (try (count t1) :ok (catch js/Error _ :err))]
                      ;; New transient over persisted s1 (sharing nodes with t1's old edit token)
                      t3 (transient s1)
                      _ (conj! t3 :from-t3 999)
                      _ (disj! t3 :a)
                      s3 (persistent! t3)]
                  (and (= (count s) n)
                       (not (contains? s :a))
                       (not (contains? s :x))
                       (contains? s 0)
                       (contains? s 2)
                       (contains? s1 :a)
                       (not (contains? s1 :x))
                       (not (contains? s1 :from-t3))
                       (contains? s2 :x)
                       (not (contains? s2 :a))
                       (contains? s3 :from-t3)
                       (not (contains? s3 :a))
                       (= [:err :err :err :err] stale-errs)
                       (if (>= n 9) (= :b (get t1 :b)) true))))
              base-sets)
            ;; Sibling transients over persistent maps of sizes 4, 8, 9, 20, 40
            base-maps (mapv #(into {} (map (fn [i] [i i]) (range %))) [4 8 9 20 40])
            map-siblings-ok
            (every?
              (fn [m]
                (let [n (count m)
                      t1 (transient m)
                      t2 (transient m)
                      t1 (assoc! t1 :a 1 0 :m1 100 100)
                      t2 (assoc! t2 :b 2 0 :m2 200 200)
                      t1 (dissoc! t1 1)
                      m1 (persistent! t1)
                      m2 (persistent! t2)
                      t3 (transient m1)
                      t3 (assoc! t3 :c 3 0 :m3)
                      m3 (persistent! t3)]
                  (and (= (count m) n)
                       (= (get m 0) 0)
                       (= (get m 1) 1)
                       (nil? (get m :a))
                       (= (get m1 0) :m1)
                       (nil? (get m1 :c))
                       (= (get m2 0) :m2)
                       (= (get m3 0) :m3)
                       (= :err (try (assoc! t1 :bad 1) :ok (catch js/Error _ :err)))
                       (= :err (try (dissoc! t1 0) :ok (catch js/Error _ :err)))
                       (= :err (try (persistent! t1) :ok (catch js/Error _ :err)))
                       (= :err (try (count t1) :ok (catch js/Error _ :err))))))
              base-maps)
            ;; Reentrant persistent! inside -hash during trie conj! cannot mutate persisted set
            t-re1 (transient (into #{} (range 20)))
            saved1 (atom nil)
            k-re1 (reify
                    IHash (-hash [_]
                            (when (nil? @saved1) (reset! saved1 (persistent! t-re1)))
                            999)
                    IEquiv (-equiv [this other] (identical? this other)))
            r1 (try (conj! t-re1 k-re1) :ok (catch js/Error _ :err))
            ;; Reentrant persistent! inside -equiv on owned trie node (leaf split) cannot mutate persisted set
            t-re2 (conj! (transient (into #{} (range 20))) 39)
            saved2 (atom nil)
            k-re2 (reify
                    IHash (-hash [_] 7)
                    IEquiv (-equiv [_ _]
                             (when (nil? @saved2) (reset! saved2 (persistent! t-re2)))
                             false))
            r2 (try (conj! t-re2 k-re2) :ok (catch js/Error _ :err))
            ;; Reentrant persistent! inside -equiv during array-set conj! cannot mutate persisted set
            t-re3 (transient #{1 2 3})
            saved3 (atom nil)
            k-re3 (reify
                    IHash (-hash [_] 1)
                    IEquiv (-equiv [_ _]
                             (when (nil? @saved3) (reset! saved3 (persistent! t-re3)))
                             false))
            r3 (try (conj! t-re3 k-re3) :ok (catch js/Error _ :err))
            ;; Reentrant persistent! inside -hash during transient hash-map assoc! cannot mutate persisted map
            tm-re (transient (into {} (map (fn [i] [i i]) (range 20))))
            saved-m (atom nil)
            km-re (reify
                    IHash (-hash [_]
                            (when (nil? @saved-m) (reset! saved-m (persistent! tm-re)))
                            999)
                    IEquiv (-equiv [this other] (identical? this other)))
            rm (try (assoc! tm-re km-re :bad) :ok (catch js/Error _ :err))
            ;; Reentrant conj! inside -equiv on a 7-member array-set transient promoting it mid-call
            t-re4 (transient #{1 2 3 4 5 6 7})
            promoted (atom false)
            k-re4 (reify
                    IHash (-hash [_] 100)
                    IEquiv (-equiv [this other]
                             (when-not @promoted
                               (reset! promoted true)
                               (conj! t-re4 8 9))
                             (identical? this other)))
            s-re4 (persistent! (conj! t-re4 k-re4))]
        [siblings-ok map-siblings-ok
         r1 (= (count @saved1) 20) (not (contains? @saved1 k-re1))
         r2 (= (count @saved2) 21) (contains? @saved2 7) (not (contains? @saved2 k-re2))
         r3 (= (count @saved3) 3) (not (contains? @saved3 k-re3))
         rm (= (count @saved-m) 20) (not (contains? @saved-m km-re))
         (= (count s-re4) 10) (contains? s-re4 8) (contains? s-re4 9) (contains? s-re4 k-re4)])"#,
        "[true true :err true true :err true true true :err true true :err true true true true true true]",
    );
}

#[test]
fn transient_set_sol_findings_and_borrowed_slots() {
    both_and_advanced(
        r#"
      (let [;; Sol #5: count retained keys in sets built from positional hash maps with mismatched cnt
            src (hash-map :a 1)
            fake-map (PersistentHashMap. nil 5 (.-root src) false nil nil)
            s5 (PersistentHashSet. nil fake-map nil)
            t5 (transient s5)
            p5 (persistent! t5)
            ;; Sol #6: preserve stored member hashes when hashing sets
            armed (atom false)
            k6 (reify
                 IHash (-hash [_]
                         (if @armed (throw (js/Error. "rehashed")) 31))
                 IEquiv (-equiv [this other] (identical? this other)))
            s6 (into #{} (conj (vec (range 8)) k6))
            _ (reset! armed true)
            h6 (hash s6)
            ;; Sol #9 / Sol review #2: 8-member transient array set duplicate takes
            ;; master's two scans (threshold probe + insertion scan), never a third.
            calls9 (atom 0)
            k9 (reify
                 IHash (-hash [_] 0)
                 IEquiv (-equiv [_ other]
                          (let [n (swap! calls9 inc)]
                            (when (> n 2) (throw (js/Error. "third-scan")))
                            (= other 0))))
            t9 (transient #{0 1 2 3 4 5 6 7})
            _ (conj! t9 k9)
            s9 (persistent! t9)]
        [(count s5) (count p5)
         (number? h6)
         (= (count s9) 8) (= @calls9 2)])"#,
        "[5 1 true true true]",
    );
    // Borrowed TransientHashSet -conj! and -persistent! slots on Delay (shares state$LKEY)
    // and TransientVector must not misinterpret receiver state in Node or compiled output.
    let borrowed = r#"
      (let [ts (transient #{1})
            tv (transient [1])
            d (delay 42)
            proto (js/Object.getPrototypeOf ts)
            conj-key (first (filter #(.includes % "$_conj_BANG_$arity$2$")
                                    (js/Object.getOwnPropertyNames proto)))
            pers-key (first (filter #(.includes % "$_persistent_BANG_$arity$1$")
                                    (js/Object.getOwnPropertyNames proto)))
            _ (aset tv conj-key (aget proto conj-key))
            _ (aset tv pers-key (aget proto pers-key))
            _ (aset d conj-key (aget proto conj-key))
            _ (aset d pers-key (aget proto pers-key))
            tv-res (persistent! (conj! tv 2))
            d-conj (try [:ok (conj! d 1)] (catch js/Error e (ex-message e)))
            d-pers (try [:ok (persistent! d)] (catch js/Error e (ex-message e)))]
        [tv-res d-conj d-pers @d])"#;
    let borrowed_expected = "[[1 2] \"No protocol method ITransientCollection/-conj! defined for this value\" \"No protocol method ITransientCollection/-persistent! defined for this value\" 42]";
    let wrapped = format!("(js/console.log (pr-str (do {borrowed})))");
    let mut host = MacroHost::new();
    let p = analyze_with_macros(&wrapped, &parse(&wrapped).unwrap(), &mut host).unwrap();
    for level in OptimizationLevel::ALL {
        let js = prepare_application(&p, ApplicationOptions::new(level)).emit_js();
        assert_eq!(
            execute(&format!("{js};")).trim(),
            borrowed_expected,
            "borrowed slots at {level:?}"
        );
    }
}

#[test]
fn transient_set_threshold_duplicate_survives_nested_disjoin() {
    both_and_advanced(
        r#"
      (let [t (transient #{0 1 2 3 4 5 6 7})
            removed (atom false)
            k (reify
                IHash (-hash [_] 0)
                IEquiv (-equiv [this other]
                         (cond
                           (identical? this other) true
                           (= other 0)
                           (do (when-not @removed
                                 (reset! removed true)
                                 (disj! t 0))
                               true)
                           :else false)))
            s (persistent! (conj! t k))]
        [(count s) (contains? s 0) (contains? s k) (identical? k (get s k))])"#,
        "[8 false true true]",
    );
}

#[test]
fn transient_bitmap_expansion_preserves_reentrant_hash_effects() {
    both_and_advanced(
        r#"
      (let [armed (atom false)
            t-ref (atom nil)
            k15-ref (atom nil)
            k0 (reify
                 IHash (-hash [_]
                         (when (and @t-ref (not @armed))
                           (dissoc! @t-ref @k15-ref)
                           (reset! armed true))
                         0)
                 IEquiv (-equiv [this other] (identical? this other)))
            k15 (reify
                  IHash (-hash [_]
                          (if @armed (throw (js/Error. "k15-rehash")) 15))
                  IEquiv (-equiv [this other] (identical? this other)))
            _ (reset! k15-ref k15)
            ;; Build an owned transient hash map with 16 branches 0..15 in its root bitmap
            t (reduce (fn [m [k v]] (assoc! m k v))
                      (transient (hash-map))
                      (concat [[k0 :v0]]
                              (map (fn [i] [i i]) (range 1 15))
                              [[k15 :v15]]))
            _ (reset! t-ref t)
            ;; Branch 16 triggers 16->17 bitmap-to-array expansion
            err (try (assoc! t 16 :v16) :ok (catch js/Error e (ex-message e)))
            _ (reset! t-ref nil)
            _ (reset! armed false)
            m (persistent! t)]
        [err (count m) (contains? m k0) (contains? m k15) (contains? m 16)])"#,
        "[\"k15-rehash\" 15 true false false]",
    );
}

#[test]
fn transient_bitmap_edits_survive_reentrant_equiv() {
    both_and_advanced(
        r#"
      (let [t1 (assoc! (transient (hash-map)) 7 :orig)
            inserted1 (atom false)
            q1 (reify
                 IHash (-hash [_] 7)
                 IEquiv (-equiv [_ other]
                          (when-not @inserted1
                            (reset! inserted1 true)
                            (assoc! t1 1 :nested))
                          (= other 7)))
            m1 (persistent! (assoc! t1 q1 :updated))
            t2 (assoc! (transient (hash-map)) 7 :orig)
            inserted2 (atom false)
            q2 (reify
                 IHash (-hash [_] 7)
                 IEquiv (-equiv [_ other]
                          (when-not @inserted2
                            (reset! inserted2 true)
                            (assoc! t2 1 :nested))
                          (= other 7)))
            m2 (persistent! (dissoc! t2 q2))]
        [(count m1) (get m1 1) (get m1 7)
         (count m2) (get m2 1) (contains? m2 7)])"#,
        "[2 :nested :updated 1 :nested false]",
    );
}

#[test]
fn hash_trie_assoc_preserves_signed_zero_values() {
    both_and_advanced(
        r#"
      (deftype SameHashKey [id]
        IHash (-hash [_] 7)
        IEquiv (-equiv [_ other] (and (instance? SameHashKey other) (= id (.-id other)))))
      (let [;; Persistent bitmap leaf: replacing +0 with -0 preserves -0
            pm0 (reduce (fn [m i] (assoc m i i)) (hash-map) (range 12))
            pm1 (assoc pm0 0 -0)
            ;; Transient collision node with two distinct equal-hash keys
            c0 (SameHashKey. :a)
            c1 (SameHashKey. :b)
            tm0 (assoc! (assoc! (transient (hash-map)) c0 0) c1 0)
            tm1 (persistent! (assoc! tm0 c1 -0))]
        [(js/Object.is (get pm1 0) -0)
         (js/Object.is (get tm1 c0) 0)
         (js/Object.is (get tm1 c1) -0)])"#,
        "[true true true]",
    );
}

#[test]
fn advanced_into_set_uses_direct_transient_set_path() {
    let source = r#"
      (let [v (vec (range 100))
            s (into #{} v)]
        [(count s) (contains? s 0) (contains? s 99) (contains? s 100)])"#;
    let mut opts = ApplicationOptions::new(OptimizationLevel::Advanced);
    opts.pseudo_names = true;
    let js = advanced_with_options(source, "[100 true true false]", opts);
    assert!(
        js.contains("function hashNodeAssocTransient("),
        "expected advanced output to retain hashNodeAssocTransient"
    );
    assert!(
        js.contains("function putMember("),
        "expected advanced output to retain putMember for instrumentation"
    );
    let instrumented = js
        .replacen(
            "function hashNodeAssocTransient(root, keyHash, entry, edit) {",
            "function hashNodeAssocTransient(root, keyHash, entry, edit) { globalThis.__transientAssocCalls = (globalThis.__transientAssocCalls || 0) + 1;",
            1,
        )
        .replacen(
            "function putMember(members, key, edit = null) {",
            "function putMember(members, key, edit = null) { globalThis.__putMemberCalls = (globalThis.__putMemberCalls || 0) + 1;",
            1,
        );
    assert_ne!(instrumented, js);
    let out = execute(&format!(
        "{instrumented};\nconsole.log(JSON.stringify([globalThis.__transientAssocCalls || 0, globalThis.__putMemberCalls || 0]));"
    ));
    let lines: Vec<_> = out.trim().lines().collect();
    assert_eq!(lines[0], "[100 true true false]");
    let counts: Vec<usize> = lines[1]
        .trim_matches(|c| c == '[' || c == ']')
        .split(',')
        .map(|s| s.parse().unwrap())
        .collect();
    assert_eq!(
        counts,
        vec![100, 0],
        "expected 100 hashNodeAssocTransient calls (8 promotion + 92 inserts) and 0 putMember calls"
    );
}

/// Complete sources in both compiler phases and both host lifecycles, each run in
/// QuickJS and Node (see `tests/support/phase_oracle.rs`).
mod phase_oracles {
    use crate::phase_oracle::{Case, run};

    #[test]
    fn transient_bodies() {
        run(&[
            Case::expression(
                "test-transient-edge-case-2",
                include_str!(
                    "../fixtures/phase-oracles/transients/test-transient-edge-case-2.cljs"
                ),
            ),
            Case::expression(
                "transient-receivers",
                include_str!("../fixtures/phase-oracles/transients/transient-receivers.cljs"),
            ),
        ]);
    }
}
