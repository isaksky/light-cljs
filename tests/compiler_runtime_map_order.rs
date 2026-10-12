//! Independently frozen complete map contracts, including ordered operand effects.
use light_cljs::MacroHost;
use std::{
    io::Write,
    process::{Command, Stdio},
};

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
fn independent_runtime_maps_preserve_complete_order_effects_and_recovery() {
    let runtime = rquickjs::Runtime::new().unwrap();
    let context = rquickjs::Context::full(&runtime).unwrap();
    let catalog = include_str!("reference/compiler-language/runtime-map-traversal-contracts.json");
    let extract = format!(
        "(() => {{ const c=JSON.parse({catalog:?}); return c.sources.filter(s => !c.nonpassing_neighbors[s.id]).map(s => [s.id,s.source,JSON.stringify(c.canonical_expected.find(p=>p.id===s.id))]); }})()"
    );
    let rows = context.with(|ctx| ctx.eval::<Vec<Vec<String>>, _>(extract.as_bytes()).unwrap());
    assert!(
        rows.len() == 60,
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
                detail.contains("duplicate")
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
