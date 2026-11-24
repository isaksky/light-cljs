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
    // The Pants build embeds no QuickJS to run this half of the check.
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
fn shared_profile_transient_extensions_and_promotion_contracts() {
    // Sol #1: In the Pants shared-heap profile, `fieldComparisons` is frozen after boot and
    // `hostExt.seq` is true. `specify!` on fresh TransientArrayMap and TransientHashMap must
    // not write to `fieldComparisons` and must honor the instance-specific `-conj!`.
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

    // Sol #1 & Sol #4: Verify the assembled shared-heap runtime keeps `tabledKind` free of
    // kinds 31/32, gates transient-map invalidation on `!hostExt.seq`, and overrides
    // `arrayMapPromote` to delegate to `mapAssocEntries` (which allocates engine maps via
    // `sharedUpdatedMapFromEntries`).
    let shared = light_cljs::runtime_manifest::SHARED_RUNTIME;
    assert!(
        shared.contains("1883361390 >>> kind & 1") && shared.contains("4114 >>> kind - 32 & 1"),
        "expected shared profile tabledKind to exclude transient map kinds 31 and 32"
    );
    assert!(
        shared.contains("function arrayMapPromote(self, _arr, key, item) {\n    return mapAssocEntries(self, key, item);"),
        "expected shared profile arrayMapPromote to delegate to mapAssocEntries"
    );
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
