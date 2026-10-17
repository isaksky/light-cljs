//! Independently frozen complete map contracts, including ordered operand effects.
use light_cljs::MacroHost;

#[test]
fn reader_identity_checks_do_not_retain_executable_names() {
    use light_cljs::compiler_data::Source;
    for (source, names, strings) in [
        ("{:a 1 :b 2 :c 3 :d 4 :e 5 :f 6 :g 7 :h 8 :i 9}", 9, 9),
        ("'{[:a ns/b] :c}", 3, 4),
        ("'#{{:a ns/b} [:c ns/d]}", 4, 6),
        ("'{:a 1 ::b 2 :foo/c 3}", 3, 5),
    ] {
        let syntax = light_cljs::parse(source).unwrap();
        let direct = light_cljs::analyze(source, &syntax).unwrap();
        // The owned-form path has already resolved reader keywords, just as
        // macro-produced data does. Both paths retain only executable names.
        let form = MacroHost::new()
            .read_compiler_form(&Source::new("identity.cljs", source))
            .unwrap()
            .unwrap();
        let owned = light_cljs::analyzer::analyze_compiler_form(&form).unwrap();
        for program in [direct, owned] {
            assert_eq!(program.named_literals.len(), names, "{source}");
            assert_eq!(program.strings.len(), strings, "{source}");
        }
    }
}

fn canonical(context: &rquickjs::Context, value: &str) -> String {
    let script = format!(
        "JSON.stringify((function sort(v) {{ if(Array.isArray(v)) return v.map(sort); if(v && typeof v==='object') {{ const out={{}}; Object.keys(v).sort().forEach(k=>out[k]=sort(v[k])); return out; }} return v; }})(JSON.parse({value:?})))"
    );
    context.with(|ctx| ctx.eval::<String, _>(script.as_bytes()).unwrap())
}

#[test]
fn independent_map_sources_preserve_thresholds_groups_metadata_and_recovery() {
    let runtime = rquickjs::Runtime::new().unwrap();
    let context = rquickjs::Context::full(&runtime).unwrap();
    let catalog = include_str!("../fixtures/contracts/literal-map-traversal-contracts.json");
    let extract = format!(
        "(() => {{ const c=JSON.parse({catalog:?}); return c.sources.filter(s => !c.nonpassing_neighbors[s.id]).map(s => [s.id,s.source,JSON.stringify(c.canonical_expected.find(p=>p.id===s.id))]); }})()"
    );
    let rows = context.with(|ctx| ctx.eval::<Vec<Vec<String>>, _>(extract.as_bytes()).unwrap());
    assert!(
        rows.len() >= 20,
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
                include_str!("../support/sci_execution_observation.js")
            );
            let raw = context.with(|ctx| {
                ctx.eval::<String, _>(script.as_bytes())
                    .unwrap_or_else(|error| {
                        panic!("{id}, macro={phase}: {error:?}: {:?}", ctx.catch())
                    })
            });
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
            assert_eq!(
                context.with(|ctx| ctx.eval::<i32, _>(js.as_bytes()).unwrap()),
                42
            );
        }
    }
}
