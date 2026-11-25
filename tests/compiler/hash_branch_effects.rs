//! Public hash-map routing effects execute against literal canonical expectations.
use crate::temp_dir::TempDir;
use light_cljs::{MacroHost, analyze_with_macros, emitter::emit_js_unit, parse};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

fn execute(unit: &str, expected: &str, directory: &Path) {
    let script = format!(
        "(() => {{ const rt = {}; return rt.core.prStr(({})(rt)); }})()",
        light_cljs::evaluator::RUNTIME_SOURCE,
        unit
    );
    #[cfg(feature = "quickjs")]
    {
        let runtime = rquickjs::Runtime::new().unwrap();
        let context = rquickjs::Context::full(&runtime).unwrap();
        let quickjs = context.with(|ctx| ctx.eval::<String, _>(script.as_bytes()).unwrap());
        fs::write(directory.join("quickjs.txt"), &quickjs).unwrap();
        assert_eq!(quickjs, expected);
    }
    let mut child = Command::new(std::env::var_os("NODE").unwrap_or_else(|| "node".into()))
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(format!("console.log({script});").as_bytes())
        .unwrap();
    let result = child.wait_with_output().unwrap();
    fs::write(directory.join("node.stdout"), &result.stdout).unwrap();
    fs::write(directory.join("node.stderr"), &result.stderr).unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(result.stderr.is_empty());
    assert_eq!(
        String::from_utf8(result.stdout).unwrap(),
        format!("{expected}\n")
    );
}

/// A new directory for one test's artifacts: `LIGHT_HASH_BRANCH_ROOT/<name>` when
/// that variable is set, kept after the run; otherwise a temporary directory.
fn artifact_root(name: &str) -> TempDir {
    match std::env::var_os("LIGHT_HASH_BRANCH_ROOT") {
        Some(root) => {
            let root = PathBuf::from(root).join(name);
            assert!(!root.exists());
            TempDir::retained(root)
        }
        None => TempDir::new(&format!("cljs-hash-branch-{name}")),
    }
}

/// A host that has already expanded a macro, for the variants that reuse one host.
fn warmed_host(root: &Path) -> MacroHost {
    let mut reused = MacroHost::new();
    fs::write(
        root.join("warmup.js"),
        light_cljs::compile_with_macros("(defmacro ready [] (+ 20 22)) (ready)", &mut reused)
            .unwrap(),
    )
    .unwrap();
    reused
}

/// The fixture rows run in this many tests, declared below, so they overlap under
/// nextest.
const PARTS: usize = 6;

/// Runs every `PARTS`-th fixture row from `part` as an application and as a macro
/// computation, each with a fresh host and with the part's one reused host, in
/// QuickJS and Node.
fn execute_rows(part: usize) {
    let root = artifact_root(&format!("rows-{part}"));
    let mut reused = warmed_host(&root);
    for row in include_str!("../fixtures/compiler-hash-branch-effects.tsv")
        .lines()
        .filter(|row| !row.is_empty() && !row.starts_with('#'))
        .skip(part)
        .step_by(PARTS)
    {
        let fields: Vec<_> = row.split('\t').collect();
        assert_eq!(fields.len(), 3);
        let (name, original, expected) = (fields[0], fields[1], fields[2]);
        for reuse in [false, true] {
            for phase in [false, true] {
                let variant = format!(
                    "{name}-{}-{}",
                    if phase { "macro" } else { "application" },
                    if reuse { "reused" } else { "fresh" }
                );
                let directory = root.join(&variant);
                fs::create_dir(&directory).unwrap();
                let source = if phase {
                    format!("(defmacro contract [] (pr-str (do {original}))) (contract)")
                } else {
                    original.to_owned()
                };
                fs::write(directory.join("source.cljs"), &source).unwrap();
                let mut fresh = MacroHost::new();
                let host = if reuse { &mut reused } else { &mut fresh };
                let program = analyze_with_macros(&source, &parse(&source).unwrap(), host)
                    .unwrap_or_else(|error| panic!("{variant}: {error}"));
                let unit = emit_js_unit(&program);
                fs::write(directory.join("unit.js"), &unit).unwrap();
                let literal = if phase {
                    format!("{expected:?}")
                } else {
                    expected.to_owned()
                };
                execute(&unit, &literal, &directory);
                println!("{variant}: QuickJS/Node {literal}");
            }
        }
    }
}

