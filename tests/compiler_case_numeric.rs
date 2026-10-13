//! Canonical numeric case constants and exact original phase evidence.
use light_cljs::MacroHost;
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};
fn hex(s: &str) -> String {
    s.bytes().map(|b| format!("{b:02x}")).collect()
}
fn unhex(s: &str) -> String {
    String::from_utf8(
        s.as_bytes()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
            .collect(),
    )
    .unwrap()
}
fn observe(script: &str, artifact: &std::path::Path) -> [(String, String); 2] {
    let runtime = rquickjs::Runtime::new().unwrap();
    let context = rquickjs::Context::full(&runtime).unwrap();
    let quickjs = context.with(|ctx| match ctx.eval::<String, _>(script.as_bytes()) {
        Ok(value) => value,
        Err(error) => format!("HARNESS_ERROR:{error:?}: {:?}", ctx.catch()),
    });
    let mut child = Command::new(std::env::var("NODE").unwrap())
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
    let out = child.wait_with_output().unwrap();
    fs::write(artifact.join("script.js"), script).unwrap();
    fs::write(artifact.join("quickjs.raw"), &quickjs).unwrap();
    fs::write(
        artifact.join("node.stdin"),
        format!("console.log({script});"),
    )
    .unwrap();
    fs::write(artifact.join("node.stdout"), &out.stdout).unwrap();
    fs::write(artifact.join("node.stderr"), &out.stderr).unwrap();
    fs::write(
        artifact.join("node.exit"),
        out.status.code().unwrap_or(-1).to_string(),
    )
    .unwrap();
    [
        ("QuickJS".into(), quickjs),
        (
            "Node".into(),
            if out.status.success() && out.stderr.is_empty() {
                String::from_utf8(out.stdout).unwrap().trim().into()
            } else {
                format!("HARNESS_ERROR:{}", String::from_utf8_lossy(&out.stderr))
            },
        ),
    ]
}
#[test]
#[ignore = "explicit original .cljc filename and actual phase audit"]
fn numeric_case_constants_execute_in_fresh_and_reused_macro_hosts() {
    let root = PathBuf::from(std::env::var("LIGHT_CASE_NUMERIC_ROOT").unwrap());
    let input = fs::read_to_string(root.join("input.tsv")).unwrap();
    let mut out = fs::File::create(root.join("observations.tsv")).unwrap();
    let mut reused = MacroHost::new();
    let warmup = light_cljs::compile_with_macros(
        "(defmacro cljc-ready [] (+ 20 22)) (cljc-ready)",
        &mut reused,
    )
    .unwrap();
    fs::write(root.join("warmup.js"), &warmup).unwrap();
    for row in input.lines() {
        let fields: Vec<_> = row.split('\t').collect();
        assert_eq!(fields.len(), 2);
        let (id, original) = (fields[0], unhex(fields[1]));
        for reuse in [false, true] {
            for phase in [false, true] {
                let variant = format!(
                    "{}-{}",
                    if phase { "macro" } else { "application" },
                    if reuse { "reused" } else { "fresh" }
                );
                let source = if phase {
                    format!(
                        "(ns user)\n(defmacro upstream [] (list 'quote (do {original})))\n(defn run [observe] (upstream))\nrun\n"
                    )
                } else {
                    format!("(ns user)\n(defn run [observe] (do {original}))\nrun\n")
                };
                let sources = root.join(id).join(&variant);
                fs::create_dir_all(&sources).unwrap();
                fs::write(sources.join("user.cljc"), &source).unwrap();
                let mut fresh = MacroHost::new();
                match light_cljs::compile_namespace_with_macros(
                    "user",
                    std::slice::from_ref(&sources),
                    if reuse { &mut reused } else { &mut fresh },
                ) {
                    Err(e) => {
                        fs::write(sources.join("rejection.txt"), e.to_string()).unwrap();
                        writeln!(
                            out,
                            "{id}\t{variant}\trejected\t{}\t{}",
                            hex(&source),
                            hex(&e.to_string())
                        )
                        .unwrap();
                    }
                    Ok(js) => {
                        let anchor = " = // Original bounded runtime";
                        assert_eq!(js.matches(anchor).count(), 1);
                        let captured = js.replacen(
                            anchor,
                            " = globalThis.lightRuntime = // Original bounded runtime",
                            1,
                        );
                        let script = format!(
                            "(() => {{const run={captured};return ({})(globalThis.lightRuntime)('{id}',observe=>run(observe));}})()",
                            include_str!("support/sci_execution_observation.js")
                        );
                        fs::write(sources.join("compiled.js"), &js).unwrap();
                        for (engine, raw) in observe(&script, &sources) {
                            writeln!(
                                out,
                                "{id}\t{variant}\t{engine}\t{}\t{}\t{}\t{}",
                                hex(&source),
                                hex(&js),
                                hex(&raw),
                                if reuse { hex(&warmup) } else { "-".into() }
                            )
                            .unwrap();
                        }
                    }
                }
            }
        }
    }
    assert_eq!(input.lines().count(), 16);
    out.flush().unwrap();
    let negatives = root.parent().unwrap().join("diagnostics");
    let input = fs::read_to_string(negatives.join("input.tsv")).unwrap();
    let mut output = fs::File::create(negatives.join("observations.tsv")).unwrap();
    for row in input.lines() {
        let fields: Vec<_> = row.split('\t').collect();
        let (id, original) = (fields[0], unhex(fields[1]));
        for reuse in [false, true] {
            for phase in [false, true] {
                let variant = format!(
                    "{}-{}",
                    if phase { "macro" } else { "application" },
                    if reuse { "reused" } else { "fresh" }
                );
                let source = if phase {
                    format!(
                        "(ns user)\n(defmacro upstream [] (list 'quote (do {original})))\n(defn run [observe] (upstream))\nrun\n"
                    )
                } else {
                    format!("(ns user)\n(defn run [observe] (do {original}))\nrun\n")
                };
                let folder = negatives.join(id).join(&variant);
                fs::create_dir_all(&folder).unwrap();
                fs::write(folder.join("user.cljc"), &source).unwrap();
                let mut fresh = MacroHost::new();
                let host = if reuse { &mut reused } else { &mut fresh };
                let error = light_cljs::compile_namespace_with_macros(
                    "user",
                    std::slice::from_ref(&folder),
                    host,
                )
                .unwrap_err()
                .to_string();
                fs::write(folder.join("rejection.txt"), &error).unwrap();
                let recovery = "(defmacro case-recovery [] (+ 20 22)) (case-recovery)";
                fs::write(folder.join("recovery.cljs"), recovery).unwrap();
                let js = light_cljs::compile_with_macros(recovery, host).unwrap();
                fs::write(folder.join("recovery.js"), &js).unwrap();
                let script = format!("JSON.stringify({js})");
                let observed = observe(&script, &folder);
                for (engine, raw) in observed {
                    assert_eq!(raw, "42", "{id} {variant} {engine}: recovery");
                    writeln!(
                        output,
                        "{id}\t{variant}\t{engine}\t{}\t{}\t{}\t{}",
                        hex(&source),
                        hex(&error),
                        hex(&js),
                        hex(&raw)
                    )
                    .unwrap();
                }
            }
        }
    }
    output.flush().unwrap();
}

