//! Lifecycle fixtures complement the actual .cljs compiler tests in capture.rs.
use super::*;
use crate::compiler_data::{DatumKind, Name, Origin, Source};

fn runner() -> PathBuf {
    std::env::var_os("GOOSE_COMPILER_RUNNER")
        .expect("run compiler.bb")
        .into()
}
fn form() -> Form {
    let source = Source::new("retained.cljs", "nil");
    Form::from_data(
        Datum::new(DatumKind::Nil),
        source.span(0..3).unwrap(),
        Origin::Source,
    )
}
fn origin(form: &Form) -> ExpansionOrigin {
    ExpansionOrigin {
        macro_name: Name::new(Some("fixture"), "macro"),
        call: form.span(),
        definition: Some(form.span()),
        helpers: vec![],
    }
}
fn inputs(e: &GooseEvaluator, source: &str) -> (ValueId, Vec<ValueId>, Form) {
    let form = form();
    let function = e.evaluate_unit(source).unwrap();
    let call = e.import_form(&form, Default::default()).unwrap();
    let env = e
        .import_data(&Datum::new(DatumKind::Nil), Default::default())
        .unwrap();
    (function, vec![call, env], form)
}
/// Install a helper service that resolves the fixture's `roots` by name,
/// as the compiler's catalog index does, and gives each the initializer
/// `initialize` returns for its unit.
fn serve(
    e: &GooseEvaluator,
    roots: &[(&str, usize)],
    initialize: impl Fn(usize) -> Result<String, String> + 'static,
) {
    serve_scoped(e, roots, None, initialize);
}
/// [`serve`] for helpers whose initializers number `gensym` after `prefix`.
fn serve_scoped(
    e: &GooseEvaluator,
    roots: &[(&str, usize)],
    prefix: Option<&'static str>,
    initialize: impl Fn(usize) -> Result<String, String> + 'static,
) {
    let roots: HashMap<String, usize> = roots
        .iter()
        .map(|&(name, unit)| (name.to_owned(), unit))
        .collect();
    let initialize = Rc::new(initialize);
    e.install_helper(crate::goose_evaluator::helper_owner(), move || {
        Rc::new(move |_, name| {
            let unit = *roots
                .get(name)
                .ok_or_else(|| format!("compiler root is not registered: {name}"))?;
            initialize(unit).map(|code| (unit, code, prefix.map(str::to_owned)))
        })
    });
}
fn cleaned(s: &GooseSession) {
    assert!(s.process.lock().unwrap().is_none());
    assert!(s.readers.lock().unwrap().is_empty());
    assert!(s.pending.lock().unwrap().is_empty());
    assert!(!s.directory.exists());
}
#[cfg(light_v8)]
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_generated_assembly() {
    // Load the exact generated source with the actual namespace compiler.
    // The temporary full project is an input fixture, never pre-expanded JS.
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let generated = root.join("target").join(format!(
        "generated-assembly-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    struct Remove(PathBuf);
    impl Drop for Remove {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _remove = Remove(generated.clone());
    assert!(
        std::process::Command::new("bb")
            .current_dir(&root)
            .arg("benchmarks/generated-project/generate.bb")
            .arg(&generated)
            .args(["--seed", "42", "--lines", "200k"])
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(
        std::fs::read(generated.join("manifest.edn")).unwrap(),
        include_bytes!("../../crates/big-boy-pants-js/evidence/generated-200k-manifest.edn")
    );
    let read = |text: &str| {
        let source = Source::new("generated-assembly.cljs", text);
        crate::compiler_reader::read_form(
            &source,
            &crate::compiler_reader::NamespaceView::new("workload.fonam", &source),
        )
        .unwrap()
        .unwrap()
    };
    let cases = [
        ("zalstok", "sample [x]", "{}"),
        ("zalstok", "sample [x] x", "{}"),
        ("zalstok", "sample [x] x (+ x 1)", "{}"),
        ("zalstok", "sample [x] ^:marked x", "{}"),
        ("zalstok", "sample [x] `(tag ~x)", "{}"),
        ("kalvas", "[2 3] 4", "{}"),
        ("dukdos", "7", "{}"),
        ("dukdos", "^:marked x", "{}"),
        ("graidamha", "^:marked x", "{}"),
        ("graidamha", "x 3 4", "{}"),
        ("gaikmenfeik", "", "{}"),
        ("gaikmenfeik", "1 2 3", "{}"),
        ("gakzoultrei-grekvavus", "[x 7] ^:marked x", "{}"),
        ("stipopi", "^:marked P (m [this])", "{}"),
        ("marore-jaipla", "^:marked R [x]", "{}"),
        ("dougri", "^:marked mm", "{}"),
        ("joktol", "^:marked x 99", "{:locals {x {}}}"),
        ("joktol", "x 99", "{:locals {x false}}"),
        ("joktol", "x 99", "{:locals {x nil}}"),
        ("joktol", "x 99", "{:locals {}}"),
        ("joktol", "x 99", "{:locals nil}"),
        ("buvaiskum", "", "{}"),
        ("buvaiskum", "1", "{}"),
        ("buvaiskum", "1 2 3", "{}"),
        ("buvaiskum", "^:marked x y", "{}"),
        ("buvaiskum", "[1 ^:marked x] {:k 2} 3", "{}"),
    ];
    let mut expected: Option<Vec<(Form, Form)>> = None;
    let mut repeated_value: Option<Datum> = None;
    let mut repeated_goose: Option<(Form, Form)> = None;
    let mut mapped_value: Option<Datum> = None;
    let mut mapped_goose: Option<(Form, Form)> = None;
    for workers in [0, 1, 2, 4, 8] {
        let _names = crate::generated_names::enter_namespace(0);
        let mut host = crate::macro_host::MacroHost::new();
        let session = (workers != 0).then(|| GooseSession::start(&runner(), workers).unwrap());
        if let Some(s) = &session {
            host.use_goose_source_macros(s.clone()).unwrap();
        }
        crate::compiler::analyze_namespace_entry_recorded(
            "workload.fonam",
            &[generated.join("src"), generated.join("macros")],
            &mut host,
            Default::default(),
            &mut Vec::new(),
            crate::compiler::EntryRecorders {
                slots: crate::scheduler::Slots::new(1).unwrap(),
                ..Default::default()
            },
        )
        .unwrap();
        let forms = cases
            .iter()
            .map(|(name, args, env)| {
                let call = read(&format!("({name} {args})"));
                let expanded = host
                    .expand_source_javascript(
                        "workload.fonam",
                        name,
                        &call,
                        &read(env).data(),
                        None,
                    )
                    .unwrap_or_else(|e| panic!("{name} workers={workers}: {e}"));
                (call, expanded)
            })
            .collect::<Vec<_>>();
        let repeated_call = read("(kalvas [2 2] 2)");
        let repeated = host
            .expand_source_javascript(
                "workload.fonam",
                "kalvas",
                &repeated_call,
                &read("{}").data(),
                None,
            )
            .unwrap();
        let mapped_call = read("(buvaiskum 2 2 2)");
        let mapped = host
            .expand_source_javascript(
                "workload.fonam",
                "buvaiskum",
                &mapped_call,
                &read("{}").data(),
                None,
            )
            .unwrap();
        let anonymous_call = read("(zalstok sample [x] #(+ % 1))");
        let anonymous = host
            .expand_source_javascript(
                "workload.fonam",
                "zalstok",
                &anonymous_call,
                &read("{}").data(),
                None,
            )
            .unwrap();
        drop(host);
        if let Some(s) = session {
            assert_eq!(s.stats().jobs, cases.len() as u64 + 3);
            let trace = s.trace();
            assert_eq!(
                trace.iter().filter(|s| s.starts_with("PRODUCED ")).count(),
                1
            );
            // The prepared branch exports canonical source with its captured
            // runtime. Inspect the actual transported compiler artifact; no
            // rewritten helper body is supplied by this test.
            let helpers = std::fs::read_dir(&s.directory)
                .unwrap()
                .map(|e| e.unwrap().path())
                .filter(|p| p.extension().is_some_and(|e| e == "gsp"))
                .map(|p| String::from_utf8_lossy(&std::fs::read(p).unwrap()).into_owned())
                .filter(|s| s.contains("rt.core.list") && s.contains("workload.common"))
                .collect::<Vec<_>>();
            assert_eq!(helpers.len(), 1);
            let source = &helpers[0];
            for emitted in ["rt.core.list", "rt.symbol", "workload.common", "norm"] {
                assert!(
                    source.contains(emitted),
                    "missing canonical helper {emitted}"
                );
            }
            eprintln!("generated callable workers={workers}: {trace:?}");
            s.close();
            cleaned(&s);
        }
        // Includes every form/scalar pool, metadata, source span and origin,
        // after the host, jobs and native session have all been destroyed.
        if let Some(reference) = &expected {
            crate::compiler_bootstrap::goose_capture::compare_concurrent_forms(
                reference.clone(),
                forms,
            );
        } else {
            expected = Some(forms);
        }
        let anonymous_spans = (0..anonymous.nodes().len())
            .filter_map(|i| {
                let f = anonymous.at(crate::compiler_data::FormId(i)).unwrap();
                (f.node().origin == Origin::AnonymousArgument).then(|| f.span().bytes())
            })
            .collect::<Vec<_>>();
        assert_eq!(
            anonymous_spans,
            if workers == 0 {
                vec![24..25, 24..25]
            } else {
                vec![20..28, 24..25]
            }
        );
        let spans = (0..repeated.nodes().len())
            .filter_map(|i| {
                let form = repeated.at(crate::compiler_data::FormId(i)).unwrap();
                form.data()
                    .value_eq(&Datum::new(DatumKind::Integer(2)))
                    .then(|| {
                        assert_eq!(form.node().origin, Origin::Source);
                        form.span().bytes()
                    })
            })
            .collect::<Vec<_>>();
        if workers == 0 {
            // The reference bridge explicitly keeps scalar provenance by
            // value (form_bridge.rs). Equal JS numbers get the last input
            // span. Pin that limitation; never erase Goose occurrence IDs
            // or normalize its more precise result to match this ambiguity.
            assert_eq!(spans, vec![14..15, 14..15, 14..15]);
            repeated_value = Some(repeated.data());
        } else {
            assert!(repeated_value.as_ref().unwrap().value_eq(&repeated.data()));
            assert_eq!(spans, vec![9..10, 11..12, 14..15]);
            if let Some(reference) = &repeated_goose {
                crate::compiler_bootstrap::goose_capture::compare_concurrent_forms(
                    vec![reference.clone()],
                    vec![(repeated_call, repeated)],
                );
            } else {
                repeated_goose = Some((repeated_call, repeated));
            }
        }
        let spans = (0..mapped.nodes().len())
            .filter_map(|i| {
                let form = mapped.at(crate::compiler_data::FormId(i)).unwrap();
                form.data()
                    .value_eq(&Datum::new(DatumKind::Integer(2)))
                    .then(|| {
                        assert_eq!(form.node().origin, Origin::Source);
                        form.span().bytes()
                    })
            })
            .collect::<Vec<_>>();
        if workers == 0 {
            assert_eq!(spans, vec![15..16, 15..16, 15..16]);
            mapped_value = Some(mapped.data());
        } else {
            assert!(mapped_value.as_ref().unwrap().value_eq(&mapped.data()));
            assert_eq!(spans, vec![11..12, 13..14, 15..16]);
            if let Some(reference) = &mapped_goose {
                crate::compiler_bootstrap::goose_capture::compare_concurrent_forms(
                    vec![reference.clone()],
                    vec![(mapped_call, mapped)],
                );
            } else {
                mapped_goose = Some((mapped_call, mapped));
            }
        }
    }
}

#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_mapseq_demand_and_throw() {
    for workers in [1, 2, 4, 8] {
        let s = GooseSession::start(&runner(), workers).unwrap();
        let e = GooseEvaluator::new(s.clone(), &Default::default()).unwrap();
        let source = r#"rt => (form,env) => {
            let calls=0; let order=0; let throws=0;
            const input=__rt.formList(1,2,3);
            const xs=rt.core.mapSeq(x=>{calls++;order=order*10+x;return __rt.formVector(x,x);},input);
            const alias=xs;
            if(calls!==0 || rt.core.sequence(xs)!==xs) return false;
            const a=__rt.cljConcat(__rt.formList(0),xs);
            const b=__rt.cljConcatAll(__rt.formList(xs,alias));
            if(calls!==3 || order!==123 || xs!==alias || __rt.cljCount(a)!==4 || __rt.cljCount(b)!==6) return false;
            if(__rt.cljNth(a,1)!==__rt.cljNth(b,0) || __rt.cljNth(b,0)!==__rt.cljNth(b,3)) return false;
            const ys=rt.core.mapSeq(x=>{if(x===2 && throws===0){throws++;throw 17;} calls++;return x;},input);
            try { __rt.cljCount(ys); return false; } catch(e) { if(e!==17 || calls!==4) return false; }
            if(__rt.cljCount(ys)!==3 || calls!==6 || throws!==1) return false;
            const empty=rt.core.mapSeq(()=>{calls++;throw 99;},null);
            const zs=rt.core.mapSeq(x=>x,input);
            const vector=rt.core.vec.call(null,zs);
            const more=rt.core.into.call(null,__rt.formVector(),rt.core.mapSeq(x=>x,input));
            return __rt.cljCount(empty)===0 && calls===6 && __rt.cljCount(vector)===3 && __rt.cljCount(more)===3 && __rt.cljCount(rt.core.mapv(x=>x+1,input))===3;
        }"#;
        let (f, args, call) = inputs(&e, source);
        for _ in 0..3 {
            let value = e.call(f, &args).unwrap();
            let output = e
                .export_form(value, &call, origin(&call), Default::default())
                .unwrap();
            assert!(
                output
                    .data()
                    .value_eq(&Datum::new(DatumKind::Boolean(true)))
            );
        }
        s.close();
        cleaned(&s);
    }
}

#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_mapseq_helper_no_replay() {
    // A real emitted helper factory executes on the released worker. A
    // demand in the second callback snapshots the first memo and its writes.
    for workers in [1, 2, 4, 8] {
        let s = GooseSession::start(&runner(), workers).unwrap();
        let e = GooseEvaluator::new(s.clone(), &Default::default()).unwrap();
        let source = r#"rt => (form,env) => {
            let calls=0;let order=0;
            const xs=rt.core.mapSeq(x=>{
                calls++;order=order*10+x;
                if(x===2) return __rt.cljReadRoot(globalThis.__lightCompilerRoots,null,'fixture/mapseq');
                return x;
            },__rt.formList(1,2,3));
            const alias=xs;
            const first=__rt.cljConcat(null,xs);
            const second=__rt.cljConcatAll(__rt.formList(alias,xs));
            return calls===3 && order===123 && __rt.cljCount(second)===6 && __rt.formValue(__rt.cljNth(first,1))===40;
        }"#;
        let (f, args, call) = inputs(&e, source);
        let called = Rc::new(Cell::new(0));
        let counter = called.clone();
        serve(&e, &[("fixture/mapseq", 42)], move |_| {
            counter.set(counter.get() + 1);
            Ok("rt => {let x=0;for(let i=0;i<40;i++)x++;return x;}".into())
        });
        for _ in 0..3 {
            let result = e.call(f, &args).unwrap();
            let result = e
                .export_form(result, &call, origin(&call), Default::default())
                .unwrap();
            assert!(
                result
                    .data()
                    .value_eq(&Datum::new(DatumKind::Boolean(true)))
            );
        }
        assert_eq!(called.get(), 3);
        assert_eq!(
            s.trace()
                .iter()
                .filter(|line| line.starts_with("PRODUCED "))
                .count(),
            1
        );
        s.close();
        cleaned(&s);
    }
}

#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_assembly_leaf_boundaries() {
    // The array is the job's own: a job cannot write its definition's rows
    // (light-cljs-795).
    let body = r#"rt => {
        const sequence=rt.core.sequence;
        return (form,env) => {
            const a=[1,2,3]; a.length=1; a.length=3;
            const alias=a;
            const before=a[0]===1 && a[1]===undefined && a[2]===undefined;
            alias.length=0; alias.length=4; alias[2]=7;
            const xs=__rt.formList(1,2);
            const joined=__rt.cljConcatAll(__rt.formList(null,xs,__rt.cljConcat(xs,__rt.formVector(3))));
            return before && a===alias && a.length===4 && a[0]===undefined && a[1]===undefined && a[2]===7 && a[3]===undefined
                && sequence.call(null,xs)===xs && __rt.cljCount(sequence(null))===0
                && __rt.cljCount(sequence(__rt.formVector(1,2)))===2
                && __rt.cljCount(joined)===5 && __rt.cljNth(joined,4)===3
                && __rt.cljCount(xs)===2 && __rt.cljCount(__rt.cljConcat(null,null))===0
                && __rt.cljContains(__rt.formMap(1,false,2,null),1)
                && __rt.cljContains(__rt.formMap(1,false,2,null),2)
                && !__rt.cljContains(__rt.formMap(1,false,2,null),3)
                && __rt.cljContains(__rt.formSet(false,null),false)
                && __rt.cljContains(__rt.formSet(false,null),null)
                && !__rt.cljContains(__rt.formVector(3),1.5);
        };
    }"#;
    let s = GooseSession::start(&runner(), 2).unwrap();
    let e = GooseEvaluator::new(s.clone(), &Default::default()).unwrap();
    let (f, args, call) = inputs(&e, body);
    for _ in 0..3 {
        let value = e.call(f, &args).unwrap();
        let form = e
            .export_form(value, &call, origin(&call), Default::default())
            .unwrap();
        assert!(form.data().value_eq(&Datum::new(DatumKind::Boolean(true))));
    }
    s.close();
    cleaned(&s);
    for (body, fault) in [
        (
            "const a=[];a.length=-1;return a;",
            "Fault.UnsupportedOperand",
        ),
        (
            "const a=[];a.length=1.5;return a;",
            "Fault.UnsupportedOperand",
        ),
        (
            "const a=[];a.length=0/0;return a;",
            "Fault.UnsupportedOperand",
        ),
        (
            "const a=[];a.length=1/0;return a;",
            "Fault.UnsupportedOperand",
        ),
        (
            "const a=[];a.length='2';return a;",
            "Fault.UnsupportedOperand",
        ),
        (
            "const a=[];a.length=4294967296;return a;",
            "Fault.UnsupportedOperand",
        ),
        ("const a=[];a.length=1048577;return a;", "Fault.MemoryLimit"),
        // Every opcode exports; one the Goose VM does not implement fails the
        // macro by name when it runs, uncatchably (light-cljs-783).
        (
            "function P(){} try { return new P(); } catch (e) { return null; }",
            "Fault.Unsupported:construct",
        ),
        (
            "return rt.core.sequence(null,null);",
            "Fault.NativeBoundary",
        ),
        (
            "return __rt.cljConcat(__rt.formMap(1,2),null);",
            "Fault.UnsupportedOperand",
        ),
        ("return rt.core.mapSeq(x=>x);", "Fault.NativeBoundary"),
        // Outside every generated-name scope gensym stays an explicit
        // native-boundary fault; in one it returns symbols (below).
        ("return rt.core.gensym();", "Fault.NativeBoundary"),
        ("return rt.core.gensym(\"x__\");", "Fault.NativeBoundary"),
        (
            "return __rt.cljCount(rt.core.mapSeq(()=>{throw 17;},__rt.formList(1)));",
            "Fault.HostEffect",
        ),
        (
            "return __rt.cljCount(rt.core.mapSeq(()=>null.x,__rt.formList(1)));",
            "Fault.TypeError",
        ),
        (
            "return rt.core.mapSeq(x=>x,__rt.formVector(1));",
            "Fault.NativeBoundary",
        ),
        (
            "return rt.core.mapSeq(x=>x,__rt.formList(1),__rt.formList(2));",
            "Fault.NativeBoundary",
        ),
        (
            "return __rt.cljFirst(rt.core.mapSeq(x=>x,__rt.formList(1)));",
            "Fault.NativeBoundary",
        ),
        (
            "return rt.core.mapSeq(x=>x,rt.core.mapSeq(x=>x,__rt.formList(1)));",
            "Fault.NativeBoundary",
        ),
        (
            "let xs;xs=rt.core.mapSeq(()=>__rt.cljCount(xs),__rt.formList(1));return __rt.cljCount(xs);",
            "Fault.NativeBoundary",
        ),
    ] {
        let s = GooseSession::start(&runner(), 1).unwrap();
        let e = GooseEvaluator::new(s.clone(), &Default::default()).unwrap();
        let (f, args, _) = inputs(&e, &format!("rt => (form,env) => {{ {body} }}"));
        let error = e.call(f, &args).unwrap_err();
        assert!(
            error.0.contains(fault) && error.0.contains("emitted-span="),
            "{body}: {error}"
        );
        s.close();
        cleaned(&s);
    }
    // In a generated-name scope (light-cljs-798) `rt.core.gensym()` and
    // `rt.core.gensym("x__")` return symbols numbered in the scope's count,
    // which each job moves past what it took, so a core macro expanded
    // natively in the scope numbers on after them.
    let s = GooseSession::start(&runner(), 1).unwrap();
    let e = GooseEvaluator::new(s.clone(), &Default::default()).unwrap();
    let _scope = crate::generated_names::enter("7".into());
    crate::generated_names::sync_gensym_scope(&e).unwrap();
    let (f, args, call) = inputs(
        &e,
        "rt => (form,env) => __rt.formVector(rt.core.gensym(), rt.core.gensym(\"x__\"), rt.core.gensym(rt.symbol(null, \"s\")))",
    );
    for round in 0..2 {
        let value = e.call(f, &args).unwrap();
        let form = e
            .export_form(value, &call, origin(&call), Default::default())
            .unwrap();
        let n = 3 * round;
        let expected = [
            format!("G__7_{}", n + 1),
            format!("x__7_{}", n + 2),
            format!("s7_{}", n + 3),
        ]
        .map(|name| Datum::new(DatumKind::Symbol(Name::new(None, &name))));
        let data = form.data();
        let DatumKind::Collection { children, .. } = data.kind() else {
            panic!("{form:?}");
        };
        assert_eq!(children.len(), 3);
        for (child, expected) in children.iter().zip(&expected) {
            assert!(child.value_eq(expected), "{child:?} vs {expected:?}");
        }
    }
    // As a collection callback, and in the definition's preparation, which
    // runs once, in the scope of the first job that needs it.
    let (f, args, call) = inputs(
        &e,
        "rt => { const p=rt.core.gensym(\"p\"); return (form,env) => __rt.formVector(p, rt.core.mapv(rt.core.gensym, __rt.formVector(\"a\",\"b\"))); }",
    );
    for expected in [["p7_7", "a7_8", "b7_9"], ["p7_7", "a7_10", "b7_11"]] {
        let value = e.call(f, &args).unwrap();
        let form = e
            .export_form(value, &call, origin(&call), Default::default())
            .unwrap();
        let data = form.data();
        let DatumKind::Collection { children, .. } = data.kind() else {
            panic!("{form:?}");
        };
        let DatumKind::Collection {
            children: mapped, ..
        } = children[1].kind()
        else {
            panic!("{form:?}");
        };
        let names = std::iter::once(&children[0]).chain(mapped.iter());
        for (child, expected) in names.zip(expected) {
            let expected = Datum::new(DatumKind::Symbol(Name::new(None, expected)));
            assert!(child.value_eq(&expected), "{child:?} vs {expected:?}");
        }
    }
    assert_eq!(
        crate::generated_names::gensym("y").as_deref(),
        Some("y7_12")
    );
    // Two arguments are an arity error V8 throws; Goose refuses them.
    let (f, args, _) = inputs(&e, "rt => (form,env) => rt.core.gensym(\"a\",\"b\")");
    let error = e.call(f, &args).unwrap_err();
    assert!(error.0.contains("Fault.NativeBoundary"), "{error}");
    s.close();
    cleaned(&s);
}
/// A helper the job demands is produced in its own frozen initializer
/// scope, numbering from one there whichever job demands it, and takes no
/// number of the job's scope, whose count the job goes on with, whether it
/// waits in place or saves its continuation; later jobs reuse the produced
/// helper (light-cljs-798, light-cljs-802). The helper's names are the same
/// whichever scope first demands it and at every worker count. The helper
/// here is hand-written; goose_compiler_helper_initializer_gensym_names_alike_at_every_slot_count
/// compiles one from CLJS.
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_gensym_helper_production_numbers_in_its_own_scope() {
    let symbols = |names: &[String]| {
        names
            .iter()
            .map(|name| Datum::new(DatumKind::Symbol(Name::new(None, name))))
            .collect::<Vec<_>>()
    };
    let configurations = [1, 2, 4, 8]
        .into_iter()
        .flat_map(|workers| [(workers, None), (workers, Some(0))])
        .flat_map(|(workers, retained)| [(workers, retained, "7"), (workers, retained, "8")]);
    for (workers, retained, demander) in configurations {
        let s = GooseSession::start_retaining(&runner(), workers, None, retained).unwrap();
        let e = GooseEvaluator::new(s.clone(), &Default::default()).unwrap();
        let _scope = crate::generated_names::enter(demander.into());
        crate::generated_names::sync_gensym_scope(&e).unwrap();
        let (f, args, call) = inputs(
            &e,
            "rt => (form,env) => { const a=rt.core.gensym(); const t=__rt.cljReadRoot(globalThis.__lightCompilerRoots,null,'fixture/tag'); const b=rt.core.gensym(\"b\"); return __rt.formVector(a,t,b); }",
        );
        serve_scoped(&e, &[("fixture/tag", 42)], Some("9_1_i"), |_| {
            Ok("rt => rt.core.gensym(\"t\")".into())
        });
        let rounds = [["G__D_1", "t9_1_i1", "bD_2"], ["G__D_3", "t9_1_i1", "bD_4"]];
        for (round, expected) in rounds.iter().enumerate() {
            let value = e.call(f, &args).unwrap();
            let form = e
                .export_form(value, &call, origin(&call), Default::default())
                .unwrap();
            let data = form.data();
            let DatumKind::Collection { children, .. } = data.kind() else {
                panic!("{form:?}");
            };
            let expected = symbols(&expected.map(|name| name.replace('D', demander)));
            assert_eq!(children.len(), 3);
            for (child, expected) in children.iter().zip(&expected) {
                assert!(
                    child.value_eq(expected),
                    "workers={workers} retained={retained:?} demander={demander} round={round}: {child:?} vs {expected:?}"
                );
            }
        }
        let next = crate::generated_names::gensym("y");
        assert_eq!(next, Some(format!("y{demander}_5")));
        s.close();
        cleaned(&s);
    }
}
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_variadic_wrappers() {
    // Exercise the actual compiler, including &form and its variadic dispatch.
    // This declaration is verbatim from seed42/200k workload.fonam.
    let source = "(defmacro gaikmenfeik [& _]\n  (+ 165 (count &form)))\n(js/console.log (pr-str [(gaikmenfeik) (gaikmenfeik 1) (gaikmenfeik 1 2 3)]))";
    let mut expected = None;
    for goose in [false, true] {
        let _names = crate::generated_names::enter_namespace(0);
        let mut host = crate::macro_host::MacroHost::new();
        let session = goose.then(|| GooseSession::start(&runner(), 1).unwrap());
        if let Some(s) = &session {
            host.use_goose_source_macros(s.clone()).unwrap();
        }
        let program = crate::analyzer::analyze_named_with_macro_warnings(
            "generated-wrapper.cljs",
            source,
            &crate::parse(source).unwrap(),
            &mut host,
            Default::default(),
            &mut Vec::new(),
        )
        .unwrap();
        let js = crate::emit_js(&program);
        let output = std::process::Command::new("node")
            .args(["-e", &js])
            .output()
            .unwrap();
        assert!(output.status.success(), "{:?}", output);
        assert_eq!(String::from_utf8(output.stdout).unwrap(), "[166 167 169]\n");
        if goose {
            assert_eq!(Some(&js), expected.as_ref());
            let s = session.unwrap();
            assert_eq!(s.stats().jobs, 3);
            assert_eq!(s.stats().preparations, 1);
            s.close();
            cleaned(&s);
        } else {
            expected = Some(js);
        }
    }
}

