//! Explicit configured-reader embedding audit; this does not exercise the CLI.
use light_cljs::{MacroHost, compiler_data::Name};
use std::{fs, io::Write, path::PathBuf};

const ID: &str = "cljs-residual-source-context-tagged-source";
const ENTRY: &str = "cljs.tagged-literals-test";
const ORIGINAL: &str = include_str!(
    "fixtures/compiler-residual-source-context/tagged-source/cljs/tagged_literals_test.cljs"
);
const WARMUP: &str = "(defmacro configured-reader-warmup [] true) (configured-reader-warmup)";

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
            .as_chunks::<2>()
            .0
            .iter()
            .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
            .collect(),
    )
    .unwrap()
}

#[test]
#[ignore = "explicit original configured source-reader embedding audit"]
fn configured_original_body_executes_in_fresh_and_reused_hosts() {
    let input =
        fs::read_to_string(std::env::var("LIGHT_CLJS_CONFIGURED_SOURCE_INPUT").unwrap()).unwrap();
    let rows: Vec<_> = input.lines().collect();
    assert_eq!(rows.len(), 1, "exact reviewed profile only");
    let fields: Vec<_> = rows[0].split('\t').collect();
    assert_eq!(fields.len(), 5);
    assert_eq!(fields[0], ID);
    assert_eq!(fields[1], ENTRY);
    assert_eq!(unhex(fields[2]), ORIGINAL);
    assert_eq!(fields[3], "cljs/tag");
    assert_eq!(fields[4], "identity");
    let root = PathBuf::from(std::env::var("LIGHT_CLJS_CONFIGURED_SOURCE_ROOT").unwrap());
    assert!(!root.exists(), "fresh artifact root required");
    let mut output =
        fs::File::create(std::env::var("LIGHT_CLJS_CONFIGURED_SOURCE_OUTPUT").unwrap()).unwrap();
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
            let (prefix, body) = ORIGINAL.split_once("\n(defn run [] ").unwrap();
            format!("{prefix}\n(defmacro upstream [] {body}\n(defn run [] (upstream))\n")
        } else {
            ORIGINAL.to_owned()
        };
        let variant_root = root.join(&variant);
        fs::create_dir_all(variant_root.join("cljs")).unwrap();
        fs::write(variant_root.join("cljs/tagged_literals_test.cljs"), &source).unwrap();
        let mut fresh = MacroHost::new();
        let host = if reuse { &mut reused } else { &mut fresh };
        host.register_compiler_data_reader(Name::new(Some("cljs"), "tag"), "identity")
            .unwrap();
        let javascript =
            light_cljs::compile_namespace_with_macros(ENTRY, &[variant_root], host).unwrap();
        let anchor = " = // Original bounded runtime";
        assert_eq!(
            javascript.matches(anchor).count(),
            1,
            "runtime observation anchor drift"
        );
        let captured = javascript.replacen(
            anchor,
            " = globalThis.lightRuntime = // Original bounded runtime",
            1,
        );
        // The exact audited profile has no effectful top-level setup. Its sole
        // original assertion executes in run, or in upstream in the MacroHost.
        let script = format!(
            "(() => {{ const run = {captured}; return ({})(globalThis.lightRuntime)('{ID}', observe => run(observe)); }})()",
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
            hex(&source),
            hex(&javascript),
            hex(&observed),
            if reuse { hex(&warmup) } else { "-".into() }
        )
        .unwrap();
    }
}
