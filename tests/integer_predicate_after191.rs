//! Diagnostic only: exact original whole mixed table with audited Closure embedding.
use light_cljs::{MacroHost, compile_namespace_with_macros};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};
const TEST_ID: &str = "integer-predicate-after191";
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
#[test]
#[ignore = "explicit audited genuine Closure dependency input"]
fn whole_original_integer_table_is_observed_in_actual_hosts() {
    let root = PathBuf::from(std::env::var("LIGHT_INTEGER_PREDICATE_AFTER191").unwrap());
    let mut reused = MacroHost::new();
    let warmup = "(defmacro integer-ready [] (+ 20 22)) (integer-ready)";
    let warmup_js = light_cljs::compile_with_macros(warmup, &mut reused).unwrap();
    fs::write(root.join("warmup.cljs"), warmup).unwrap();
    fs::write(root.join("warmup.js"), warmup_js).unwrap();
    for lifecycle in ["fresh", "reused"] {
        for phase in ["application", "macro"] {
            let folder = root.join(lifecycle).join(phase);
            fs::create_dir_all(&folder).unwrap();
            let source = fs::read_to_string(root.join(format!("{phase}.cljs"))).unwrap();
            fs::write(folder.join("integer_predicate_after191.cljs"), &source).unwrap();
            let mut fresh = MacroHost::new();
            let host = if lifecycle == "fresh" {
                &mut fresh
            } else {
                &mut reused
            };
            let js = match compile_namespace_with_macros(
                "integer-predicate-after191",
                std::slice::from_ref(&folder),
                host,
            ) {
                Ok(js) => js,
                Err(e) => {
                    fs::write(folder.join("compile-error.txt"), e.to_string()).unwrap();
                    println!("integer-after191-rejection\t{lifecycle}\t{phase}\t{e}");
                    continue;
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
            println!("integer-after191-observation\t{lifecycle}\t{phase}\tQuickJS\t{qjs}");
            let node_input = format!("console.log({script});");
            fs::write(folder.join("node.stdin"), &node_input).unwrap();
            let raw = node(&node_input);
            fs::write(folder.join("node.stdout"), &raw).unwrap();
            println!(
                "integer-after191-observation\t{lifecycle}\t{phase}\tNode\t{}",
                raw.trim_end()
            );
        }
    }
}