#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_arguments_across_helper_suspension() {
    // The demanded scalar suspends inside nested rest/call frames; live
    // actuals and an escaped arguments object must both relocate privately.
    for workers in [1, 2] {
        let s = GooseSession::start(&runner(), workers).unwrap();
        let e = GooseEvaluator::new(s.clone(), &Default::default()).unwrap();
        let source = "rt => { function keep(a) { return () => arguments; } const prepared=keep(17,18); return (form,env) => { const saved=keep(7,8); function inner(a,...rest) { const value=__rt.cljReadRoot(globalThis.__lightCompilerRoots,null,'fixture/wrapped'); return value+arguments.length+arguments[0]+rest[0]+saved()[1]; } const wrapped=__rt.fnApplyNull(inner); const result=wrapped.call(null,2,3,4); return result+prepared()[0]+(__rt.fnApplyReceiver(wrapped)===null?0:1000); }; }";
        let (f, args, call) = inputs(&e, source);
        let called = Rc::new(Cell::new(0));
        let counter = called.clone();
        serve(&e, &[("fixture/wrapped", 42)], move |_| {
            counter.set(counter.get() + 1);
            Ok("rt => 40".into())
        });
        for _ in 0..3 {
            let result = e.call(f, &args).unwrap();
            let result = e
                .export_form(result, &call, origin(&call), Default::default())
                .unwrap();
            assert!(result.data().value_eq(&Datum::new(DatumKind::Integer(73))));
        }
        assert_eq!(called.get(), 3);
        assert_eq!(
            s.trace()
                .iter()
                .filter(|line| line.starts_with("PRODUCED "))
                .count(),
            1
        );
        s.close();
        cleaned(&s);
    }
}

#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_arguments_identity() {
    // Equal contents do not make arguments objects equal. Prepared aliases
    // retain their identity after snapshot restore and repeated worker calls.
    let source = r#"rt => {
        function f() { return arguments; }
        const saved=f(1,2); const alias=saved;
        return (form,env) => {
            const a=f(); const b=f(); const c=f(1,2); const d=f(1,2);
            const map=__rt.formMap(a,1,b,2,a,3);
            const updated=__rt.formAssoc(map,b,4);
            const set=__rt.formSet(a,b,a);
            return a!==b && c!==d
                && !__rt.formEquals(a,b) && !__rt.formEquals(c,d)
                && __rt.formEquals(a,a) && __rt.formEquals(saved,alias)
                && !__rt.formEquals(saved,c) && !__rt.formEquals(c,saved)
                && !__rt.formEquals(c,__rt.formVector(1,2))
                && !__rt.formEquals(__rt.formVector(1,2),c)
                && !__rt.formEquals(__rt.formVector(a),__rt.formVector(b))
                && __rt.formEquals(__rt.formVector(saved),__rt.formVector(alias))
                && __rt.formCount(map)===2 && __rt.formGet(map,a,null)===3
                && __rt.formGet(map,b,null)===2 && __rt.formGet(map,f(),null)===null
                && __rt.formCount(updated)===2 && __rt.formGet(updated,a,null)===3
                && __rt.formGet(updated,b,null)===4 && __rt.formCount(set)===2;
        };
    }"#;
    for workers in [1, 2] {
        let s = GooseSession::start(&runner(), workers).unwrap();
        let e = GooseEvaluator::new(s.clone(), &Default::default()).unwrap();
        let (f, args, call) = inputs(&e, source);
        for _ in 0..3 {
            let result = e.call(f, &args).unwrap();
            let result = e
                .export_form(result, &call, origin(&call), Default::default())
                .unwrap();
            assert!(
                result
                    .data()
                    .value_eq(&Datum::new(DatumKind::Boolean(true)))
            );
        }
        s.close();
        cleaned(&s);
    }
}

#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_wrapper_conventions_and_boundaries() {
    for (body, expected) in [
        (
            "const f=function(x){return this===null && x===form;}; return __rt.cljCallable(f)===false && __rt.cljCallMember(f,null)===f.call && __rt.call(f,__rt.cljCallMember(f,null),null,form);",
            None,
        ),
        (
            "const k=__rt.formKeyword(null,'k');const m=__rt.formMap(k,form);const v=__rt.formVector(form);const s=__rt.formSet(k); return __rt.cljCallable(k) && __rt.cljCallable(m) && __rt.cljCallable(v) && __rt.cljCallable(s) && __rt.cljInvoke(k,m,null,1)===form && __rt.cljInvoke(m,k,null,1)===form && __rt.cljInvoke(v,0.5,null,1)===form && __rt.cljInvoke(s,k,null,1)===k;",
            None,
        ),
        (
            "return __rt.cljCallMember({},null);",
            Some("Fault.NativeBoundary"),
        ),
        (
            "return __rt.cljInvoke(()=>1,null,null,1);",
            Some("Fault.NativeBoundary"),
        ),
        ("return rt.intrinsicRegExp();", Some("Fault.NativeBoundary")),
        ("return rt.core.inc.call(null,40)===41;", None),
        // The emitted prologue's core binding is V8's own-property
        // descriptor of rt.core, its value the leaf itself (light-cljs-807).
        (
            "const d=rt.nativeCoreBinding('gensym'); const {value: g, get: h}=d; return g===rt.core.gensym && h===undefined && d.writable===true && d.enumerable===true && d.configurable===true && Object.getPrototypeOf(d)===Object.prototype && rt.nativeCoreBinding('gensym')!==d;",
            None,
        ),
        (
            "return rt.nativeCoreBinding('notACoreLeaf');",
            Some("Fault.NativeBoundary:nativeCoreBinding:-no-core-leaf-notACoreLeaf"),
        ),
        (
            "try { return rt.nativeCoreBinding(); } catch (e) { return null; }",
            Some("Fault.NativeBoundary:nativeCoreBinding:-no-core-leaf"),
        ),
        (
            "const call=(function(){}).call; return call();",
            Some("Fault.TypeError"),
        ),
        (
            "const f=__rt.fnApplyNull(function(...args){return args.length;}); return __rt.fnApplyReceiver(f)===null && f.call(null,1,2,3)===3;",
            None,
        ),
        (
            "const f=function(){return this;}; return __rt.fnApplyReceiver(f)===f && f.call()===undefined && f.call(null)===null && f.call(7)===7;",
            None,
        ),
        (
            "function f(a){arguments[0]=3; return a;} return f(1);",
            Some("Fault.UnsupportedOperand"),
        ),
        (
            "function f(){return arguments.callee;} return f();",
            Some("Fault.UnsupportedOperand"),
        ),
        (
            "function f(){return arguments.__proto__;} return f();",
            Some("Fault.UnsupportedOperand"),
        ),
        (
            "const f=__rt.fnApplyNull(7); return f;",
            Some("Fault.TypeError"),
        ),
    ] {
        let s = GooseSession::start(&runner(), 1).unwrap();
        let e = GooseEvaluator::new(s.clone(), &Default::default()).unwrap();
        let (f, args, call) = inputs(&e, &format!("rt => (form,env) => {{ {body} }}"));
        let result = e.call(f, &args);
        if let Some(expected) = expected {
            let error = result.unwrap_err();
            assert!(
                error.0.contains(expected) && error.0.contains("emitted-span="),
                "{body}: {error}"
            );
        } else {
            let value = e
                .export_form(result.unwrap(), &call, origin(&call), Default::default())
                .unwrap();
            assert!(
                value.data().value_eq(&Datum::new(DatumKind::Boolean(true))),
                "{body}"
            );
        }
        s.close();
        cleaned(&s);
    }
}
/// The runner's final JOBS line (compiler_session.goose `Jobs`), by name.
#[derive(Debug)]
pub(super) struct JobsLine {
    pub retention: u64,
    pub spawned: u64,
    pub peak_live: u64,
    pub peak_blocked: u64,
    pub waits: u64,
    pub saves: u64,
    pub answers: u64,
    pub live: u64,
    pub blocked: u64,
    /// Jobs given to job workers, workers retired at the table-byte cap and
    /// the workers' total reset time (light-cljs-795).
    pub jobs: u64,
    pub retired: u64,
    pub reset_ns: u64,
    /// The workers' snapshot installs and helper installs (time and count),
    /// and the coordinator's definition hand-off (light-cljs-796).
    pub restore_ns: u64,
    pub install_ns: u64,
    pub installs: u64,
    pub handoff_ns: u64,
    /// The cell owner's totals (light-cljs-799, cell_owner.goose `Stats`):
    /// cells made, identities numbered, operations served, refused
    /// compare-and-sets (swap! reruns), delay waits and serving time; then
    /// the bytes of cell values it held at the end (light-cljs-809).
    pub cells: u64,
    pub identities: u64,
    pub cell_operations: u64,
    pub conflicts: u64,
    pub delay_waits: u64,
    pub owner_ns: u64,
    pub cell_bytes: u64,
}
pub(super) fn jobs_line(trace: &[String]) -> JobsLine {
    let line = trace
        .iter()
        .find(|s| s.starts_with("JOBS "))
        .unwrap_or_else(|| panic!("no JOBS line: {trace:?}"));
    let n = line
        .split_whitespace()
        .skip(2)
        .map(|n| n.parse::<u64>().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(n.len(), 29, "{line}");
    JobsLine {
        retention: n[0],
        spawned: n[1],
        peak_live: n[8],
        peak_blocked: n[9],
        waits: n[10],
        saves: n[11],
        answers: n[12],
        live: n[13],
        blocked: n[14],
        jobs: n[15],
        retired: n[16],
        reset_ns: n[17],
        restore_ns: n[18],
        install_ns: n[19],
        installs: n[20],
        handoff_ns: n[21],
        cells: n[22],
        identities: n[23],
        cell_operations: n[24],
        conflicts: n[25],
        delay_waits: n[26],
        owner_ns: n[27],
        cell_bytes: n[28],
    }
}
/// Threads: the coordinator, the input thread, the cell owner
/// (light-cljs-799), `workers + retention` reapers and the contained job
/// workers it spawned, which serve many jobs each (light-cljs-795); every
/// thread but the coordinator joined.
pub(super) fn assert_threads(trace: &[String], workers: usize) -> JobsLine {
    let jobs = jobs_line(trace);
    let reapers = workers as u64 + jobs.retention;
    assert_eq!(
        trace.iter().find(|s| s.starts_with("THREADS ")).unwrap(),
        &format!(
            "THREADS 0 {} {} {}",
            3 + reapers + jobs.spawned,
            3 + reapers + jobs.peak_live,
            2 + reapers + jobs.spawned
        ),
        "{jobs:?}"
    );
    assert!(jobs.peak_live <= reapers, "{jobs:?}");
    assert!(jobs.peak_blocked <= jobs.retention, "{jobs:?}");
    assert_eq!(
        (jobs.live, jobs.blocked),
        (0, 0),
        "every worker and retention slot released: {jobs:?}"
    );
    jobs
}
/// Every dispatch (a start, a helper production or a resume) is a job given
/// to a job worker, except a resume answered to a worker blocked in place,
/// which keeps its own job. Workers are reused: none is spawned per job, and
/// each retires only at the table-byte cap or ends at the session's close.
pub(super) fn assert_jobs(trace: &[String], workers: usize, dispatches: u64) -> JobsLine {
    let jobs = assert_threads(trace, workers);
    assert_eq!(jobs.jobs + jobs.answers, dispatches, "{jobs:?}");
    assert!(jobs.spawned <= jobs.jobs.max(1), "{jobs:?}");
    assert!(jobs.retired <= jobs.spawned, "{jobs:?}");
    // Only a job installs a snapshot, and a helper is installed only for an
    // answer or a resumed continuation (light-cljs-796).
    assert!(jobs.restore_ns == 0 || jobs.jobs > 0, "{jobs:?}");
    assert!(jobs.installs <= jobs.answers + jobs.saves, "{jobs:?}");
    assert_eq!(jobs.installs > 0, jobs.install_ns > 0, "{jobs:?}");
    jobs
}
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_preparation_identity_and_private_calls() {
    // Each call reads the definition's state into a private object: a job
    // cannot write its definition's rows (light-cljs-795).
    let source = "rt => { let state = {n: 40}; return (form, env) => { const mine = {n: state.n}; mine.n = mine.n + 1; return mine.n; }; }";
    for workers in [1, 2, 4, 8] {
        // Identical compiler identity in a new session must prepare afresh.
        for _ in 0..2 {
            let session =
                GooseSession::start_mode(&runner(), workers, Some("readiness-test")).unwrap();
            thread::scope(|scope| {
                let handles = (0..8)
                    .map(|_| {
                        let session = session.clone();
                        scope.spawn(move || {
                            let e = GooseEvaluator::new(session, &Default::default()).unwrap();
                            let (_, args, call) = inputs(&e, source);
                            let f = e.source_macro_function(777, source).unwrap();
                            for _ in 0..2 {
                                let result = e.call(f, &args).unwrap();
                                let form = e
                                    .export_form(result, &call, origin(&call), Default::default())
                                    .unwrap();
                                assert!(form.data().value_eq(&Datum::new(DatumKind::Integer(41))));
                            }
                        })
                    })
                    .collect::<Vec<_>>();
                for h in handles {
                    h.join().unwrap();
                }
            });
            assert_eq!(session.stats().exports, 1);
            assert_eq!(session.stats().preparations, 1);
            assert_eq!(session.stats().jobs, 16);
            let e = GooseEvaluator::new(session.clone(), &Default::default()).unwrap();
            let (_, args, _) = inputs(&e, source);
            let other = e.source_macro_function(778, source).unwrap();
            e.call(other, &args).unwrap();
            assert_eq!(
                session.stats().exports,
                2,
                "distinct context despite equal source"
            );
            assert_eq!(session.stats().preparations, 2);
            let conflict = e
                .source_macro_function(777, "rt => (form, env) => 99")
                .unwrap();
            assert!(
                e.call(conflict, &args)
                    .unwrap_err()
                    .0
                    .contains("identity/source conflict")
            );
            cleaned(&session);
            assert_jobs(&session.trace(), workers, 17);
        }
    }
}
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_prepared_definition_keeps_live_helper_contexts() {
    for workers in [1, 2, 4, 8] {
        let session = GooseSession::start(&runner(), workers).unwrap();
        for unit in [41, 42] {
            let e = GooseEvaluator::new(session.clone(), &Default::default()).unwrap();
            let (_, args, call) = inputs(&e, "rt => (form, env) => 0");
            let source = "rt => (form, env) => __rt.cljReadRoot(globalThis.__lightCompilerRoots, null, 'fixture/value')";
            let f = e.source_macro_function(777, source).unwrap();
            let callbacks = Rc::new(Cell::new(0));
            let calls = callbacks.clone();
            serve(&e, &[("fixture/value", unit)], move |requested| {
                assert_eq!(requested, unit);
                calls.set(calls.get() + 1);
                // The real compiler likewise has equivalent native/canonical
                // emissions before/after releasing one catalog unit's IR.
                Ok(if calls.get() == 1 {
                    format!("rt => {unit}")
                } else {
                    format!("rt => ({} + 1)", unit - 1)
                })
            });
            for invocation in 0..3 {
                // Bootstrap ownership can release a declaring view's handle
                // while another published declaration still holds its code.
                let function = if invocation == 2 {
                    e.release(f).unwrap();
                    assert!(e.cached_source_macro_function(777).is_none());
                    assert!(e.call(f, &args).unwrap_err().0.contains("released"));
                    e.source_macro_function(777, source).unwrap()
                } else {
                    f
                };
                let result = e.call(function, &args).unwrap();
                let result = e
                    .export_form(result, &call, origin(&call), Default::default())
                    .unwrap();
                assert!(
                    result
                        .data()
                        .value_eq(&Datum::new(DatumKind::Integer(unit as i64)))
                );
            }
            assert_eq!(
                callbacks.get(),
                3,
                "callbacks stay live on every real body call"
            );
        }
        assert_eq!(session.stats().exports, 3, "macro and two contextual units");
        assert_eq!(session.stats().preparations, 1);
        assert_eq!(session.stats().jobs, 6);
        assert_eq!(
            session
                .trace()
                .iter()
                .filter(|s| s.starts_with("PRODUCED "))
                .count(),
            2
        );
        session.close();
        cleaned(&session);
    }
}
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_preparation_failure_cleanup() {
    for source in [
        "rt => 42",
        "rt => { null.x; return (form, env) => 1; }",
        "rt => { globalThis.forbidden = 1; return (form, env) => 1; }",
    ] {
        let session = GooseSession::start(&runner(), 2).unwrap();
        let e = GooseEvaluator::new(session.clone(), &Default::default()).unwrap();
        let (f, args, _) = inputs(&e, source);
        let failure = e.call(f, &args).unwrap_err();
        assert!(
            failure.0.contains("preparation-") && failure.0.contains("emitted-span="),
            "{failure}"
        );
        assert_eq!(session.stats().preparations, 0);
        assert_eq!(session.stats().jobs, 0);
        cleaned(&session);
    }
    let session = GooseSession::start(&runner(), 1).unwrap();
    let e = GooseEvaluator::new(
        session.clone(),
        &EvaluatorOptions {
            timeout: Duration::from_millis(50),
            ..Default::default()
        },
    )
    .unwrap();
    let (f, args, _) = inputs(&e, "rt => { while (true) {} return (form, env) => 1; }");
    let failure = e.call(f, &args).unwrap_err();
    // Native execution can hit its finite VM instruction budget before the
    // wall deadline. Both must retire the preparing session, never publish it.
    assert!(
        failure.0.contains("deadline")
            || failure.0.contains("preparation-Fault.InstructionLimit")
            || failure.0.contains("preparation-Fault.WorkLimit"),
        "{failure}"
    );
    assert_eq!(session.stats().preparations, 0);
    assert_eq!(session.stats().jobs, 0);
    cleaned(&session);
}
#[cfg(unix)]
fn fake(body: &str) -> Arc<GooseSession> {
    use std::os::unix::fs::PermissionsExt;
    let path = std::env::temp_dir().join(format!(
        "goose-protocol-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::write(&path, format!("#!/bin/sh\nprintf 'READY 1 1\\n'\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    let session = GooseSession::start(&path, 1);
    std::fs::remove_file(path).unwrap();
    session.unwrap()
}
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_handles_are_owner_scoped() {
    let session = GooseSession::start(&runner(), 1).unwrap();
    let a = GooseEvaluator::new(session.clone(), &Default::default()).unwrap();
    let b = GooseEvaluator::new(session.clone(), &Default::default()).unwrap();
    let x = a.import_form(&form(), Default::default()).unwrap();
    let y = b.import_form(&form(), Default::default()).unwrap();
    assert_eq!(x.index, y.index);
    assert!(b.release(x).unwrap_err().0.contains("foreign"));
    a.release(x).unwrap();
    assert!(a.release(x).unwrap_err().0.contains("released"));
    let second = GooseSession::start(&runner(), 1).unwrap();
    let c = GooseEvaluator::new(second.clone(), &Default::default()).unwrap();
    let z = c.import_form(&form(), Default::default()).unwrap();
    assert_eq!(z.index, x.index);
    assert!(c.release(x).is_err());
    assert!(a.release(x).unwrap_err().0.contains("released"));
    let mut owner = crate::MacroHost::new();
    owner.use_goose_source_macros(session.clone()).unwrap();
    assert!(
        crate::MacroHost::new()
            .use_goose_source_macros(session.clone())
            .is_err()
    );
    crate::compile_with_macros("(defmacro m [] 1) (m)", &mut owner).unwrap();
    assert!(
        crate::compile_with_macros("(defmacro m [] 2) (m)", &mut owner).is_err(),
        "a new compilation must not reuse old unit identities"
    );
    session.close();
    assert!(a.import_form(&form(), Default::default()).is_err());
    b.release(y).unwrap();
    second.close();
    cleaned(&session);
    cleaned(&second);
}
#[cfg(unix)]
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_exit_and_malformed_replies_cleanup() {
    for (body, malformed_graph) in [
        ("read id\nexit 0", false),
        ("read id\nprintf 'malformed\\n'\nread stop", false),
        ("read id\nprintf 'DONE 999\\n'\nread stop", false),
        (
            "read id\nprintf x > \"$1/$id.out\"\nprintf 'DONE %s\\n' \"$id\"\nread stop",
            true,
        ),
    ] {
        let session = fake(body);
        let evaluator = GooseEvaluator::new(session.clone(), &Default::default()).unwrap();
        let (function, args, form) = inputs(&evaluator, "rt => (form, env) => 1");
        let result = evaluator.call(function, &args);
        if malformed_graph {
            let value = result.unwrap();
            assert!(
                evaluator
                    .export_form(value, &form, origin(&form), Default::default())
                    .is_err()
            );
        } else {
            assert!(result.is_err());
        }
        cleaned(&session);
    }
}
/// close()'s watchdog (light-cljs-812) with fake runners: one that goes
/// silent after the close request is killed at the quiet bound; one that
/// keeps replying past the quiet bound and then exits is waited for; one
/// that replies forever is killed at the overall bound. Each kill is
/// reported, and the last handle's drop is bounded too.
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_close_watchdog_bounds_the_wait() {
    let killed = |s: &GooseSession| s.trace().iter().any(|l| l.starts_with("KILLED "));
    let quiet = Duration::from_millis(300);
    let session = fake("read stop\nexec sleep 30");
    let started = Instant::now();
    session.close_within(quiet, Duration::from_secs(20));
    let elapsed = started.elapsed();
    assert!(killed(&session), "{:?}", session.trace());
    assert!(
        elapsed >= quiet && elapsed < Duration::from_secs(5),
        "{elapsed:?}"
    );
    cleaned(&session);
    // Replies every 100 ms for about 1.5 s, five times the quiet bound.
    let session = fake(
        "read stop\ni=0\nwhile [ $i -lt 15 ]; do printf 'CLOSED 1\\n'; sleep 0.1; i=$((i+1)); done\nexit 0",
    );
    let started = Instant::now();
    session.close_within(quiet, Duration::from_secs(20));
    let elapsed = started.elapsed();
    assert!(!killed(&session), "{:?}", session.trace());
    assert!(elapsed >= Duration::from_secs(1), "{elapsed:?}");
    cleaned(&session);
    let session = fake("read stop\nwhile true; do printf 'CLOSED 1\\n'; sleep 0.05; done");
    let limit = Duration::from_secs(1);
    let started = Instant::now();
    session.close_within(quiet, limit);
    let elapsed = started.elapsed();
    assert!(killed(&session), "{:?}", session.trace());
    assert!(
        elapsed >= limit && elapsed < Duration::from_secs(5),
        "{elapsed:?}"
    );
    cleaned(&session);
    // Dropping the last handle closes with the production bounds.
    let session = fake("read stop\nexec sleep 30");
    let directory = session.directory.clone();
    let started = Instant::now();
    drop(session);
    assert!(started.elapsed() < CLOSE_QUIET + Duration::from_secs(5));
    assert!(!directory.exists());
}
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_worker_failure_and_owned_diagnostic() {
    let session = GooseSession::start(&runner(), 1).unwrap();
    let mut host = crate::MacroHost::new();
    host.use_goose_source_macros(session.clone()).unwrap();
    let source = "(defmacro bad [] (nth [1] 10))\n(bad)";
    let error = crate::analyzer::analyze_named_with_macro_warnings(
        "negative.cljs",
        source,
        &crate::parse(source).unwrap(),
        &mut host,
        Default::default(),
        &mut Vec::new(),
    )
    .unwrap_err();
    session.close();
    drop(host);
    cleaned(&session);
    let message = error.to_string();
    assert!(message.contains("negative.cljs:2:1"), "{message}");
    assert!(
        message.contains("definition negative.cljs:1:1"),
        "{message}"
    );
    assert!(
        message.contains("function=") && message.contains("emitted-span="),
        "{message}"
    );
    // Independent reference: compare the supported origin and call location.
    let reference = crate::analyzer::analyze_named_with_macro_warnings(
        "negative.cljs",
        source,
        &crate::parse(source).unwrap(),
        &mut crate::MacroHost::new(),
        Default::default(),
        &mut Vec::new(),
    )
    .unwrap_err()
    .to_string();
    assert!(
        reference.contains("negative.cljs:2:1")
            && reference.contains("definition negative.cljs:1:1"),
        "{reference}"
    );
}
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_helper_failure_keeps_instruction_context() {
    let session = GooseSession::start(&runner(), 1).unwrap();
    let mut host = crate::MacroHost::new();
    host.use_goose_source_macros(session.clone()).unwrap();
    let source = "(def broken (nth [1] 10))\n(defmacro bad [] broken)\n(bad)";
    let error = crate::analyzer::analyze_named_with_macro_warnings(
        "helper-negative.cljs",
        source,
        &crate::parse(source).unwrap(),
        &mut host,
        Default::default(),
        &mut Vec::new(),
    )
    .unwrap_err();
    session.close();
    drop(host);
    cleaned(&session);
    let message = error.to_string();
    assert!(message.contains("helper-negative.cljs:3:1"), "{message}");
    assert!(
        message.contains("definition helper-negative.cljs:2:1"),
        "{message}"
    );
    assert!(message.contains("TypeError function="), "{message}");
    for field in ["function=", "pc=", "emitted-span="] {
        let value = message.split_once(field).expect(&message).1;
        assert!(value.starts_with(|c: char| c.is_ascii_digit()), "{message}");
    }
}

#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_caught_runtime_errors_preserve_identity() {
    for body in ["null.x;", "let x = 1; x();"] {
        let source = format!(
            "(defmacro caught [] (js* \"(() => {{try {{{body}}} catch(e) {{return e === null;}}}})()\")) (caught)"
        );
        let reference = crate::compile_with_macros(&source, &mut crate::MacroHost::new()).unwrap();
        assert!(reference.contains("false"), "{reference}");
        let session = GooseSession::start(&runner(), 1).unwrap();
        let mut host = crate::MacroHost::new();
        host.use_goose_source_macros(session.clone()).unwrap();
        assert_eq!(
            crate::compile_with_macros(&source, &mut host).unwrap(),
            reference
        );
        session.close();
        cleaned(&session);
    }
    // Explicitly thrown primitive values keep their ordinary JS semantics.
    let source = "(defmacro caught [] (js* \"(() => {try {throw null;} catch(e) {return e === null;}})()\")) (caught)";
    let reference = crate::compile_with_macros(source, &mut crate::MacroHost::new()).unwrap();
    let session = GooseSession::start(&runner(), 1).unwrap();
    let mut host = crate::MacroHost::new();
    host.use_goose_source_macros(session.clone()).unwrap();
    assert_eq!(
        crate::compile_with_macros(source, &mut host).unwrap(),
        reference
    );
    session.close();
    cleaned(&session);
    // The caught error is the realm's (light-cljs-788): its type's `name`
    // and prototype, as in the V8 build.
    let source = "(defmacro caught [] (js* \"(() => {try {null.x;} catch(e) {return e.name === 'TypeError' && Object.getPrototypeOf(e) === TypeError.prototype;}})()\")) (caught)";
    let reference = crate::compile_with_macros(source, &mut crate::MacroHost::new()).unwrap();
    assert!(reference.contains("true"), "{reference}");
    let session = GooseSession::start(&runner(), 1).unwrap();
    let mut host = crate::MacroHost::new();
    host.use_goose_source_macros(session.clone()).unwrap();
    assert_eq!(
        crate::compile_with_macros(source, &mut host).unwrap(),
        reference
    );
    session.close();
    cleaned(&session);
}
/// Control flow across helper waits (light-cljs-803): a pending Return,
/// Throw or Jump completion of a finally block that demands a root, and a
/// ToPrimitive native frame whose `valueOf` demands one, survive the wait,
/// in place (retained workers) and as a saved continuation (none retained).
/// A session cancelled during any of these waits runs the job's pending
/// finally blocks, innermost first and across frames, and the runner
/// reports the keys their writes named, so a skipped finally block of a
/// saved waiter fails here.
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_control_flow_across_helper_waits_and_cancellation() {
    const ROOTS: [(&str, usize); 3] = [("fixture/a", 1), ("fixture/b", 2), ("fixture/s", 3)];
    // Each body runs inside two finally blocks of the macro's frame; the
    // result is the cleanup order's digits, then the completion values.
    let cases: [(&str, &str, i64, &[usize]); 5] = [
        (
            "return",
            "function f() { try { try { note(1); return 7; } finally { note(2); rec.waited = read('fixture/a'); note(3); } } finally { rec.frame = 1; note(4); } } value = f() + rec.waited;",
            123489047,
            &[1],
        ),
        (
            "throw",
            "function g() { try { try { note(1); throw 7; } finally { note(2); rec.waited = read('fixture/a'); note(3); } } finally { rec.frame = 1; note(4); } } try { g(); } catch (e) { value = e + rec.waited; }",
            123489047,
            &[1],
        ),
        (
            "jump",
            "function h() { let n = 0; try { for (let i = 0; i < 2; i++) { try { note(1); if (i == 0) continue; n += 7; break; } finally { note(2); n += read(i == 0 ? 'fixture/a' : 'fixture/b'); note(3); } } } finally { rec.frame = 1; note(4); } return n; } value = h();",
            123123489097,
            &[1, 2],
        ),
        (
            "to-primitive",
            "let calls = 0; const o = { valueOf: function () { try { calls += 1; note(1); return read('fixture/a'); } finally { rec.frame = 1; note(4); } } }; value = (o + 2) + calls * 100;",
            1489142,
            &[1],
        ),
        // A string from the helper converts as a string.
        (
            "to-primitive-string",
            "let calls = 0; const s = { valueOf: function () { try { calls += 1; note(1); return read('fixture/s'); } finally { rec.frame = 1; note(4); } } }; value = ((s + 'y') === 'xy' ? 5 : 0) + calls * 100;",
            1489105,
            &[3],
        ),
    ];
    // fixture/a and fixture/b are the numbers 40 and 50, fixture/s is 'x'.
    let helper = |unit: usize| -> Result<String, String> {
        Ok(if unit == 3 {
            "rt => 'x'".to_owned()
        } else {
            format!("rt => {}", 30 + 10 * unit)
        })
    };
    for retained in [0, 2] {
        for (name, body, expected, units) in cases {
            let source = format!(
                "rt => (form, env) => {{ const read = name => __rt.cljReadRoot(globalThis.__lightCompilerRoots, null, name); const rec = {{}}; let order = 0; const note = d => {{ order = order * 10 + d; }}; let value = 0; try {{ try {{ {body} }} finally {{ rec.inner = 1; note(8); }} }} finally {{ rec.outer = 1; note(9); }} return order * 1000 + value; }}"
            );
            let context = format!("{name} retained={retained}");
            // Completed: every completion value and the cleanup order, each
            // root produced and each method run exactly once.
            let session =
                GooseSession::start_retaining(&runner(), 1, Some("readiness-test"), Some(retained))
                    .unwrap();
            let e = GooseEvaluator::new(session.clone(), &Default::default()).unwrap();
            let (function, args, call) = inputs(&e, &source);
            let demanded = Rc::new(RefCell::new(Vec::new()));
            let seen = demanded.clone();
            serve(&e, &ROOTS, move |unit| {
                seen.borrow_mut().push(unit);
                helper(unit)
            });
            let result = e
                .call(function, &args)
                .unwrap_or_else(|error| panic!("{context}: {error:?}"));
            let result = e
                .export_form(result, &call, origin(&call), Default::default())
                .unwrap();
            assert!(
                result
                    .data()
                    .value_eq(&Datum::new(DatumKind::Integer(expected))),
                "{context}: {:?}",
                result.data()
            );
            assert_eq!(*demanded.borrow(), units, "{context}");
            session.close();
            cleaned(&session);
            let trace = session.trace();
            let jobs = assert_threads(&trace, 1);
            // Each wait blocks in place or is saved as a continuation.
            let n = units.len() as u64;
            assert_eq!(
                (jobs.waits, jobs.saves),
                if retained == 0 { (0, n) } else { (n, 0) },
                "{context}: {jobs:?}"
            );
            // Cancelled during each wait: the finally blocks pending there run.
            for &target in units {
                let session = GooseSession::start_retaining(
                    &runner(),
                    1,
                    Some("readiness-test"),
                    Some(retained),
                )
                .unwrap();
                let e = GooseEvaluator::new(session.clone(), &Default::default()).unwrap();
                let (function, args, _) = inputs(&e, &source);
                let control = e.control();
                serve(&e, &ROOTS, move |unit| {
                    if unit == target {
                        control.cancel();
                    }
                    helper(unit)
                });
                let error = e.call(function, &args).unwrap_err().to_string();
                assert!(error.contains("cancelled"), "{context} {target}: {error}");
                cleaned(&session);
                let trace = session.trace();
                let reports = trace
                    .iter()
                    .filter(|line| line.contains("cancelled"))
                    .collect::<Vec<_>>();
                assert_eq!(reports.len(), 1, "{context} {target}: {trace:?}");
                assert!(
                    reports[0].starts_with("ERROR ")
                        && reports[0].ends_with(" cancelled cleanup=frame,inner,outer"),
                    "{context} {target}: {trace:?}"
                );
                let jobs = assert_threads(&trace, 1);
                let n = units.iter().position(|&u| u == target).unwrap() as u64 + 1;
                assert_eq!(
                    (jobs.waits, jobs.saves),
                    if retained == 0 { (0, n) } else { (n, 0) },
                    "{context} {target}: {jobs:?}"
                );
            }
        }
    }
}
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_cancel_and_deadline_cleanup() {
    for source in [
        "rt => (form, env) => __rt.cljReadRoot(globalThis.__lightCompilerRoots, null, 'fixture/value')",
        "rt => (form, env) => __rt.cljCount(rt.core.mapSeq(x=>__rt.cljReadRoot(globalThis.__lightCompilerRoots,null,'fixture/value'),__rt.formList(1,2)))",
    ] {
        for cancel in [false, true] {
            let session = GooseSession::start(&runner(), 1).unwrap();
            let options = EvaluatorOptions {
                timeout: if cancel {
                    Duration::from_secs(2)
                } else {
                    Duration::from_millis(30)
                },
                ..Default::default()
            };
            let evaluator = GooseEvaluator::new(session.clone(), &options).unwrap();
            let (function, args, _) = inputs(&evaluator, source);
            serve(&evaluator, &[("fixture/value", 0)], |_| {
                thread::sleep(Duration::from_millis(60));
                Ok("rt => __rt.formVector(1)".into())
            });
            let control = evaluator.control();
            let s = session.clone();
            let cancelling = cancel.then(|| {
                thread::spawn(move || {
                    let until = Instant::now() + Duration::from_secs(1);
                    while !s.trace().iter().any(|line| line.starts_with("NEED "))
                        && Instant::now() < until
                    {
                        thread::sleep(Duration::from_millis(1));
                    }
                    control.cancel();
                })
            });
            let error = evaluator.call(function, &args).unwrap_err().to_string();
            if let Some(t) = cancelling {
                t.join().unwrap();
            }
            assert!(
                error.contains(if cancel { "cancelled" } else { "deadline" }),
                "{error}"
            );
            cleaned(&session);
        }
    }
}

#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_rejects_shared_writes_before_mutation() {
    for source in [
        "(defmacro effect [] (js* \"(globalThis.gooseForbidden = 1)\") 1) (effect)",
        // The realm (light-cljs-788) defines Array; a name the image lacks
        // stays a boundary.
        "(defmacro effect [] (js* \"(typeof globalThis.gooseAbsentName === 'undefined' ? 1 : 2)\")) (effect)",
    ] {
        let session = GooseSession::start(&runner(), 1).unwrap();
        let mut host = crate::MacroHost::new();
        host.use_goose_source_macros(session.clone()).unwrap();
        let error = crate::compile_with_macros(source, &mut host)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("NativeBoundary") && error.contains("writes=0"),
            "{error}"
        );
        cleaned(&session);
        assert!(
            crate::compile_with_macros(source, &mut crate::MacroHost::new()).is_ok(),
            "V8 separately supports this explicitly rejected Goose job"
        );
    }
}