row_tests!(execute_rows;
    hash_branch_effects_execute_in_fresh_and_reused_phases_0(0),
    hash_branch_effects_execute_in_fresh_and_reused_phases_1(1),
    hash_branch_effects_execute_in_fresh_and_reused_phases_2(2),
    hash_branch_effects_execute_in_fresh_and_reused_phases_3(3),
    hash_branch_effects_execute_in_fresh_and_reused_phases_4(4),
    hash_branch_effects_execute_in_fresh_and_reused_phases_5(5),
);

#[test]
fn hash_branch_failures_leave_fresh_and_reused_hosts_usable() {
    let root = artifact_root("failures");
    let mut reused = warmed_host(&root);
    // Uncaught branch hash/equality failures must leave the actual MacroHost usable.
    let failures = [
        (
            "branch-hash",
            "stored-hash-failed",
            "(let [fail (atom false) stored (reify IHash (-hash [this] (if @fail (throw (js/Error. \"stored-hash-failed\")) 7))) query (reify IHash (-hash [this] 39) IEquiv (-equiv [this other] false)) old (hash-map stored 1)] (reset! fail true) (assoc old query 2))",
        ),
        (
            "branch-equality",
            "query-equiv-failed",
            "(let [stored (reify IHash (-hash [this] 7)) query (reify IHash (-hash [this] 39) IEquiv (-equiv [this other] (throw (js/Error. \"query-equiv-failed\"))))] (assoc (hash-map stored 1) query 2))",
        ),
    ];
    for (name, message, body) in failures {
        for reuse in [false, true] {
            let mut fresh = MacroHost::new();
            let host = if reuse { &mut reused } else { &mut fresh };
            let source = format!("(defmacro branch-failure [] {body}) (branch-failure)");
            let error = light_cljs::compile_with_macros(&source, host)
                .unwrap_err()
                .to_string();
            assert!(error.contains(message), "{error}");
            let directory = root.join(format!(
                "{name}-{}-recovery",
                if reuse { "reused" } else { "fresh" }
            ));
            fs::create_dir(&directory).unwrap();
            fs::write(directory.join("failure.cljs"), source).unwrap();
            fs::write(directory.join("failure.txt"), error).unwrap();
            let recovery = "(defmacro recovered-branch [] (count (assoc (hash-map 7 1) 39 2))) (recovered-branch)";
            let program = analyze_with_macros(recovery, &parse(recovery).unwrap(), host).unwrap();
            let unit = emit_js_unit(&program);
            fs::write(directory.join("source.cljs"), recovery).unwrap();
            fs::write(directory.join("unit.js"), &unit).unwrap();
            execute(&unit, "2", &directory);
        }
    }
    // A required key comparison must fail without poisoning the actual host.
    let broken = "(defmacro broken [] (let [a (atom 1) k (reify IEquiv (-equiv [this other] (throw (js/Error. \"key-failed\"))))] (add-watch a nil (fn [& _])) (remove-watch a k))) (broken)";
    let error = light_cljs::compile_with_macros(broken, &mut reused)
        .unwrap_err()
        .to_string();
    assert!(error.contains("key-failed"), "{error}");
    fs::write(root.join("failure.txt"), error).unwrap();
    let recovery = "(defmacro recovered [] (let [a (atom 1)] (add-watch a nil (fn [& _])) (remove-watch a :absent) (reset! a 2))) (recovered)";
    let program = analyze_with_macros(recovery, &parse(recovery).unwrap(), &mut reused).unwrap();
    let directory = root.join("recovery");
    fs::create_dir(&directory).unwrap();
    fs::write(directory.join("source.cljs"), recovery).unwrap();
    let unit = emit_js_unit(&program);
    fs::write(directory.join("unit.js"), &unit).unwrap();
    execute(&unit, "2", &directory);
}

/// Complete sources in both compiler phases and both host lifecycles, each run in
/// QuickJS and Node (see `tests/support/phase_oracle.rs`).
mod phase_oracles {
    use crate::phase_oracle::{Case, run};

    #[test]
    fn fixed_hash_collisions() {
        run(&[Case::expression(
            "phm-fixed-hash",
            include_str!("../fixtures/phase-oracles/hash-branch-effects/phm-fixed-hash.cljs"),
        )]);
    }
}

