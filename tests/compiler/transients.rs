//! Canonical transient collection ownership, public dispatch and macro parity.
use crate::temp_dir::TempDir;
use light_cljs::evaluator::{Evaluator, EvaluatorOptions, RUNTIME_SOURCE};
use light_cljs::{MacroHost, analyze_with_macros, emitter::emit_js_unit, parse};
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
    let e = Evaluator::new(EvaluatorOptions::default()).unwrap();
    assert_eq!(
        e.print(e.evaluate(&p).unwrap()).unwrap(),
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