/// A built-in step or a realm property the VM does not implement yet
/// (light-cljs-788) fails by name, even under a `catch`, and the compiler's
/// diagnostic names it; the V8 build runs both.
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_names_unsupported_builtin_steps() {
    for (body, name) in [
        ("Object(1)", "Object:-primitive-wrapper-objects"),
        ("[1].filter((x) => x)", "pending-filter"),
    ] {
        let source = format!(
            "(defmacro m [] (js* \"(() => {{ try {{ {body}; return 1; }} catch (e) {{ return 2; }} }})()\")) (m)"
        );
        assert!(crate::compile_with_macros(&source, &mut crate::MacroHost::new()).is_ok());
        let session = GooseSession::start(&runner(), 1).unwrap();
        let mut host = crate::MacroHost::new();
        host.use_goose_source_macros(session.clone()).unwrap();
        let error = crate::compile_with_macros(&source, &mut host)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("Fault.UnsupportedOperand") && error.contains(name),
            "{error}"
        );
        cleaned(&session);
    }
}

#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_rejects_root_reads_that_bypass_the_compiler_intrinsic() {
    let source = "(def value 1) (defmacro bypass [] (js* \"globalThis.__lightCompilerRoots['cljs.user/value']\")) (bypass)";
    let session = GooseSession::start(&runner(), 1).unwrap();
    let mut host = crate::MacroHost::new();
    host.use_goose_source_macros(session.clone()).unwrap();
    let error = crate::compile_with_macros(source, &mut host)
        .unwrap_err()
        .to_string();
    assert!(error.contains("DependencyNeed"), "{error}");
    cleaned(&session);
    assert!(crate::compile_with_macros(source, &mut crate::MacroHost::new()).is_ok());
}

#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_injected_worker_abort_cleans_entire_session() {
    // A deliberately truncated real worker packet causes a native assertion,
    // distinct from a user-program TypeError. No fake protocol process here.
    for workers in [1, 2, 4, 8] {
        let session = GooseSession::start(&runner(), workers).unwrap();
        let e = GooseEvaluator::new(session.clone(), &Default::default()).unwrap();
        let (function, args, _) = inputs(&e, "rt => (form, env) => 42");
        session
            .export(
                &session.directory.join("definition-2.gsp"),
                "rt => (form, env) => 1",
            )
            .unwrap();
        std::fs::write(session.directory.join("1.definition"), "2").unwrap();
        std::fs::write(session.directory.join("1.gfb"), b"GFB1").unwrap();
        let (tx, rx) = mpsc::channel();
        session.pending.lock().unwrap().insert(1, tx);
        {
            let mut process = session.process.lock().unwrap();
            let input = &mut process.as_mut().unwrap().input;
            writeln!(input, "1").unwrap();
            input.flush().unwrap();
        }
        let until = Instant::now() + Duration::from_secs(5);
        loop {
            match rx.recv_timeout(Duration::from_millis(2)) {
                Ok(Event::Failure(message)) => {
                    assert!(message.contains("worker-abort"), "{message}");
                    break;
                }
                Ok(Event::Start | Event::Resumed) => {}
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    assert!(
                        Instant::now() < until,
                        "native worker did not report the injected abort"
                    );
                }
                other => panic!("native abort must be an infrastructure failure: {other:?}"),
            }
        }
        assert!(
            e.call(function, &args)
                .unwrap_err()
                .0
                .contains("worker-abort")
        );
        session.close();
        cleaned(&session);
        assert!(
            session
                .trace()
                .iter()
                .any(|s| s.contains("worker-abort") && s.contains("assert failed")),
            "contained worker diagnostic must reach the owning request: {:?}",
            session.trace()
        );
        assert!(session.trace().iter().any(|s| s == "CLOSED 1"));
        let fresh = GooseSession::start(&runner(), workers).unwrap();
        let e = GooseEvaluator::new(fresh.clone(), &Default::default()).unwrap();
        let (function, args, call) = inputs(&e, "rt => (form, env) => 42");
        let result = e.call(function, &args).unwrap();
        assert!(
            e.export_form(result, &call, origin(&call), Default::default())
                .unwrap()
                .data()
                .value_eq(&Datum::new(DatumKind::Integer(42)))
        );
        fresh.close();
        cleaned(&fresh);
    }
}

#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_cleanup_abort_cancels_a_started_replacement() {
    for workers in [1, 2, 4, 8] {
        let session =
            GooseSession::start_mode(&runner(), workers, Some("cleanup-abort-test")).unwrap();
        let e = GooseEvaluator::new(session.clone(), &Default::default()).unwrap();
        let (_, args, _) = inputs(&e, "rt => (form, env) => 42");
        let values = args
            .iter()
            .map(|&id| e.get(id).unwrap())
            .collect::<Vec<_>>();
        let limits = ConversionLimits::default();
        let budget = bridge::Budget::new(limits.nodes, e.control.clone());
        let bytes = bridge::Graph::arguments(&values, limits, &budget)
            .unwrap()
            .encode()
            .unwrap();
        session
            .export(
                &session.directory.join("definition-1.gsp"),
                "rt => (form, env) => 42",
            )
            .unwrap();
        let send = |id| {
            std::fs::write(session.directory.join(format!("{id}.definition")), "1").unwrap();
            std::fs::write(session.directory.join(format!("{id}.gfb")), &bytes).unwrap();
            let (tx, rx) = mpsc::channel();
            session.pending.lock().unwrap().insert(id, tx);
            let mut process = session.process.lock().unwrap();
            let input = &mut process.as_mut().unwrap().input;
            writeln!(input, "{id}").unwrap();
            input.flush().unwrap();
            rx
        };
        let wait = |ready: &dyn Fn() -> bool| {
            let until = Instant::now() + Duration::from_secs(5);
            while !ready() {
                assert!(
                    Instant::now() < until,
                    "native cleanup stalled: {:?}",
                    session.trace()
                );
                thread::yield_now();
            }
        };
        let first = send(1);
        loop {
            match first.recv_timeout(Duration::from_secs(5)).unwrap() {
                Event::Done(_) => break,
                Event::Start | Event::Resumed => {}
                other => panic!("unexpected first result: {other:?}"),
            }
        }
        wait(&|| session.directory.join("cleanup-ready").exists());
        // Worker 1 has published its result, released its execution slot and
        // still runs. Its replacement gets a worker of its own even at limit
        // 1, held before it executes; poison must cancel it, not forget it.
        let replacement = send(2);
        wait(&|| session.trace().iter().any(|s| s == "START 2"));
        wait(&|| session.directory.join("claim-ready").exists());
        std::fs::write(session.directory.join("cleanup-abort"), "abort").unwrap();
        loop {
            match replacement.recv_timeout(Duration::from_secs(5)).unwrap() {
                Event::Failure(message) => {
                    assert!(message.contains("worker-abort"), "{message}");
                    break;
                }
                Event::Start | Event::Resumed => {}
                other => panic!("replacement must fail without execution: {other:?}"),
            }
        }
        std::fs::write(session.directory.join("claim-release"), "release").unwrap();
        {
            let mut process = session.process.lock().unwrap();
            let input = &mut process.as_mut().unwrap().input;
            writeln!(input, "0").unwrap();
            input.flush().unwrap();
        }
        // Observe native closure before calling close: close()'s kill
        // fallback must not turn a deadlocked native shutdown into a pass.
        wait(&|| session.trace().iter().any(|s| s == "CLOSED 1"));
        assert!(!session.directory.join("2.out").exists());
        assert!(!session.trace().iter().any(|s| s == "DONE 2"));
        assert_threads(&session.trace(), workers);
        session.close();
        cleaned(&session);
    }
}

#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_every_job_runs_in_its_own_worker() {
    // Each call mutates the prepared definition's closure state: a counter,
    // a new property and an array element. Those rows are the definition's,
    // not the job's, so the first write faults NativeBoundary before it
    // mutates (light-cljs-795, as amended by light-cljs-797) instead of
    // being undone silently when the job ends; one worker serves every job.
    let source = "rt => { let state = {n: 40}; let list = []; return (form, env) => { state.n = state.n + 1; state.seen = (state.seen || 0) + 1; list[list.length] = 1; return state.n + state.seen + list.length; }; }";
    for workers in [1, 2, 4, 8] {
        let session = GooseSession::start_mode(&runner(), workers, Some("readiness-test")).unwrap();
        let e = GooseEvaluator::new(session.clone(), &Default::default()).unwrap();
        let (_, args, _) = inputs(&e, source);
        let f = e.source_macro_function(901, source).unwrap();
        let message = e.call(f, &args).unwrap_err().0;
        assert!(message.contains("Fault.NativeBoundary"), "{message}");
        session.close();
        cleaned(&session);
        let jobs = assert_jobs(&session.trace(), workers, 1);
        assert_eq!((jobs.spawned, jobs.waits), (1, 0), "{jobs:?}");
    }
}

/// Submit raw job `id` of definition `key` with the fixture's arguments.
fn submit_key(session: &GooseSession, bytes: &[u8], id: u64, key: u64) -> mpsc::Receiver<Event> {
    std::fs::write(
        session.directory.join(format!("{id}.definition")),
        key.to_string(),
    )
    .unwrap();
    std::fs::write(session.directory.join(format!("{id}.gfb")), bytes).unwrap();
    let (tx, rx) = mpsc::channel();
    session.pending.lock().unwrap().insert(id, tx);
    let mut process = session.process.lock().unwrap();
    let input = &mut process.as_mut().unwrap().input;
    writeln!(input, "{id}").unwrap();
    input.flush().unwrap();
    rx
}

