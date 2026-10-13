//! Canonical map value equality operand direction and actual phase evidence.
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
fn map_value_equiv_sources_execute_in_fresh_and_reused_macro_hosts() {
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
    assert_eq!(input.lines().count(), 32);
    out.flush().unwrap();
}

fn node(script: &str) -> String {
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
        .write_all(format!("console.log({script});").as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        out.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

fn canonical(context: &rquickjs::Context, value: &str) -> String {
    let script = format!(
        "JSON.stringify((function sort(v) {{ if(Array.isArray(v)) return v.map(sort); if(v && typeof v==='object') {{ const out={{}}; Object.keys(v).sort().forEach(k=>out[k]=sort(v[k])); return out; }} return v; }})(JSON.parse({value:?})))"
    );
    context.with(|ctx| ctx.eval::<String, _>(script.as_bytes()).unwrap())
}

#[test]
fn map_value_equiv_preserves_exact_contracts_and_same_host_recovery() {
    let runtime = rquickjs::Runtime::new().unwrap();
    let context = rquickjs::Context::full(&runtime).unwrap();
    let catalog = include_str!("fixtures/contracts/map-value-equiv-contracts.json");
    let extract = format!(
        "(() => {{ const c=JSON.parse({catalog:?}); return c.sources.map(s => [s.id,s.source,JSON.stringify(s.canonical_expected)]); }})()"
    );
    let rows = context.with(|ctx| ctx.eval::<Vec<Vec<String>>, _>(extract.as_bytes()).unwrap());
    assert!(
        rows.len() == 32,
        "missing independently accepted source contracts"
    );
    let observer_runtime = light_cljs::compile("[]").unwrap().replacen(
        " = // Original bounded runtime",
        " = globalThis.lightRuntime = // Original bounded runtime",
        1,
    );
    let mut host = MacroHost::new();
    for row in rows {
        let [id, source, expected] = row.as_slice() else {
            panic!("invalid complete contract")
        };
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
                "(() => {{const observerRuntime={observer_runtime};const value={captured};return ({})(globalThis.lightRuntime)('{id}',()=>value);}})()",
                include_str!("support/sci_execution_observation.js")
            );
            let raw = context.with(|ctx| {
                ctx.eval::<String, _>(script.as_bytes())
                    .unwrap_or_else(|error| {
                        panic!("{id}, macro={phase}: {error:?}: {:?}", ctx.catch())
                    })
            });
            assert_eq!(
                canonical(&context, &node(&script)),
                canonical(&context, expected),
                "Node: {id}, macro={phase}"
            );
            // Sort JSON object property names only. Collection tags, every
            // sequence position, effects and output remain exact and complete.
            assert_eq!(
                canonical(&context, &raw),
                canonical(&context, expected),
                "{id}, macro={phase}"
            );
        }
    }
    for phase in [false, true] {
        let source = "(= (hash-map :a 7) (hash-map :a (reify IEquiv (-equiv [_ other] (throw (ex-info \"value-equiv\" {:operand other}))))))";
        let input = if phase {
            format!("(defmacro failed-equiv [] (list 'quote {source})) (failed-equiv)")
        } else {
            source.into()
        };
        let compiled = light_cljs::compile_with_macros(&input, &mut host);
        if phase {
            assert!(compiled.unwrap_err().to_string().contains("value-equiv"));
        } else {
            let js = compiled.unwrap();
            let script = format!(
                "(() => {{ try {{ const value={js}; return 'returned'; }} catch(e) {{ return e.message; }} }})()"
            );
            assert_eq!(node(&script), "value-equiv");
            assert_eq!(
                context.with(|ctx| ctx.eval::<String, _>(script.as_bytes()).unwrap()),
                "value-equiv"
            );
        }
        let js = light_cljs::compile_with_macros(
            "(defmacro equiv-recovery [] (= {:a 42} {:a 42})) (equiv-recovery)",
            &mut host,
        )
        .unwrap();
        assert_eq!(node(&js), "true");
        assert!(context.with(|ctx| ctx.eval::<bool, _>(js.as_bytes()).unwrap()));
    }
    for source in [
        "{:a 1 :a 2}",
        "'{{:x 1} 1 {:x 1} 2}",
        "'{[1 2] :vector (1 2) :list}",
        "^{:marked true} {:a 1 :a 2}",
        "{:a 1 :b}",
        "{:a 1",
        "'{:a 1",
    ] {
        for phase in [false, true] {
            let input = if phase {
                format!("(defmacro invalid [] (list 'quote {source})) (invalid)")
            } else {
                source.into()
            };
            let error = light_cljs::compile_with_macros(&input, &mut host).unwrap_err();
            let detail = error.to_string();
            assert!(
                detail.contains("No value supplied for key")
                    || detail.contains("TypeError")
                    || detail.contains("duplicate")
                    || detail.contains("DuplicateMapKey")
                    || detail.contains("even number")
                    || detail.contains("expected"),
                "{source}: {detail}"
            );
            let js = light_cljs::compile_with_macros(
                "(defmacro map-recovery [] (+ 20 22)) (map-recovery)",
                &mut host,
            )
            .unwrap();
            assert_eq!(node(&js), "42");
            assert_eq!(
                context.with(|ctx| ctx.eval::<i32, _>(js.as_bytes()).unwrap()),
                42
            );
        }
    }
}
