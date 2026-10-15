//! MapEntry fallback uses public entry protocols in both engines and phases.
use crate::temp_dir::TempDir;
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

#[test]
fn map_entry_reduce_kv_fallback_executes_in_fresh_and_reused_phases() {
    let root = TempDir::new_named_by("LIGHT_REDUCE_KV_ENTRIES_ROOT", "cljs-reduce-kv");
    let mut reused = MacroHost::new();
    fs::write(
        root.join("warmup.js"),
        light_cljs::compile_with_macros("(defmacro ready [] (+ 20 22)) (ready)", &mut reused)
            .unwrap(),
    )
    .unwrap();
    for row in include_str!("../fixtures/compiler-reduce-kv-entries.tsv")
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
    // An uncaught entry fallback failure must leave the actual macro host usable.
    let broken = "(defmacro broken [] (reduce-kv (fn [a k v] v) nil (find {:a 5} :a))) (broken)";
    let error = light_cljs::compile_with_macros(broken, &mut reused)
        .unwrap_err()
        .to_string();
    assert!(error.contains("IMapEntry/-key"), "{error}");
    fs::write(root.join("failure.txt"), error).unwrap();
    let recovery =
        "(defmacro recovered [] (pr-str (reduce-kv (fn [a k v] (+ a v)) 0 [1 2]))) (recovered)";
    let program = analyze_with_macros(recovery, &parse(recovery).unwrap(), &mut reused).unwrap();
    let directory = root.join("recovery");
    fs::create_dir(&directory).unwrap();
    fs::write(directory.join("source.cljs"), recovery).unwrap();
    let unit = emit_js_unit(&program);
    fs::write(directory.join("unit.js"), &unit).unwrap();
    execute(&unit, "\"3\"", &directory);
}