/// Reused job workers (light-cljs-795): the jobs of `plan` (definition keys,
/// in order, one at a time) on a session whose workers retire above `cap`
/// table bytes. Returns each job's outcome (its result or failure, the
/// FINISH line with steps and writes, and the NEED lines) and the JOBS line.
fn reuse_session(
    workers: usize,
    cap: Option<u64>,
    retained: Option<usize>,
    plan: &[u64],
) -> (Vec<(u64, String)>, JobsLine) {
    let definitions: [(u64, &str); 17] = [
        // Pure: garbage in every table, a fresh object and array.
        (
            1,
            "rt => (form, env) => { const o = {a: 1}; o.b = 2; const xs = []; for (let i = 0; i < 50; i++) xs[xs.length] = {k: i, o: o}; return o.a + o.b + xs.length; }",
        ),
        // Writes into rows the job did not allocate fault before they mutate:
        // the definition's object, a closure environment, a global, an
        // intrinsic.
        (
            2,
            "rt => { const state = {n: 0}; return (form, env) => { state.n = state.n + 1; return state.n; }; }",
        ),
        (
            3,
            "rt => { let n = 0; return (form, env) => { n = n + 1; return n; }; }",
        ),
        (4, "rt => (form, env) => { globalThis.leak = 1; return 1; }"),
        (5, "rt => (form, env) => { rt.leak = 1; return 1; }"),
        // A limit and a VM fault.
        (
            6,
            "rt => (form, env) => { let x = 0; while (true) { x = x + 1; } }",
        ),
        (7, "rt => (form, env) => { const o = null; return o.x; }"),
        // Root reads write __lightCompilerRoots and the published-helper
        // cache; every job still asks the compiler.
        (
            8,
            "rt => (form, env) => { const f = __rt.cljReadRoot(globalThis.__lightCompilerRoots, null, 'fixture/function'); return f.call(null) + 1; }",
        ),
        (
            9,
            "rt => (form, env) => { const v = __rt.cljReadRoot(globalThis.__lightCompilerRoots, null, 'fixture/value'); return v + 1; }",
        ),
        // Many rows: tables above a small cap retire the worker.
        (
            10,
            "rt => (form, env) => { const xs = []; for (let i = 0; i < 20000; i++) xs[xs.length] = {i: i}; return xs.length; }",
        ),
        // Element stores into the definition's array: an existing element
        // and a new one.
        (
            11,
            "rt => { const xs = [1, 2]; return (form, env) => { xs[0] = 5; return xs[0]; }; }",
        ),
        (
            12,
            "rt => { const xs = [1, 2]; return (form, env) => { xs[xs.length] = 3; return xs.length; }; }",
        ),
        // A root read whose receiver is not the compiler roots object would
        // have the answer written into a retained image row.
        (
            13,
            "rt => (form, env) => { const v = __rt.cljReadRoot(globalThis, null, 'fixture/value'); return v + 1; }",
        ),
        // A missing dependency (key -1) is a catchable failure; the job's own
        // object, array and captured local stay writable after it resumes.
        (
            14,
            "rt => (form, env) => { const o = {n: 0}; const xs = []; let k = 0; const f = () => { k = 2; }; try { __rt.cljReadRoot(globalThis.__lightCompilerRoots, null, 'fixture/missing'); } catch (e) { o.n = 1; xs[0] = 4; f(); } return o.n + k + xs[0]; }",
        ),
        // The realm (light-cljs-788), inherited from the image: a job's
        // mutation of an intrinsic faults before it mutates, and the next job
        // sees the intrinsics as built, its literals' prototypes, a built-in
        // method, and a VM fault's error object with its type's prototype
        // and a message.
        (
            15,
            "rt => (form, env) => { Array.prototype.leak = 1; return 1; }",
        ),
        (
            16,
            "rt => (form, env) => { let n = 0; if (Object.getPrototypeOf([]) === Array.prototype && Object.getPrototypeOf({}) === Object.prototype) n += 1; if (typeof Object.prototype.hasOwnProperty === 'function') n += 2; if ([].leak === undefined && !Array.prototype.hasOwnProperty('leak')) n += 4; try { const o = null; o.x; } catch (e) { if (Object.getPrototypeOf(e) === TypeError.prototype && typeof e.message === 'string' && e.message.length > 0) n += 8; } return n; }",
        ),
        // A pure job's own atom and volatile (light-cljs-799) are rows like
        // any other: swap! with a built-in and with a closure and extra
        // arguments, reset!, vreset!, compare-and-set!.
        (
            17,
            "rt => (form, env) => { const a = rt.core.atom(0); rt.core.swap(a, rt.core.inc); rt.core.swap(a, (x, y) => x + y, 5); rt.core.reset(a, rt.core.deref(a) + 1); const v = rt.core.volatile(5); rt.core.vreset(v, rt.core.deref(v) + rt.core.deref(a)); return rt.core.deref(v) + (rt.core.compareAndSet(a, 7, 10) ? 100 : 0) + (rt.core.compareAndSet(a, 7, 11) ? 1000 : 0); }",
        ),
    ];
    let mut env = vec![];
    if let Some(cap) = cap {
        env.push(("BBP_WORKER_TABLE_BYTES", cap.to_string()));
    }
    if let Some(retained) = retained {
        env.push(("BBP_RETAINED_WORKERS", retained.to_string()));
    }
    let session =
        GooseSession::start_env(&runner(), workers, Some("readiness-test"), &env).unwrap();
    let e = GooseEvaluator::new(session.clone(), &Default::default()).unwrap();
    let (_, args, call) = inputs(&e, "rt => (form, env) => 0");
    let values = args
        .iter()
        .map(|&id| e.get(id).unwrap())
        .collect::<Vec<_>>();
    let limits = ConversionLimits::default();
    let budget = bridge::Budget::new(limits.nodes, e.control.clone());
    let graph = bridge::Graph::arguments(&values, limits, &budget).unwrap();
    let bytes = graph.encode().unwrap();
    for (key, source) in definitions {
        session
            .export(
                &session.directory.join(format!("definition-{key}.gsp")),
                source,
            )
            .unwrap();
    }
    let mut outcomes = Vec::new();
    for (index, &key) in plan.iter().enumerate() {
        let id = index as u64 + 1;
        if key == 14 {
            session.answer_key(id, 0, "-1").unwrap();
        }
        if key == 8 || key == 9 {
            let helper = if key == 8 {
                "rt => {const result=41; return function(){return result;};}"
            } else {
                "rt => 41"
            };
            session
                .export(&session.directory.join(format!("{id}-0.helper")), helper)
                .unwrap();
            session.answer_key(id, 0, &key.to_string()).unwrap();
        }
        let rx = submit_key(&session, &bytes, id, key);
        let result = loop {
            match rx.recv_timeout(Duration::from_secs(20)).unwrap() {
                Event::Done(_) => {
                    let out = std::fs::read(session.directory.join(format!("{id}.out"))).unwrap();
                    let output = bridge::Graph::decode(&out, limits, &budget)
                        .unwrap()
                        .expansion(&call, origin(&call), &graph.originals, limits, &budget)
                        .unwrap();
                    break format!("{:?}", output.data());
                }
                Event::Failure(message) => break message,
                Event::Start
                | Event::Resumed
                | Event::Produced
                | Event::Need(_, _)
                | Event::Ready => {}
            }
        };
        let trace = session.trace();
        let lines = trace
            .iter()
            .filter(|s| {
                s.starts_with(&format!("FINISH {id} "))
                    || s.starts_with(&format!("NEED {id} "))
                    || s.starts_with(&format!("ERROR {id} "))
            })
            .cloned()
            .collect::<Vec<_>>();
        // Without the job id, so equal jobs compare equal.
        let outcome = format!("{result} {lines:?}").replace(&format!(" {id} "), " # ");
        outcomes.push((key, outcome));
        // The worker publishes its reset after the result: let the
        // coordinator see it idle before the next job.
        thread::sleep(Duration::from_millis(20));
    }
    session.close();
    cleaned(&session);
    let trace = session.trace();
    assert!(
        !String::from_utf8_lossy(&session.stderr.lock().unwrap()).contains("assert failed"),
        "{trace:?}"
    );
    // A start per job, a resume per root read and one helper production per
    // root the session produced.
    let reads = plan.iter().filter(|&&k| [8, 9, 14].contains(&k)).count() as u64;
    let productions = [8, 9].iter().filter(|k| plan.contains(k)).count() as u64;
    let jobs = assert_jobs(&trace, workers, plan.len() as u64 + reads + productions);
    (outcomes, jobs)
}

#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_reused_workers_start_every_job_from_the_image() {
    // Job 1 first in a fresh worker, then after every other kind of job on
    // the same reused worker, and the whole plan once more with a cap of 0,
    // which retires the worker after every job (a fresh worker each): every
    // job's result, events, steps and limit outcome are the same.
    let plan = [
        1, 2, 1, 3, 1, 4, 1, 5, 1, 6, 1, 7, 1, 8, 8, 1, 9, 9, 1, 11, 1, 12, 1, 13, 1, 14, 14, 1,
        16, 15, 16, 1, 17, 1, 17,
    ];
    for workers in [1, 4] {
        let (reused, jobs) = reuse_session(workers, Some(16 << 20), None, &plan);
        let (fresh, control) = reuse_session(workers, Some(0), None, &plan);
        assert_eq!(reused, fresh);
        let first = &reused[0].1;
        assert!(first.contains("Double(53.0)"), "{first}");
        for (key, outcome) in &reused {
            let expected = match key {
                1 => "Double(53.0)",
                2..=5 | 11..=13 | 15 => "Fault.NativeBoundary",
                16 => "Double(15.0)",
                17 => "Double(112.0)",
                14 => "Double(7.0)",
                6 => "Fault.InstructionLimit",
                7 => "Fault.TypeError",
                _ => "Double(42.0)",
            };
            assert!(outcome.contains(expected), "{key}: {outcome}");
            if *key == 1 {
                assert_eq!(outcome, first, "job 1 after other jobs");
            }
            // Root reads ask the compiler in every job.
            if [8, 9, 14].contains(key) {
                assert!(outcome.contains("NEED "), "{key}: {outcome}");
            }
        }
        // Two workers served the whole plan (a helper production needs a second
        // while the demanding job blocks); the control spawned one a job.
        assert_eq!((jobs.spawned, jobs.retired), (2, 0), "{jobs:?}");
        assert!(jobs.reset_ns > 0, "{jobs:?}");
        assert_eq!(
            (control.spawned, control.retired),
            (control.jobs, control.jobs),
            "{control:?}"
        );
    }
}

#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_missing_dependency_resume_keeps_the_jobs_rows_writable() {
    // A missing dependency resumes in its blocked worker, or (retention 0)
    // from a saved continuation in any worker: either way the rows the job
    // allocated before the wait stay its own.
    for retained in [None, Some(0)] {
        let (outcomes, jobs) = reuse_session(1, Some(16 << 20), retained, &[14, 1, 14]);
        for (key, outcome) in &outcomes {
            let expected = if *key == 14 {
                "Double(7.0)"
            } else {
                "Double(53.0)"
            };
            assert!(outcome.contains(expected), "{retained:?} {key}: {outcome}");
        }
        assert_eq!(outcomes[0], outcomes[2]);
        assert_eq!(
            jobs.saves,
            if retained == Some(0) { 2 } else { 0 },
            "{jobs:?}"
        );
    }
}

#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_reused_worker_retires_at_its_table_cap() {
    // Under a 1 MB cap the large job's tables retire its worker; the next
    // job gets a new worker and the same result.
    let (outcomes, jobs) = reuse_session(1, Some(1 << 20), None, &[1, 10, 1, 1]);
    assert!(outcomes[1].1.contains("Double(20000.0)"), "{outcomes:?}");
    assert_eq!(outcomes[0], outcomes[2]);
    assert_eq!(outcomes[0], outcomes[3]);
    assert_eq!(
        (jobs.jobs, jobs.spawned, jobs.retired),
        (4, 2, 1),
        "{jobs:?}"
    );
    // The default cap (16 MiB, light-cljs-791) keeps the worker through the
    // large job: one worker serves the plan and none retires.
    let (defaults, jobs) = reuse_session(1, None, None, &[1, 10, 1, 1]);
    assert_eq!(defaults, outcomes);
    assert_eq!(
        (jobs.jobs, jobs.spawned, jobs.retired),
        (4, 1, 0),
        "{jobs:?}"
    );
}

/// Submit raw job `id` of definition 1 with the fixture's arguments.
fn submit(session: &GooseSession, bytes: &[u8], id: u64) -> mpsc::Receiver<Event> {
    std::fs::write(session.directory.join(format!("{id}.definition")), "1").unwrap();
    std::fs::write(session.directory.join(format!("{id}.gfb")), bytes).unwrap();
    let (tx, rx) = mpsc::channel();
    session.pending.lock().unwrap().insert(id, tx);
    let mut process = session.process.lock().unwrap();
    let input = &mut process.as_mut().unwrap().input;
    writeln!(input, "{id}").unwrap();
    input.flush().unwrap();
    rx
}
fn await_need(rx: &mpsc::Receiver<Event>) {
    loop {
        match rx.recv_timeout(Duration::from_secs(10)).unwrap() {
            Event::Need(_, _) => return,
            Event::Start | Event::Resumed => {}
            other => panic!("expected a dependency wait: {other:?}"),
        }
    }
}

#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_blocked_waits_release_worker_and_retention() {
    // Jobs that wait on a compiler root nobody answers. Each scenario ends
    // with every job worker reaped and no retention slot held.
    let fixture = |workers: usize, mode: Option<&str>, retained: Option<usize>| {
        let session = GooseSession::start_retaining(&runner(), workers, mode, retained).unwrap();
        let e = GooseEvaluator::new(session.clone(), &Default::default()).unwrap();
        let (_, args, _) = inputs(&e, "rt => (form, env) => 0");
        let values = args
            .iter()
            .map(|&id| e.get(id).unwrap())
            .collect::<Vec<_>>();
        let limits = ConversionLimits::default();
        let budget = bridge::Budget::new(limits.nodes, e.control.clone());
        let bytes = bridge::Graph::arguments(&values, limits, &budget)
            .unwrap()
            .encode()
            .unwrap();
        session.export(&session.directory.join("definition-1.gsp"), "rt => (form, env) => __rt.cljReadRoot(globalThis.__lightCompilerRoots, null, 'fixture/value')").unwrap();
        (session, bytes)
    };
    // Cancellation: a blocked worker leaves when the session closes.
    let (session, bytes) = fixture(1, Some("readiness-test"), Some(4));
    await_need(&submit(&session, &bytes, 1));
    session.close();
    cleaned(&session);
    let jobs = assert_threads(&session.trace(), 1);
    assert_eq!(
        (
            jobs.jobs,
            jobs.waits,
            jobs.peak_blocked,
            jobs.answers,
            jobs.saves
        ),
        (1, 1, 1, 0, 0),
        "{jobs:?}"
    );
    // Over the budget: the first wait blocks, later ones save a continuation
    // and their workers go idle (a later job may reuse one); the blocked one
    // is cancelled at close, and each saved one is restored at close to run
    // its finally blocks, a job of its own (light-cljs-803).
    let (session, bytes) = fixture(1, Some("readiness-test"), Some(1));
    for id in 1..=3 {
        await_need(&submit(&session, &bytes, id));
    }
    session.close();
    cleaned(&session);
    let jobs = assert_threads(&session.trace(), 1);
    assert_eq!(
        (jobs.jobs, jobs.waits, jobs.peak_blocked, jobs.saves),
        (3 + 2, 3, 1, 2),
        "{jobs:?}"
    );
    assert!((2..=3).contains(&jobs.spawned), "{jobs:?}");
    // Saturated retention: eight simultaneous waits at limit 8 with a budget
    // of one. Jobs above 10 (cleanup-abort-test) leave their answers queued
    // until released, so seven save answers sit on the Answer queue at once.
    let (session, bytes) = fixture(8, Some("cleanup-abort-test"), Some(1));
    let replies = (11..=18)
        .map(|id| submit(&session, &bytes, id))
        .collect::<Vec<_>>();
    let until = Instant::now() + Duration::from_secs(10);
    // SCHED's events count reaches 8 once the coordinator handled all eight waits.
    while !session
        .trace()
        .iter()
        .any(|s| s.starts_with("SCHED ") && s.split_whitespace().nth(10) == Some("8"))
    {
        assert!(Instant::now() < until, "{:?}", session.trace());
        thread::yield_now();
    }
    std::fs::write(session.directory.join("answer-release"), "release").unwrap();
    for rx in &replies {
        await_need(rx);
    }
    session.close();
    cleaned(&session);
    let jobs = assert_threads(&session.trace(), 8);
    assert_eq!(
        (
            jobs.jobs,
            jobs.spawned,
            jobs.waits,
            jobs.peak_blocked,
            jobs.saves
        ),
        // Each saved continuation's cleanup at close is a job (light-cljs-803).
        (8 + 7, 8, 8, 1, 7),
        "{jobs:?}"
    );
    // A contained abort while blocked (cleanup-abort-test job 3) poisons the
    // session and releases that worker's retention slot.
    let (session, bytes) = fixture(1, Some("cleanup-abort-test"), Some(4));
    let rx = submit(&session, &bytes, 3);
    loop {
        match rx.recv_timeout(Duration::from_secs(10)).unwrap() {
            Event::Failure(message) => {
                assert!(message.contains("worker-abort"), "{message}");
                break;
            }
            Event::Start | Event::Resumed | Event::Need(_, _) => {}
            other => panic!("expected the blocked worker's abort: {other:?}"),
        }
    }
    {
        let mut process = session.process.lock().unwrap();
        let input = &mut process.as_mut().unwrap().input;
        writeln!(input, "0").unwrap();
        input.flush().unwrap();
    }
    let until = Instant::now() + Duration::from_secs(5);
    while !session.trace().iter().any(|s| s == "CLOSED 1") {
        assert!(Instant::now() < until, "{:?}", session.trace());
        thread::yield_now();
    }
    let jobs = assert_threads(&session.trace(), 1);
    assert_eq!(
        (jobs.jobs, jobs.spawned, jobs.waits, jobs.peak_blocked),
        (1, 1, 1, 1),
        "{jobs:?}"
    );
    session.close();
    cleaned(&session);
}

/// A saved continuation whose resume is queued when the session is
/// cancelled runs its finally blocks once restored, skipping its catch
/// block (light-cljs-803). cleanup-abort-test holds job 4's resume before it
/// runs until the coordinator has taken the close.
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_cancel_reaches_a_queued_resume() {
    let session =
        GooseSession::start_retaining(&runner(), 1, Some("cleanup-abort-test"), Some(0)).unwrap();
    let e = GooseEvaluator::new(session.clone(), &Default::default()).unwrap();
    let (_, args, _) = inputs(&e, "rt => (form, env) => 0");
    let values = args
        .iter()
        .map(|&id| e.get(id).unwrap())
        .collect::<Vec<_>>();
    let limits = ConversionLimits::default();
    let budget = bridge::Budget::new(limits.nodes, e.control.clone());
    let bytes = bridge::Graph::arguments(&values, limits, &budget)
        .unwrap()
        .encode()
        .unwrap();
    session.export(&session.directory.join("definition-1.gsp"), "rt => (form, env) => { const rec = {}; try { try { __rt.cljReadRoot(globalThis.__lightCompilerRoots, null, 'fixture/missing'); } catch (e) { rec.caught = 1; } } finally { rec.resumed = 1; } return 0; }").unwrap();
    let rx = submit(&session, &bytes, 4);
    await_need(&rx);
    // A missing dependency: the coordinator queues the resume (mode 3).
    session.answer_key(4, 0, "-1").unwrap();
    let until = Instant::now() + Duration::from_secs(10);
    while !session.directory.join("resume-ready").exists() {
        assert!(Instant::now() < until, "{:?}", session.trace());
        thread::sleep(Duration::from_millis(1));
    }
    {
        let mut process = session.process.lock().unwrap();
        let input = &mut process.as_mut().unwrap().input;
        writeln!(input, "0").unwrap();
        input.flush().unwrap();
    }
    // SCHED's commands count reaches 2 (the job and the close) in the turn
    // that puts the cancellation token.
    while !session
        .trace()
        .iter()
        .any(|s| s.starts_with("SCHED ") && s.split_whitespace().nth(9) == Some("2"))
    {
        assert!(Instant::now() < until, "{:?}", session.trace());
        thread::yield_now();
    }
    std::fs::write(session.directory.join("resume-release"), "release").unwrap();
    while !session.trace().iter().any(|s| s == "CLOSED 1") {
        assert!(Instant::now() < until, "{:?}", session.trace());
        thread::yield_now();
    }
    let trace = session.trace();
    assert!(
        trace
            .iter()
            .any(|s| s == "ERROR 4 cancelled cleanup=resumed"),
        "{trace:?}"
    );
    let jobs = assert_threads(&trace, 1);
    assert_eq!((jobs.saves, jobs.jobs), (1, 2), "{jobs:?}");
    session.close();
    cleaned(&session);
}
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_drains_ready_batches_without_progress_backoff() {
    // Default: waits block in their workers. Some(0): every wait saves a
    // continuation and a fresh worker restores it.
    for (callable, retained) in [false, true]
        .into_iter()
        .flat_map(|c| [(c, None), (c, Some(0))])
    {
        for workers in [1, 2, 4, 8] {
            let session =
                GooseSession::start_retaining(&runner(), workers, Some("dispatch-test"), retained)
                    .unwrap();
            let e = GooseEvaluator::new(session.clone(), &Default::default()).unwrap();
            let (_, args, call) = inputs(&e, "rt => (form, env) => 0");
            let values = args
                .iter()
                .map(|&id| e.get(id).unwrap())
                .collect::<Vec<_>>();
            let limits = ConversionLimits::default();
            let budget = bridge::Budget::new(limits.nodes, e.control.clone());
            let graph = bridge::Graph::arguments(&values, limits, &budget).unwrap();
            let bytes = graph.encode().unwrap();
            let result = if callable {
                "value.call(null)"
            } else {
                "value"
            };
            session.export(&session.directory.join("definition-1.gsp"),
            &format!("rt => {{ return (form, env) => {{ let state = {{n: 0}}; state.n = state.n + 1; let value = __rt.cljReadRoot(globalThis.__lightCompilerRoots, null, 'fixture/value'); return state.n + {result}; }}; }}")).unwrap();
            let mut replies = Vec::new();
            for id in 1..=8 {
                std::fs::write(session.directory.join(format!("{id}.definition")), "1").unwrap();
                std::fs::write(session.directory.join(format!("{id}.gfb")), &bytes).unwrap();
                // Real emitted helper, with all keys available before dispatch.
                // Concurrent waiters must still elect exactly one producer.
                session
                    .export(
                        &session.directory.join(format!("{id}-0.helper")),
                        if callable {
                            "rt => {const result=41; return function(){return result;};}"
                        } else {
                            "rt => 41"
                        },
                    )
                    .unwrap();
                session.answer_key(id, 0, "9").unwrap();
                let (tx, rx) = mpsc::channel();
                session.pending.lock().unwrap().insert(id, tx);
                replies.push(rx);
            }
            {
                let mut process = session.process.lock().unwrap();
                let input = &mut process.as_mut().unwrap().input;
                for id in 1..=8 {
                    writeln!(input, "{id}").unwrap();
                }
                input.flush().unwrap();
            }
            for (index, rx) in replies.into_iter().enumerate() {
                loop {
                    match rx.recv_timeout(Duration::from_secs(10)).unwrap() {
                        Event::Done(_) => break,
                        Event::Start | Event::Resumed | Event::Produced | Event::Need(_, _) => {}
                        other => panic!("unexpected reply: {other:?}"),
                    }
                }
                let bytes =
                    std::fs::read(session.directory.join(format!("{}.out", index + 1))).unwrap();
                let output = bridge::Graph::decode(&bytes, limits, &budget)
                    .unwrap()
                    .expansion(&call, origin(&call), &graph.originals, limits, &budget)
                    .unwrap();
                assert!(
                    output.data().value_eq(&Datum::new(DatumKind::Integer(42))),
                    "continuation replayed mutation"
                );
            }
            session.close();
            cleaned(&session);
            let trace = session.trace();
            assert_eq!(
                trace.iter().filter(|s| s.starts_with("PREPARED ")).count(),
                1
            );
            assert_eq!(
                trace.iter().filter(|s| s.starts_with("PRODUCED ")).count(),
                1
            );
            assert_eq!(trace.iter().filter(|s| s.starts_with("START ")).count(), 8);
            assert_eq!(trace.iter().filter(|s| s.starts_with("RESUME ")).count(), 8);
            let mut previous = [0; 11];
            let mut observed = 0;
            for line in trace.iter().filter(|s| s.starts_with("SCHED ")) {
                let values = line
                    .split_whitespace()
                    .skip(2)
                    .map(|s| s.parse::<u64>().unwrap())
                    .collect::<Vec<_>>();
                assert_eq!(values[0], previous[0] + 1);
                if values[1] > previous[1] {
                    assert_eq!(values[2], previous[2], "progress must not back off: {line}");
                    assert_eq!(values[3], previous[3], "progress must not sleep: {line}");
                } else {
                    assert_eq!(values[2], previous[2] + 1);
                }
                previous.copy_from_slice(&values);
                observed += 1;
            }
            assert!(observed > 0);
            assert_eq!(previous[9], 8, "queued input drains in one turn");
            assert_eq!(
                previous[10], workers as u64,
                "simultaneous ready completions drain in one turn"
            );
            assert_eq!(
                previous[8], 17,
                "eight waits, one helper, eight completed bodies"
            );
            assert_eq!(previous[7], 9, "eight queued requests and shutdown");
            let jobs = assert_jobs(&trace, workers, 17);
            // Each of the eight resumed bodies installs the helper once, whether
            // answered in place or restored from its continuation, however many
            // jobs a worker served (light-cljs-796).
            assert_eq!(jobs.installs, 8, "{jobs:?}");
            assert!(jobs.restore_ns > 0 && jobs.handoff_ns > 0, "{jobs:?}");
            if retained == Some(0) {
                assert_eq!((jobs.jobs, jobs.saves, jobs.waits), (17, 8, 0), "{jobs:?}");
            } else {
                assert_eq!((jobs.jobs, jobs.answers, jobs.waits), (9, 8, 8), "{jobs:?}");
                assert!(jobs.peak_blocked >= 1, "{jobs:?}");
            }
        }
    }
}

