//! Public hash-map routing effects execute against literal canonical expectations.
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
    let runtime = rquickjs::Runtime::new().unwrap();
    let context = rquickjs::Context::full(&runtime).unwrap();
    let quickjs = context.with(|ctx| ctx.eval::<String, _>(script.as_bytes()).unwrap());
    fs::write(directory.join("quickjs.txt"), &quickjs).unwrap();
    assert_eq!(quickjs, expected);
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
/// that variable is set, kept after the run; otherwise a temporary directory that
/// `finish` removes.
fn artifact_root(name: &str) -> (PathBuf, bool) {
    let retained = std::env::var_os("LIGHT_HASH_BRANCH_ROOT");
    let root = match &retained {
        Some(root) => PathBuf::from(root).join(name),
        None => std::env::temp_dir().join(format!(
            "light-cljs-hash-branch-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )),
    };
    assert!(!root.exists());
    fs::create_dir_all(&root).unwrap();
    (root, retained.is_some())
}

fn finish(root: PathBuf, retained: bool) {
    if !retained {
        fs::remove_dir_all(root).unwrap();
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
    let (root, retained) = artifact_root(&format!("rows-{part}"));
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
    finish(root, retained);
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
    let (root, retained) = artifact_root("failures");
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
    finish(root, retained);
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
