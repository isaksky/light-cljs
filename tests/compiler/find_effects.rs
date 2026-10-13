//! Public find effects execute against literal canonical expectations.
use light_cljs::{MacroHost, analyze_with_macros, emitter::emit_js_unit, parse};
use std::{
    fs,
    io::Write,
    path::Path,
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

fn uncaught_application(failed_unit: &str, recovery_unit: &str, message: &str, directory: &Path) {
    let script = format!(
        "const rt = {}; rt.core.prStr(({})(rt));",
        light_cljs::evaluator::RUNTIME_SOURCE,
        failed_unit
    );
    let runtime = rquickjs::Runtime::new().unwrap();
    let context = rquickjs::Context::full(&runtime).unwrap();
    context.with(|ctx| {
        assert!(ctx.eval::<String, _>(script.as_bytes()).is_err());
        let failure = ctx.catch();
        let actual: String = failure.as_object().unwrap().get("message").unwrap();
        assert_eq!(actual, message);
        fs::write(directory.join("application-quickjs-failure.txt"), actual).unwrap();
        let recovery = format!("rt.core.prStr(({})(rt));", recovery_unit);
        let actual: String = ctx.eval(recovery.as_bytes()).unwrap();
        assert_eq!(actual, "42");
        fs::write(directory.join("application-quickjs-recovered.txt"), actual).unwrap();
    });
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
        .write_all(script.as_bytes())
        .unwrap();
    let result = child.wait_with_output().unwrap();
    assert!(!result.status.success());
    assert!(result.stdout.is_empty());
    assert!(String::from_utf8_lossy(&result.stderr).contains(message));
    fs::write(
        directory.join("application-node-failure.stderr"),
        result.stderr,
    )
    .unwrap();
}

#[test]
fn find_effects_execute_in_fresh_and_reused_phases() {
    let retained = std::env::var_os("LIGHT_FIND_ROOT");
    let root = retained
        .as_ref()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!(
                "light-cljs-find-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ))
        });
    assert!(!root.exists());
    fs::create_dir_all(&root).unwrap();
    let mut reused = MacroHost::new();
    fs::write(
        root.join("warmup.js"),
        light_cljs::compile_with_macros("(defmacro ready [] (+ 20 22)) (ready)", &mut reused)
            .unwrap(),
    )
    .unwrap();
    for row in include_str!("../fixtures/compiler-find-effects.tsv")
        .lines()
        .filter(|row| !row.is_empty() && !row.starts_with('#'))
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
    // Uncaught lookup failures must leave the actual MacroHost usable.
    let failures = [
        (
            "find-hash",
            "find-hash-failed",
            "(let [query (reify IHash (-hash [this] (throw (js/Error. \"find-hash-failed\"))))] (find (hash-map 7 :value) query))",
        ),
        (
            "find-equiv",
            "find-equiv-failed",
            "(let [stored (reify IHash (-hash [this] 7)) query (reify IHash (-hash [this] 7) IEquiv (-equiv [this other] (throw (js/Error. \"find-equiv-failed\"))))] (-find (hash-map stored :value) query))",
        ),
    ];
    for (name, message, body) in failures {
        for reuse in [false, true] {
            let directory = root.join(format!(
                "{name}-{}-recovery",
                if reuse { "reused" } else { "fresh" }
            ));
            fs::create_dir(&directory).unwrap();
            let mut fresh = MacroHost::new();
            let host = if reuse { &mut reused } else { &mut fresh };
            let source = format!("(defmacro lookup-failure [] {body}) (lookup-failure)");
            let error = light_cljs::compile_with_macros(&source, host)
                .unwrap_err()
                .to_string();
            assert!(error.contains(message), "{error}");
            fs::write(directory.join("failure.cljs"), source).unwrap();
            fs::write(directory.join("failure.txt"), error).unwrap();
            let recovery = "(defmacro recovered-find [] (val (find (hash-map :key 42) :key))) (recovered-find)";
            let program = analyze_with_macros(recovery, &parse(recovery).unwrap(), host).unwrap();
            let unit = emit_js_unit(&program);
            fs::write(directory.join("source.cljs"), recovery).unwrap();
            fs::write(directory.join("unit.js"), &unit).unwrap();
            let failed_program = analyze_with_macros(body, &parse(body).unwrap(), host).unwrap();
            let failed_unit = emit_js_unit(&failed_program);
            fs::write(directory.join("application-failure.cljs"), body).unwrap();
            fs::write(directory.join("application-failure-unit.js"), &failed_unit).unwrap();
            uncaught_application(&failed_unit, &unit, message, &directory);
            execute(&unit, "42", &directory);
        }
    }
    if retained.is_none() {
        fs::remove_dir_all(root).unwrap();
    }
}
