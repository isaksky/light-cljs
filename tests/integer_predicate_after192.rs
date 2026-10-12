//! Acceptance: exact original whole mixed table with audited Closure embedding.
use light_cljs::{MacroHost, compile_namespace_with_macros};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};
const TEST_ID: &str = "integer-predicate-after192";
fn node(input: &str) -> String {
    let mut child = Command::new("node")
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
        .write_all(input.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    if !out.status.success() {
        return format!("ERROR {}", String::from_utf8_lossy(&out.stderr));
    }
    assert!(out.stderr.is_empty(), "unexpected Node stderr");
    String::from_utf8(out.stdout).unwrap()
}
fn assert_complete_table(packet: &str) {
    let runtime = rquickjs::Runtime::new().unwrap();
    let context = rquickjs::Context::full(&runtime).unwrap();
    let passed = context.with(|ctx| {
        ctx.eval::<bool, _>(format!(
            "(() => {{const packet=JSON.parse({packet:?}); const rows=packet.outcome.value.$vector[1].$vector; return packet.outcome.status==='returned' && rows.length===112 && rows.every(row=>row.$vector[1]===true);}})()"
        )).unwrap()
    });
    assert!(passed, "all 112 original comparisons must pass: {packet}");
}

#[test]
#[ignore = "explicit audited genuine Closure dependency input"]
fn whole_original_integer_table_passes_in_actual_hosts() {
    let root = PathBuf::from(std::env::var("LIGHT_INTEGER_PREDICATE_AFTER192").unwrap());
    let mut reused = MacroHost::new();
    // A located receiver failure must leave the existing macro host reusable.
    let invalid = "(defmacro invalid-integer [] (int? (throw (js/Error. \"integer recovery\")))) (invalid-integer)";
    let error = light_cljs::compile_with_macros(invalid, &mut reused).unwrap_err();
    assert!(error.to_string().contains("integer recovery"), "{error}");
    fs::write(root.join("recovery.cljs"), invalid).unwrap();
    fs::write(root.join("recovery.error"), error.to_string()).unwrap();
    let warmup = "(defmacro integer-ready [] (+ 20 22)) (integer-ready)";
    let warmup_js = light_cljs::compile_with_macros(warmup, &mut reused).unwrap();
    fs::write(root.join("warmup.cljs"), warmup).unwrap();
    fs::write(root.join("warmup.js"), warmup_js).unwrap();
    for lifecycle in ["fresh", "reused"] {
        for phase in ["application", "macro"] {
            let folder = root.join(lifecycle).join(phase);
            fs::create_dir_all(&folder).unwrap();
            let source = fs::read_to_string(root.join(format!("{phase}.cljs"))).unwrap();
            fs::write(folder.join("integer_predicate_after192.cljs"), &source).unwrap();
            let mut fresh = MacroHost::new();
            let host = if lifecycle == "fresh" {
                &mut fresh
            } else {
                &mut reused
            };
            let js = match compile_namespace_with_macros(
                "integer-predicate-after192",
                std::slice::from_ref(&folder),
                host,
            ) {
                Ok(js) => js,
                Err(e) => {
                    fs::write(folder.join("compile-error.txt"), e.to_string()).unwrap();
                    println!("integer-after192-rejection\t{lifecycle}\t{phase}\t{e}");
                    panic!("complete integer acceptance must compile: {e}");
                }
            };
            fs::write(folder.join("compiled.js"), &js).unwrap();
            let anchor = " = // Original bounded runtime";
            assert_eq!(js.matches(anchor).count(), 1);
            let captured = js.replacen(
                anchor,
                " = globalThis.lightRuntime = // Original bounded runtime",
                1,
            );
            let script = format!(
                "(() => {{const run={captured};return ({})(globalThis.lightRuntime)('{TEST_ID}',observe=>run(observe));}})()",
                include_str!("support/sci_execution_observation.js")
            );
            fs::write(folder.join("script.js"), &script).unwrap();
            let input = format!("(0, eval)({script:?})");
            fs::write(folder.join("quickjs.stdin"), &input).unwrap();
            let runtime = rquickjs::Runtime::new().unwrap();
            let context = rquickjs::Context::full(&runtime).unwrap();
            let qjs = context.with(|ctx| match ctx.eval::<String, _>(input.as_bytes()) {
                Ok(value) => value,
                Err(e) => format!("ERROR {e:?} {:?}", ctx.catch()),
            });
            fs::write(folder.join("quickjs.stdout"), &qjs).unwrap();
            println!("integer-after192-observation\t{lifecycle}\t{phase}\tQuickJS\t{qjs}");
            assert_complete_table(&qjs);
            let node_input = format!("console.log({script});");
            fs::write(folder.join("node.stdin"), &node_input).unwrap();
            let raw = node(&node_input);
            fs::write(folder.join("node.stdout"), &raw).unwrap();
            assert_complete_table(&raw);
            println!(
                "integer-after192-observation\t{lifecycle}\t{phase}\tNode\t{}",
                raw.trim_end()
            );
        }
    }
}