// PARK is emitted immediately before the atomic native receive, after all
// payload sources were rechecked. This hook gates publication, never elapsed
// latency. The deadline only bounds a broken protocol.
fn await_park(session: &GooseSession, after: usize) {
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        if session
            .trace()
            .iter()
            .filter(|s| s.starts_with("PARK "))
            .count()
            > after
        {
            return;
        }
        assert!(
            Instant::now() < until,
            "coordinator did not park: {:?}",
            session.trace()
        );
        thread::yield_now();
    }
}

#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_readiness_delayed_helper_and_idle_close_cancel() {
    for workers in [1, 2, 4, 8] {
        let session = GooseSession::start_mode(&runner(), workers, Some("readiness-test")).unwrap();
        await_park(&session, 0);
        // Repeated/spurious file notices contain no job and cannot produce a
        // terminal reply. Wake is coalesced even while the input pipe drains.
        {
            let mut process = session.process.lock().unwrap();
            let input = &mut process.as_mut().unwrap().input;
            for _ in 0..256 {
                writeln!(input, "-2").unwrap();
            }
            input.flush().unwrap();
        }
        let e = GooseEvaluator::new(session.clone(), &Default::default()).unwrap();
        let (function, args, call) = inputs(
            &e,
            "rt => (form, env) => __rt.cljReadRoot(globalThis.__lightCompilerRoots, null, 'fixture/value')",
        );
        let s = session.clone();
        let (release, delayed) = mpsc::channel();
        let publishing = thread::spawn(move || {
            let until = Instant::now() + Duration::from_secs(10);
            loop {
                let trace = s.trace();
                if let Some(need) = trace.iter().rposition(|s| s.starts_with("NEED "))
                    && trace[need + 1..].iter().any(|s| s.starts_with("PARK "))
                {
                    release.send(()).unwrap();
                    return;
                }
                assert!(
                    Instant::now() < until,
                    "helper wait never parked: {trace:?}"
                );
                thread::yield_now();
            }
        });
        serve(&e, &[("fixture/value", 41)], move |unit| {
            assert_eq!(unit, 41);
            delayed.recv_timeout(Duration::from_secs(10)).unwrap();
            Ok("rt => 41".into())
        });
        let result = e.call(function, &args).unwrap();
        publishing.join().unwrap();
        let output = e
            .export_form(result, &call, origin(&call), Default::default())
            .unwrap();
        assert!(output.data().value_eq(&Datum::new(DatumKind::Integer(41))));
        assert_eq!(
            session
                .trace()
                .iter()
                .filter(|s| s.starts_with("DONE "))
                .count(),
            1
        );
        assert_eq!(
            session
                .trace()
                .iter()
                .filter(|s| s.starts_with("PRODUCED "))
                .count(),
            1
        );
        session.close();
        cleaned(&session);
        assert!(
            session.trace().iter().any(|s| s == "CLOSED 1"),
            "graceful idle close"
        );

        let session = GooseSession::start_mode(&runner(), workers, Some("readiness-test")).unwrap();
        let e = GooseEvaluator::new(session.clone(), &Default::default()).unwrap();
        let (function, args, _) = inputs(&e, "rt => (form, env) => 1");
        await_park(&session, 0);
        e.control().cancel();
        assert!(e.call(function, &args).unwrap_err().0.contains("cancelled"));
        cleaned(&session);
        assert!(
            session.trace().iter().any(|s| s == "CLOSED 1"),
            "idle cancellation wakes close"
        );
    }
}

#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_reclaims_inactive_waiters_between_batches() {
    for retained in [None, Some(0)] {
        let session =
            GooseSession::start_retaining(&runner(), 1, Some("readiness-test"), retained).unwrap();
        let e = GooseEvaluator::new(session.clone(), &Default::default()).unwrap();
        let (function, args, _) = inputs(
            &e,
            "rt => (form, env) => __rt.cljReadRoot(globalThis.__lightCompilerRoots, null, 'fixture/value')",
        );
        serve(&e, &[("fixture/value", 41)], |_| Ok("rt => 41".into()));
        for _ in 0..256 {
            let result = e.call(function, &args).unwrap();
            assert!(
                e.export_form(result, &form(), origin(&form()), Default::default())
                    .unwrap()
                    .data()
                    .value_eq(&Datum::new(DatumKind::Integer(41)))
            );
            e.release(result).unwrap();
        }
        session.close();
        cleaned(&session);
        let jobs = assert_jobs(&session.trace(), 1, 513);
        assert_eq!(
            jobs.saves,
            if retained == Some(0) { 256 } else { 0 },
            "{jobs:?}"
        );
        assert!(
            session.trace().iter().any(|s| s == "CLOSED 1"),
            "bounded failure closes natively"
        );
        assert!(
            !String::from_utf8_lossy(&session.stderr.lock().unwrap()).contains("assert failed")
        );
    }
}

#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_large_continuations_reclaim_across_batches() {
    use crate::parser::CollectionKind;
    // The pinned build suspends 529 times, with a largest saved continuation
    // just over 3 MB. Repeat a similarly sized transported argument twice as
    // often in one session: cumulative copying must not consume retention.
    // Retention 0 saves every wait, as this test's continuations need.
    let session =
        GooseSession::start_retaining(&runner(), 1, Some("readiness-test"), Some(0)).unwrap();
    // Transport visits alone exceed the default one-million session polls.
    // Keep a finite test budget, below the generated project's 100 billion.
    let options = EvaluatorOptions {
        poll_budget: 100_000_000,
        timeout: Duration::from_secs(120),
        ..Default::default()
    };
    let e = GooseEvaluator::new(session.clone(), &options).unwrap();
    let (function, args, call) = inputs(
        &e,
        "rt => (form, env, x) => { const shared = __rt.cljReadRoot(globalThis.__lightCompilerRoots, null, 'fixture/value'); return __rt.formVector(__rt.cljCount(x), shared); }",
    );
    serve(&e, &[("fixture/value", 41)], |_| Ok("rt => 41".into()));
    let payload = Datum::new(DatumKind::Collection {
        kind: CollectionKind::Vector,
        children: (0..14_000)
            .map(|n| Datum::new(DatumKind::Integer(n)))
            .collect(),
    });
    let input = e.import_data(&payload, Default::default()).unwrap();
    let expected = Datum::new(DatumKind::Collection {
        kind: CollectionKind::Vector,
        children: vec![
            Datum::new(DatumKind::Integer(14_000)),
            Datum::new(DatumKind::Integer(41)),
        ]
        .into(),
    });
    let mut retained = Vec::new();
    for batch in 0..2 {
        for iteration in 0..529 {
            let result = e.call(function, &[args[0], args[1], input]).unwrap();
            let output = e
                .export_form(result, &call, origin(&call), Default::default())
                .unwrap();
            assert!(output.data().value_eq(&expected));
            if iteration == 528 {
                retained.push(output);
            }
            e.release(result).unwrap();
        }
        assert_eq!(session.stats().jobs, (batch + 1) * 529);
        assert_eq!(session.stats().preparations, 1);
        assert_eq!(session.stats().exports, 2);
        assert_eq!(e.slots.borrow().len(), 4);
    }
    e.release(input).unwrap();
    session.close();
    cleaned(&session);
    assert_eq!(assert_jobs(&session.trace(), 1, 2 * 1058 + 1).saves, 1058);
    drop(e);
    drop(session);
    for output in retained {
        assert!(output.data().value_eq(&expected));
        assert_eq!(output.span().source().name(), "retained.cljs");
    }
}

#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_active_waiter_limit_stays_bounded() {
    let session = GooseSession::start(&runner(), 1).unwrap();
    let e = GooseEvaluator::new(session.clone(), &Default::default()).unwrap();
    let (_, args, _) = inputs(&e, "rt => (form, env) => 0");
    let values = args
        .iter()
        .map(|&id| e.get(id).unwrap())
        .collect::<Vec<_>>();
    let limits = ConversionLimits::default();
    let budget = bridge::Budget::new(limits.nodes, e.control.clone());
    let bytes = bridge::Graph::arguments(&values, limits, &budget)
        .unwrap()
        .encode()
        .unwrap();
    session.export(&session.directory.join("definition-1.gsp"), "rt => (form, env) => __rt.cljReadRoot(globalThis.__lightCompilerRoots, null, 'fixture/value')").unwrap();
    let mut replies = Vec::new();
    for id in 1..=129 {
        std::fs::write(session.directory.join(format!("{id}.definition")), "1").unwrap();
        std::fs::write(session.directory.join(format!("{id}.gfb")), &bytes).unwrap();
        let (tx, rx) = mpsc::channel();
        session.pending.lock().unwrap().insert(id, tx);
        replies.push(rx);
    }
    {
        let mut process = session.process.lock().unwrap();
        let input = &mut process.as_mut().unwrap().input;
        for id in 1..=129 {
            writeln!(input, "{id}").unwrap();
        }
        input.flush().unwrap();
    }
    for (index, rx) in replies.into_iter().enumerate() {
        loop {
            match rx.recv_timeout(Duration::from_secs(10)).unwrap() {
                Event::Start | Event::Resumed => {}
                Event::Need(_, _) if index < 128 => break,
                Event::Failure(message) if index == 128 => {
                    assert!(message.contains("waiter-budget"), "{message}");
                    break;
                }
                other => panic!("unexpected admission outcome for {}: {other:?}", index + 1),
            }
        }
    }
    // Request the native close and wait for it. Cancellation cancels the 32
    // waiters blocked in place and runs every one of the 96 saved
    // continuations as a cleanup job, one per execution slot (light-cljs-803):
    // 60-110 ms on the release runner alone, more on the debug runner or under
    // load. The test observes the native close itself rather than through
    // GooseSession::close(), whose kill fallback could end the session.
    {
        let mut process = session.process.lock().unwrap();
        let input = &mut process.as_mut().unwrap().input;
        writeln!(input, "0").unwrap();
        input.flush().unwrap();
    }
    let until = Instant::now() + Duration::from_secs(30);
    while !session.trace().iter().any(|s| s == "CLOSED 1") {
        assert!(Instant::now() < until, "{:?}", session.trace());
        thread::sleep(Duration::from_millis(1));
    }
    // Every admitted waiter is cancelled exactly once. The refused job 129
    // may report its own cancellation too: its worker can see the cancel
    // before the refusal's answer.
    let trace = session.trace();
    let mut cancelled = trace
        .iter()
        .filter_map(|s| s.strip_suffix(" cancelled")?.strip_prefix("ERROR "))
        .map(|id| id.parse::<u64>().unwrap())
        .filter(|&id| id != 129)
        .collect::<Vec<_>>();
    cancelled.sort_unstable();
    assert_eq!(cancelled, (1..=128).collect::<Vec<_>>());
    session.close();
    cleaned(&session);
    assert!(!String::from_utf8_lossy(&session.stderr.lock().unwrap()).contains("assert failed"));
}

/// `GooseSession::close()` itself lets the runner finish cancellation
/// cleanup (light-cljs-812): saved waiters whose finally blocks together
/// take well over 100 ms all run before the runner exits, and close() does
/// not kill it. Before the fix close() killed the runner 100 ms after its
/// close request: about 56 of the 128 cleanups ran on the release runner.
/// Each finally block stays under the default instruction limit.
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_close_waits_for_slow_cancellation_cleanup() {
    const JOBS: u64 = 128;
    let session = GooseSession::start_retaining(&runner(), 1, None, Some(0)).unwrap();
    let e = GooseEvaluator::new(session.clone(), &Default::default()).unwrap();
    let (_, args, _) = inputs(&e, "rt => (form, env) => 0");
    let values = args
        .iter()
        .map(|&id| e.get(id).unwrap())
        .collect::<Vec<_>>();
    let limits = ConversionLimits::default();
    let budget = bridge::Budget::new(limits.nodes, e.control.clone());
    let bytes = bridge::Graph::arguments(&values, limits, &budget)
        .unwrap()
        .encode()
        .unwrap();
    // Each job's finally block spins before it records its write.
    session.export(&session.directory.join("definition-1.gsp"), "rt => (form, env) => { const rec = {}; try { __rt.cljReadRoot(globalThis.__lightCompilerRoots, null, 'fixture/value'); } finally { let n = 0; for (let i = 0; i < 40000; i++) { n = n + i; } rec.slow = n; } return 0; }").unwrap();
    for id in 1..=JOBS {
        await_need(&submit(&session, &bytes, id));
    }
    let started = Instant::now();
    session.close();
    let elapsed = started.elapsed();
    cleaned(&session);
    let trace = session.trace();
    assert!(trace.iter().any(|s| s == "CLOSED 1"), "{trace:?}");
    assert!(!trace.iter().any(|s| s.starts_with("KILLED")), "{trace:?}");
    let mut cleaned_up = trace
        .iter()
        .filter_map(|s| {
            s.strip_suffix(" cancelled cleanup=slow")?
                .strip_prefix("ERROR ")
        })
        .map(|id| id.parse::<u64>().unwrap())
        .collect::<Vec<_>>();
    cleaned_up.sort_unstable();
    assert_eq!(cleaned_up, (1..=JOBS).collect::<Vec<_>>(), "{trace:?}");
    // The cleanup is slow enough that the old 100 ms kill cut it short.
    assert!(elapsed > Duration::from_millis(200), "{elapsed:?}");
}

#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_pool_alternates_payloads_and_keeps_owned_forms() {
    use crate::parser::CollectionKind;
    let mut retained = Vec::new();
    for workers in [1, 2, 4, 8] {
        let session = GooseSession::start_mode(&runner(), workers, Some("readiness-test")).unwrap();
        let e = GooseEvaluator::new(session.clone(), &Default::default()).unwrap();
        let (_, args, call) = inputs(&e, "rt => (form, env) => 0");
        let functions = [0, 100].map(|base| e.source_macro_function(777 + base,
            &format!("rt => {{ return (form, env, x) => {{ let state = {{n: {base}}}; state.n = state.n + 1; return __rt.formVector(x, state.n); }}; }}")).unwrap());
        for iteration in 0..24 {
            let size = [1, 32, 256, 1][iteration % 4];
            let input = Datum::new(DatumKind::Collection {
                kind: CollectionKind::Vector,
                children: (0..size)
                    .map(|n| Datum::new(DatumKind::Integer(n)))
                    .collect(),
            });
            let payload = e.import_data(&input, Default::default()).unwrap();
            let result = e
                .call(functions[iteration % 2], &[args[0], args[1], payload])
                .unwrap();
            let output = e
                .export_form(result, &call, origin(&call), Default::default())
                .unwrap();
            let expected = Datum::new(DatumKind::Collection {
                kind: CollectionKind::Vector,
                children: vec![
                    input,
                    Datum::new(DatumKind::Integer(if iteration % 2 == 0 { 1 } else { 101 })),
                ]
                .into(),
            });
            assert!(output.data().value_eq(&expected));
            retained.push((output, expected));
            e.release(payload).unwrap();
            e.release(result).unwrap();
        }
        session.close();
        cleaned(&session);
        assert_eq!(session.stats().preparations, 2);
        assert_jobs(&session.trace(), workers, 24);
    }
    for (output, expected) in retained {
        assert!(
            output.data().value_eq(&expected),
            "form remains owned after worker reuse and teardown"
        );
        assert_eq!(output.span().source().name(), "retained.cljs");
    }
}

#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_cancel_reaps_job_workers() {
    for source in [
        "rt => (form, env) => { while (true) {} }",
        "rt => (form, env) => __rt.cljCount(rt.core.mapSeq(()=>{while(true){}},__rt.formList(1)))",
        "rt => (form, env) => {const f=__rt.cljReadRoot(globalThis.__lightCompilerRoots,null,'fixture/infinite');return f.call(null);}",
    ] {
        for workers in [1, 2, 4, 8] {
            let session =
                GooseSession::start_mode(&runner(), workers, Some("readiness-test")).unwrap();
            let e = GooseEvaluator::new(session.clone(), &Default::default()).unwrap();
            let (function, args, _) = inputs(&e, source);
            serve(&e, &[("fixture/infinite", 42)], |_| {
                Ok("rt => function(){while(true){}}".into())
            });
            let gate = if source.contains("fixture/infinite") {
                "RESUME "
            } else {
                "START "
            };
            let s = session.clone();
            let control = e.control();
            let cancelling = thread::spawn(move || {
                let until = Instant::now() + Duration::from_secs(10);
                while !s.trace().iter().any(|s| s.starts_with(gate)) {
                    assert!(Instant::now() < until, "job was never admitted");
                    thread::yield_now();
                }
                control.cancel();
                s.close();
            });
            assert!(e.call(function, &args).is_err());
            cancelling.join().unwrap();
            cleaned(&session);
            let trace = session.trace();
            assert!(trace.iter().any(|s| s.contains("cancelled")), "{trace:?}");
            assert!(
                trace.iter().any(|s| s == "CLOSED 1"),
                "native shutdown joined the pool: {trace:?}"
            );
            assert_threads(&trace, workers);
        }
    }
}

#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_callable_nested_demand_aliases_and_no_replay() {
    for workers in [1, 2, 4, 8] {
        let s = GooseSession::start(&runner(), workers).unwrap();
        let e = GooseEvaluator::new(s.clone(), &Default::default()).unwrap();
        let (f, args, call) = inputs(
            &e,
            r#"rt => {return (form,env) => {const state={n:0};
            state.n++;
            const f=__rt.cljReadRoot(globalThis.__lightCompilerRoots,null,'fixture/function');
            const alias=__rt.cljReadRoot(globalThis.__lightCompilerRoots,null,'fixture/alias');
            const result=f.call(null,state);
            return state.n===2 && f===alias && result===43;
        };}"#,
        );
        let roots = [
            ("fixture/function", 42),
            ("fixture/alias", 42),
            ("fixture/number", 43),
        ];
        let callbacks = Rc::new(Cell::new(0));
        let count = callbacks.clone();
        serve(&e, &roots, move |unit| {
            count.set(count.get() + 1);
            Ok(if unit == 42 {
                "rt => function(state){state.n++; const n=__rt.cljReadRoot(globalThis.__lightCompilerRoots,null,'fixture/number'); return this===null ? n+state.n : -100;}".into()
            } else {
                assert_eq!(unit, 43);
                "rt => 41".into()
            })
        });
        for _ in 0..3 {
            let result = e.call(f, &args).unwrap();
            let output = e
                .export_form(result, &call, origin(&call), Default::default())
                .unwrap();
            assert!(
                output
                    .data()
                    .value_eq(&Datum::new(DatumKind::Boolean(true)))
            );
        }
        assert_eq!(
            callbacks.get(),
            9,
            "every demand reaches the compiler root service"
        );
        let trace = s.trace();
        assert_eq!(
            trace.iter().filter(|s| s.starts_with("PRODUCED ")).count(),
            2
        );
        assert_eq!(trace.iter().filter(|s| s.starts_with("RESUME ")).count(), 9);
        eprintln!("callable nested workers={workers}: {trace:?}");
        s.close();
        cleaned(&s);
    }
}

#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_callable_throw_and_publication_errors() {
    for workers in [1, 2, 4, 8] {
        for (helper, caught, expected) in [
            ("rt => function(x){throw x;}", true, None),
            (
                "rt => function(x){throw x;}",
                false,
                Some("HostEffect function="),
            ),
            (
                "rt => {let n=0;return function(){n++;return n;};}",
                false,
                Some("Error.Unsupported"),
            ),
            (
                "rt => {const x={n:1};return function(){return x.n;};}",
                false,
                Some("Error.Unsupported"),
            ),
        ] {
            let s = GooseSession::start(&runner(), workers).unwrap();
            let e = GooseEvaluator::new(s.clone(), &Default::default()).unwrap();
            let body = if caught {
                "let n=0;n++;const f=__rt.cljReadRoot(globalThis.__lightCompilerRoots,null,'fixture/throw');try{f.call(null,form);}catch(x){if(n!==1)return false;return x;}"
            } else {
                "const f=__rt.cljReadRoot(globalThis.__lightCompilerRoots,null,'fixture/throw');return f.call(null,form);"
            };
            let (f, args, call) = inputs(&e, &format!("rt => (form,env) => {{{body}}}"));
            serve(&e, &[("fixture/throw", 42)], move |_| Ok(helper.into()));
            let result = e.call(f, &args);
            if let Some(expected) = expected {
                let message = result.unwrap_err().to_string();
                assert!(message.contains(expected), "{message}");
                assert!(message.contains("emitted-span="), "{message}");
            } else {
                let output = e
                    .export_form(result.unwrap(), &call, origin(&call), Default::default())
                    .unwrap();
                assert!(output.data().value_eq(&call.data()));
                assert_eq!(output.span().bytes(), call.span().bytes());
                assert_eq!(output.span().source().name(), call.span().source().name());
            }
            s.close();
            cleaned(&s);
        }
    }
}