#[test]
fn persistent_array_promotion_then_transient_growth() {
    // Sol #8: Promoting an 8-key array map persistently must insert the 9th key
    // persistently (capacity 9, not 12) so a later transient expands and rehashes
    // retained keys on insertion 16 rather than deferring to 17.
    // Exercise both custom-key promotion (via mapAssocEntries) and plain-key
    // promotion (via arrayMapPromote's direct hashNodeAssoc path).
    let custom_source = r#"
      (let [step (atom :init)
            hashes (atom [])
            k31 (reify
                  IHash (-hash [_] (swap! hashes conj @step) 31)
                  IEquiv (-equiv [a b] (identical? a b)))
            m8 (array-map 0 0 1 1 2 2 3 3 4 4 5 5 6 6 k31 :k31)
            _ (reset! step :promote-7)
            m9 (assoc m8 7 7)
            t (transient m9)
            t (reduce (fn [acc k]
                        (reset! step k)
                        (assoc! acc k k))
                      t
                      (range 8 17))
            m18 (persistent! t)
            _ (reset! step :lookup)]
        [(count m18) (get m18 k31) @hashes])"#;
    let plain_source = r#"
      (let [step (atom :init)
            hashes (atom [])
            k30 (reify
                  IHash (-hash [_] (swap! hashes conj @step) 30)
                  IEquiv (-equiv [a b] (identical? a b)))
            m8 (array-map 0 0 1 1 2 2 3 3 4 4 5 5 6 6 31 31)
            m9 (assoc m8 7 7)
            t (transient m9)
            _ (reset! step :insert-k30)
            t (assoc! t k30 :k30)
            t (reduce (fn [acc k]
                        (reset! step k)
                        (assoc! acc k k))
                      t
                      (range 9 17))
            m18 (persistent! t)
            _ (reset! step :lookup)]
        [(count m18) (get m18 k30) @hashes])"#;
    for (name, source, expected) in [
        (
            "array-promotion-transient-growth-custom",
            custom_source,
            "[18 :k31 [:promote-7 16 :lookup]]",
        ),
        (
            "array-promotion-transient-growth-plain",
            plain_source,
            "[18 :k30 [:insert-k30 16 :lookup]]",
        ),
    ] {
        let root = artifact_root(name);
        let mut host = MacroHost::new();
        let program = analyze_with_macros(source, &parse(source).unwrap(), &mut host).unwrap();
        let unit = emit_js_unit(&program);
        execute(&unit, expected, &root);

        let wrapped = format!("(js/console.log (pr-str (do {source})))");
        let mut adv_host = MacroHost::new();
        let adv_program =
            analyze_with_macros(&wrapped, &parse(&wrapped).unwrap(), &mut adv_host).unwrap();
        let js = light_cljs::prepare_application(
            &adv_program,
            light_cljs::ApplicationOptions::new(light_cljs::OptimizationLevel::Advanced),
        )
        .emit_js();
        let out = super::optimization::execute(&format!("{js};"));
        assert_eq!(out.trim(), expected);
    }
}

#[test]
fn persistent_array_promotion_preserves_threshold_equiv() {
    // Sol #2: At 8 entries, a custom query key must still observe the threshold
    // entryIndex probe before promotion hashes retained keys.
    let source = r#"
      (let [calls (atom 0)
            k (reify
                IHash (-hash [_] 31)
                IEquiv (-equiv [_ other]
                         (let [n (swap! calls inc)]
                           (and (> n 8) (= other 0)))))
            m8 (array-map 0 :orig 1 1 2 2 3 3 4 4 5 5 6 6 7 7)
            m (assoc m8 k :updated)]
        [(count m) (get m 0) @calls (instance? PersistentArrayMap m)])"#;
    // 8 misses in arrayIndex + 1 hit in putCollectionEntry's entryIndex + 1 hit in putEntry's entryIndex.
    let expected = "[8 :updated 10 true]";
    let root = artifact_root("array-promotion-threshold-equiv");
    let mut host = MacroHost::new();
    let program = analyze_with_macros(source, &parse(source).unwrap(), &mut host).unwrap();
    let unit = emit_js_unit(&program);
    execute(&unit, expected, &root);

    let wrapped = format!("(js/console.log (pr-str (do {source})))");
    let mut adv_host = MacroHost::new();
    let adv_program =
        analyze_with_macros(&wrapped, &parse(&wrapped).unwrap(), &mut adv_host).unwrap();
    let js = light_cljs::prepare_application(
        &adv_program,
        light_cljs::ApplicationOptions::new(light_cljs::OptimizationLevel::Advanced),
    )
    .emit_js();
    let out = super::optimization::execute(&format!("{js};"));
    assert_eq!(out.trim(), expected);
}

