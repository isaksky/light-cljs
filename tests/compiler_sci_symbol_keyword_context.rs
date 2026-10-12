//! Exact original SCI symbol/keyword expression in its original user namespace.
//! Application and actual MacroHost observations are independent lifecycle frames.
use light_cljs::MacroHost;
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};
const ID: &str = "sci-execution-sci-symbols-keywords";
const ORIGINAL: &str = "['x 'alpha/x :x :alpha/x ::x]";
const WARMUP: &str = "(defmacro phase-ready [] true) (phase-ready)";
fn hex(value: &str) -> String {
    value
        .as_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn unhex(value: &str) -> String {
    assert_eq!(value.len() % 2, 0);
    String::from_utf8(
        value
            .as_bytes()
            .as_chunks::<2>().0.iter()
            .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
            .collect(),
    )
    .unwrap()
}
fn canonical(context: &rquickjs::Context, value: &str) -> String {
    let script = format!(
        "JSON.stringify((function sort(v) {{ if(Array.isArray(v)) return v.map(sort); if(v && typeof v==='object') {{ const result={{}}; Object.keys(v).sort().forEach(k=>result[k]=sort(v[k])); return result; }} return v; }})(JSON.parse({value:?})))"
    );
    context.with(|ctx| ctx.eval::<String, _>(script.as_bytes()).unwrap())
}

#[test]
#[ignore = "Explicit exact original source and current both-pin provenance helper"]
fn original_symbol_keyword_source_executes_in_both_phases_and_lifecycles() {
    let input =
        fs::read_to_string(std::env::var("LIGHT_SCI_SYMBOL_KEYWORD_INPUT").unwrap()).unwrap();
    let rows: Vec<_> = input.lines().collect();
    assert_eq!(rows.len(), 1);
    let fields: Vec<_> = rows[0].split('\t').collect();
    assert_eq!(fields.len(), 3);
    assert_eq!(fields[0], ID);
    assert_eq!(unhex(fields[1]), ORIGINAL);
    let expected = unhex(fields[2]);
    let root = PathBuf::from(std::env::var("LIGHT_SCI_SYMBOL_KEYWORD_ROOT").unwrap());
    assert!(!root.exists());
    let mut output =
        fs::File::create(std::env::var("LIGHT_SCI_SYMBOL_KEYWORD_OUTPUT").unwrap()).unwrap();
    let mut reused = MacroHost::new();
    let warmup = light_cljs::compile_with_macros(WARMUP, &mut reused).unwrap();
    for (phase, reuse) in [
        ("application", false),
        ("macro", false),
        ("application", true),
        ("macro", true),
    ] {
        let variant = format!("{phase}-{}", if reuse { "reused" } else { "fresh" });
        let source = if phase == "macro" {
            format!(
                "(ns user)\n(defmacro upstream [] (list 'quote {ORIGINAL}))\n(defn run [] (upstream))\n"
            )
        } else {
            format!("(ns user)\n(defn run [] {ORIGINAL})\n")
        };
        let variantroot = root.join(&variant);
        fs::create_dir_all(&variantroot).unwrap();
        fs::write(variantroot.join("user.cljs"), &source).unwrap();
        let mut fresh = MacroHost::new();
        let javascript = light_cljs::compile_namespace_with_macros(
            "user",
            &[variantroot],
            if reuse { &mut reused } else { &mut fresh },
        )
        .unwrap();
        let anchor = " = // Original bounded runtime";
        assert_eq!(javascript.matches(anchor).count(), 1);
        let captured = javascript.replacen(
            anchor,
            " = globalThis.lightRuntime = // Original bounded runtime",
            1,
        );
        let script = format!(
            "(() => {{ const run = {captured}; return ({}) (globalThis.lightRuntime)('{ID}',observe=>run(observe)); }})()",
            include_str!("support/sci_execution_observation.js")
        );
        let runtime = rquickjs::Runtime::new().unwrap();
        runtime.set_memory_limit(128 * 1024 * 1024);
        runtime.set_max_stack_size(4 * 1024 * 1024);
        let context = rquickjs::Context::full(&runtime).unwrap();
        let quickjs = context.with(|ctx| ctx.eval::<String, _>(script.as_bytes()).unwrap());
        assert_eq!(
            canonical(&context, &quickjs),
            canonical(&context, &expected),
            "{variant} QuickJS"
        );
        let mut node = Command::new(std::env::var_os("NODE").unwrap_or_else(|| "node".into()))
            .arg("-")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        node.stdin
            .take()
            .unwrap()
            .write_all(format!("console.log({script});").as_bytes())
            .unwrap();
        let result = node.wait_with_output().unwrap();
        assert!(result.status.success());
        assert!(result.stderr.is_empty());
        let node = String::from_utf8(result.stdout).unwrap();
        assert_eq!(
            canonical(&context, &node),
            canonical(&context, &expected),
            "{variant} Node"
        );
        for (engine, raw) in [("QuickJS", quickjs), ("Node", node)] {
            writeln!(
                output,
                "{variant}\t{engine}\t{}\t{}\t{}\t{}",
                hex(&source),
                hex(&javascript),
                hex(&raw),
                if reuse { hex(&warmup) } else { "-".into() }
            )
            .unwrap();
            output.flush().unwrap();
        }
    }
}
