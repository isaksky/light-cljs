//! Canonical case character/string constants and exact original phase evidence.
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
            .chunks_exact(2)
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
fn accepted_case_constants_execute_in_fresh_and_reused_macro_hosts() {
    let root = PathBuf::from(std::env::var("LIGHT_SCI_CLJC_ACCEPTED_ROOT").unwrap());
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
                    &[sources.clone()],
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
    assert_eq!(input.lines().count(), 10);
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
                let error =
                    light_cljs::compile_namespace_with_macros("user", &[folder.clone()], host)
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
fn canonical_case_constants_and_same_host_diagnostic_recovery() {
    let mut host = MacroHost::new();
    let controls: &[(&str, &str, &str)] = &[
        (
            r###"sci-execution-sci-control-case-char-string"###,
            r###"[(case \a "a" 1 \a 2 3) (case "a" \a 1 "a" 2 3)]"###,
            r###"{"id":"sci-execution-sci-control-case-char-string","outcome":{"status":"returned","value":{"$vector":[1,1]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"case-control-characters"###,
            r###"[(case \a \a 1 \b 2 3) (case \z \a 1 \b 2 3)]"###,
            r###"{"id":"case-control-characters","outcome":{"status":"returned","value":{"$vector":[1,3]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"case-control-strings"###,
            r###"[(case "a" "a" 1 "b" 2 3) (case "z" "a" 1 "b" 2 3)]"###,
            r###"{"id":"case-control-strings","outcome":{"status":"returned","value":{"$vector":[1,3]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"case-control-mixed-grouped-default"###,
            r###"[(case \a ("a" \a) 1 ("b" \b) 2 3) (case "b" ("a" \a) 1 ("b" \b) 2 3) (case \z ("a" \a) 1 ("b" \b) 2 3)]"###,
            r###"{"id":"case-control-mixed-grouped-default","outcome":{"status":"returned","value":{"$vector":[1,2,3]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"case-control-selector-branch-effects"###,
            r###"(let [events (atom []) result (case (do (swap! events conj :selector) \a) "a" (do (swap! events conj :first) 11) \a (do (swap! events conj :second) 22) (do (swap! events conj :default) 33))] [result @events])"###,
            r###"{"id":"case-control-selector-branch-effects","outcome":{"status":"returned","value":{"$vector":[11,{"$vector":[{"$keyword":{"namespace":null,"name":"selector","spelling":"selector"}},{"$keyword":{"namespace":null,"name":"first","spelling":"first"}}]}]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"case-control-default-effects"###,
            r###"(let [events (atom []) result (case (do (swap! events conj :selector) \z) "a" (do (swap! events conj :first) 11) \a (do (swap! events conj :second) 22) (do (swap! events conj :default) 33))] [result @events])"###,
            r###"{"id":"case-control-default-effects","outcome":{"status":"returned","value":{"$vector":[33,{"$vector":[{"$keyword":{"namespace":null,"name":"selector","spelling":"selector"}},{"$keyword":{"namespace":null,"name":"default","spelling":"default"}}]}]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"case-control-nested-vector-list"###,
            r###"[(case [[\a]] [["a"]] 1 [[\a]] 2 3) (case '(\a) ["a"] 1 [\a] 2 3)]"###,
            r###"{"id":"case-control-nested-vector-list","outcome":{"status":"returned","value":{"$vector":[1,1]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"case-control-map-set-constants"###,
            r###"[(case {:x \a} {:x "a"} 1 {:x \a} 2 3) (case #{\a} #{"a"} 1 #{\a} 2 3)]"###,
            r###"{"id":"case-control-map-set-constants","outcome":{"status":"returned","value":{"$vector":[1,1]}},"trace":[],"stdout":""}"###,
        ),
        (
            r###"case-control-unicode-character"###,
            r###"[(case \u0061 "a" 1 \a 2 3) (case \a \u0061 7 9)]"###,
            r###"{"id":"case-control-unicode-character","outcome":{"status":"returned","value":{"$vector":[1,7]}},"trace":[],"stdout":""}"###,
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
    let diagnostics: &[(&str, &str)] = &[
        (
            r###"(case 1 1 :integer 1.0 :float)"###,
            "duplicate case test constant",
        ),
        (
            r###"(case \a \a 1 \a 2 3)"###,
            r###"duplicate case test constant"###,
        ),
        (
            r###"(case \a \a 1 \u0061 2 3)"###,
            r###"duplicate case test constant"###,
        ),
        (
            r###"(case "a" "a" 1 "a" 2 3)"###,
            r###"duplicate case test constant"###,
        ),
        (
            r###"(case \a (\a \b) 1 \b 2 3)"###,
            r###"duplicate case test constant"###,
        ),
        (
            r###"(case* 1 [[1]] [2] 3)"###,
            r###"case* must switch on a symbol"###,
        ),
        (
            r###"(let [x 1] (case* x [1] [2] 3))"###,
            r###"case* requires constant groups"###,
        ),
        (
            r###"(let [x 1] (case* x [[:a]] [2] 3))"###,
            r###"case* tests must be numbers, strings, or constants"###,
        ),
    ];
    for (source, diagnostic) in diagnostics {
        for phase in [false, true] {
            let input = if phase {
                format!("(defmacro upstream [] (list 'quote (do {source}))) (upstream)")
            } else {
                source.to_string()
            };
            let error = light_cljs::compile_with_macros(&input, &mut host)
                .unwrap_err()
                .to_string();
            assert!(error.contains(diagnostic), "{error}");
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
