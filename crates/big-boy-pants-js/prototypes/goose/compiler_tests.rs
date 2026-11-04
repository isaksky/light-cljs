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
        include_bytes!("generated-200k-manifest.edn")
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
        drop(host);
        if let Some(s) = session {
            assert_eq!(s.stats().jobs, cases.len() as u64 + 2);
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
        e.register_compiler_root("fixture/mapseq", Some(42), true, "mapseq.cljs", None)
            .unwrap();
        let called = Rc::new(Cell::new(0));
        let counter = called.clone();
        e.install_helper(Rc::new(move |_, _| {
            counter.set(counter.get() + 1);
            Ok("rt => {let x=0;for(let i=0;i<40;i++)x++;return x;}".into())
        }));
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
    let body = r#"rt => {
        const a=[1,2,3]; a.length=1; a.length=3;
        const sequence=rt.core.sequence;
        return (form,env) => {
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
        (
            "return rt.core.sequence(null,null);",
            "Fault.NativeBoundary",
        ),
        (
            "return __rt.cljConcat(__rt.formMap(1,2),null);",
            "Fault.UnsupportedOperand",
        ),
        ("return rt.core.mapSeq(x=>x);", "Fault.NativeBoundary"),
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
        e.register_compiler_root("fixture/wrapped", Some(42), true, "wrapper.cljs", None)
            .unwrap();
        let called = Rc::new(Cell::new(0));
        let counter = called.clone();
        e.install_helper(Rc::new(move |_, _| {
            counter.set(counter.get() + 1);
            Ok("rt => 40".into())
        }));
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
pub(super) fn assert_pool(trace: &[String], workers: usize, chunks: u64) {
    let pools = trace
        .iter()
        .filter(|s| s.starts_with("POOL "))
        .map(|s| {
            s.split_whitespace()
                .skip(2)
                .map(|n| n.parse::<u64>().unwrap())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        pools.len(),
        workers,
        "one private storage owner per execution worker: {trace:?}"
    );
    assert_eq!(pools.iter().map(|p| p[1]).sum::<u64>(), chunks);
    assert!(pools.iter().all(|p| p[1] == p[2]), "every dispatch reset");
    assert!(
        pools.iter().map(|p| p[5]).sum::<u64>() > 0,
        "reused table addresses"
    );
    assert!(pools.iter().all(|p| p[6] > 0 && p[3] <= 536_870_912));
    assert!(pools.iter().all(|p| p[3] == p[8..12].iter().sum::<u64>()));
    assert!(
        pools.iter().all(|p| p[4] <= 27 * 4),
        "finite high water despite repeated jobs"
    );
    assert_eq!(
        trace.iter().find(|s| s.starts_with("THREADS ")).unwrap(),
        &format!(
            "THREADS 0 {} {} {}",
            workers * 2 + 2,
            workers * 2 + 2,
            workers * 2 + 1
        )
    );
}
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_preparation_identity_and_private_calls() {
    let source = "rt => { let state = {n: 40}; return (form, env) => { state.n = state.n + 1; return state.n; }; }";
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
            assert_pool(&session.trace(), workers, 17);
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
            e.register_compiler_root("fixture/value", Some(unit), true, "fixture.cljs", None)
                .unwrap();
            let callbacks = Rc::new(Cell::new(0));
            let calls = callbacks.clone();
            e.install_helper(Rc::new(move |_, requested| {
                assert_eq!(requested, unit);
                calls.set(calls.get() + 1);
                // The real compiler likewise has equivalent native/canonical
                // emissions before/after releasing one catalog unit's IR.
                Ok(if calls.get() == 1 {
                    format!("rt => {unit}")
                } else {
                    format!("rt => ({} + 1)", unit - 1)
                })
            }));
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
    // Error properties/prototypes are explicitly outside the bounded surface.
    let source = "(defmacro caught [] (js* \"(() => {try {null.x;} catch(e) {return e.name;}})()\")) (caught)";
    assert!(crate::compile_with_macros(source, &mut crate::MacroHost::new()).is_ok());
    let session = GooseSession::start(&runner(), 1).unwrap();
    let mut host = crate::MacroHost::new();
    host.use_goose_source_macros(session.clone()).unwrap();
    let error = crate::compile_with_macros(source, &mut host)
        .unwrap_err()
        .to_string();
    assert!(error.contains("UnsupportedOperand"), "{error}");
    cleaned(&session);
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
            evaluator
                .register_compiler_root("fixture/value", Some(0), true, "fixture.cljs", None)
                .unwrap();
            evaluator.install_helper(Rc::new(|_, _| {
                thread::sleep(Duration::from_millis(60));
                Ok("rt => __rt.formVector(1)".into())
            }));
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
        "(defmacro effect [] (js* \"(typeof globalThis.Array === 'undefined' ? 1 : 2)\")) (effect)",
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
fn goose_compiler_cleanup_abort_reclaims_queued_and_in_transit_work() {
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
                Event::Done => break,
                Event::Start | Event::Resumed => {}
                other => panic!("unexpected first result: {other:?}"),
            }
        }
        wait(&|| session.directory.join("cleanup-ready").exists());
        // Worker 1 has published its result but still owns cleanup. At limit 1
        // this replacement stays queued; with peers it is held between dequeue
        // and claim publication. Neither case may be forgotten by poison.
        let replacement = send(2);
        wait(&|| session.trace().iter().any(|s| s == "START 2"));
        if workers > 1 {
            wait(&|| session.directory.join("claim-ready").exists());
        }
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
        // Observe native closure before calling close: the host's 100 ms kill
        // fallback must not turn a deadlocked native shutdown into a pass.
        wait(&|| session.trace().iter().any(|s| s == "CLOSED 1"));
        assert!(!session.directory.join("2.out").exists());
        assert!(!session.trace().iter().any(|s| s == "DONE 2"));
        assert!(session.trace().iter().any(|s| s
            == &format!(
                "THREADS 0 {} {} {}",
                workers * 2 + 2,
                workers * 2 + 2,
                workers * 2 + 1
            )));
        session.close();
        cleaned(&session);
    }
}

#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_drains_ready_batches_without_progress_backoff() {
    for callable in [false, true] {
        for workers in [1, 2, 4, 8] {
            let session =
                GooseSession::start_mode(&runner(), workers, Some("dispatch-test")).unwrap();
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
            &format!("rt => {{ let state = {{n: 0}}; return (form, env) => {{ state.n = state.n + 1; let value = __rt.cljReadRoot(globalThis.__lightCompilerRoots, null, 'fixture/value'); return state.n + {result}; }}; }}")).unwrap();
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
                        Event::Done => break,
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
            let threads = trace.iter().find(|s| s.starts_with("THREADS ")).unwrap();
            assert_eq!(
                threads,
                &format!(
                    "THREADS 0 {} {} {}",
                    workers * 2 + 2,
                    workers * 2 + 2,
                    workers * 2 + 1
                ),
                "coordinator + input + configured execution workers and join-only supervisors"
            );
            assert_pool(&trace, workers, 17);
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
        e.register_compiler_root("fixture/value", Some(41), true, "fixture.cljs", None)
            .unwrap();
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
        e.install_helper(Rc::new(move |_, unit| {
            assert_eq!(unit, 41);
            delayed.recv_timeout(Duration::from_secs(10)).unwrap();
            Ok("rt => 41".into())
        }));
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
    let session = GooseSession::start_mode(&runner(), 1, Some("readiness-test")).unwrap();
    let e = GooseEvaluator::new(session.clone(), &Default::default()).unwrap();
    let (function, args, _) = inputs(
        &e,
        "rt => (form, env) => __rt.cljReadRoot(globalThis.__lightCompilerRoots, null, 'fixture/value')",
    );
    e.register_compiler_root("fixture/value", Some(41), true, "fixture.cljs", None)
        .unwrap();
    e.install_helper(Rc::new(|_, _| Ok("rt => 41".into())));
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
    assert_pool(&session.trace(), 1, 513);
    assert!(
        session.trace().iter().any(|s| s == "CLOSED 1"),
        "bounded failure closes natively"
    );
    assert!(!String::from_utf8_lossy(&session.stderr.lock().unwrap()).contains("assert failed"));
}

#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_large_continuations_reclaim_across_batches() {
    use crate::parser::CollectionKind;
    // The pinned build suspends 529 times, with a largest saved continuation
    // just over 3 MB. Repeat a similarly sized transported argument twice as
    // often in one session: cumulative copying must not consume retention.
    let session = GooseSession::start_mode(&runner(), 1, Some("readiness-test")).unwrap();
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
    e.register_compiler_root("fixture/value", Some(41), true, "fixture.cljs", None)
        .unwrap();
    e.install_helper(Rc::new(|_, _| Ok("rt => 41".into())));
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
    assert_pool(&session.trace(), 1, 2 * 1058 + 1);
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
    session.close();
    cleaned(&session);
    assert!(session.trace().iter().any(|s| s == "CLOSED 1"));
    assert!(!String::from_utf8_lossy(&session.stderr.lock().unwrap()).contains("assert failed"));
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
            &format!("rt => {{ let state = {{n: {base}}}; return (form, env, x) => {{ state.n = state.n + 1; return __rt.formVector(x, state.n); }}; }}")).unwrap());
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
        assert_pool(&session.trace(), workers, 24);
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
fn goose_compiler_cancel_reaps_persistent_execution_workers() {
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
            e.register_compiler_root("fixture/infinite", Some(42), true, "cancel.cljs", None)
                .unwrap();
            e.install_helper(Rc::new(|_, _| Ok("rt => function(){while(true){}}".into())));
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
            assert_eq!(
                trace.iter().find(|s| s.starts_with("THREADS ")).unwrap(),
                &format!(
                    "THREADS 0 {} {} {}",
                    workers * 2 + 2,
                    workers * 2 + 2,
                    workers * 2 + 1
                )
            );
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
            r#"rt => {const state={n:0};return (form,env) => {
            state.n++;
            const f=__rt.cljReadRoot(globalThis.__lightCompilerRoots,null,'fixture/function');
            const alias=__rt.cljReadRoot(globalThis.__lightCompilerRoots,null,'fixture/alias');
            const result=f.call(null,state);
            return state.n===2 && f===alias && result===43;
        };}"#,
        );
        for (name, unit) in [
            ("fixture/function", 42),
            ("fixture/alias", 42),
            ("fixture/number", 43),
        ] {
            e.register_compiler_root(name, Some(unit), true, "callable.cljs", None)
                .unwrap();
        }
        let callbacks = Rc::new(Cell::new(0));
        let count = callbacks.clone();
        e.install_helper(Rc::new(move |_, unit| {
            count.set(count.get()+1);
            Ok(if unit == 42 {
                "rt => function(state){state.n++; const n=__rt.cljReadRoot(globalThis.__lightCompilerRoots,null,'fixture/number'); return this===null ? n+state.n : -100;}".into()
            } else { assert_eq!(unit,43); "rt => 41".into() })
        }));
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
            e.register_compiler_root("fixture/throw", Some(42), true, "throw.cljs", None)
                .unwrap();
            e.install_helper(Rc::new(move |_, _| Ok(helper.into())));
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
