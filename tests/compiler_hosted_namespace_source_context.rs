//! Scoped real goog delegation and exact upstream Error->map source providers.
use light_cljs::MacroHost;
use std::{fs, io::Write, path::PathBuf};
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
#[test]
#[ignore = "explicit exact original bodies with scoped real source providers"]
fn original_namespace_bodies_execute_in_fresh_and_reused_hosts() {
    let input =
        fs::read_to_string(std::env::var("LIGHT_CLJS_HOSTED_NAMESPACE_INPUT").unwrap()).unwrap();
    let rows: Vec<_> = input.lines().collect();
    assert_eq!(rows.len(), 2, "exact two reviewed profiles");
    let root = PathBuf::from(std::env::var("LIGHT_CLJS_HOSTED_NAMESPACE_ROOT").unwrap());
    assert!(!root.exists());
    let mut output =
        fs::File::create(std::env::var("LIGHT_CLJS_HOSTED_NAMESPACE_OUTPUT").unwrap()).unwrap();
    let mut reused = MacroHost::new();
    let warmup = light_cljs::compile_with_macros(
        "(defmacro hosted-namespace-warmup [] true) (hosted-namespace-warmup)",
        &mut reused,
    )
    .unwrap();
    for (index, row) in rows.iter().enumerate() {
        let fields: Vec<_> = row.split('\t').collect();
        assert_eq!(fields.len(), 7);
        let (id, entry, path) = if index == 0 {
            (
                "cljs-hosted-original-goog-context",
                "cljs.ns-test",
                "goog.cljs",
            )
        } else {
            (
                "cljs-hosted-original-repl-context",
                "cljs.repl-test",
                "cljs/repl.cljs",
            )
        };
        assert_eq!(fields[0], id);
        assert_eq!(fields[1], entry);
        assert_eq!(fields[4], path);
        let original = unhex(fields[2]);
        let macro_source = unhex(fields[3]);
        let provider = unhex(fields[5]);
        let loader = unhex(fields[6]);
        if index == 0 {
            assert_eq!(
                provider,
                "(ns goog)\n(defn isArrayLike [x] (js/goog.isArrayLike x))\n"
            );
            assert_eq!(original.matches("(assert ").count(), 3);
        } else {
            assert!(provider.starts_with("(ns cljs.repl)\n(defn Error->map"));
            assert_eq!(original.matches("(assert ").count(), 2);
            assert_eq!(loader, "true");
        }
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
            let variant_root = root.join(id).join(&variant);
            fs::create_dir_all(variant_root.join("cljs")).unwrap();
            fs::write(variant_root.join(path), &provider).unwrap();
            fs::write(
                variant_root.join(entry.replace('.', "/").replace('-', "_") + ".cljs"),
                source,
            )
            .unwrap();
            let mut fresh = MacroHost::new();
            let javascript = light_cljs::compile_namespace_with_macros(
                entry,
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
            let script = format!(
                "(() => {{ {loader}; const run = {captured}; return ({})(globalThis.lightRuntime)('{id}',observe => run(observe)); }})()",
                include_str!("support/sci_execution_observation.js")
            );
            let runtime = rquickjs::Runtime::new().unwrap();
            runtime.set_memory_limit(128 * 1024 * 1024);
            runtime.set_max_stack_size(4 * 1024 * 1024);
            let context = rquickjs::Context::full(&runtime).unwrap();
            let observed = context.with(|ctx| ctx.eval::<String, _>(script.as_bytes()).unwrap());
            writeln!(
                output,
                "{id}\t{variant}\t{}\t{}\t{}\t{}",
                hex(source),
                hex(&javascript),
                hex(&observed),
                if reuse { hex(&warmup) } else { "-".into() }
            )
            .unwrap();
        }
    }
}
