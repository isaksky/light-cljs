//! Explicit genuine Closure library profile, preserving the entire import body.
use light_cljs::MacroHost;
use std::{fs, io::Write, path::PathBuf};
const ID: &str = "cljs-hosted-original-import-context";
const ENTRY: &str = "cljs.import-test";
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
            .chunks_exact(2)
            .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
            .collect(),
    )
    .unwrap()
}
#[test]
#[ignore = "explicit complete original import body with genuine audited Closure bytes"]
fn original_import_body_executes_in_fresh_and_reused_hosts() {
    let request =
        fs::read_to_string(std::env::var("LIGHT_CLJS_HOSTED_IMPORT_INPUT").unwrap()).unwrap();
    let rows: Vec<_> = request.lines().collect();
    assert_eq!(rows.len(), 1);
    let fields: Vec<_> = rows[0].split('\t').collect();
    assert_eq!(fields.len(), 5);
    assert_eq!(fields[0], ID);
    assert_eq!(fields[1], ENTRY);
    let original = unhex(fields[2]);
    let loader = unhex(fields[3]);
    let macro_source = unhex(fields[4]);
    assert!(original.contains("(:import goog.math.Long"));
    assert_eq!(original.matches("(assert ").count(), 6);
    assert!(loader.starts_with("(function(){globalThis.CLOSURE_NO_DEPS = true;"));
    let root = PathBuf::from(std::env::var("LIGHT_CLJS_HOSTED_IMPORT_ROOT").unwrap());
    assert!(!root.exists(), "fresh artifact root required");
    let mut output =
        fs::File::create(std::env::var("LIGHT_CLJS_HOSTED_IMPORT_OUTPUT").unwrap()).unwrap();
    let mut reused = MacroHost::new();
    let warmup = light_cljs::compile_with_macros(
        "(defmacro hosted-import-warmup [] true) (hosted-import-warmup)",
        &mut reused,
    )
    .unwrap();
    for (phase, reuse) in [
        ("application", false),
        ("macro", false),
        ("application", true),
        ("macro", true),
    ] {
        let variant = format!("{phase}-{}", if reuse { "reused" } else { "fresh" });
        let source = if phase == "macro" {
            &macro_source
        } else {
            &original
        };
        let variant_root = root.join(&variant);
        fs::create_dir_all(variant_root.join("cljs")).unwrap();
        fs::write(variant_root.join("cljs/import_test.cljs"), source).unwrap();
        let mut fresh = MacroHost::new();
        let javascript = light_cljs::compile_namespace_with_macros(
            ENTRY,
            &[variant_root],
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
        // The audited exact dependency files and source are pure at module
        // initialization. No arbitrary namespace/Closure profile is accepted.
        let script = format!(
            "(() => {{ {loader}; const run = {captured}; return ({})(globalThis.lightRuntime)('{ID}',observe => run(observe)); }})()",
            include_str!("support/sci_execution_observation.js")
        );
        let runtime = rquickjs::Runtime::new().unwrap();
        runtime.set_memory_limit(128 * 1024 * 1024);
        runtime.set_max_stack_size(4 * 1024 * 1024);
        let context = rquickjs::Context::full(&runtime).unwrap();
        let observed = context.with(|ctx| ctx.eval::<String, _>(script.as_bytes()).unwrap());
        writeln!(
            output,
            "{variant}\t{}\t{}\t{}\t{}",
            hex(source),
            hex(&javascript),
            hex(&observed),
            if reuse { hex(&warmup) } else { "-".into() }
        )
        .unwrap();
    }
}