#[cfg(light_v8)]
#[test]
#[ignore = "full pinned target; build native runner and run explicitly"]
fn generated_200k_complete_forms() {
    use crate::compiler_bootstrap::goose_capture::{
        GENERATED_FORMS, compare_concurrent_forms, compare_generated_reference,
        validate_generated_anonymous, validate_generated_scalar_cases,
    };
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let generated = root.join("target/generated-200k");
    assert_eq!(
        std::fs::read(generated.join("manifest.edn")).unwrap(),
        include_bytes!("../../crates/big-boy-pants-js/evidence/generated-200k-manifest.edn")
    );
    let runner = PathBuf::from(std::env::var_os("GOOSE_COMPILER_RUNNER").unwrap());
    let mut reference: Option<Vec<(Name, Form, Form)>> = None;
    let mut goose_reference: Option<Vec<(Name, Form, Form)>> = None;
    let mut reference_counts = None;
    for workers in [0, 1, 2, 4, 8] {
        eprintln!("begin complete generated forms workers={workers}");
        let _names = crate::generated_names::enter_namespace(0);
        let mut host = crate::MacroHost::new();
        host.limits.expansions = 10_000_000;
        host.limits.generated_nodes = 1_000_000_000;
        host.evaluator_options.timeout = std::time::Duration::from_secs(3600);
        host.evaluator_options.memory_limit = 2_147_483_648;
        host.evaluator_options.poll_budget = 100_000_000_000;
        let session = (workers != 0)
            .then(|| GooseSession::start_mode(&runner, workers, Some("readiness-test")).unwrap());
        if let Some(s) = &session {
            host.use_goose_source_macros(s.clone()).unwrap();
        }
        *GENERATED_FORMS.lock().unwrap() = Some(Vec::new());
        let census = crate::engine_census::EngineCensus::now();
        let program = crate::compiler::analyze_namespace_entry_recorded(
            "workload.main",
            &[generated.join("src"), generated.join("macros")],
            &mut host,
            Default::default(),
            &mut Vec::new(),
            crate::compiler::EntryRecorders {
                slots: crate::scheduler::Slots::new(workers.max(1)).unwrap(),
                ..Default::default()
            },
        )
        .unwrap_or_else(|e| panic!("workers={workers}: {e}"));
        // The Goose builds create no engine (light-cljs-778).
        let created = crate::engine_census::EngineCensus::now().since(census);
        assert!(
            workers == 0 || created.is_empty(),
            "workers={workers}: {created:?}"
        );
        let mut requests = GENERATED_FORMS.lock().unwrap().take().unwrap();
        requests.sort_by_key(|r| {
            (
                r.1.span().source().name().to_owned(),
                r.1.span().bytes().start,
                format!("{:?}", r.0),
                r.1.nodes().len(),
            )
        });
        assert_eq!(requests.len(), 5226);
        let mut counts = std::collections::BTreeMap::new();
        for r in &requests {
            *counts.entry(format!("{:?}", r.0)).or_insert(0) += 1;
        }
        assert_eq!(counts.len(), 615);
        if let Some(expected) = &reference_counts {
            assert_eq!(expected, &counts, "qualified macro entry counts");
        } else {
            reference_counts = Some(counts);
        }
        drop(program);
        drop(host);
        if let Some(s) = session {
            let stats = s.stats();
            assert_eq!(
                (stats.jobs, stats.preparations, stats.exports),
                (5226, 615, 672)
            );
            s.close();
            assert!(s.process.lock().unwrap().is_none());
            assert!(s.readers.lock().unwrap().is_empty());
            assert!(s.pending.lock().unwrap().is_empty());
            assert!(!s.directory.exists());
            assert_threads(&s.trace(), workers);
        }
        let mut aliases = 0;
        let mut anonymous = 0;
        let mut inherited = 0;
        let mut returned = 0;
        let mut scalars = 0;
        let mut comparison_failures = 0;
        if let Some(expected) = &reference {
            for (a, b) in expected.iter().zip(&requests) {
                assert_eq!(a.0, b.0);
                let (raw, prior, output) = validate_generated_anonymous(&b.1, &a.1, &b.2);
                anonymous += raw;
                inherited += prior;
                returned += output;
                scalars += validate_generated_scalar_cases(&b.1, &b.2);
                match std::panic::catch_unwind(|| {
                    compare_generated_reference(&b.1, &a.1, &b.1)
                        + compare_generated_reference(&b.1, &a.2, &b.2)
                }) {
                    Ok(count) => aliases += count,
                    Err(_) => {
                        comparison_failures += 1;
                        eprintln!("unclassified expansion {:?} {:?}", b.0, b.1.span());
                    }
                }
            }
            assert_eq!(
                comparison_failures, 0,
                "every reference difference must be classified"
            );
            assert_eq!(scalars, 5, "both repeated-scalar calls were reached");
            if let Some(expected) = &goose_reference {
                compare_concurrent_forms(
                    expected
                        .iter()
                        .map(|r| (r.1.clone(), r.2.clone()))
                        .collect(),
                    requests
                        .iter()
                        .map(|r| (r.1.clone(), r.2.clone()))
                        .collect(),
                );
            } else {
                goose_reference = Some(requests);
            }
        } else {
            reference = Some(requests);
        }
        eprintln!(
            "complete generated forms workers={workers}: 5226 expansions, 615 identities, {aliases} reference alias ambiguities, {anonymous} raw input/{inherited} common compiler input/{returned} returned anonymous-parameter checks, {scalars} scalar occurrence checks, closed"
        );
    }
}