#[test]
fn persistent_array_promotion_with_coalescing_keys() {
    // Sol #3: When retained keys coalesce during reinsertion at the 8-entry
    // threshold so the resulting count stays <= 8, preserve PersistentArrayMap,
    // insertion order, and retained values.
    let source = r#"
      (let [coalesce (atom false)
            k1 (reify
                 IHash (-hash [_] 31)
                 IEquiv (-equiv [this other]
                          (or (identical? this other)
                              (and @coalesce (= (hash other) 31)))))
            k2 (reify
                 IHash (-hash [_] 31)
                 IEquiv (-equiv [this other]
                          (or (identical? this other)
                              (and @coalesce (= (hash other) 31)))))
            m8 (array-map k1 :v1 :a 1 :b 2 :c 3 :d 4 :e 5 :f 6 k2 :v2)
            _ (reset! coalesce true)
            m (assoc m8 :ninth 9)]
        [(instance? PersistentArrayMap m)
         (instance? PersistentHashMap m)
         (count m)
         (get m k1)
         (get m :ninth)
         (vec (rest (keys m)))])"#;
    let expected = "[true false 8 :v2 9 [:a :b :c :d :e :f :ninth]]";
    let root = artifact_root("array-promotion-coalescing-keys");
    let mut host = MacroHost::new();
    let program = analyze_with_macros(source, &parse(source).unwrap(), &mut host).unwrap();
    let unit = emit_js_unit(&program);
    execute(&unit, expected, &root);

    let wrapped = format!("(js/console.log (pr-str (do {source})))");
    let mut adv_host = MacroHost::new();
    let adv_program =
        analyze_with_macros(&wrapped, &parse(&wrapped).unwrap(), &mut adv_host).unwrap();
    let js = light_cljs::prepare_application(
        &adv_program,
        light_cljs::ApplicationOptions::new(light_cljs::OptimizationLevel::Advanced),
    )
    .emit_js();
    let out = super::optimization::execute(&format!("{js};"));
    assert_eq!(out.trim(), expected);
}

#[test]
fn collision_queries_preserve_equiv_calls() {
    let source = r#"
      (deftype CollKey [h id]
        IHash (-hash [_] h)
        IEquiv (-equiv [_ other] (and (instance? CollKey other) (= id (.-id other)))))
      (let [c0 (CollKey. 7 :c0)
            c1 (CollKey. 7 :c1)
            m (hash-map c0 :v0 c1 :v1)
            log (atom [])
            q (reify
                IHash (-hash [_] 39)
                IEquiv (-equiv [_ other]
                         (swap! log conj (.-id other))
                         false))
            r-get (get m q :missing)
            log-get @log
            _ (reset! log [])
            r-find (find m q)
            log-find @log
            _ (reset! log [])
            r-dissoc (dissoc m q)
            log-dissoc @log
            _ (reset! log [])
            t (transient m)
            r-tget (get t q :missing)
            log-tget @log
            _ (reset! log [])
            r-tdissoc (persistent! (dissoc! t q))
            log-tdissoc @log]
        [r-get log-get
         r-find log-find
         (= m r-dissoc) log-dissoc
         r-tget log-tget
         (= m r-tdissoc) log-tdissoc])"#;
    let expected =
        "[:missing [:c0 :c1] nil [:c0 :c1] true [:c0 :c1] :missing [:c0 :c1] true [:c0 :c1]]";
    let root = artifact_root("collision-queries-equiv");
    let mut host = MacroHost::new();
    let program = analyze_with_macros(source, &parse(source).unwrap(), &mut host).unwrap();
    let unit = emit_js_unit(&program);
    execute(&unit, expected, &root);

    let wrapped = format!("(js/console.log (pr-str (do {source})))");
    let mut adv_host = MacroHost::new();
    let adv_program =
        analyze_with_macros(&wrapped, &parse(&wrapped).unwrap(), &mut adv_host).unwrap();
    let js = light_cljs::prepare_application(
        &adv_program,
        light_cljs::ApplicationOptions::new(light_cljs::OptimizationLevel::Advanced),
    )
    .emit_js();
    let out = super::optimization::execute(&format!("{js};"));
    assert_eq!(out.trim(), expected);
}