fn canonical(context: &rquickjs::Context, value: &str) -> String {
    let script = format!(
        "JSON.stringify((function sort(v) {{ if(Array.isArray(v)) return v.map(sort); if(v && typeof v==='object') {{ const out={{}}; Object.keys(v).sort().forEach(k=>out[k]=sort(v[k])); return out; }} return v; }})(JSON.parse({value:?})))"
    );
    context.with(|ctx| ctx.eval::<String, _>(script.as_bytes()).unwrap())
}

#[test]
fn canonical_numeric_reader_identity_and_same_host_recovery() {
    let mut host = MacroHost::new();
    let controls: &[(&str, &str, &str)] = &[
        (
            r###"numeric-case-integer-float"###,
            r###"(case 1 1 :integer 1.0 :float)"###,
            r###"{"id":"numeric-case-integer-float","outcome":{"status":"returned","value":{"$keyword":{"namespace":null,"name":"integer","spelling":"integer"}}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"numeric-case-float-first"###,
            r###"(case 1 1.0 :float 1 :integer)"###,
            r###"{"id":"numeric-case-float-first","outcome":{"status":"returned","value":{"$keyword":{"namespace":null,"name":"float","spelling":"float"}}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"numeric-case-reader-spellings"###,
            r###"[(case 1 0x1 :hex 1.0 :float) (case 1 01 :octal 1e0 :float) (case 1 1N :bigint 1.0 :float) (case 1 2/2 :ratio 1.0 :float)]"###,
            r###"{"id":"numeric-case-reader-spellings","outcome":{"status":"returned","value":{"$vector":[{"$keyword":{"namespace":null,"name":"hex","spelling":"hex"}},{"$keyword":{"namespace":null,"name":"octal","spelling":"octal"}},{"$keyword":{"namespace":null,"name":"bigint","spelling":"bigint"}},{"$keyword":{"namespace":null,"name":"ratio","spelling":"ratio"}}]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"numeric-case-decimal"###,
            r###"(case 1 1 :integer 1M :decimal 1.0 :float)"###,
            r###"{"id":"numeric-case-decimal","outcome":{"status":"returned","value":{"$keyword":{"namespace":null,"name":"integer","spelling":"integer"}}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"numeric-case-grouped"###,
            r###"[(case 1 (1 1.0) :first 2 :second :default) (case 2 (1 1.0) :first (2 2.0) :second :default) (case 3 (1 1.0) :first :default)]"###,
            r###"{"id":"numeric-case-grouped","outcome":{"status":"returned","value":{"$vector":[{"$keyword":{"namespace":null,"name":"first","spelling":"first"}},{"$keyword":{"namespace":null,"name":"second","spelling":"second"}},{"$keyword":{"namespace":null,"name":"default","spelling":"default"}}]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"numeric-case-nested"###,
            r###"[(case [1] [1] :integer [1.0] :float) (case [[1]] [[1.0]] :float [[1]] :integer) (case '(1) [1.0] :float [1] :integer)]"###,
            r###"{"id":"numeric-case-nested","outcome":{"status":"returned","value":{"$vector":[{"$keyword":{"namespace":null,"name":"integer","spelling":"integer"}},{"$keyword":{"namespace":null,"name":"float","spelling":"float"}},{"$keyword":{"namespace":null,"name":"float","spelling":"float"}}]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"numeric-case-map-set"###,
            r###"[(case {:x 1} {:x 1} :integer {:x 1.0} :float) (case #{1} #{1.0} :float #{1} :integer)]"###,
            r###"{"id":"numeric-case-map-set","outcome":{"status":"returned","value":{"$vector":[{"$keyword":{"namespace":null,"name":"integer","spelling":"integer"}},{"$keyword":{"namespace":null,"name":"float","spelling":"float"}}]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"numeric-case-nonfinite"###,
            r###"[(case ##Inf ##Inf :infinity ##-Inf :negative :default) (case ##-Inf ##Inf :infinity ##-Inf :negative :default) (case ##NaN ##NaN :nan :default)]"###,
            r###"{"id":"numeric-case-nonfinite","outcome":{"status":"returned","value":{"$vector":[{"$keyword":{"namespace":null,"name":"infinity","spelling":"infinity"}},{"$keyword":{"namespace":null,"name":"negative","spelling":"negative"}},{"$keyword":{"namespace":null,"name":"default","spelling":"default"}}]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"numeric-case-rounding"###,
            r###"[(case 9007199254740992 9007199254740992 :first 9007199254740993 :second) (case 9007199254740993 9007199254740993 :first 9007199254740992 :second) (case 9007199254740992 9007199254740992.0 :float 9007199254740993 :integer)]"###,
            r###"{"id":"numeric-case-rounding","outcome":{"status":"returned","value":{"$vector":[{"$keyword":{"namespace":null,"name":"first","spelling":"first"}},{"$keyword":{"namespace":null,"name":"first","spelling":"first"}},{"$keyword":{"namespace":null,"name":"float","spelling":"float"}}]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"numeric-case-effects"###,
            r###"(let [events (atom []) result (case (do (swap! events conj :selector) 1) 1 (do (swap! events conj :integer) 11) 1.0 (do (swap! events conj :float) 22) (do (swap! events conj :default) 33))] [result @events])"###,
            r###"{"id":"numeric-case-effects","outcome":{"status":"returned","value":{"$vector":[11,{"$vector":[{"$keyword":{"namespace":null,"name":"selector","spelling":"selector"}},{"$keyword":{"namespace":null,"name":"integer","spelling":"integer"}}]}]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"numeric-case-default-effects"###,
            r###"(let [events (atom []) result (case (do (swap! events conj :selector) 3) 1 (do (swap! events conj :integer) 11) 1.0 (do (swap! events conj :float) 22) (do (swap! events conj :default) 33))] [result @events])"###,
            r###"{"id":"numeric-case-default-effects","outcome":{"status":"returned","value":{"$vector":[33,{"$vector":[{"$keyword":{"namespace":null,"name":"selector","spelling":"selector"}},{"$keyword":{"namespace":null,"name":"default","spelling":"default"}}]}]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"numeric-case-zero-integer-float"###,
            r###"[(case 0 0 :integer -0.0 :float) (case -0.0 -0.0 :float 0 :integer)]"###,
            r###"{"id":"numeric-case-zero-integer-float","outcome":{"status":"returned","value":{"$vector":[{"$keyword":{"namespace":null,"name":"integer","spelling":"integer"}},{"$keyword":{"namespace":null,"name":"float","spelling":"float"}}]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"numeric-case-nan-failure"###,
            r###"(try (case ##NaN ##NaN :a ##NaN :b) (catch :default e :missing))"###,
            r###"{"id":"numeric-case-nan-failure","outcome":{"status":"returned","value":{"$keyword":{"namespace":null,"name":"missing","spelling":"missing"}}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"numeric-case-decimal-rounding"###,
            r###"(case 1 1.0000000000000001M :precise 1M :rounded)"###,
            r###"{"id":"numeric-case-decimal-rounding","outcome":{"status":"returned","value":{"$keyword":{"namespace":null,"name":"precise","spelling":"precise"}}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"numeric-case-nested-nan"###,
            r###"(case [##NaN] [##NaN] :a [##NaN] :b :default)"###,
            r###"{"id":"numeric-case-nested-nan","outcome":{"status":"returned","value":{"$keyword":{"namespace":null,"name":"default","spelling":"default"}}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"numeric-case-wide-integer"###,
            r###"(case 9223372036854775808N 9223372036854775808N :first 9223372036854775809N :second)"###,
            r###"{"id":"numeric-case-wide-integer","outcome":{"status":"returned","value":{"$keyword":{"namespace":null,"name":"first","spelling":"first"}}},"trace":[],"stdout":""}"###,
        ),
    ];
    for (id, source, expected) in controls {
        for phase in [false, true] {
            let input = if phase {
                format!("(defmacro upstream [] (list 'quote (do {source}))) (upstream)")
            } else {
                source.to_string()
            };
            let js = light_cljs::compile_with_macros(&input, &mut host)
                .unwrap_or_else(|error| panic!("{id}, macro={phase}: {error}"));
            let captured = js.replacen(
                " = // Original bounded runtime",
                " = globalThis.lightRuntime = // Original bounded runtime",
                1,
            );
            let script = format!(
                "(() => {{const value={captured};return ({})(globalThis.lightRuntime)('{id}',()=>value);}})()",
                include_str!("support/sci_execution_observation.js")
            );
            let runtime = rquickjs::Runtime::new().unwrap();
            let context = rquickjs::Context::full(&runtime).unwrap();
            let raw = context.with(|ctx| ctx.eval::<String, _>(script.as_bytes()).unwrap());
            assert_eq!(
                canonical(&context, &raw),
                canonical(&context, expected),
                "{id}, macro={phase}"
            );
        }
    }
    for source in [
        r###"[(case 0 -0.0 :negative 0.0 :positive 0 :integer) (case -0.0 0.0 :positive -0.0 :negative)]"###,
        r###"(case 1 1 :a 0x1 :b)"###,
        r###"(case 1 1.0 :a 1e0 :b)"###,
        r###"(case 1 1 :a 1N :b)"###,
        r###"(case 1 1 :a 2/2 :b)"###,
        r###"(case 1 (1 2.0) :a 2e0 :b)"###,
        r###"(case [1] [1] :a [0x1] :b)"###,
        r###"(case 0 -0.0 :a -0e0 :b)"###,
        r###"(case ##Inf ##Inf :a ##Inf :b)"###,
        r###"(case 9007199254740992 9007199254740992.0 :a 9007199254740993.0 :b)"###,
        r###"(case 1 1M :a 1.0M :b)"###,
    ] {
        for phase in [false, true] {
            let input = if phase {
                format!("(defmacro upstream [] (list 'quote (do {source}))) (upstream)")
            } else {
                source.to_string()
            };
            let error = light_cljs::compile_with_macros(&input, &mut host)
                .unwrap_err()
                .to_string();
            assert!(
                error.contains("duplicate case test constant"),
                "{source}: {error}"
            );
            let js = light_cljs::compile_with_macros(
                "(defmacro case-recovery [] (+ 20 22)) (case-recovery)",
                &mut host,
            )
            .unwrap();
            let runtime = rquickjs::Runtime::new().unwrap();
            let context = rquickjs::Context::full(&runtime).unwrap();
            assert_eq!(
                context.with(|ctx| ctx.eval::<i32, _>(js.as_bytes()).unwrap()),
                42
            );
        }
    }
}