/// The sources of a namespace build with a pool's worth of macro units:
/// `m.macros` declares a helper and macros that call it, and each of eight
/// namespaces expands `bump` inside eight definitions, deferred units the
/// pool's workers take, each of which demands the helper; `body`, unless
/// empty, replaces the first definition's body in `app.w0`, the first unit
/// after the helper's, so that no unit before it in source order is still
/// running when it fails.
fn pooled_sources(body: &str) -> HashMap<String, String> {
    let mut files = HashMap::new();
    files.insert(
        "m.macros".to_owned(),
        "(ns m.macros)
         (defn helper [xs] (mapv inc xs))
         (defmacro bump [& xs] (list 'quote (helper (vec xs))))
         (defmacro boom [] (nth [1] 10))
         (defmacro spin [] (loop [i 0] (recur (inc i))))"
            .to_owned(),
    );
    let mut main = String::from("(ns app.main (:require");
    for n in 0..8 {
        let mut source = format!("(ns app.w{n} (:require [m.macros :as m]))\n");
        for d in 0..8 {
            if n == 0 && d == 0 && !body.is_empty() {
                source.push_str(&format!("(defn f{d} [] {body})\n"));
            } else {
                source.push_str(&format!("(defn f{d} [] (m/bump {n} {d}))\n"));
            }
        }
        files.insert(format!("app.w{n}"), source);
        main.push_str(&format!(" [app.w{n}]"));
    }
    main.push_str("))\n(def all [");
    for n in 0..8 {
        for d in 0..8 {
            main.push_str(&format!(" (app.w{n}/f{d})"));
        }
    }
    main.push_str("])\n");
    files.insert("app.main".to_owned(), main);
    files
}
/// Analyze and emit `app.main` of `files` at `slots` compiler slots, with
/// the Goose session `goose` if any: the emitted JavaScript or the error,
/// the build's stats and the engines the build created.
fn pooled_build(
    files: &HashMap<String, String>,
    slots: usize,
    goose: Option<&Arc<GooseSession>>,
    options: EvaluatorOptions,
) -> (
    Result<String, String>,
    crate::compile_stats::CompileStats,
    crate::engine_census::EngineCensus,
) {
    pooled_build_seeded(files, slots, goose, options, 0)
}

/// [`pooled_build`] under the scheduler's stress perturbation with `seed`
/// (one job to a batch) when it is not zero, in builds that have it.
fn pooled_build_seeded(
    files: &HashMap<String, String>,
    slots: usize,
    goose: Option<&Arc<GooseSession>>,
    options: EvaluatorOptions,
    seed: u64,
) -> (
    Result<String, String>,
    crate::compile_stats::CompileStats,
    crate::engine_census::EngineCensus,
) {
    let _ = seed;
    let before = crate::engine_census::EngineCensus::now();
    let provider = |request: &crate::source_provider::LoadRequest| {
        let name = String::from_utf16_lossy(request.namespace.name());
        let extension = if name.starts_with("m.") {
            "cljc"
        } else {
            "cljs"
        };
        Ok(files.get(&name).map(|source| {
            crate::source_provider::SourceInput::new(
                format!("{}.{extension}", name.replace('.', "/")),
                source.clone(),
            )
        }))
    };
    let mut host = crate::MacroHost::new();
    host.evaluator_options = options;
    if let Some(session) = goose {
        host.use_goose_source_macros(session.clone()).unwrap();
    }
    let mut stats = crate::compile_stats::CompileStats::default();
    let result = crate::compiler::analyze_namespace_entry_recorded_with_provider(
        "app.main",
        &provider,
        &mut host,
        Default::default(),
        &mut Vec::new(),
        crate::compiler::EntryRecorders {
            stats: Some(&mut stats),
            slots: crate::scheduler::Slots::new(slots).unwrap(),
            #[cfg(any(debug_assertions, feature = "scheduling-stress"))]
            perturbation: (seed > 0).then(|| crate::scheduler::Perturbation {
                seed,
                limits: Some(crate::scheduler::Limits {
                    batch_jobs: 1,
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        },
    )
    .map(|(program, _)| crate::emit_js(&program))
    .map_err(|error| error.to_string());
    drop(host);
    let census = crate::engine_census::EngineCensus::now().since(before);
    (result, stats, census)
}

/// A namespace build whose macros bind `(gensym)` and `(gensym "x")` names,
/// call a helper that gensyms, and take a prefix from the call, in eight
/// namespaces of deferred units,
/// two expansions to a unit: at compiler slots 1/2/4/8 on the Goose path
/// the JavaScript is the V8 build's, names included, since each job numbers
/// in the compiler's scope of the unit it expands in (light-cljs-798).
#[cfg(light_v8)]
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_gensym_numbers_in_compiler_scopes() {
    let mut files = HashMap::new();
    files.insert(
        "m.gen".to_owned(),
        "(ns m.gen)
         (defn fresh [p] (gensym p))
         (defmacro named [p] (list (quote quote) [(gensym p) (gensym p)]))
         (defmacro bind [x]
           (let [a (gensym) b (gensym \"x\")]
             (list 'let [a x b (list 'inc a)] (list 'vector a b (list 'quote [a b (fresh \"h\")])))))"
            .to_owned(),
    );
    let mut main = String::from("(ns app.main (:require");
    for n in 0..8 {
        let mut source = format!("(ns app.w{n} (:require [m.gen :as g]))\n");
        for d in 0..4 {
            source.push_str(&format!(
                "(defn f{d} [] [(g/bind {n}) (g/bind {d}) (g/named \"n\")])\n"
            ));
        }
        files.insert(format!("app.w{n}"), source);
        main.push_str(&format!(" [app.w{n}]"));
    }
    main.push_str("))\n(def all [");
    for n in 0..8 {
        for d in 0..4 {
            main.push_str(&format!(" (app.w{n}/f{d})"));
        }
    }
    main.push_str("])\n");
    files.insert("app.main".to_owned(), main);
    // Every expansion binds two names of its own and quotes a third.
    let names = |js: &str, base: &str| {
        let mut names = std::collections::BTreeSet::new();
        for (at, _) in js.match_indices(base) {
            let rest = &js[at + base.len()..];
            let end = rest
                .find(|c: char| !(c.is_ascii_digit() || c == '_'))
                .unwrap_or(rest.len());
            if end > 0 {
                names.insert(rest[..end].to_owned());
            }
        }
        names.len()
    };
    let (reference, _, _) = pooled_build(&files, 1, None, Default::default());
    let reference = reference.unwrap();
    for base in ["G__", "x", "h"] {
        assert!(names(&reference, base) >= 64, "{base}: {reference}");
    }
    for slots in [1, 2, 4, 8] {
        let session = GooseSession::start(&runner(), slots).unwrap();
        let (output, _, census) = pooled_build(&files, slots, Some(&session), Default::default());
        assert_eq!(output.as_deref(), Ok(reference.as_str()), "slots={slots}");
        assert!(census.is_empty(), "slots={slots}: {census:?}");
        assert_eq!(session.stats().jobs, 96, "slots={slots}");
        session.close();
        cleaned(&session);
    }
}

/// A macro that quotes a compiled `(def x (gensym))`: the definition's
/// prologue reads `rt.nativeCoreBinding("gensym")` (light-cljs-807).
#[cfg(light_v8)]
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_def_gensym_used_by_a_macro() {
    let mut files = HashMap::new();
    files.insert(
        "m.gen".to_owned(),
        "(ns m.gen)\n(def x (gensym))\n(defmacro m [] (list 'quote x))".to_owned(),
    );
    files.insert(
        "app.main".to_owned(),
        "(ns app.main (:require [m.gen :as g]))\n(def y (g/m))\n".to_owned(),
    );
    let (reference, _, _) = pooled_build(&files, 1, None, Default::default());
    let reference = reference.unwrap();
    // y is the quoted symbol the definition's gensym made, numbered in the
    // bundle's own initializer scope (light-cljs-802).
    let quoted = reference
        .split("rt.symbol(null, \"G__")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .unwrap_or_else(|| panic!("{reference}"));
    assert!(quoted.ends_with("_i1"), "{reference}");
    for slots in [1, 4] {
        let session = GooseSession::start(&runner(), slots).unwrap();
        let (output, _, _) = pooled_build(&files, slots, Some(&session), Default::default());
        assert_eq!(output.as_deref(), Ok(reference.as_str()), "slots={slots}");
        session.close();
        cleaned(&session);
    }
}

/// A CLJS helper namespace whose definitions call `gensym`, used by a macro
/// from eight namespaces: on the Goose path at compiler slots 1/2/4/8, and
/// under the scheduler's stress seeds, the JavaScript is the V8 build's,
/// since the helper's initializers number in their bundle's own frozen
/// scope (`_iN`) whichever job demands them first (light-cljs-802).
#[cfg(light_v8)]
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_helper_initializer_gensym_names_alike_at_every_slot_count() {
    let mut files = HashMap::new();
    files.insert(
        "m.gen".to_owned(),
        r#"(ns m.gen)
(def tag (gensym "t"))
(def pair [(gensym "p") (gensym "p")])
(defmacro tagged [x] (list 'quote [tag (first pair) (second pair) x (gensym "e")]))"#
            .to_owned(),
    );
    let mut main = String::from("(ns app.main (:require");
    for n in 0..8 {
        files.insert(
            format!("app.w{n}"),
            format!(
                "(ns app.w{n} (:require [m.gen :as g]))\n(def v (g/tagged {n}))\n(defn f [] (g/tagged :f))\n"
            ),
        );
        main.push_str(&format!(" [app.w{n}]"));
    }
    main.push_str("))\n(def all [");
    for n in 0..8 {
        main.push_str(&format!(" app.w{n}/v (app.w{n}/f)"));
    }
    main.push_str("])\n");
    files.insert("app.main".to_owned(), main);
    let (reference, _, _) = pooled_build(&files, 1, None, Default::default());
    let reference = reference.unwrap();
    // Every expansion quotes the helper's one set of names, from its own
    // scope.
    for (prefix, count) in [("t", 1), ("p", 2)] {
        let needle = format!("rt.symbol(null, \"{prefix}");
        let quoted = reference
            .match_indices(&needle)
            .map(|(at, _)| {
                reference[at + needle.len() - prefix.len()..]
                    .split('"')
                    .next()
                    .unwrap()
            })
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(quoted.len(), count, "{prefix}: {reference}");
        assert!(quoted.iter().all(|q| q.contains("_i")), "{quoted:?}");
    }
    let seeds = if cfg!(any(debug_assertions, feature = "scheduling-stress")) {
        std::env::var("LIGHT_CLJS_STRESS_SEEDS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1)
    } else {
        0
    };
    for slots in [1, 2, 4, 8] {
        for seed in 0..=seeds {
            let session = GooseSession::start(&runner(), slots).unwrap();
            let (output, _, census) =
                pooled_build_seeded(&files, slots, Some(&session), Default::default(), seed);
            assert_eq!(
                output.as_deref(),
                Ok(reference.as_str()),
                "slots={slots} seed={seed}"
            );
            assert!(census.is_empty(), "slots={slots} seed={seed}: {census:?}");
            session.close();
            cleaned(&session);
        }
    }
}

/// A namespace build on the Goose compiler path at compiler slots 1/2/4/8
/// declares and expands its macros, and its pool's Goose-bound workers
/// analyze macro units that demand a helper, with no bootstrap: no V8
/// platform request, V8 isolate, QuickJS runtime or Pants runtime
/// (light-cljs-778). The output is the V8 build's. The census is
/// process-wide: run sequentially (compiler.bb does).
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_namespace_pool_runs_no_engine() {
    let files = pooled_sources("");
    let (reference, _, _) = pooled_build(&files, 1, None, Default::default());
    let reference = reference.unwrap();
    for slots in [1, 2, 4, 8] {
        let session = GooseSession::start(&runner(), slots).unwrap();
        let (output, stats, census) =
            pooled_build(&files, slots, Some(&session), Default::default());
        assert_eq!(output.as_deref(), Ok(reference.as_str()), "slots={slots}");
        assert!(census.is_empty(), "slots={slots}: {census:?}");
        // One Goose job per macro call; the helper is demanded by name.
        assert_eq!(session.stats().jobs, 64, "slots={slots}");
        let trace = session.trace();
        assert!(
            trace.iter().any(|line| line.starts_with("NEED ")),
            "slots={slots}: {trace:?}"
        );
        let units = stats.deferred_units;
        if slots > 1 {
            // Every worker is Goose-bound, with no isolate, and some
            // analyzed units that expand `bump`.
            assert_eq!(units.pool_workers, slots - 1, "slots={slots}");
            assert_eq!(units.pool_engine_workers, slots - 1, "slots={slots}");
            assert_eq!(units.worker_isolates, 0, "slots={slots}");
            assert!(units.pooled > 0, "slots={slots}: {units:?}");
            assert!(
                units.pool_engine_participants > 0,
                "slots={slots}: {units:?}"
            );
            assert_eq!(units.engine_less_units, 0, "slots={slots}: {units:?}");
        }
        session.close();
        cleaned(&session);
    }
}

/// On the Goose compiler path at compiler slots 1/2/4/8, a macro that
/// throws, one the Goose VM faults, a build past its deadline and one
/// cancelled while its macros run, each with definitions a worker may
/// analyze, fail with their error, tear the Goose session down, join every
/// native thread, and create no engine (light-cljs-778).
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_namespace_pool_failure_and_cancellation_tear_down() {
    for (body, expected) in [
        ("(m/boom)", "macro m.macros/boom: Goose"),
        ("(m/spin)", "Fault.InstructionLimit"),
        ("", "deadline"),
        ("", "cancelled"),
    ] {
        let files = pooled_sources(body);
        for slots in [1, 2, 4, 8] {
            let session =
                GooseSession::start_mode(&runner(), slots, Some("readiness-test")).unwrap();
            let options = EvaluatorOptions {
                timeout: Duration::from_millis(if expected == "deadline" { 1 } else { 60_000 }),
                ..Default::default()
            };
            // The session's control, which this evaluator makes, is the
            // build's: it is cancelled at a later check, once some of the
            // build's Goose jobs have run.
            let control = GooseEvaluator::new(session.clone(), &options)
                .unwrap()
                .control();
            if expected == "cancelled" {
                control.cancel_after_checks(40);
            }
            let (output, _, census) = pooled_build(&files, slots, Some(&session), options);
            let error = output.unwrap_err();
            assert!(error.contains(expected), "{body} slots={slots}: {error}");
            if expected == "cancelled" {
                let jobs = session.stats().jobs;
                assert!(jobs > 0 && jobs < 64, "slots={slots}: {jobs} jobs");
            }
            assert!(census.is_empty(), "{body} slots={slots}: {census:?}");
            // The pool's threads are joined once the build returns. A Goose
            // call that failed closed the session; one the analysis stopped
            // before any call is its owner's to close, as the CLI's is.
            let closed = session.process.lock().unwrap().is_none();
            assert!(closed || expected == "deadline", "{expected} slots={slots}");
            session.close();
            cleaned(&session);
            let trace = session.trace();
            assert!(trace.iter().any(|s| s == "CLOSED 1"), "{trace:?}");
            assert_threads(&trace, slots);
        }
    }
}

/// Fresh unit isolates, the reference for persistent V8 isolates, have no
/// meaning on the Goose compiler path, which runs none: a pooled build
/// asking for them fails with a named error before its pool starts, and
/// creates no engine (light-cljs-778).
#[cfg(any(debug_assertions, feature = "scheduling-stress"))]
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_refuses_fresh_unit_isolates() {
    let session = GooseSession::start(&runner(), 2).unwrap();
    let mut host = crate::MacroHost::new();
    host.use_goose_source_macros(session.clone()).unwrap();
    let census = crate::engine_census::EngineCensus::now();
    let provider = |request: &crate::source_provider::LoadRequest| {
        let name = String::from_utf16_lossy(request.namespace.name());
        Ok((name == "app.main").then(|| {
            crate::source_provider::SourceInput::new("app/main.cljs", "(ns app.main) (defn f [] 1)")
        }))
    };
    let error = crate::compiler::analyze_namespace_entry_recorded_with_provider(
        "app.main",
        &provider,
        &mut host,
        Default::default(),
        &mut Vec::new(),
        crate::compiler::EntryRecorders {
            slots: crate::scheduler::Slots::new(2).unwrap(),
            perturbation: Some(crate::scheduler::Perturbation {
                fresh_isolates: true,
                ..Default::default()
            }),
            ..Default::default()
        },
    )
    .map(drop)
    .unwrap_err()
    .to_string();
    assert!(
        error.contains(&crate::compilation_session::goose_fresh_error(
            crate::compiler_bootstrap::BUILD_ENGINE
        )),
        "{error}"
    );
    assert!(
        crate::engine_census::EngineCensus::now()
            .since(census)
            .is_empty()
    );
    drop(host);
    session.close();
    cleaned(&session);
}
/// The profile mode restores the base snapshot in a thread program of its own
/// (light-cljs-794); its single metrics map carries that worker's restore.
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_profile_restore_worker() {
    let directory = std::env::temp_dir().join(format!(
        "goose-profile-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let base =
        big_boy_pants_js::frontend::goose::export_source("goose-base.js", "rt => null").unwrap();
    std::fs::write(directory.join("base.gsp"), base.bytes).unwrap();
    let mut child = std::process::Command::new(runner())
        .arg(&directory)
        .args(["1", "profile"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let started = std::time::Instant::now();
    while child.try_wait().unwrap().is_none() {
        assert!(
            started.elapsed() < std::time::Duration::from_secs(60),
            "profile mode did not exit"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let output = child.wait_with_output().unwrap();
    std::fs::remove_dir_all(&directory).unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    let lines: Vec<&str> = text.lines().filter(|l| !l.starts_with("READY ")).collect();
    assert_eq!(lines.len(), 1, "{text}");
    let field = |name: &str| -> i64 {
        let rest = lines[0].split(&format!(":{name} ")).nth(1).unwrap();
        rest.split([' ', '}']).next().unwrap().parse().unwrap()
    };
    assert!(field("restore-ns") >= 0);
    assert!(field("restore-copy-bytes") > 0);
    assert_eq!(
        field("restore-copy-bytes") * field("raw-copy-repeats"),
        field("raw-copy-payload-bytes")
    );
}

/// The integers `return N;` of the emitted namespaces' initializers, after
/// the runtime (which ends at the first namespace switch), in order.
fn initializer_numbers(js: &str) -> Vec<u64> {
    let start = js
        .find("rt.core.currentNs = ")
        .unwrap_or_else(|| panic!("{js}"));
    js[start..]
        .split("return ")
        .skip(1)
        .filter_map(|rest| rest.split(';').next()?.parse().ok())
        .collect()
}

/// light-cljs-799: blocker_affinity.rs's counter fixture on the Goose path.
/// The macro namespace's atom becomes one process-wide cell when its helper
/// is published, so `dep`'s expansion and `main`'s each take the next value
/// at every slot count: never `1 1` (the V8 build's worker isolates without
/// `--cache-blocker`), two distinct values. The same with a published
/// volatile and `vswap!` (which expands to `-deref` and `-vreset!`), whose
/// expansion also uses a job's own volatile.
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_cells_counter_fixture() {
    for provider in [
        "(ns m.state) (def counter (atom 0)) (defmacro next-value [] (swap! counter inc))",
        "(ns m.state) (def counter (volatile! 0)) (defmacro next-value [] (let [w (volatile! 10)] (vswap! w inc) (- (vswap! counter inc) (- @w 11))))",
    ] {
        let mut files = HashMap::new();
        files.insert("m.state".to_owned(), provider.to_owned());
        files.insert(
            "app.dep".to_owned(),
            "(ns app.dep (:require [m.state :refer [next-value]])) (def value (next-value))"
                .to_owned(),
        );
        files.insert(
        "app.main".to_owned(),
        "(ns app.main (:require [app.dep :as d] [m.state :refer [next-value]])) (js/console.log d/value (next-value))".to_owned(),
    );
        for slots in [1, 2, 4, 8] {
            let session = GooseSession::start(&runner(), slots).unwrap();
            let (output, _, _) = pooled_build(&files, slots, Some(&session), Default::default());
            let js = output.unwrap();
            let value = initializer_numbers(&js);
            let logged = js
                .split("[\"log\"])(")
                .nth(1)
                .and_then(|rest| rest.split(')').next())
                .unwrap_or_else(|| panic!("{js}"));
            let last: u64 = logged.rsplit(", ").next().unwrap().parse().unwrap();
            assert_eq!(value.len(), 1, "slots={slots}: {value:?}");
            let mut both = [value[0], last];
            both.sort();
            if provider.contains("volatile!") && slots > 1 {
                // vswap! reads and then writes, as a volatile does in
                // Clojure: two expansions at once may both take 1.
                assert!(both[0] == 1 && both[1] <= 2, "slots={slots}: {logged}");
            } else {
                assert_eq!(both, [1, 2], "slots={slots}: {logged}");
            }
            session.close();
            cleaned(&session);
        }
    }
}

/// light-cljs-808 through the compiler: a macro namespace's
/// `(def d (delay (swap! counter inc)))` is one shared delay, realized once
/// for the session at every slot count, so `dep`'s expansion and `main`'s
/// both read 1 from it, and take the counter's next values after it: 12 and
/// 13, in either order (a delay realized per job would give the second 34).
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_cells_delay_fixture() {
    let mut files = HashMap::new();
    files.insert(
        "m.lazy".to_owned(),
        "(ns m.lazy) (def counter (atom 0)) (def d (delay (swap! counter inc))) (defmacro once [] (+ (* 10 @d) (if (realized? d) (swap! counter inc) 100)))".to_owned(),
    );
    files.insert(
        "app.dep".to_owned(),
        "(ns app.dep (:require [m.lazy :refer [once]])) (def value (once))".to_owned(),
    );
    files.insert(
        "app.main".to_owned(),
        "(ns app.main (:require [app.dep :as d] [m.lazy :refer [once]])) (js/console.log d/value (once))".to_owned(),
    );
    for slots in [1, 2, 4, 8] {
        let session = GooseSession::start(&runner(), slots).unwrap();
        let (output, _, _) = pooled_build(&files, slots, Some(&session), Default::default());
        let js = output.unwrap();
        let value = initializer_numbers(&js);
        let logged = js
            .split("[\"log\"])(")
            .nth(1)
            .and_then(|rest| rest.split(')').next())
            .unwrap_or_else(|| panic!("{js}"));
        let last: u64 = logged.rsplit(", ").next().unwrap().parse().unwrap();
        assert_eq!(value.len(), 1, "slots={slots}: {value:?}");
        // The two expansions may run in either order.
        let mut both = [value[0], last];
        both.sort();
        assert_eq!(both, [12, 13], "slots={slots}: {logged}");
        session.close();
        cleaned(&session);
    }
}

/// A registry macro (light-cljs-799): eight namespaces register an entry in
/// the macro namespace's atom, and a consumer that requires all of them sees
/// every entry, at every slot count; 64 expansions across the job workers
/// take 64 distinct values of one counter, so the cell ends at exactly 64.
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_cells_registry_and_shared_increments() {
    let mut files = HashMap::new();
    files.insert(
        "m.reg".to_owned(),
        "(ns m.reg)
         (def registry (atom []))
         (def counter (atom 0))
         (defmacro register [k] (swap! registry into [k]) nil)
         (defmacro bump [] (swap! counter inc))
         (defmacro entries [] (list 'quote @registry))"
            .to_owned(),
    );
    let mut main = String::from("(ns app.main (:require [m.reg :refer [entries]]");
    let mut uses = String::new();
    for n in 0..8 {
        let mut source = format!(
            "(ns app.r{n} (:require [m.reg :refer [register bump]]))\n(register {})\n",
            1000 + n
        );
        for d in 0..8 {
            source.push_str(&format!("(def v{d} (bump))\n"));
            uses.push_str(&format!(" app.r{n}/v{d}"));
        }
        files.insert(format!("app.r{n}"), source);
        main.push_str(&format!(" [app.r{n}]"));
    }
    main.push_str(&format!("))\n(js/console.log (entries){uses})\n"));
    files.insert("app.main".to_owned(), main);
    for slots in [1, 2, 4, 8] {
        let session = GooseSession::start(&runner(), slots).unwrap();
        let (output, _, _) = pooled_build(&files, slots, Some(&session), Default::default());
        let js = output.unwrap();
        let mut values = initializer_numbers(&js);
        values.retain(|&n| n < 1000);
        values.sort();
        assert_eq!(values, (1..=64).collect::<Vec<_>>(), "slots={slots}");
        let consumer = &js[js.rfind("rt.core.currentNs = ").unwrap()..];
        for n in 0..8 {
            assert!(
                consumer.contains(&format!("{}", 1000 + n)),
                "slots={slots}: {consumer}"
            );
        }
        session.close();
        cleaned(&session);
    }
}

/// One session (traced, so it prints its JOBS line), the fixture's root
/// `fixture/counter` a helper whose initializer makes an atom, a cell once
/// published, produced once for the session; each job of `bodies` in turn,
/// as the raw protocol submits them, so a failing job does not retire the
/// session (the evaluator retires it, as the V8 build retires an isolate):
/// its result or failure, and the JOBS line.
fn cell_session(
    workers: usize,
    env: &[(&str, String)],
    bodies: &[&str],
) -> (Vec<Result<String, String>>, JobsLine) {
    // A body may read `fixture/number` (the number 41) after the counter:
    // the job's second dependency wait.
    let jobs = bodies
        .iter()
        .map(|&body| {
            let reads: &[&str] = if body.contains("fixture/number") {
                &["fixture/number"]
            } else {
                &[]
            };
            (body, reads)
        })
        .collect::<Vec<_>>();
    cell_session_roots(workers, env, &[("fixture/number", "rt => 41")], &jobs)
}

/// The raw protocol's inputs for one session: the call's GFB bytes, the
/// call form and the argument graph its expansions are read against.
struct RawInputs {
    bytes: Vec<u8>,
    call: Form,
    graph: bridge::Graph,
    limits: ConversionLimits,
    budget: bridge::Budget,
}
fn raw_inputs(e: &GooseEvaluator) -> RawInputs {
    let (_, args, call) = inputs(e, "rt => (form, env) => 0");
    let values = args
        .iter()
        .map(|&id| e.get(id).unwrap())
        .collect::<Vec<_>>();
    let limits = ConversionLimits::default();
    let budget = bridge::Budget::new(limits.nodes, e.control.clone());
    let graph = bridge::Graph::arguments(&values, limits, &budget).unwrap();
    let bytes = graph.encode().unwrap();
    RawInputs {
        bytes,
        call,
        graph,
        limits,
        budget,
    }
}
/// Submit job `id`, whose definition (key 100 + id) reads the counter
/// (`a`, the job's first dependency wait) and runs `body`.
fn raw_job(session: &GooseSession, raw: &RawInputs, id: u64, body: &str) -> mpsc::Receiver<Event> {
    let key = 100 + id;
    session
        .export(
            &session.directory.join(format!("definition-{key}.gsp")),
            &format!(
                "rt => (form, env) => {{ const a = __rt.cljReadRoot(globalThis.__lightCompilerRoots, null, 'fixture/counter'); {body} }}"
            ),
        )
        .unwrap();
    raw_helper(session, id, 0, 42, "rt => rt.core.atom(0)");
    submit_key(session, &raw.bytes, id, key)
}
/// Job `id`'s dependency wait `sequence` reads unit `unit`, whose
/// initializer is `source` when this job is the first to demand it.
fn raw_helper(session: &GooseSession, id: u64, sequence: u64, unit: u64, source: &str) {
    session
        .export(
            &session.directory.join(format!("{id}-{sequence}.helper")),
            source,
        )
        .unwrap();
    session.answer_key(id, sequence, &unit.to_string()).unwrap();
}
/// Job `id`'s result (its expansion's data) or failure.
fn raw_outcome(
    session: &GooseSession,
    raw: &RawInputs,
    rx: &mpsc::Receiver<Event>,
    id: u64,
) -> Result<String, String> {
    loop {
        match rx.recv_timeout(Duration::from_secs(20)).unwrap() {
            Event::Done(_) => {
                let out = std::fs::read(session.directory.join(format!("{id}.out"))).unwrap();
                let output = bridge::Graph::decode(&out, raw.limits, &raw.budget)
                    .unwrap()
                    .expansion(
                        &raw.call,
                        origin(&raw.call),
                        &raw.graph.originals,
                        raw.limits,
                        &raw.budget,
                    )
                    .unwrap();
                break Ok(format!("{:?}", output.data()));
            }
            Event::Failure(message) => break Err(message),
            _ => {}
        }
    }
}
/// Wait until the session's trace has a line starting with `prefix`.
fn raw_trace(session: &GooseSession, prefix: &str) {
    let started = Instant::now();
    while !session.trace().iter().any(|line| line.starts_with(prefix)) {
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "no {prefix:?}: {:?}",
            session.trace()
        );
        thread::sleep(Duration::from_millis(2));
    }
}
/// Close the session, check its cleanup, and its JOBS line: one dispatch
/// per start, per resume and per helper production.
fn raw_close(session: &GooseSession, workers: usize, dispatches: u64) -> JobsLine {
    session.close();
    cleaned(session);
    let trace = session.trace();
    assert!(
        !String::from_utf8_lossy(&session.stderr.lock().unwrap()).contains("assert failed"),
        "{trace:?}"
    );
    assert_jobs(&trace, workers, dispatches)
}

/// cell_session with more roots: `helpers` are the roots a body may read
/// after the counter (units 43, 44, ... in order, with their initializers),
/// and each job of `jobs` is its body and the roots it reads, in order (its
/// dependency waits 1, 2, ...).
fn cell_session_roots(
    workers: usize,
    env: &[(&str, String)],
    helpers: &[(&str, &str)],
    jobs: &[(&str, &[&str])],
) -> (Vec<Result<String, String>>, JobsLine) {
    let session = GooseSession::start_env(&runner(), workers, Some("readiness-test"), env).unwrap();
    let e = GooseEvaluator::new(session.clone(), &Default::default()).unwrap();
    let raw = raw_inputs(&e);
    let mut outcomes = Vec::new();
    let mut produced = vec![false; helpers.len()];
    let mut reads = 0;
    for (index, (body, roots)) in jobs.iter().enumerate() {
        let id = index as u64 + 1;
        for (sequence, root) in roots.iter().enumerate() {
            let at = helpers
                .iter()
                .position(|(name, _)| name == root)
                .unwrap_or_else(|| panic!("no helper {root}"));
            raw_helper(
                &session,
                id,
                sequence as u64 + 1,
                43 + at as u64,
                helpers[at].1,
            );
            produced[at] = true;
            reads += 1;
        }
        let rx = raw_job(&session, &raw, id, body);
        outcomes.push(raw_outcome(&session, &raw, &rx, id));
        thread::sleep(Duration::from_millis(20));
    }
    // A start and a resume per root read of each job (the counter's
    // included), and one production per root the session produced.
    let productions = 1 + produced.iter().filter(|&&p| p).count() as u64;
    let jobs = raw_close(
        &session,
        workers,
        2 * jobs.len() as u64 + reads + productions,
    );
    (outcomes, jobs)
}

/// A job that faults after its swap! leaves the swap in place for the next
/// job (an effect, as in shadow-cljs); storing a mutable JavaScript object
/// and adding a watch fail by name before they change the cell.
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_cell_effects_and_named_errors() {
    for workers in [1, 2, 4, 8] {
        let (outcomes, jobs) = cell_session(
            workers,
            &[],
            &[
                "rt.core.swap(a, rt.core.inc); const o = null; return o.x;",
                "return rt.core.deref(a);",
                "rt.core.reset(a, {n: 1}); return 0;",
                "rt.core.addWatch(a, 1, () => 0); return 0;",
                "return rt.core.compareAndSet(a, 1, 5) ? rt.core.deref(a) : -1;",
            ],
        );
        assert!(
            outcomes[0]
                .as_ref()
                .unwrap_err()
                .contains("Fault.TypeError"),
            "{outcomes:?}"
        );
        assert_eq!(outcomes[1].as_deref(), Ok("Double(1.0)"), "{outcomes:?}");
        assert!(
            outcomes[2]
                .as_ref()
                .unwrap_err()
                .contains("Fault.NativeBoundary:reset:-cannot-share-a-mutable-JavaScript-object"),
            "{outcomes:?}"
        );
        assert!(
            outcomes[3].as_ref().unwrap_err().contains(
                "Fault.NativeBoundary:addWatch:-watches-and-validators-are-not-supported"
            ),
            "{outcomes:?}"
        );
        // Neither changed the cell: it still holds the faulting job's 1.
        assert_eq!(outcomes[4].as_deref(), Ok("Double(5.0)"), "{outcomes:?}");
        assert_eq!((jobs.cells, jobs.conflicts), (1, 0), "{jobs:?}");
        assert!(
            jobs.cell_operations >= 6 && jobs.owner_ns > 0 && jobs.delay_waits == 0,
            "{jobs:?}"
        );
    }
}

/// `(swap! a (fn [x] (reset! a x) (inc x)))` on an atom holding 0: the
/// function's reset! stores the value it read, so the cell still holds that
/// value's identity and the swap stores 1 without rerunning (Clojure's
/// Atom.swap, not a version counter). A stored vector keeps its identity:
/// storing back what was read is no conflict either, and a later load in the
/// job is the same object.
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_cell_swap_same_value_reset() {
    let (outcomes, jobs) = cell_session(
        1,
        &[],
        &[
            "let calls = 0; const r = rt.core.swap(a, (x) => { calls++; rt.core.reset(a, x); return x + 1; }); return r * 100 + calls * 10 + rt.core.deref(a);",
            "rt.core.reset(a, rt.vectorFromArray([1, 2])); let calls = 0; const v = rt.core.deref(a); rt.core.swap(a, (x) => { calls++; rt.core.reset(a, x); return x; }); return (rt.core.deref(a) === v ? 1 : 0) + calls * 10;",
        ],
    );
    assert_eq!(outcomes[0].as_deref(), Ok("Double(111.0)"), "{outcomes:?}");
    assert_eq!(outcomes[1].as_deref(), Ok("Double(11.0)"), "{outcomes:?}");
    assert_eq!((jobs.cells, jobs.conflicts), (1, 0), "{jobs:?}");
}

/// Cells and identities past the session budget (BBP_CELL_BUDGET) fail the
/// operation with the named error, and the session closes cleanly.
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_cell_budget() {
    let (outcomes, jobs) = cell_session(
        1,
        &[("BBP_CELL_BUDGET", "2".to_owned())],
        &[
            "rt.core.reset(a, rt.vectorFromArray([1])); return 1;",
            "rt.core.reset(a, rt.vectorFromArray([2])); return 2;",
            "return 3;",
        ],
    );
    assert_eq!(outcomes[0].as_deref(), Ok("Double(1.0)"), "{outcomes:?}");
    assert!(
        outcomes[1]
            .as_ref()
            .unwrap_err()
            .contains("Fault.NativeBoundary:reset:-cell-budget"),
        "{outcomes:?}"
    );
    assert_eq!(outcomes[2].as_deref(), Ok("Double(3.0)"), "{outcomes:?}");
    // The cell and the first vector's identity; the second was refused.
    assert_eq!((jobs.cells, jobs.identities), (1, 1), "{jobs:?}");
    // A compare-and-set the cell refuses numbers nothing: a swap whose
    // first attempt loses to a write stores its vector within the budget.
    let (outcomes, jobs) = cell_session(
        1,
        &[("BBP_CELL_BUDGET", "2".to_owned())],
        &[
            "const v = rt.vectorFromArray([1]); let calls = 0; rt.core.swap(a, (x) => { calls++; if (calls === 1) rt.core.reset(a, 5); return v; }); return calls * 10 + (rt.core.deref(a) === v ? 1 : 0);",
        ],
    );
    assert_eq!(outcomes[0].as_deref(), Ok("Double(21.0)"), "{outcomes:?}");
    assert_eq!((jobs.identities, jobs.conflicts), (1, 1), "{jobs:?}");
}

/// Strings keep every UTF-16 unit through a cell (non-Latin-1 units,
/// surrogate pairs and a lone surrogate); compare-and-set! compares before
/// it shares its replacement, so a miss is false and leaves a job's own
/// atom unshared; swap-vals! and reset-vals! answer the exact pair, the
/// one of the attempt that stored after a conflicting write; the V8 build's
/// local swap-vals! pairs the state replaced at the store; swap! calls its
/// function with a null `this` and may call swap! itself.
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_cell_values_pairs_and_callbacks() {
    for workers in [1, 2, 4, 8] {
        let (outcomes, jobs) = cell_session(
            workers,
            &[],
            &[
                "rt.core.reset(a, '\\u0100\\ud83d\\ude00\\ud800x'); return 0;",
                "const s = rt.core.deref(a); return s === '\\u0100\\ud83d\\ude00\\ud800x' ? s.length : -1;",
                "rt.core.reset(a, 0); const miss = rt.core.compareAndSet(a, 1, {n: 1}); const b = rt.core.atom(0); const local = rt.core.compareAndSet(a, 1, b); rt.core.reset(b, {n: 1}); return (miss ? 1 : 0) + (local ? 1 : 0) + rt.core.deref(a) + 7;",
                "let calls = 0; return rt.core.swapVals(a, (x) => { calls++; if (calls === 1) rt.core.reset(a, 7); return x + 1; });",
                "return rt.core.resetVals(a, 3);",
                "const b = rt.core.atom(0); return rt.core.swapVals(b, (x) => { rt.core.reset(b, 10); return 2; });",
                "const inner = rt.core.atom(0); const outer = rt.core.atom(inner); const local = rt.core.swap(outer, rt.core.swap, rt.core.inc); const shared = rt.core.atom(a); const n = rt.core.swap(shared, rt.core.swap, rt.core.inc); return local * 10 + n;",
                "const s = rt.core.swap(rt.core.atom(0), Object.prototype.toString); const t = rt.core.swap(rt.core.atom(0), function (x) { 'use strict'; return this === null ? 1 : 0; }); return s === '[object Null]' ? t + 1 : -1;",
            ],
        );
        let ok = |i: usize| {
            outcomes[i]
                .as_deref()
                .unwrap_or_else(|e| panic!("{i}: {e}"))
        };
        assert_eq!(ok(0), "Double(0.0)", "{outcomes:?}");
        assert_eq!(ok(1), "Double(5.0)", "{outcomes:?}");
        assert_eq!(ok(2), "Double(7.0)", "{outcomes:?}");
        for (i, pair) in [
            (3, ["7.0", "8.0"]),
            (4, ["8.0", "3.0"]),
            (5, ["10.0", "2.0"]),
        ] {
            let text = ok(i);
            let first = text.find(&format!("Double({})", pair[0]));
            let second = text.rfind(&format!("Double({})", pair[1]));
            assert!(
                matches!((first, second), (Some(f), Some(s)) if f < s),
                "{i}: {text}"
            );
        }
        assert_eq!(ok(6), "Double(14.0)", "{outcomes:?}");
        assert_eq!(ok(7), "Double(2.0)", "{outcomes:?}");
        assert_eq!(jobs.conflicts, 1, "{jobs:?}");
    }
}

/// A swap! whose function waits for a helper resumes from a saved
/// continuation (retention 0) in a fresh worker (every worker retires after
/// its job): the value it read keeps its identity, the function is not
/// called again without a conflicting write, and the cell holds its result,
/// the helper's value itself (a form, kept by identity).
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_cell_swap_across_helper_suspension() {
    for workers in [1, 2, 4, 8] {
        let (outcomes, jobs) = cell_session(
            workers,
            &[
                ("BBP_RETAINED_WORKERS", "0".to_owned()),
                ("BBP_WORKER_TABLE_BYTES", "0".to_owned()),
            ],
            &[
                "rt.core.reset(a, rt.vectorFromArray([1])); const v = rt.core.deref(a); let calls = 0; const r = rt.core.swap(a, (x) => { calls++; const n = __rt.cljReadRoot(globalThis.__lightCompilerRoots, null, 'fixture/number'); return x === v ? n : -1; }); return r * 100 + calls * 10 + (rt.core.deref(a) === r ? 1 : 0);",
                "return rt.core.deref(a);",
            ],
        );
        assert_eq!(outcomes[0].as_deref(), Ok("Double(4111.0)"), "{outcomes:?}");
        assert_eq!(outcomes[1].as_deref(), Ok("Double(41.0)"), "{outcomes:?}");
        assert_eq!(jobs.conflicts, 0, "{jobs:?}");
        assert!(jobs.saves >= 3 && jobs.retired == jobs.spawned, "{jobs:?}");
    }
}

/// A root read in a fixture's JavaScript.
fn root(name: &str) -> String {
    format!("__rt.cljReadRoot(globalThis.__lightCompilerRoots, null, '{name}')")
}
/// The session variants the delay and identity tests run in: 1, 2, 4 and 8
/// execution slots, with workers that wait in place and with every wait a
/// saved continuation and every worker retired after its job.
fn delay_variants() -> Vec<(usize, Vec<(&'static str, String)>)> {
    let mut variants = Vec::new();
    for workers in [1, 2, 4, 8] {
        variants.push((workers, vec![]));
        variants.push((
            workers,
            vec![
                ("BBP_RETAINED_WORKERS", "0".to_owned()),
                ("BBP_WORKER_TABLE_BYTES", "0".to_owned()),
            ],
        ));
    }
    variants
}

/// light-cljs-808: a published delay is one process-wide cell. Two jobs
/// force it at once: the first claims it and runs its thunk, which counts
/// its calls in an atom it captured (a cell too) and then waits for a
/// helper the test holds back (the barrier) until the second job's force
/// waits for the claim; both get the one value, and a later force takes it
/// without a call. A thunk that throws throws its error (type and message)
/// to every force, also later ones, one error object in a job, and runs
/// once. A cycle of claims (each
/// job's thunk forces the delay the other claimed) fails by name, in both
/// jobs: the second, released by the first's failure, claims the first's
/// delay and then reaches its own claim.
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_shared_delay_realizes_once() {
    let number = "rt => 41";
    let delay = format!(
        "rt => {{ const calls = rt.core.atom(0); return new rt.core.Delay(() => {{ rt.core.swap(calls, rt.core.inc); const n = {}; return rt.core.deref(calls) * 100 + n; }}, null); }}",
        root("fixture/number")
    );
    let force = |name: &str| format!("const d = {}; return rt.core.deref(d);", root(name));
    for (workers, env) in delay_variants() {
        let session =
            GooseSession::start_env(&runner(), workers, Some("readiness-test"), &env).unwrap();
        let e = GooseEvaluator::new(session.clone(), &Default::default()).unwrap();
        let raw = raw_inputs(&e);
        // Job 1 claims and stops in its thunk at fixture/number (unanswered).
        raw_helper(&session, 1, 1, 43, &delay);
        let first = raw_job(&session, &raw, 1, &force("fixture/delay"));
        raw_trace(&session, "NEED 1 2 fixture/number");
        // Job 2's force waits for job 1's claim.
        raw_helper(&session, 2, 1, 43, &delay);
        let second = raw_job(&session, &raw, 2, &force("fixture/delay"));
        raw_trace(&session, "WAIT 2 ");
        raw_helper(&session, 1, 2, 44, number);
        let outcomes = [
            raw_outcome(&session, &raw, &first, 1),
            raw_outcome(&session, &raw, &second, 2),
        ];
        // A later force: the value, no call.
        raw_helper(&session, 3, 1, 43, &delay);
        let third = raw_job(
            &session,
            &raw,
            3,
            &format!(
                "const d = {}; return rt.core.deref(d) + (rt.core.realized(d) ? 1000 : 0) + (rt.core.isDelay(d) ? 10000 : 0);",
                root("fixture/delay")
            ),
        );
        let later = raw_outcome(&session, &raw, &third, 3);
        // Starts, root reads (3 + 2 + 2), one delay wake, productions
        // (counter, delay, number).
        let jobs = raw_close(&session, workers, 3 + 7 + 1 + 3);
        assert_eq!(
            outcomes,
            [
                Ok("Double(141.0)".to_owned()),
                Ok("Double(141.0)".to_owned())
            ],
            "workers={workers} {env:?}"
        );
        assert_eq!(
            later.as_deref(),
            Ok("Double(11141.0)"),
            "workers={workers} {env:?}"
        );
        assert_eq!(jobs.delay_waits, 1, "workers={workers} {env:?}: {jobs:?}");
        // The delay and the atom its thunk captured, and the counter.
        assert_eq!(jobs.cells, 3, "{jobs:?}");
    }

    // A thunk that throws: every force throws its error, and it ran once.
    let bad = format!(
        "rt => new rt.core.Delay(() => {{ const c = {}; rt.core.swap(c, rt.core.inc); {}; throw TypeError('boom'); }}, null)",
        root("fixture/calls"),
        root("fixture/number")
    );
    let calls = "rt => rt.core.atom(0)";
    let catch = format!(
        "const d = {}; try {{ rt.core.force(d); return -1; }} catch (e) {{ let again = null; try {{ rt.core.deref(d); }} catch (f) {{ again = f; }} return (e.message === 'boom' ? 1 : 0) + (e.name === 'TypeError' ? 10 : 0) + (again === e ? 100 : 0); }}",
        root("fixture/bad")
    );
    for (workers, env) in delay_variants() {
        let session =
            GooseSession::start_env(&runner(), workers, Some("readiness-test"), &env).unwrap();
        let e = GooseEvaluator::new(session.clone(), &Default::default()).unwrap();
        let raw = raw_inputs(&e);
        raw_helper(&session, 1, 1, 43, &bad);
        raw_helper(&session, 1, 2, 45, calls);
        let first = raw_job(&session, &raw, 1, &catch);
        raw_trace(&session, "NEED 1 3 fixture/number");
        raw_helper(&session, 2, 1, 43, &bad);
        let second = raw_job(&session, &raw, 2, &catch);
        raw_trace(&session, "WAIT 2 ");
        raw_helper(&session, 1, 3, 44, number);
        let outcomes = [
            raw_outcome(&session, &raw, &first, 1),
            raw_outcome(&session, &raw, &second, 2),
        ];
        raw_helper(&session, 3, 1, 43, &bad);
        raw_helper(&session, 3, 2, 45, calls);
        let third = raw_job(
            &session,
            &raw,
            3,
            &format!(
                "const d = {}; let first = null; try {{ rt.core.deref(d); }} catch (e) {{ first = e; }} let r = 0; try {{ rt.core.force(d); }} catch (e) {{ r = e === first && e.message === 'boom' ? 1 : 0; }} const c = {}; return r + rt.core.deref(c) * 10 + (rt.core.realized(d) ? 100 : 0);",
                root("fixture/bad"),
                root("fixture/calls")
            ),
        );
        let later = raw_outcome(&session, &raw, &third, 3);
        // Starts, root reads (4 + 2 + 3), one delay wake, productions
        // (counter, bad, calls, number).
        let jobs = raw_close(&session, workers, 3 + 9 + 1 + 4);
        assert_eq!(
            outcomes,
            [
                Ok("Double(111.0)".to_owned()),
                Ok("Double(111.0)".to_owned())
            ],
            "workers={workers} {env:?}"
        );
        assert_eq!(
            later.as_deref(),
            Ok("Double(111.0)"),
            "workers={workers} {env:?}"
        );
        assert_eq!(jobs.delay_waits, 1, "workers={workers} {env:?}: {jobs:?}");
    }

    // A cycle of claims: job 1 claims d1, job 2 claims d2, each thunk
    // forces the other delay after the gate.
    let cyclic = |other: &str| {
        format!(
            "rt => new rt.core.Delay(() => {{ {}; const o = {}; return rt.core.deref(o); }}, null)",
            root("fixture/gate"),
            root(other)
        )
    };
    let (d1, d2) = (cyclic("fixture/d2"), cyclic("fixture/d1"));
    for (workers, env) in delay_variants() {
        let session =
            GooseSession::start_env(&runner(), workers, Some("readiness-test"), &env).unwrap();
        let e = GooseEvaluator::new(session.clone(), &Default::default()).unwrap();
        let raw = raw_inputs(&e);
        raw_helper(&session, 1, 1, 43, &d1);
        let first = raw_job(&session, &raw, 1, &force("fixture/d1"));
        raw_trace(&session, "NEED 1 2 fixture/gate");
        // Job 2: d2, its gate (the session's first), d1 (job 1 holds it);
        // after job 1 fails, d1's thunk again, whose roots the job holds
        // already: gate, then d2 (its own claim).
        raw_helper(&session, 2, 1, 44, &d2);
        raw_helper(&session, 2, 2, 45, "rt => 0");
        raw_helper(&session, 2, 3, 43, &d1);
        let second = raw_job(&session, &raw, 2, &force("fixture/d2"));
        raw_trace(&session, "WAIT 2 ");
        raw_helper(&session, 1, 2, 45, "rt => 0");
        raw_helper(&session, 1, 3, 44, &d2);
        let outcomes = [
            raw_outcome(&session, &raw, &first, 1),
            raw_outcome(&session, &raw, &second, 2),
        ];
        // Starts, root reads (4 + 4), one delay wake, productions (counter,
        // d1, d2, gate).
        let jobs = raw_close(&session, workers, 2 + 8 + 1 + 4);
        for outcome in &outcomes {
            assert!(
                outcome
                    .as_ref()
                    .unwrap_err()
                    .contains("Fault.NativeBoundary:deref:-a-cycle-of-delay-claims"),
                "workers={workers} {env:?}: {outcomes:?}"
            );
        }
        assert_eq!(jobs.delay_waits, 1, "workers={workers} {env:?}: {jobs:?}");
    }
}

/// light-cljs-808: published values keep their identity across jobs. One
/// expansion stores a published vector (a form helper's value) into an
/// atom; later ones, each on a fresh worker (every worker retires after
/// its job), find it identical to the helper's value, whichever they read
/// first, and so are the vector's elements inside a stored vector. Two
/// references to one cell, one captured by a callable helper and one stored
/// in a cell value, are one object in a job, in either order.
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_cells_preserve_published_alias_identity() {
    let helpers = [
        ("fixture/token", "rt => rt.vectorFromArray([1, 2])"),
        (
            "fixture/getter",
            "rt => { const c = rt.core.atom(5); return () => c; }",
        ),
        // An object a publication holds both directly and as a cell's
        // initial content: a callable helper and a form helper.
        (
            "fixture/pair",
            "rt => { const t = rt.vectorFromArray([1]); const c = rt.core.atom(t); return () => rt.core.deref(c) === t ? 1 : 0; }",
        ),
        (
            "fixture/boxed",
            "rt => { const t = rt.vectorFromArray([1]); return rt.vectorFromArray([rt.core.atom(t), t]); }",
        ),
    ];
    let token = root("fixture/token");
    let getter = root("fixture/getter");
    let jobs: [(String, &[&str]); 10] = [
        (
            format!(
                "const t = {token}; rt.core.reset(a, t); return rt.core.deref(a) === t ? 1 : 0;"
            ),
            &["fixture/token"],
        ),
        (
            format!("const t = {token}; return t === rt.core.deref(a) ? 1 : 0;"),
            &["fixture/token"],
        ),
        (
            format!("const s = rt.core.deref(a); const t = {token}; return t === s ? 1 : 0;"),
            &["fixture/token"],
        ),
        (
            format!(
                "const t = {token}; rt.core.reset(a, rt.vectorFromArray([t, rt.vectorFromArray([t])])); return 0;"
            ),
            &["fixture/token"],
        ),
        (
            format!(
                "const v = rt.core.deref(a); const t = {token}; return rt.core.mapv((x) => x === t ? 1 : 0, v);"
            ),
            &["fixture/token"],
        ),
        (
            format!(
                "const g = {getter}; rt.core.reset(a, rt.vectorFromArray([g()])); return rt.core.deref(g()) + 1;"
            ),
            &["fixture/getter"],
        ),
        (
            format!(
                "const g = {getter}; const v = rt.core.deref(a); return rt.core.mapv((x) => x === g() ? 1 : 0, v);"
            ),
            &["fixture/getter"],
        ),
        (
            format!(
                "const v = rt.core.deref(a); const g = {getter}; rt.core.swap(g(), rt.core.inc); return rt.core.mapv((x) => x === g() ? rt.core.deref(x) : 0, v);"
            ),
            &["fixture/getter"],
        ),
        (
            format!("return {}();", root("fixture/pair")),
            &["fixture/pair"],
        ),
        (
            format!(
                "const v = {}; let c = null; let t = null; let i = 0; rt.core.mapv((x) => {{ if (i++ === 0) c = x; else t = x; return 0; }}, v); return rt.core.deref(c) === t ? 1 : 0;",
                root("fixture/boxed")
            ),
            &["fixture/boxed"],
        ),
    ];
    let jobs = jobs
        .iter()
        .map(|(body, reads)| (body.as_str(), *reads))
        .collect::<Vec<_>>();
    for (workers, env) in delay_variants() {
        let (outcomes, line) = cell_session_roots(workers, &env, &helpers, &jobs);
        let ok = |i: usize| {
            outcomes[i]
                .as_deref()
                .unwrap_or_else(|e| panic!("{i}: {e}"))
                .to_owned()
        };
        for i in 0..3 {
            assert_eq!(ok(i), "Double(1.0)", "{i}: workers={workers} {env:?}");
        }
        // The stored vector [token [token]]: its first element, a node
        // below the root, is the helper's value; its second is not.
        let pair = ok(4);
        assert!(
            matches!((pair.find("Double(1.0)"), pair.find("Double(0.0)")), (Some(one), Some(zero)) if one < zero)
                && pair.matches("Double").count() == 2,
            "workers={workers} {env:?}: {pair}"
        );
        assert_eq!(ok(5), "Double(6.0)", "workers={workers} {env:?}");
        for i in [6, 7] {
            assert!(
                ok(i).contains("kind: Vector") && !ok(i).contains("Double(0.0)"),
                "{i}: workers={workers} {env:?}: {}",
                ok(i)
            );
        }
        assert!(
            ok(7).contains("Double(6.0)"),
            "workers={workers} {env:?}: {}",
            ok(7)
        );
        for i in [8, 9] {
            assert_eq!(ok(i), "Double(1.0)", "{i}: workers={workers} {env:?}");
        }
        if !env.is_empty() {
            assert_eq!(line.retired, line.spawned, "{line:?}");
        }
        // The same through a prepared definition, in two jobs.
        let session =
            GooseSession::start_env(&runner(), workers, Some("readiness-test"), &env).unwrap();
        let e = GooseEvaluator::new(session.clone(), &Default::default()).unwrap();
        let raw = raw_inputs(&e);
        let mut outcomes = Vec::new();
        for id in 1..=2u64 {
            session
                .export(
                    &session.directory.join("definition-101.gsp"),
                    "rt => { const t = rt.vectorFromArray([1]); const c = rt.core.atom(t); return (form, env) => rt.core.deref(c) === t ? 1 : 0; }",
                )
                .unwrap();
            let rx = submit_key(&session, &raw.bytes, id, 101);
            outcomes.push(raw_outcome(&session, &raw, &rx, id));
        }
        raw_close(&session, workers, 2);
        assert_eq!(
            outcomes,
            [Ok("Double(1.0)".to_owned()), Ok("Double(1.0)".to_owned())],
            "workers={workers} {env:?}"
        );
    }
}

/// light-cljs-809: functions in cells. One expansion stores a closure that
/// captures a cell (a published atom) and a number into an atom; a later
/// one changes the captured cell, and a later one, each on a fresh worker
/// in the retiring variants, calls the stored closure and gets the cell's
/// current value plus the number, and two loads are one function. Storing
/// a closure that captures a mutable object, or a variable it may assign,
/// fails by name before the cell changes. A stored function is a swap!
/// function in any job, swap! may store a closure over the closure it read,
/// a published callable helper stored in a cell is the helper's value in a
/// later job (whichever it reads first), a built-in stored is the built-in,
/// and a definition's atom may start out holding a closure.
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_cells_hold_functions() {
    let helpers = [
        ("fixture/box", "rt => rt.core.atom(10)"),
        (
            "fixture/adder",
            "rt => { const k = 100; return (x) => x + k; }",
        ),
    ];
    let boxed = root("fixture/box");
    let adder = root("fixture/adder");
    let jobs: [(String, &[&str]); 15] = [
        (
            format!(
                "const b = {boxed}; const n = 5; rt.core.reset(a, () => rt.core.deref(b) + n); return 0;"
            ),
            &["fixture/box"],
        ),
        (
            format!("rt.core.reset({boxed}, 20); return 0;"),
            &["fixture/box"],
        ),
        (
            "const f = rt.core.deref(a); return f() * 10 + (rt.core.deref(a) === f ? 1 : 0);"
                .to_owned(),
            &[],
        ),
        (
            "const o = {x: 1}; rt.core.reset(a, () => o.x); return 0;".to_owned(),
            &[],
        ),
        (
            "let m = 1; const g = () => m; m = 2; rt.core.reset(a, g); return 0;".to_owned(),
            &[],
        ),
        ("return rt.core.deref(a)();".to_owned(), &[]),
        (
            "const n = 2; rt.core.reset(a, (x) => x + n); return 0;".to_owned(),
            &[],
        ),
        (
            "const c = rt.core.atom(1); rt.core.swap(c, rt.core.deref(a)); rt.core.swap(c, rt.core.deref(a)); return rt.core.deref(c);"
                .to_owned(),
            &[],
        ),
        (
            "rt.core.swap(a, (f) => (x) => f(x) * 10); return rt.core.deref(a)(1);".to_owned(),
            &[],
        ),
        (
            format!("rt.core.reset(a, {adder}); return rt.core.deref(a)(1);"),
            &["fixture/adder"],
        ),
        (
            format!(
                "const s = rt.core.deref(a); const g = {adder}; return (s === g ? 1000 : 0) + s(1);"
            ),
            &["fixture/adder"],
        ),
        (
            "rt.core.reset(a, rt.core.inc); return 0;".to_owned(),
            &[],
        ),
        (
            "const f = rt.core.deref(a); return f(41) + (f === rt.core.inc ? 1000 : 0);"
                .to_owned(),
            &[],
        ),
        // Stored while a for-in walk and a catch handler are live.
        (
            "const o = {x: 1, y: 2}; let r = 0; try { for (const k in o) { const n = r + 1; rt.core.reset(a, () => n); r = rt.core.deref(a)(); } } catch (e) { r = -1; } return r;"
                .to_owned(),
            &[],
        ),
        ("return rt.core.deref(a)();".to_owned(), &[]),
    ];
    let jobs = jobs
        .iter()
        .map(|(body, reads)| (body.as_str(), *reads))
        .collect::<Vec<_>>();
    for (workers, env) in delay_variants() {
        let (outcomes, line) = cell_session_roots(workers, &env, &helpers, &jobs);
        let at = |i: usize| (i, outcomes[i].clone());
        let ok = |i: usize, value: &str| (i, Ok(value.to_owned()));
        assert_eq!(at(0), ok(0, "Double(0.0)"), "workers={workers} {env:?}");
        assert_eq!(at(2), ok(2, "Double(251.0)"), "workers={workers} {env:?}");
        for (i, what) in [
            (
                3,
                "Fault.NativeBoundary:reset:-cannot-share-a-mutable-JavaScript-object",
            ),
            (
                4,
                "Fault.NativeBoundary:reset:-cannot-share-a-function-that-captures-a-variable-it-may-assign",
            ),
        ] {
            assert!(
                outcomes[i].as_ref().is_err_and(|e| e.contains(what)),
                "{i}: workers={workers} {env:?}: {outcomes:?}"
            );
        }
        // Neither refused store changed the cell.
        assert_eq!(at(5), ok(5, "Double(25.0)"), "workers={workers} {env:?}");
        assert_eq!(at(7), ok(7, "Double(5.0)"), "workers={workers} {env:?}");
        assert_eq!(at(8), ok(8, "Double(30.0)"), "workers={workers} {env:?}");
        assert_eq!(at(9), ok(9, "Double(101.0)"), "workers={workers} {env:?}");
        assert_eq!(
            at(10),
            ok(10, "Double(1101.0)"),
            "workers={workers} {env:?}"
        );
        assert_eq!(
            at(12),
            ok(12, "Double(1042.0)"),
            "workers={workers} {env:?}"
        );
        assert_eq!(at(13), ok(13, "Double(2.0)"), "workers={workers} {env:?}");
        assert_eq!(at(14), ok(14, "Double(2.0)"), "workers={workers} {env:?}");
        // The counter and the box; no swap! here met a conflicting write.
        assert_eq!((line.cells, line.conflicts), (2, 0), "{line:?}");
        if !env.is_empty() {
            assert_eq!(line.retired, line.spawned, "{line:?}");
        }
        // A definition's atom holding a closure from the start, in two jobs.
        let session =
            GooseSession::start_env(&runner(), workers, Some("readiness-test"), &env).unwrap();
        let e = GooseEvaluator::new(session.clone(), &Default::default()).unwrap();
        let raw = raw_inputs(&e);
        let mut outcomes = Vec::new();
        for id in 1..=2u64 {
            session
                .export(
                    &session.directory.join("definition-101.gsp"),
                    "rt => { const k = 7; const g = () => k; const c = rt.core.atom(g); return (form, env) => (rt.core.deref(c) === g ? 10 : 0) + rt.core.deref(c)(); }",
                )
                .unwrap();
            let rx = submit_key(&session, &raw.bytes, id, 101);
            outcomes.push(raw_outcome(&session, &raw, &rx, id));
        }
        raw_close(&session, workers, 2);
        assert_eq!(
            outcomes,
            [Ok("Double(17.0)".to_owned()), Ok("Double(17.0)".to_owned())],
            "workers={workers} {env:?}"
        );
    }
}

/// light-cljs-809: what a swap! on a cell costs in a macro job, for a number
/// (no graph: the fixed cost), a value encoding to about 64 bytes (a
/// one-number vector, 136 bytes of GFB) and to 64 KB (1,260 numbers),
/// against probe P18's flat-byte figures (9.6 to 9.8 and 28.8 to 33.8
/// microseconds a swap at one worker). The call's
/// source file is 100 KB, as a real namespace's is: a cell store exports
/// only what the value reaches, not the job's source text. Each swap loads
/// the value it stored (an identity the job holds, so no import), encodes
/// the value again and compare-and-sets it. Prints the figures; the
/// assertion is only that the swaps ran.
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_cell_swap_cost() {
    let session = GooseSession::start_env(
        &runner(),
        1,
        Some("readiness-test"),
        &[("BBP_RESET_CHECK", "0".to_owned())],
    )
    .unwrap();
    let e = GooseEvaluator::new(session.clone(), &Default::default()).unwrap();
    let mut raw = raw_inputs(&e);
    let mut text = String::from("nil\n");
    while text.len() < 100_000 {
        text.push_str(";; a namespace's source text\n");
    }
    let source = Source::new("large.cljs", &text);
    let call = Form::from_data(
        Datum::new(DatumKind::Nil),
        source.span(0..3).unwrap(),
        Origin::Source,
    );
    let call_id = e.import_form(&call, Default::default()).unwrap();
    let env = e
        .import_data(&Datum::new(DatumKind::Nil), Default::default())
        .unwrap();
    let values = [call_id, env]
        .iter()
        .map(|&id| e.get(id).unwrap())
        .collect::<Vec<_>>();
    raw.graph = bridge::Graph::arguments(&values, raw.limits, &raw.budget).unwrap();
    raw.bytes = raw.graph.encode().unwrap();
    raw.call = call;
    assert!(raw.bytes.len() > 100_000);
    let mut id = 0;
    let mut timed = |value: &str, swaps: usize| {
        id += 1;
        let body = format!(
            "const xs = []; for (let i = 0; i < 1260; i++) xs[i] = i; const v = {value}; rt.core.reset(a, v); const g = (x) => v; for (let i = 0; i < {swaps}; i++) rt.core.swap(a, g); return 0;"
        );
        let started = Instant::now();
        let rx = raw_job(&session, &raw, id, &body);
        assert_eq!(
            raw_outcome(&session, &raw, &rx, id).as_deref(),
            Ok("Double(0.0)")
        );
        started.elapsed()
    };
    // The last, a closure capturing a number, is published from the whole
    // job heap, as a callable helper is (callable::encode_cell).
    for (what, value, swaps) in [
        ("a number", "1", 4000),
        ("a one-number vector", "rt.vectorFromArray([1])", 4000),
        ("a 1,260-number vector", "rt.vectorFromArray(xs)", 1000),
        (
            "a closure",
            "(() => { const n = 1; return () => n; })()",
            1000,
        ),
    ] {
        // Seven samples, sorted: the fixed cost is two owner round trips,
        // whose thread wake-ups vary from run to run.
        let mut per_swap = Vec::new();
        for _ in 0..7 {
            let base = timed(value, 0);
            let run = timed(value, swaps);
            per_swap.push(run.saturating_sub(base).as_secs_f64() * 1e6 / swaps as f64);
        }
        per_swap.sort_by(f64::total_cmp);
        assert!(per_swap[0] > 0.0);
        println!("cell swap: {what}, {swaps} swaps: {per_swap:.1?} microseconds a swap");
    }
    session.close();
}

/// light-cljs-808: a job's own delay follows the V8 build's deferredValue
/// (its thunk runs once, a thunk that throws stays and runs again at the
/// next force, a thunk that waits for a helper resumes in a fresh worker),
/// `force` and `deref` passed to built-ins as callbacks force pending,
/// realized and shared delays, a delay realized before it was published
/// travels as a form helper's value, and an atom holding another atom
/// (a bare cell reference) swaps without a conflict.
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_delays_in_jobs_and_cell_references() {
    let helpers = [
        ("fixture/number", "rt => 41"),
        (
            "fixture/ready",
            "rt => { const d = new rt.core.Delay(() => 7, null); rt.core.force(d); return d; }",
        ),
    ];
    let number = root("fixture/number");
    let ready = root("fixture/ready");
    let bodies: [(String, &[&str]); 7] = [
        (
            "let calls = 0; const d = new rt.core.Delay(() => { calls++; return 5; }, null); const before = rt.core.realized(d); const x = rt.core.deref(d) + rt.core.force(d); return calls * 100 + x + (before ? 1000 : 0) + (rt.core.realized(d) ? 10000 : 0);".to_owned(),
            &[],
        ),
        (
            "let calls = 0; const d = new rt.core.Delay(() => { calls++; if (calls === 1) throw TypeError('x'); return 3; }, null); let r = 0; try { rt.core.force(d); } catch (e) { r = 1; } return rt.core.force(d) * 100 + calls * 10 + r;".to_owned(),
            &[],
        ),
        (
            format!("const d = new rt.core.Delay(() => {number} + 1, null); return rt.core.deref(d) + (rt.core.realized(d) ? 100 : 0);"),
            &["fixture/number"],
        ),
        (
            "const v = rt.vectorFromArray([new rt.core.Delay(() => 4, null), 6]); return rt.core.mapv(rt.core.force, v);".to_owned(),
            &[],
        ),
        (
            format!("const d = {ready}; rt.core.reset(a, rt.vectorFromArray([d])); return rt.core.deref(d) + (rt.core.realized(d) ? 100 : 0);"),
            &["fixture/ready"],
        ),
        (
            "return rt.core.mapv(rt.core.deref, rt.core.deref(a));".to_owned(),
            &[],
        ),
        (
            "const b = rt.core.atom(1); rt.core.reset(a, b); let calls = 0; const r = rt.core.swap(a, (x) => { calls++; return x; }); const c = rt.core.atom(2); const pair = rt.core.resetVals(a, c); return (r === b ? 1 : 0) + calls * 10 + (rt.core.compareAndSet(a, c, b) ? 100 : 0) + (rt.core.deref(a) === b ? 1000 : 0);".to_owned(),
            &[],
        ),
    ];
    let jobs = bodies
        .iter()
        .map(|(body, reads)| (body.as_str(), *reads))
        .collect::<Vec<_>>();
    for (workers, env) in delay_variants() {
        let (outcomes, line) = cell_session_roots(workers, &env, &helpers, &jobs);
        let ok = |i: usize| {
            outcomes[i]
                .as_deref()
                .unwrap_or_else(|e| panic!("{i}: workers={workers} {env:?}: {e}"))
                .to_owned()
        };
        assert_eq!(ok(0), "Double(10110.0)", "workers={workers} {env:?}");
        assert_eq!(ok(1), "Double(321.0)", "workers={workers} {env:?}");
        assert_eq!(ok(2), "Double(142.0)", "workers={workers} {env:?}");
        let forced = ok(3);
        assert!(
            matches!((forced.find("Double(4.0)"), forced.find("Double(6.0)")), (Some(f), Some(s)) if f < s),
            "workers={workers} {env:?}: {forced}"
        );
        assert_eq!(ok(4), "Double(107.0)", "workers={workers} {env:?}");
        assert!(
            ok(5).contains("Double(7.0)"),
            "workers={workers} {env:?}: {}",
            ok(5)
        );
        assert_eq!(ok(6), "Double(1111.0)", "workers={workers} {env:?}");
        assert_eq!(line.conflicts, 0, "{line:?}");
    }
}
