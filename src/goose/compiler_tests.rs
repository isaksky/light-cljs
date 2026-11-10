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
            // The prepared branch exports the native emission the job's
            // demand would, never the canonical one, whose core calls a
            // native form would reach (light-cljs-821). Inspect the actual
            // transported compiler artifacts; no rewritten helper body is
            // supplied by this test.
            let exports = std::fs::read_dir(&s.directory)
                .unwrap()
                .map(|e| e.unwrap().path())
                .filter(|p| p.extension().is_some_and(|e| e == "gsp"))
                .map(|p| String::from_utf8_lossy(&std::fs::read(p).unwrap()).into_owned())
                .collect::<Vec<_>>();
            assert!(
                exports.iter().all(|s| !s.contains("rt.core.list")),
                "a canonical helper was exported"
            );
            let helpers = exports
                .iter()
                .filter(|s| {
                    s.contains("__rt.formList(__rt.formSymbol(\"workload.common\", \"norm\")")
                })
                .count();
            assert_eq!(helpers, 1);
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
fn goose_compiler_runtime_eval() {
    // light-cljs-856: a macro's JavaScript compiles code at run time in the
    // session's job worker. Each call compiles again on the reused worker,
    // whose reset drops the code it added (BBP_RESET_CHECK=1 in compiler.bb
    // compares its tables with the image after every job).
    // light-cljs-863: the definition's factory compiles too, in its
    // preparation: an indirect eval's closure and a direct eval's value
    // join the prepared definition, whose snapshot carries their code to
    // every job. So does a demanded helper's initializer, in its production
    // job: a closure helper publishes the eval code as its code suffix, and
    // a data helper's value was computed by eval code. The serial control
    // (one retained heap) compiles in its macro job, its definition's
    // factory and its helpers' initializers alike.
    for (workers, mode) in [(1, None), (2, None), (1, Some("serial"))] {
        let s = GooseSession::start_mode(&runner(), workers, mode).unwrap();
        let e = GooseEvaluator::new(s.clone(), &Default::default()).unwrap();
        let source = r#"rt => {
            const add=(0,eval)("(function add(x) { return x + 1; })");
            const direct=(function(x) { return eval("x * 2"); })(20);
            return (form,env) => {
                const bump=(0,eval)("let n=40; () => ++n");
                let syntax=false;
                try { (0,eval)("1 +"); } catch(e) { syntax=e instanceof SyntaxError; }
                const twice=__rt.cljReadRoot(globalThis.__lightCompilerRoots,null,'fixture/twice');
                const value=__rt.cljReadRoot(globalThis.__lightCompilerRoots,null,'fixture/value');
                return (0,eval)("1 + 1")===2 && (0,eval)(5)===5 && bump()===41 && bump()===42 && syntax
                    && add(direct)===41 && twice(21)===42 && __rt.formValue(value)===42;
            };
        }"#;
        let (f, args, call) = inputs(&e, source);
        serve(
            &e,
            &[("fixture/twice", 42), ("fixture/value", 43)],
            |unit| {
                Ok(if unit == 42 {
                    r#"rt => (0,eval)("const k = 2; (function twice(x) { return k * x; })")"#.into()
                } else {
                    r#"rt => (0,eval)("40 + 2")"#.into()
                })
            },
        );
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
        let trace = s.trace();
        assert_eq!(
            trace
                .iter()
                .filter(|line| line.starts_with("PREPARED "))
                .count(),
            1
        );
        assert_eq!(
            trace
                .iter()
                .filter(|line| line.starts_with("PRODUCED "))
                .count(),
            2
        );
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
            const b=[]; b.length='2';
            const xs=__rt.formList(1,2);
            const joined=__rt.cljConcatAll(__rt.formList(null,xs,__rt.cljConcat(xs,__rt.formVector(3))));
            return before && b.length===2 && a===alias && a.length===4 && a[0]===undefined && a[1]===undefined && a[2]===7 && a[3]===undefined
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
            "Fault.RangeError",
        ),
        (
            "const a=[];a.length=1.5;return a;",
            "Fault.RangeError",
        ),
        (
            "const a=[];a.length=0/0;return a;",
            "Fault.RangeError",
        ),
        (
            "const a=[];a.length=1/0;return a;",
            "Fault.RangeError",
        ),
        (
            "const a=[];a.length=4294967296;return a;",
            "Fault.RangeError",
        ),
        ("const a=[];a.length=1048577;return a;", "Fault.MemoryLimit"),
        // Every opcode exports; one the Goose VM does not implement fails the
        // macro by name when it runs, uncatchably (light-cljs-783).
        (
            "function P(){} try { const it = {next(){ return {value: new P(), done: false}; }}; it[Symbol.iterator] = function(){ return this; }; for (const x of it) { using r = null; return x instanceof P; } return 0; } catch (e) { return null; }",
            "Fault.Unsupported:using_check",
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
        // Operands the V8 build's runtime accepts and this one does not
        // implement fail by name, not as a cljs.core Error (light-cljs-873):
        // a JavaScript array as a map entry source, a set of entries (its
        // iteration order), the first item of a map, set-validator! with a
        // nil validator on a value that is no reference.
        (
            "return rt.core.into(__rt.formMap(), __rt.formVector([]));",
            "Fault.NativeBoundary",
        ),
        (
            "return __rt.cljConj(__rt.formMap(), __rt.formSet(__rt.formVector(1, 2)));",
            "Fault.NativeBoundary",
        ),
        (
            "return __rt.cljFirst(__rt.formMap(1, 2));",
            "Fault.NativeBoundary",
        ),
        (
            "return rt.core.setValidator(null, null);",
            "Fault.NativeBoundary",
        ),
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
        // Arguments objects are ordinary objects (light-cljs-854): an element
        // write leaves the strict parameter alone, and a strict `callee` is
        // the %ThrowTypeError% accessor.
        (
            "function f(a){arguments[0]=3; return a===1 && arguments[0]===3;} return f(1);",
            None,
        ),
        (
            "function f(){return arguments.callee;} return f();",
            Some("Fault.TypeError"),
        ),
        // Its prototype is %Object.prototype%, which `__proto__` reads
        // (light-cljs-883). Returned as the macro result, that realm object
        // is refused by the form bridge, which reports no emitted span.
        (
            "function f(){return arguments.__proto__===Object.prototype && Object.getPrototypeOf(arguments)===Object.prototype;} return f();",
            None,
        ),
        (
            "function f(){return arguments.__proto__;} return f();",
            Some("bridge-Error.Unsupported"),
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
            let bridge = expected.starts_with("bridge-");
            assert!(
                error.0.contains(expected) && (bridge || error.0.contains("emitted-span=")),
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
    /// Collections inside jobs (light-cljs-847, reclaim.goose): 0 while
    /// every job stays below its table margins.
    pub reclaims: u64,
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
    assert_eq!(n.len(), 30, "{line}");
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
        reclaims: n[29],
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
    // These jobs stay far below their table margins, so none collects
    // (light-cljs-847).
    assert_eq!(jobs.reclaims, 0, "{jobs:?}");
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
    // An uncaught exception's description leads the error (light-cljs-874).
    for (source, described) in [
        ("rt => 42", None),
        (
            "rt => { null.x; return (form, env) => 1; }",
            Some("TypeError: cannot read property 'x' of null"),
        ),
        (
            "rt => { globalThis.forbidden = 1; return (form, env) => 1; }",
            None,
        ),
        ("rt => { throw 42; }", Some("42")),
        ("rt => { throw Error('init'); }", Some("Error: init")),
    ] {
        let session = GooseSession::start(&runner(), 2).unwrap();
        let e = GooseEvaluator::new(session.clone(), &Default::default()).unwrap();
        let (f, args, _) = inputs(&e, source);
        let failure = e.call(f, &args).unwrap_err();
        assert!(
            failure.0.contains("preparation-") && failure.0.contains("emitted-span="),
            "{failure}"
        );
        match described {
            Some(text) => assert!(
                failure.0.starts_with(&format!("{text}; Goose: ERROR ")),
                "{failure}"
            ),
            None => assert!(failure.0.starts_with("Goose: "), "{failure}"),
        }
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
    // Each fake reads the call's frame header (`C id definition bytes`) and
    // answers it. A reply frame (light-cljs-816) for an unknown id, over the
    // graph bound, or that ends early fails the call explicitly; one in the
    // old line shape is malformed.
    for (body, malformed_graph, expected) in [
        ("read c id rest\nexit 0", false, "subprocess exited"),
        (
            "read c id rest\nprintf 'malformed\\n'\nread stop",
            false,
            "malformed subprocess reply",
        ),
        (
            "read c id rest\nprintf 'DONE %s\\n' \"$id\"\nread stop",
            false,
            "malformed subprocess reply",
        ),
        (
            "read c id rest\nprintf 'DONE 999 0 0 0 0 0 1\\nx'\nread stop",
            false,
            "uncorrelated subprocess reply",
        ),
        (
            "read c id rest\nprintf 'DONE %s 0 0 0 0 0 16777217\\n' \"$id\"\nread stop",
            false,
            "oversized subprocess reply",
        ),
        (
            "read c id rest\nprintf 'DONE %s 0 0 0 0 0 100\\nxx' \"$id\"\nexit 0",
            false,
            "truncated subprocess reply",
        ),
        (
            "read c id rest\nprintf 'DONE %s 0 0 0 0 0 1\\nx' \"$id\"\nread stop",
            true,
            "",
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
            let failure = result.unwrap_err();
            assert!(failure.0.contains(expected), "{body}: {failure}");
        }
        cleaned(&session);
    }
}
/// The runner fails the session explicitly on a call frame over the graph
/// bound, one whose payload ends early (the compiler's side closed while it
/// was half-written), or a command it does not know (light-cljs-816): its
/// FAULT reaches every pending call, and it still closes and exits.
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_runner_refuses_bad_frames() {
    for (case, fault) in [
        ("oversized", "input-oversized"),
        ("truncated", "input-truncated"),
        ("unknown", "input-malformed"),
        ("answer", "input-malformed"),
    ] {
        let session = GooseSession::start(&runner(), 1).unwrap();
        let (tx, rx) = mpsc::channel();
        session.pending.lock().unwrap().insert(1, tx);
        match case {
            "oversized" => session.send("C 1 1 16777217\n", &[]).unwrap(),
            "truncated" => {
                session.send("C 1 1 1000\n", b"GFB2-short").unwrap();
                // The compiler's end of the pipe goes mid-frame.
                drop(session.input.lock().unwrap().take());
            }
            "unknown" => session.send("X 1 2 3\n", &[]).unwrap(),
            _ => session.send("A 1 0 7\n", &[]).unwrap(),
        }
        let message = loop {
            match rx.recv_timeout(Duration::from_secs(10)).unwrap() {
                Event::Failure(message) => break message,
                Event::Start | Event::Resumed => {}
                other => panic!("{case}: expected the runner's fault: {other:?}"),
            }
        };
        assert!(message.contains(fault), "{case}: {message}");
        let until = Instant::now() + Duration::from_secs(10);
        while !session.trace().iter().any(|s| s == "CLOSED 1") {
            assert!(Instant::now() < until, "{case}: {:?}", session.trace());
            thread::sleep(Duration::from_millis(1));
        }
        session.close();
        cleaned(&session);
        assert!(
            !session.trace().iter().any(|s| s.starts_with("KILLED ")),
            "{case}: {:?}",
            session.trace()
        );
    }
}
/// The runner's input credits bound the argument bytes it holds
/// (light-cljs-816): a call's credits go back only when its payload leaves
/// the coordinator's queue. 2,000 calls of 300 KiB each (600 MB in all),
/// sent as fast as the credits allow to one execution slot, keep a backlog
/// for most of the run; the runner's resident memory, sampled throughout,
/// stays far below what keeping consumed payloads until the backlog
/// emptied takes (light-cljs-816's review measured both).
#[cfg(unix)]
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_input_credit_lifetimes_stay_bounded() {
    let session = GooseSession::start(&runner(), 1).unwrap();
    let e = GooseEvaluator::new(session.clone(), &Default::default()).unwrap();
    // A 300 KiB payload: a call form holding 150 Ki code units of string
    // text, since a source travels as its identity, not its text
    // (light-cljs-817).
    let raw = raw_inputs_from(
        &e,
        "big.cljs",
        "\"x\"",
        DatumKind::String(vec![u16::from(b'x'); 150 << 10].into()),
    );
    assert!(raw.bytes.len() >= 300 << 10, "{}", raw.bytes.len());
    session
        .export(
            &session.directory.join("definition-1.gsp"),
            // Each body runs long enough for the backlog to persist.
            "rt => (form, env) => { let x = 0; for (let i = 0; i < 50000; i++) x = x + 1; return x - 49993; }",
        )
        .unwrap();
    let calls = 2000u64;
    let replies = (1..=calls)
        .map(|id| {
            let (tx, rx) = mpsc::channel();
            session.pending.lock().unwrap().insert(id, tx);
            rx
        })
        .collect::<Vec<_>>();
    let pid = session.process.lock().unwrap().as_ref().unwrap().id();
    let stop = Arc::new(AtomicBool::new(false));
    let sampler = {
        let stop = stop.clone();
        thread::spawn(move || {
            let mut peak = 0u64;
            while !stop.load(Ordering::Relaxed) {
                let out = std::process::Command::new("ps")
                    .args(["-o", "rss=", "-p", &pid.to_string()])
                    .output()
                    .unwrap();
                if let Ok(kib) = String::from_utf8_lossy(&out.stdout).trim().parse::<u64>() {
                    peak = peak.max(kib << 10);
                }
                thread::sleep(Duration::from_millis(20));
            }
            peak
        })
    };
    let writer = {
        let (session, bytes) = (session.clone(), raw.bytes.clone());
        thread::spawn(move || {
            for id in 1..=calls {
                session
                    .send(&format!("C {id} 1 {}\n", bytes.len()), &bytes)
                    .unwrap();
            }
        })
    };
    // The first result decodes to 7; one conversion budget would not cover
    // decoding every call's copy of the string.
    assert!(raw_outcome(&raw, &replies[0]).unwrap().contains('7'));
    for rx in &replies[1..] {
        loop {
            match rx.recv_timeout(Duration::from_secs(20)).unwrap() {
                Event::Done(_, out) => break assert!(out.len() >= 32),
                Event::Failure(message) => panic!("{message}"),
                _ => {}
            }
        }
    }
    writer.join().unwrap();
    stop.store(true, Ordering::Relaxed);
    let peak = sampler.join().unwrap();
    eprintln!("runner resident memory peak {peak}");
    assert!(
        peak > 0 && peak < 320 << 20,
        "runner resident memory {peak}"
    );
    session.close();
    cleaned(&session);
}
/// close() while a compiler thread is blocked half-way through writing a
/// call's frame to a runner that stopped reading (light-cljs-816): close()
/// waits for the input only within its quiet bound, kills the runner, the
/// writer fails, and nothing is left behind.
#[cfg(unix)]
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_close_while_a_frame_is_half_written() {
    let session = fake("read c id rest\nexec sleep 30");
    let writer = {
        let session = session.clone();
        thread::spawn(move || {
            // Far more than a pipe holds: the write blocks part-way.
            let payload = vec![0u8; 4 << 20];
            session.send(&format!("C 1 1 {}\n", payload.len()), &payload)
        })
    };
    let until = Instant::now() + Duration::from_secs(5);
    while session.input.try_lock().is_ok() {
        assert!(Instant::now() < until, "the writer never took the input");
        thread::yield_now();
    }
    thread::sleep(Duration::from_millis(50));
    assert!(!writer.is_finished(), "the frame fit in the pipe");
    let quiet = Duration::from_millis(300);
    let started = Instant::now();
    session.close_within(quiet, Duration::from_secs(20));
    let elapsed = started.elapsed();
    assert!(
        elapsed >= quiet && elapsed < Duration::from_secs(5),
        "{elapsed:?}"
    );
    assert!(
        writer.join().unwrap().is_err(),
        "the half-written frame must fail"
    );
    assert!(session.trace().iter().any(|l| l.starts_with("KILLED ")));
    assert!(session.send("0\n", &[]).unwrap_err().0.contains("closed"));
    cleaned(&session);
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
    // cljs.core's nth throws its Error, a HostEffect fault (light-cljs-873).
    assert!(
        message.contains("HostEffect:Index-out-of-bounds function="),
        "{message}"
    );
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
/// Compile `(defmacro m [] body) (def x (m))` with the V8 build and on the
/// Goose path (light-cljs-872). Where the V8 build compiles it, the Goose
/// path's output is the same. Where `uncaught` names the V8 build's
/// JavaScript exception, the Goose path fails with an uncaught exception of
/// that fault, never a NativeBoundary (the diagnostic's text is
/// light-cljs-874's).
#[cfg(light_v8)]
fn macro_errors_match_v8(body: &str, uncaught: Option<(&str, &str)>) {
    source_matches_v8(&format!("(defmacro m [] {body}) (def x (m))"), uncaught);
}
/// [`macro_errors_match_v8`] of a whole compiled `source`.
#[cfg(light_v8)]
fn source_matches_v8(source: &str, uncaught: Option<(&str, &str)>) {
    let reference = crate::compile_with_macros(source, &mut crate::MacroHost::new());
    let session = GooseSession::start(&runner(), 1).unwrap();
    let mut host = crate::MacroHost::new();
    host.use_goose_source_macros(session.clone()).unwrap();
    let goose = crate::compile_with_macros(source, &mut host);
    session.close();
    drop(host);
    cleaned(&session);
    match uncaught {
        None => assert_eq!(
            goose.unwrap_or_else(|e| panic!("{source}: {e}")),
            reference.unwrap_or_else(|e| panic!("{source}: {e}")),
            "{source}"
        ),
        Some((exception, fault)) => {
            let reference = reference.unwrap_err().to_string();
            assert!(reference.contains(exception), "{source}: {reference}");
            let message = goose.unwrap_err().to_string();
            assert!(
                message.contains(&format!("Fault.{fault}")) && !message.contains("NativeBoundary"),
                "{source}: {message}"
            );
        }
    }
}
/// An Error a macro constructs with `js/Error.` (light-cljs-872).
#[cfg(light_v8)]
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_macro_constructs_an_error() {
    macro_errors_match_v8(r#"(let [e (js/Error. "x")] (.-message e))"#, None);
    macro_errors_match_v8(
        r#"(let [e (js/TypeError. "y")] [(.-name e) (instance? js/Error e)])"#,
        None,
    );
}
/// `js/` globals, which the emitter reads through its `host` helper, and
/// compiled member reads of them (light-cljs-872).
#[cfg(light_v8)]
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_macro_reads_host_globals() {
    macro_errors_match_v8("(.-name js/Error)", None);
    macro_errors_match_v8("(.-length js/Object)", None);
    macro_errors_match_v8(r#"(js/Array.isArray (js* "[1]"))"#, None);
}
/// A thrown Error caught by a macro's `try` (light-cljs-872).
#[cfg(light_v8)]
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_macro_catches_a_thrown_error() {
    macro_errors_match_v8(
        r#"(try (throw (js* "new Error(\"x\")")) (catch :default e (.-message e)))"#,
        None,
    );
    macro_errors_match_v8("(try (throw 7) (catch :default e e))", None);
    macro_errors_match_v8(
        r#"(try (throw (js/RangeError. "r")) (catch js/RangeError e (ex-message e)))"#,
        None,
    );
}
/// A property read of nil is a TypeError, caught or uncaught (light-cljs-872).
#[cfg(light_v8)]
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_macro_property_read_of_nil_is_a_type_error() {
    macro_errors_match_v8(
        "(try (let [x (identity nil)] (.-foo x)) (catch :default e 1))",
        None,
    );
    macro_errors_match_v8(
        r#"(try (let [x (identity nil)] (.-foo x)) (catch :default e (.-name e)))"#,
        None,
    );
    macro_errors_match_v8(
        r#"(let [x (identity nil)] (aget x "foo"))"#,
        Some(("TypeError: Cannot read properties of null", "TypeError")),
    );
}
/// ex-info, its readers and an uncaught ExceptionInfo (light-cljs-872).
#[cfg(light_v8)]
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_macro_throws_and_catches_ex_info() {
    macro_errors_match_v8(
        r#"(try (throw (ex-info "b" {})) (catch :default e 1))"#,
        None,
    );
    macro_errors_match_v8(
        r#"(try (throw (ex-info "bad" {:a 1})) (catch :default e (ex-message e)))"#,
        None,
    );
    macro_errors_match_v8(
        r#"(let [e (ex-info "m" {:a 1 :b 2} (js/Error. "c"))] [(ex-data e) (ex-message (ex-cause e)) (ex-data (js/Error. "z")) (ex-message 5) (instance? js/Error e) (.-name e) (js* "Object.getPrototypeOf(~{}) !== Error.prototype && Object.getPrototypeOf(Object.getPrototypeOf(~{})) === Error.prototype" e e)])"#,
        None,
    );
    macro_errors_match_v8(
        r#"(throw (ex-info "bad arg" {:a 1}))"#,
        Some(("Error: bad arg", "HostEffect")),
    );
}
/// cljs.core's ExceptionInfo constructor (light-cljs-875): direct
/// construction, `instance?`, a `catch` of the type, constructor identity,
/// its `name` and `length`, and `new` from JavaScript.
#[cfg(light_v8)]
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_macro_uses_the_exception_info_constructor() {
    macro_errors_match_v8(
        r#"(let [e (ExceptionInfo. "x" {:a 1} nil)] [(ex-message e) (ex-data e) (ex-cause e) (instance? ExceptionInfo e) (instance? js/Error e) (.-name e)])"#,
        None,
    );
    macro_errors_match_v8(
        r#"(let [e (ex-info "m" {})] [(instance? ExceptionInfo e) (= ExceptionInfo (.-constructor e)) (js* "~{} === ~{}" ExceptionInfo (.-constructor e)) (instance? ExceptionInfo (js/Error. "z")) (instance? ExceptionInfo 5)])"#,
        None,
    );
    macro_errors_match_v8(
        r#"(try (throw (ExceptionInfo. "boom" {:k 2} (js/Error. "c"))) (catch ExceptionInfo e [(ex-data e) (ex-message (ex-cause e))]))"#,
        None,
    );
    macro_errors_match_v8(
        r#"[(.-name ExceptionInfo) (.-length ExceptionInfo) (= (.-prototype ExceptionInfo) (js/Object.getPrototypeOf (ex-info "p" {})))]"#,
        None,
    );
    macro_errors_match_v8(
        r#"(let [e (js* "new ~{}(\"j\", 1)" ExceptionInfo)] [(ex-message e) (ex-data e) (nil? (ex-cause e)) (instance? ExceptionInfo e)])"#,
        None,
    );
    macro_errors_match_v8(
        r#"[(let [E ExceptionInfo] (ex-data (E. "a" 4 nil))) (let [e (js* "Reflect.construct(~{}, [\"r\", 3], Error)" ExceptionInfo)] [(instance? ExceptionInfo e) (instance? js/Error e) (ex-message e)])]"#,
        None,
    );
    // Constructed with another newTarget: the new object inherits its
    // `prototype` (Object.prototype when that is no object) before the
    // fields are assigned, so an inherited getter-only `message` refuses
    // the assignment (a TypeError: the V8 build's runtime is strict code).
    macro_errors_match_v8(
        r#"(js* "(() => { function F() {} F.prototype = 1; const a = Reflect.construct(~{}, ['x', 1, null], F); function G() {} G.prototype = {get message() { return 'inherited'; }}; let b; try { Reflect.construct(~{}, ['y', 2, null], G); } catch (e) { b = e.name; } return [Object.getPrototypeOf(a) === Object.prototype, a.message, a.data, b].join(); })()" ExceptionInfo ExceptionInfo)"#,
        None,
    );
    // A light-cljs value is never an instance of an error type.
    macro_errors_match_v8(
        "[(instance? ExceptionInfo {}) (instance? ExceptionInfo []) (instance? ExceptionInfo (delay 1)) (instance? js/Error {:a 1}) (instance? js/TypeError '(1))]",
        None,
    );
    // Called with a receiver, it fills that object in, as the V8 build's does.
    macro_errors_match_v8(
        r#"(let [o (js* "{}")] (.call ExceptionInfo o "q" 1 2) [(.-message o) (.-data o) (.-cause o) (.-name o)])"#,
        None,
    );
    // A helper that constructs or tests for it: the helper's publication
    // reaches the realm's ExceptionInfo rows from `rt`.
    source_matches_v8(
        r#"(defn mk [m] (ExceptionInfo. m {:z 1} nil)) (defn is [e] (instance? ExceptionInfo e)) (defmacro m [] [(ex-data (mk "h")) (is (mk "i")) (is 1)]) (def x (m))"#,
        None,
    );
    // instance? of the Delay leaf, rt.core.isInstance's other case.
    macro_errors_match_v8(
        "(let [d (delay 1)] [(instance? Delay d) (instance? Delay 5)])",
        None,
    );
    macro_errors_match_v8(
        r#"(throw (ExceptionInfo. "bad arg" {:a 1} nil))"#,
        Some(("Error: bad arg", "HostEffect")),
    );
}
/// ExceptionInfo called with a realm row as its receiver (light-cljs-875):
/// the V8 build fills ExceptionInfo.prototype in, while the Goose path's
/// write rule (decision 3) faults NativeBoundary, which JavaScript cannot
/// catch, before any field changes.
#[cfg(light_v8)]
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_exception_info_call_cannot_write_realm_rows() {
    let call = r#"(defmacro m [] (do (.call ExceptionInfo (.-prototype ExceptionInfo) "q" 1 2) (js* "Object.hasOwn(~{}, 'data')" (.-prototype ExceptionInfo)))) (def x (m))"#;
    let reference = crate::compile_with_macros(call, &mut crate::MacroHost::new()).unwrap();
    assert!(reference.contains("true"), "{reference}");
    let session = GooseSession::start(&runner(), 1).unwrap();
    let mut host = crate::MacroHost::new();
    host.use_goose_source_macros(session.clone()).unwrap();
    let message = crate::compile_with_macros(call, &mut host)
        .unwrap_err()
        .to_string();
    assert!(message.contains("Fault.NativeBoundary"), "{message}");
    session.close();
    drop(host);
    cleaned(&session);
}
/// cljs.core's identical? (light-cljs-876): `val(a) === val(b)`, with the
/// V8 build's interned keywords (literals and a macro argument's).
#[cfg(light_v8)]
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_macro_identical_matches_v8() {
    macro_errors_match_v8(
        r#"[(identical? 1 1) (identical? "a" "a") (identical? "a" (str "a")) (identical? 1 1.0) (identical? ##NaN ##NaN) (identical? nil nil) (identical? true true) (identical? 0 (js* "-0")) (identical? 'a 'a) (identical? :a :a) (identical? :a/b :a/b) (identical? :a :b) (identical? :a :b/a) (identical? [1] [1]) (let [v [1]] (identical? v v)) (let [a (atom 1)] (identical? a a))]"#,
        None,
    );
    source_matches_v8(
        "(defmacro m [k j] [(identical? k :a) (identical? k j) (identical? (first [k]) j) (identical? (get {k 1} :a) 1) (identical? k 'a)]) (def x (m :a :a))",
        None,
    );
    // The leaves' names and lengths, and which of them is a constructor:
    // identical is an arrow function in the V8 build.
    let construct = r#"(js* "(() => { try { Reflect.construct(Object, [], ~{}); return 'ok'; } catch (e) { return e.name; } })()" "#;
    macro_errors_match_v8(
        &format!(
            "(pr-str [(.-name str) (.-length str) (.-name pr-str) (.-length pr-str) (.-name identical?) (.-length identical?) {construct}identical?) {construct}pr-str) {construct}str)])"
        ),
        None,
    );
}
/// Keyword literals and empty collection literals (light-cljs-878): as in
/// the V8 build, a literal keyword is one object per namespace and name
/// (a literal, a helper's, a published definition's and a macro
/// argument's alike), and an empty literal is its kind's one empty
/// collection, while an imported `[]` is its own.
#[cfg(light_v8)]
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_literal_identity_matches_v8() {
    macro_errors_match_v8(
        r#"[(js* "~{} === ~{}" :a :a) (js* "~{} === ~{}" :n/a :n/a) (js* "~{} === ~{}" :a :n/a) (js* "~{} === ~{}" :a :b) (js* "Object.is(~{}, ~{})" :a :a) (js* "~{} !== ~{}" :a :a) (js* "~{} == ~{}" :a :a) (let [f (fn [] :a)] (identical? (f) (f))) (identical? :a :a) (identical? [] []) (identical? {} {}) (identical? #{} #{}) (identical? '() '()) (js* "~{} === ~{}" [] []) (let [f (fn [] [])] (identical? (f) (f))) (let [f (fn [] {})] (identical? (f) (f))) (identical? [] '()) (identical? [] {}) (identical? [] [1])]"#,
        None,
    );
    source_matches_v8(
        r#"(defn h [] :a) (defn e [] []) (def k :a) (def v []) (defmacro m [x y] [(identical? (h) :a) (js* "~{} === ~{}" (h) :a) (identical? k :a) (identical? (h) k) (identical? x :a) (js* "~{} === ~{}" x :a) (identical? x (first [x])) (identical? (e) []) (identical? v []) (identical? y []) (js* "~{} === ~{}" y []) (identical? y y)]) (def x (m :a []))"#,
        None,
    );
    // Inside a published definition's value, in a cell value, and in a
    // macro's result, where the shared objects are aliases.
    source_matches_v8(
        "(def v [[] {} #{} '() {:a []}]) (defmacro m [] [(identical? (first v) []) (identical? (nth v 1) {}) (identical? (nth v 2) #{}) (identical? (nth v 3) '()) (identical? (:a (nth v 4)) []) v]) (def x (m))",
        None,
    );
    source_matches_v8(
        "(def a (atom [])) (def b (atom :q)) (defmacro m [y] (do (swap! b (fn [_] y)) [(identical? @a []) (identical? @b :q) (identical? @b y) (compare-and-set! a [] 1) @a (do (reset! a {}) (identical? @a {})) (compare-and-set! a {} 2) @a])) (def x (m []))",
        None,
    );
    macro_errors_match_v8(
        "[[] [] {} {} [[] []] '([] []) {[] 1} #{[]} (let [a [] b []] (list 'do a b {}))]",
        None,
    );
    // An empty literal in a publication and in its cell's initial value is
    // the realm's one object in both (the cell's identity for it is the
    // realm's, not the publication place's).
    source_matches_v8(
        "(def v [[] {} #{} '() (atom [[] {} #{} '()]) (atom [])]) (defmacro m [] (let [c @(nth v 4)] [(identical? (first v) (first c)) (identical? (nth v 1) (nth c 1)) (identical? (nth v 2) (nth c 2)) (identical? (nth v 3) (nth c 3)) (identical? (first c) []) (identical? @(nth v 5) []) (identical? (first v) @(nth v 5))])) (def x (m))",
        None,
    );
    // A keyword in a cell is the same keyword wherever it was made: a swap!
    // whose function resets the atom to the keyword it read commits, and a
    // compare-and-set! expecting a keyword literal matches.
    source_matches_v8(
        "(def a (atom :a)) (def b (atom :n/a)) (def c (atom [:k])) (defmacro m [] [(swap! a (fn [_] (reset! a :a) :b)) @a (compare-and-set! b :n/a 1) (compare-and-set! b 1 :n/b) (swap! b (fn [x] (reset! b :n/b) [x])) (identical? (first @b) :n/b) (identical? (first @c) :k)]) (def x (m))",
        None,
    );
}
/// cljs.core's str and pr-str (light-cljs-876): scalars, collections, a
/// macro's arguments, atoms, volatiles, delays, errors and functions, and
/// both as callbacks.
#[cfg(light_v8)]
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_macro_str_and_pr_str_match_v8() {
    for body in [
        r#"(str nil 1.5 -0 true :k :n/k 'a 'n/b "é" 123456789012 1.5e-7 -1.25 ##NaN ##Inf ##-Inf (js* "-0") false)"#,
        r#"[(str) (str "a" (js* "undefined")) (str "x") (str 1) (str :a.b/c)]"#,
        r#"(str [1 "a" :k] '(1 2) #{3} {:a "b"} (list) [] #{} {} [nil "q\"" 'x])"#,
        r#"(pr-str 1 "a\n\"b\\\t\r" nil true :k 'a/b ##NaN ##Inf ##-Inf 1e21 0.1 (js* "-0") (js* "undefined") "é ")"#,
        "(pr-str [1 [2] {:a #{:b}} '(x y)] (concat [1] [2]) (seq [1 2]) (rest [1 2]) (into {} [[:a 1] [:b 2]]) (assoc {:a 1} :b 2) (conj #{1} 2) (mapv inc [1 2]) (list 1 2) [] {} #{} (list))",
        "[(pr-str (atom {:a 1})) (str (atom 1)) (pr-str (volatile! [1])) (str (volatile! 1)) (pr-str (delay 1)) (let [d (delay 2)] @d (pr-str d)) (str (delay 1)) (pr-str [(atom (atom 1))])]",
        r#"[(pr-str (js/Error. "c")) (pr-str (js/TypeError. "c")) (str (js/Error. "c")) (str (js/RangeError.)) (pr-str (js/Error.)) (pr-str [(js/Error. "v")])]"#,
        r#"[(pr-str (js* "(() => { const e = new Error('m'); e.constructor = null; return e; })()")) (pr-str (js* "(() => { const e = new Error('m'); e.name = 'Custom'; return e; })()"))]"#,
        r#"[(pr-str (fn foo [])) (str (js* "{}")) (str (js/Error. "x") (js* "{}"))]"#,
        r#"[(mapv str [1 :a "b" nil]) (mapv pr-str [1 :a "b" nil])]"#,
        r#"(str (js/Number. 1.5) (js/String. "s") (js/Boolean. true) (js/Boolean. false) js/Symbol.iterator)"#,
        "(pr-str str)",
    ] {
        macro_errors_match_v8(body, None);
    }
    source_matches_v8(
        "(defmacro m [k] [(str k) (pr-str k)]) (def x (m [:a \"b\" {:c #{1}} (d e/f) ^:foo [1] 1.5 nil true ns/sym]))",
        None,
    );
    // An atom holding itself overflows the printer's stack in both.
    macro_errors_match_v8(
        "(let [a (atom nil)] (reset! a a) (pr-str a))",
        Some(("RangeError", "StackLimit")),
    );
}
/// ExceptionInfo printed by its prototype's toString (light-cljs-876),
/// which 875 left pending: `#error {:message ..., :data ..., :cause ...}`
/// through str, pr-str and `.toString`, of any receiver.
#[cfg(light_v8)]
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_macro_prints_exception_info_as_v8() {
    for body in [
        r#"[(pr-str (ex-info "x" {:a 1})) (str (ex-info "x" {:a 1})) (pr-str (ex-info "x" nil)) (pr-str (ex-info nil {})) (pr-str (ex-info 5 {} nil)) (pr-str [(ex-info "in" {:k 1})]) (str "e: " (ex-info "x" {:a 1}) "!")]"#,
        r#"(.toString (ex-info "x" {:a 1} (js/Error. "c")))"#,
        // Data and cause print when cljs.core's truth holds: 0, "" and NaN.
        r#"(pr-str [(str (ex-info "x" 0 "")) (pr-str (ex-info "x" ##NaN false)) (pr-str (ex-info "x" false nil)) (.toString (ex-info "x" "" 0))])"#,
        r#"(pr-str (ExceptionInfo. "m" {:k [1 "s"]} (ex-info "inner" {} (js/TypeError. "t"))))"#,
        r#"(let [o (js* "{message: 'm', data: 1}")] (.call (.-toString (.-prototype ExceptionInfo)) o))"#,
        r#"[(.-length (.-toString (.-prototype ExceptionInfo))) (.-name (.-toString (.-prototype ExceptionInfo))) (js* "typeof ~{}" (.-toString (.-prototype ExceptionInfo))) (js* "(() => { const r = []; for (const k in ~{}) r.push(k); return r.join(); })()" (.-prototype ExceptionInfo))]"#,
    ] {
        macro_errors_match_v8(body, None);
    }
    // A strict `this` of null is the TypeError of reading its message.
    macro_errors_match_v8(
        r#"(js* "~{}.call(null)" (.-toString (.-prototype ExceptionInfo)))"#,
        Some(("TypeError", "TypeError")),
    );
}
/// Maps and sets print in the V8 build's traversal order (light-cljs-877):
/// literal, reader and array maps and sets in insertion order at any size;
/// a map `__rt.formMap` made (a literal of more than 8 entries, or of more
/// than 8 key positions whose keys are equal at run time) and a set of more
/// than 8 members, or one an assoc or conj took past 8 entries, in
/// ClojureScript's hash order, kept through updates, metadata, cell values
/// and published definitions.
#[cfg(light_v8)]
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_macro_prints_hash_ordered_collections_as_v8() {
    for body in [
        "[(pr-str {:a 1 :b 2 :c 3 :d 4 :e 5 :f 6 :g 7 :h 8 :i 9}) (str {:a 1 :b 2 :c 3 :d 4 :e 5 :f 6 :g 7 :h 8 :i 9})]",
        "[(pr-str #{1 2 3 4 5 6 7 8 9 10}) (str #{1 2 3 4 5 6 7 8 9 10}) (pr-str #{:a :b :c :d :e :f :g :h :i}) (str #{:a :b :c :d :e :f :g :h :i})]",
        "[(pr-str (into #{} (range 12))) (str (into #{} (range 12)))]",
        // Every hashed key kind: nil first, strings, symbols, namespaced
        // keywords, integers, negative and fractional numbers, booleans,
        // vectors, lists, maps and sets.
        r#"(pr-str {nil 1 "s" 2 'sym 3 :ns/kw 4 1.5 5 -3 6 true 7 [1 2] 8 {:x 1} 9 #{2} 10 '(a) 11 false 12 "é" 13 'n/s 14 2147483648 15 -0.25 16 1e300 17})"#,
        "(pr-str #{-1 -2 0.5 2147483647 -2147483648 9007199254740991 1e21 ##Inf ##-Inf 7})",
        // Promotion past 8 entries; replacing a key does not promote.
        "[(pr-str (assoc {:a 1 :b 2 :c 3 :d 4 :e 5 :f 6 :g 7 :h 8} :i 9)) (pr-str (assoc {:a 1 :b 2 :c 3 :d 4 :e 5 :f 6 :g 7 :h 8} :a 0)) (pr-str (conj #{1 2 3 4 5 6 7 8} 9)) (pr-str (conj #{1 2 3 4 5 6 7 8} 8)) (pr-str (into {} [[:z 1] [:y 2] [:x 3] [:w 4] [:v 5] [:u 6] [:t 7] [:s 8] [:r 9] [:q 10]]))]",
        // A hashed map or set stays hashed through updates and nesting.
        "[(pr-str (assoc {:a 1 :b 2 :c 3 :d 4 :e 5 :f 6 :g 7 :h 8 :i 9} :j 10)) (pr-str (conj (conj #{1 2 3 4 5 6 7 8} 9) 10))]",
        "(pr-str [{:a #{1 2 3 4 5 6 7 8 9}} #{{:k 1 :l 2 :m 3 :n 4 :o 5 :p 6 :q 7 :r 8 :s 9}}])",
        // A literal of more than 8 key positions whose keys are equal at run
        // time is a small hash-ordered map.
        r#"(let [a "x" b "x" c "x" d "x" e "x" f "x" g "x"] (pr-str {a 1 b 2 c 3 d 4 e 5 f 6 g 7 "b" 8 "a" 9}))"#,
        "(let [a 1 b 1 c 1 d 1 e 1 f 1 g 1] [(pr-str {a 1 b 2 c 3 d 4 e 5 f 6 g 7 :k 8 :j 9}) (pr-str (assoc {a 1 b 2 c 3 d 4 e 5 f 6 g 7 :k 8 :j 9} :c 3))])",
    ] {
        macro_errors_match_v8(body, None);
    }
    // A macro's argument map or set, as the V8 build's form bridge
    // decodes it (mapFromArray and setFromArray).
    for argument in [
        "{:i 1 :h 2 :g 3 :f 4 :e 5 :d 6 :c 7 :b 8 :a 9}",
        "{:h 2 :g 3 :f 4 :e 5 :d 6 :c 7 :b 8 :a 9}",
        "#{9 8 7 6 5 4 3 2 1 0}",
        "#{8 7 6 5 4 3 2 1}",
        "[{:i 1 :h 2 :g 3 :f 4 :e 5 :d 6 :c 7 :b 8 :a 9 \"j\" #{9 8 7 6 5 4 3 2 1 0}}]",
    ] {
        source_matches_v8(
            &format!("(defmacro m [k] [(str k) (pr-str k)]) (def x (m {argument}))"),
            None,
        );
    }
    // A macro's hash-ordered result goes to the compiler in its traversal
    // order, as the V8 build's form bridge encodes it.
    for result in [
        "{:a 1 :b 2 :c 3 :d 4 :e 5 :f 6 :g 7 :h 8 :i 9}",
        "#{1 2 3 4 5 6 7 8 9 10}",
        "(into #{} (range 12))",
        "[(assoc {:a 1 :b 2 :c 3 :d 4 :e 5 :f 6 :g 7 :h 8} :i 9) {:k #{:a :b :c :d :e :f :g :h :i}}]",
        "(let [a 1 b 1 c 1 d 1 e 1 f 1 g 1] {a 1 b 2 c 3 d 4 e 5 f 6 g 7 :k 8 :j 9})",
        "'{:a 1 :b 2 :c 3 :d 4 :e 5 :f 6 :g 7 :h 8 :i 9}",
    ] {
        macro_errors_match_v8(result, None);
    }
    // Promotion rebuilds the entries: reader-distinct keys equal once read
    // keep the first key and the last value.
    macro_errors_match_v8(
        "(pr-str (assoc {1 :first 1.0 :last 2 :two 3 :three 4 :four 5 :five 6 :six 7 :seven} 8 :eight))",
        None,
    );
    source_matches_v8(
        "(defmacro m [k] k) (def x (m {:i 1 :h 2 :g 3 :f 4 :e 5 :d 6 :c 7 :b 8 :a 9})) (def y (m #{9 8 7 6 5 4 3 2 1 0}))",
        None,
    );
    // A hash-ordered value through a macro namespace's atom and definition.
    source_matches_v8(
        "(def reg (atom nil)) (def table #{:k :j :i :h :g :f :e :d :c}) (defmacro put [] (reset! reg {:a 1 :b 2 :c 3 :d 4 :e 5 :f 6 :g 7 :h 8 :i 9}) nil) (defmacro show [] [(pr-str @reg) (pr-str table) (pr-str (assoc @reg :z 0))]) (put) (def x (show))",
        None,
    );
}
/// A macro argument's map or set with keys or members equal at run time
/// but not to the reader (1 and 1.0) merges as the V8 build's form bridge
/// decodes it (light-cljs-879): the first key with the last value, the
/// first member; a set is hash-ordered past 8 distinct members, a map past
/// 16 operands. Returned unchanged, it is the reader's form again.
#[cfg(light_v8)]
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_merges_equal_keys_of_macro_arguments_as_v8() {
    for argument in [
        "#{1 1.0 2 3 4 5 6 7}",
        "#{1 1.0 2 3 4 5 6 7 8}",
        "#{1 1.0 2 3 4 5 6 7 8 9}",
        "#{9 8 7 6 5 4 3 2 1 1.0 0}",
        "#{1 1.0}",
        "{1 :first 1.0 :last 2 :two}",
        "{1 :a 1.0 :b 2 :c 3 :d 4 :e 5 :f 6 :g 7 :h}",
        "{1 :a 1.0 :b 2 :c 3 :d 4 :e 5 :f 6 :g 7 :h 8 :i}",
        "{1 :a 2 :b 1.0 :c 3 :d 4 :e 5 :f 6 :g 7 :h 8 :i 9 :j}",
        "#{[1] [1.0] #{2 2.0} #{2}}",
        "[{1 #{1 1.0}} ^{:m {1 1 1.0 2}} [3]]",
    ] {
        source_matches_v8(
            &format!("(defmacro m [k] [(str k) (pr-str k) (count k)]) (def x (m {argument}))"),
            None,
        );
        source_matches_v8(&format!("(defmacro m [k] k) (def x (m {argument}))"), None);
    }
    for (argument, operation) in [
        ("#{1 1.0 2 3 4 5 6 7}", "(conj k 8)"),
        ("#{1 1.0 2 3 4 5 6 7}", "(conj (conj k 8) 9)"),
        ("#{1 1.0 2 3 4 5 6 7 8}", "(conj k 9)"),
        ("#{1 1.0 2 3 4 5 6 7 8}", "(conj k 1.0)"),
        (
            "{1 :a 1.0 :b 2 :c 3 :d 4 :e 5 :f 6 :g 7 :h}",
            "(assoc k 8 :i)",
        ),
        (
            "{1 :a 1.0 :b 2 :c 3 :d 4 :e 5 :f 6 :g 7 :h}",
            "(assoc (assoc k 8 :i) 9 :j)",
        ),
        (
            "{1 :a 1.0 :b 2 :c 3 :d 4 :e 5 :f 6 :g 7 :h 8 :i}",
            "(assoc k 9 :j)",
        ),
        ("{1 :a 1.0 :b 2 :c}", "(assoc k 1 :z)"),
        ("{1 :a 1.0 :b 2 :c}", "(get k 1)"),
    ] {
        source_matches_v8(
            &format!(
                "(defmacro m [k] (let [r {operation}] [(str r) (pr-str r)])) (def x (m {argument}))"
            ),
            None,
        );
        source_matches_v8(
            &format!("(defmacro m [k] {operation}) (def x (m {argument}))"),
            None,
        );
    }
    // Large arguments stay within the budgets: 512 keys each merging with
    // an equal double, returned unchanged, and 5,000 distinct one-element
    // vectors.
    let pairs: String = (1..=512)
        .map(|i| format!("{i} :first{i} "))
        .chain((1..=512).map(|i| format!("{i}.0 :last{i} ")))
        .collect();
    source_matches_v8(&format!("(defmacro m [k] k) (def x (m {{{pairs}}}))"), None);
    source_matches_v8(
        &format!("(defmacro m [k] [(count k) (get k 7)]) (def x (m {{{pairs}}}))"),
        None,
    );
    let vectors: String = (0..5000).map(|i| format!("[{i}] ")).collect();
    source_matches_v8(
        &format!("(defmacro m [k] (count k)) (def x (m #{{{vectors}}}))"),
        None,
    );
}
/// cljs.core's range (light-cljs-877), realized: empty and one-element
/// boundaries, fractional and negative steps, NaN bounds, and an integer
/// range's indexing, which floors a fractional index, after rest too.
#[cfg(light_v8)]
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_macro_range_matches_v8() {
    for body in [
        "(pr-str [(range 5) (range 2 5) (range 0 10 3) (range 5 0 -2) (range 0 1 0.25) (range 5 1) (range 1 1 0) (range -3) (range 0 -3 -1)])",
        "(pr-str [(range ##NaN) (range ##NaN 3) (range 0 ##NaN) (range 0.5 3) (count (range 0 10 3)) (first (range 3)) (rest (range 3)) (seq (range 0))])",
        "[(nth (range 3) 0.5 :missing) (nth (range 3) 1.5) (nth (rest (range 4)) 0.5) (nth (range 0 1 0.25) 1) (nth (range 3) 2)]",
        "(js* \"(() => { try { return ~{}; } catch (e) { return e.message; } })()\" (nth (range 0 1 0.25) 0.5))",
    ] {
        macro_errors_match_v8(body, None);
    }
    // A small map __rt.formMap makes is hash-ordered (NewFormMap); the V8
    // build's macros have no __rt.
    let source = r#"(defmacro m [] [(pr-str (js* "__rt.formMap(2, 'b', 1, 'a')")) (pr-str (js* "__rt.formMap(32, 0, 1, 0)"))]) (def x (m))"#;
    let session = GooseSession::start(&runner(), 1).unwrap();
    let mut host = crate::MacroHost::new();
    host.use_goose_source_macros(session.clone()).unwrap();
    let js = crate::compile_with_macros(source, &mut host).unwrap();
    assert!(
        js.contains(r#"{1 \"a\", 2 \"b\"}"#) && js.contains("{32 0, 1 0}"),
        "{js}"
    );
    session.close();
    drop(host);
    cleaned(&session);
}
/// A hash-ordered map or set keeps its order in a process-wide cell's value
/// and a published definition (light-cljs-877): one expansion stores a
/// hash-ordered map in the macro namespace's atom and the next prints it
/// and the published set as it prints fresh ones, at every slot count.
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_cells_keep_hash_order() {
    let mut files = HashMap::new();
    files.insert(
        "m.state".to_owned(),
        "(ns m.state) (def reg (atom nil)) (def table #{:k :j :i :h :g :f :e :d :c}) \
         (defmacro put [] (reset! reg {:a 1 :b 2 :c 3 :d 4 :e 5 :f 6 :g 7 :h 8 :i 9}) nil) \
         (defmacro show [] (let [v @reg] [(= (pr-str v) (pr-str {:a 1 :b 2 :c 3 :d 4 :e 5 :f 6 :g 7 :h 8 :i 9})) \
           (= (pr-str table) (pr-str #{:k :j :i :h :g :f :e :d :c})) \
           (= (pr-str v) \"{:a 1, :b 2, :c 3, :d 4, :e 5, :f 6, :g 7, :h 8, :i 9}\")]))"
            .to_owned(),
    );
    files.insert(
        "app.main".to_owned(),
        "(ns app.main (:require [m.state :refer [put show]])) (put) (js/console.log (show))"
            .to_owned(),
    );
    for slots in [1, 2, 4] {
        let session = GooseSession::start(&runner(), slots).unwrap();
        let (output, _, _) = pooled_build(&files, slots, Some(&session), Default::default());
        let js = output.unwrap();
        let logged = js
            .split("[\"log\"])(")
            .nth(1)
            .unwrap_or_else(|| panic!("{js}"));
        assert!(
            logged.contains("[true, true, false]"),
            "slots={slots}: {logged}"
        );
        session.close();
        cleaned(&session);
    }
}
/// What the Goose printer does not do fails by name (light-cljs-876): a
/// toString that is JavaScript code, a JavaScript object or array, and a
/// hash-ordered map or set holding an object the V8 build hashes by its
/// identity (light-cljs-877) are UnsupportedOperand faults naming the
/// built-in, never a NativeBoundary or a different text.
#[cfg(light_v8)]
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_printer_fails_by_name() {
    for (source, detail) in [
        (
            r#"(defmacro m [] (str (js* "{toString() { return 'ts'; }}"))) (def x (m))"#,
            "UnsupportedOperand:str:",
        ),
        (
            r#"(defmacro m [] (str (js* "[1, 2]"))) (def x (m))"#,
            "UnsupportedOperand:str:",
        ),
        (
            r#"(defmacro m [] (pr-str (js* "{}"))) (def x (m))"#,
            "UnsupportedOperand:prStr:",
        ),
        // A hash-ordered set or map whose member or key the V8 build
        // hashes by its identity (light-cljs-877).
        (
            "(defmacro m [] (pr-str (conj #{1 2 3 4 5 6 7 8} (atom 1)))) (def x (m))",
            "UnsupportedOperand:prStr:-the-hash-order-of-a-set-holding-a-JavaScript-object",
        ),
        (
            "(defmacro m [] (str (assoc {:a 1 :b 2 :c 3 :d 4 :e 5 :f 6 :g 7 :h 8} (fn []) 9))) (def x (m))",
            "UnsupportedOperand:str:-the-hash-order-of-a-map-keyed-by-a-JavaScript-object",
        ),
        // The V8 build gives a runtime function's JavaScript source.
        (
            "(defmacro m [] (str str)) (def x (m))",
            "UnsupportedOperand:str:-the-source-text-of-a-light-cljs-runtime-function",
        ),
    ] {
        crate::compile_with_macros(source, &mut crate::MacroHost::new()).unwrap();
        let session = GooseSession::start(&runner(), 1).unwrap();
        let mut host = crate::MacroHost::new();
        host.use_goose_source_macros(session.clone()).unwrap();
        let message = crate::compile_with_macros(source, &mut host)
            .unwrap_err()
            .to_string();
        assert!(message.contains(detail), "{source}: {message}");
        session.close();
        drop(host);
        cleaned(&session);
    }
}
/// Symbol values (light-cljs-885): inside one job, a macro's own symbols,
/// registered ones included, behave as the V8 build's, and `str` prints one
/// as its descriptive string.
#[cfg(light_v8)]
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_macro_symbols_match_v8() {
    macro_errors_match_v8(
        r#"(let [s (js/Symbol "d") o (js* "{}")] (aset o s 1) [(str s) (str (js/Symbol)) (js* "typeof ~{}" s) (.-description s) (aget o s) (js* "~{} === ~{}" s s) (js* "~{} === Symbol('d')" s) (identical? (js/Symbol.for "k") (js/Symbol.for "k")) (js/Symbol.keyFor (js/Symbol.for "k")) (js* "Symbol.keyFor(~{}) === undefined" s) (js* "Object(~{}) instanceof Symbol" s) (str (js/Symbol.for "k") js/Symbol.iterator)])"#,
        None,
    );
    // pr-str, and str of a collection, print a symbol as the V8 build's
    // runtime does: #object[Symbol(description)].
    macro_errors_match_v8(
        r#"(let [s (js/Symbol "d")] [(pr-str s) (pr-str [s (js/Symbol) (js/Symbol.for "é")]) (str [s]) (pr-str js/Symbol.iterator) (pr-str {:k s})])"#,
        None,
    );
}
/// A symbol a job made never leaves it (light-cljs-885; cells.goose
/// publish_cells): a definition that holds one, unique or registered,
/// directly or in a collection, fails by name when a macro reads it, as a
/// cell value holding one does; a macro's own symbols stay usable.
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_job_symbols_are_not_shareable() {
    for (source, detail) in [
        (
            r#"(def r (js/Symbol.for "k")) (defmacro m [] (str (identical? r (js/Symbol.for "k")))) (def x (m))"#,
            "symbol-unshareable",
        ),
        (
            r#"(def u (js/Symbol "u")) (defmacro m [] (str u)) (def x (m))"#,
            "symbol-unshareable",
        ),
        (
            r#"(def v [1 (js/Symbol "u") (js/Symbol.for "k")]) (defmacro m [] (count v)) (def x (m))"#,
            "symbol-unshareable",
        ),
        (
            r#"(def a (atom 0)) (defmacro m [] (do (reset! a (js/Symbol.for "k")) 1)) (def x (m))"#,
            "cannot-share-a-symbol",
        ),
    ] {
        let session = GooseSession::start(&runner(), 1).unwrap();
        let mut host = crate::MacroHost::new();
        host.use_goose_source_macros(session.clone()).unwrap();
        let message = crate::compile_with_macros(source, &mut host)
            .unwrap_err()
            .to_string();
        assert!(message.contains(detail), "{source}: {message}");
        session.close();
        drop(host);
        cleaned(&session);
    }
}
/// The printer's output is bounded by the job's unit limit before its
/// buffer grows (light-cljs-876): a string of 2^18 units shared ten
/// thousand times would print about 2^31 units (4 GB of buffer), but the
/// job stops with MemoryLimit after a few copies.
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_printer_output_is_bounded() {
    let source = r#"(defmacro m [] (let [s (js* "(() => { let s = 'x'; for (let i = 0; i < 18; i++) s = s + s; return s; })()") v [s s s s s s s s s s] w [v v v v v v v v v v] y [w w w w w w w w w w]] (pr-str [y y y y y y y y y y]))) (def x (m))"#;
    let session = GooseSession::start(&runner(), 1).unwrap();
    let mut host = crate::MacroHost::new();
    host.use_goose_source_macros(session.clone()).unwrap();
    let message = crate::compile_with_macros(source, &mut host)
        .unwrap_err()
        .to_string();
    assert!(message.contains("Fault.MemoryLimit"), "{message}");
    session.close();
    drop(host);
    cleaned(&session);
}
/// get-in, whose `get` of a number is nil, inside a `js*` try (light-cljs-872).
#[cfg(light_v8)]
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_macro_get_in_matches_v8() {
    macro_errors_match_v8(
        r#"(js* "(() => { try { return ~{}; } catch (e) { return e.name + \":\" + e.message; } })()" (get-in 5 [:a]))"#,
        None,
    );
    macro_errors_match_v8(
        "[(get-in {:a {:b 2}} [:a :b]) (get-in {:a 1} [:z] 9) (get-in {:a nil} [:a] 9) (get-in [[1 2]] [0 1]) (get-in #{:k} [:k]) (get :a 1)]",
        None,
    );
}
/// The runtime's own Errors (light-cljs-872): a wrong arity of ex-info or
/// get-in, caught as an Error with the V8 build's message, or uncaught.
#[cfg(light_v8)]
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_macro_runtime_errors_match_v8() {
    for call in [r#"(ex-info "x")"#, "(get-in 1 2 3 4 5 6 7 8 9 10 11 12)"] {
        macro_errors_match_v8(
            &format!("(try {call} (catch js/Error e [(.-name e) (ex-message e)]))"),
            None,
        );
    }
    macro_errors_match_v8(r#"(ex-info "x")"#, Some(("Invalid arity: 1", "HostEffect")));
}
/// Compile `(defmacro m [] body) (def x (m))`, whose macro throws, with the
/// V8 build and on the Goose path (light-cljs-874): both diagnostics place
/// the description where the V8 build does, after the same location and
/// macro name; the V8 build's is `v8`, the Goose path's `goose` followed by
/// its fault details.
#[cfg(light_v8)]
fn uncaught_matches_v8(body: &str, v8: &str, goose: &str) {
    let source = format!("(defmacro m [] {body}) (def x (m))");
    let reference = crate::compile_with_macros(&source, &mut crate::MacroHost::new())
        .unwrap_err()
        .to_string();
    let session = GooseSession::start(&runner(), 1).unwrap();
    let mut host = crate::MacroHost::new();
    host.use_goose_source_macros(session.clone()).unwrap();
    let message = crate::compile_with_macros(&source, &mut host)
        .unwrap_err()
        .to_string();
    session.close();
    drop(host);
    cleaned(&session);
    let v8_at = reference
        .find(&format!("/m: {v8}; definition "))
        .unwrap_or_else(|| panic!("{body}: {reference}"));
    let goose_at = message
        .find(&format!("/m: {goose}; Goose: ERROR "))
        .unwrap_or_else(|| panic!("{body}: {message}"));
    assert_eq!(reference[..v8_at], message[..goose_at], "{body}");
    let details = &message[goose_at..];
    assert!(
        details.contains(" function=")
            && details.contains(" emitted-span=")
            && details.contains("; definition "),
        "{body}: {message}"
    );
}
/// The pellet's macros (light-cljs-874): a thrown primitive, an Error that
/// JavaScript or cljs.core makes, an ExceptionInfo, and an engine-raised
/// TypeError (the same name, Goose's message, light-cljs-871).
#[cfg(light_v8)]
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_uncaught_macro_exceptions_are_described_as_v8() {
    uncaught_matches_v8("(throw 42)", "42", "42");
    for (body, text) in [
        (
            r#"(js* "(() => { throw new Error(\"j\"); })()")"#,
            "Error: j",
        ),
        (r#"(throw (js/Error. "stop here"))"#, "Error: stop here"),
        (r#"(throw (ex-info "bad arg" {:a 1}))"#, "Error: bad arg"),
        ("(nth [1] 10)", "Error: Index out of bounds"),
        (r#"(throw "plain")"#, "plain"),
        ("(throw nil)", "null"),
    ] {
        uncaught_matches_v8(body, text, text);
    }
    // A computed read names no key in Goose's (QuickJS's) wording.
    uncaught_matches_v8(
        r#"(let [x (identity nil)] (aget x "foo"))"#,
        "TypeError: Cannot read properties of null (reading 'foo')",
        "TypeError: cannot read property of null",
    );
}
/// The description rules (light-cljs-874; the Pants engine's exceptions.rs
/// uncaught_error_objects_report_name_and_message and
/// unhandled_failures_report_the_source_position_and_call_chain): an error
/// object as Error.prototype.toString prints it from data properties, one
/// whose name or message would run JavaScript or is an object as "an
/// object", a primitive as JavaScript prints it, any other object "an
/// object"; a fault's error keeps its kind and message when rethrown after
/// they changed; the text is one line of UTF-8, bounded.
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_uncaught_exceptions_are_described() {
    let long = "x".repeat(3000);
    let long_body = format!("throw '{long}';");
    let long_text = format!("{}...", "x".repeat(1024));
    // A 131,072-unit key: the message passes the runner's 65,536-byte line.
    let long_key_body = "let k = 'k'; for (let i = 0; i < 17; i++) k += k; try { null[k] = 1; } catch (e) { throw e; }";
    let long_key_text = format!(
        "{}...",
        &format!("TypeError: cannot set property '{}", "k".repeat(1024))[..1024]
    );
    let cases: Vec<(&str, &str)> = vec![
        ("throw Error('boom');", "Error: boom"),
        (
            "let e = Error('boom'); e.name = 'Custom'; throw e;",
            "Custom: boom",
        ),
        ("let e = Error('boom'); e.name = ''; throw e;", "boom"),
        ("throw Error();", "Error"),
        ("throw new TypeError('t');", "TypeError: t"),
        ("let e = Error('boom'); e.message = 5; throw e;", "Error: 5"),
        (
            // Object.defineProperty of an accessor is not implemented on
            // Goose yet; an inheriting literal's getter is the same case.
            "throw {__proto__: Error.prototype, message: 'boom', get name() { throw 99; }};",
            "an object",
        ),
        (
            "let e = Error('boom'); e.message = {toString() { throw 99; }}; throw e;",
            "an object",
        ),
        (
            "throw {__proto__: TypeError.prototype, message: 'inherits'};",
            "TypeError: inherits",
        ),
        ("throw undefined;", "undefined"),
        ("throw null;", "null"),
        ("throw true;", "true"),
        ("throw -1.5;", "-1.5"),
        ("throw 1 / 0;", "Infinity"),
        ("throw 'a\\nb\\u0001';", "a\\nb\\u0001"),
        (
            "throw '\\u00e9\\ud83d\\ude00\\ud800';",
            "\u{e9}\u{1f600}\u{fffd}",
        ),
        ("throw Symbol.iterator;", "Symbol(Symbol.iterator)"),
        ("throw {};", "an object"),
        ("throw __rt.formVector(1);", "an object"),
        (
            "return null.foo;",
            "TypeError: cannot read property 'foo' of null",
        ),
        (
            "try { null.foo; } catch (e) { e.message = 'x'; e.name = 'Custom'; throw e; }",
            "TypeError: cannot read property 'foo' of null",
        ),
        (
            "try { x; let x = 1; } finally { }",
            "ReferenceError: cannot access 'x' before initialization",
        ),
        (
            "return __rt.cljNth(__rt.formVector(1), 10);",
            "Error: Index out of bounds",
        ),
        (&long_body, &long_text),
        // A rethrown fault's message stays on one line and bounded, in
        // the fault details too.
        (
            "try { null['a\\nb'] = 1; } catch (e) { throw e; }",
            "TypeError: cannot set property 'a\\nb' of null",
        ),
        (&long_key_body, &long_key_text),
        // Spaces ending the description are the thrown value's.
        ("throw 'stop ';", "stop "),
        ("throw 'nb\\u00a0';", "nb\u{a0}"),
        ("throw ' ';", " "),
    ];
    // A failed call ends its session, so each case has its own.
    for (body, text) in cases {
        let s = GooseSession::start(&runner(), 1).unwrap();
        let e = GooseEvaluator::new(s.clone(), &Default::default()).unwrap();
        let (f, args, _) = inputs(&e, &format!("rt => (form,env) => {{ {body} }}"));
        let error = e.call(f, &args).unwrap_err().to_string();
        assert!(
            error.starts_with(&format!("{text}; Goose: ERROR "))
                && error.contains(" emitted-span="),
            "{body}: {error}"
        );
        s.close();
        cleaned(&s);
    }
}
/// Each call of `calls` in a macro's `(try call (catch :default e ...))`,
/// giving the caught error's name and message (or the call's value), on
/// the Goose path as on the V8 build (light-cljs-873).
#[cfg(light_v8)]
fn core_errors_match_v8(calls: &[&str]) {
    let caught: Vec<String> = calls
        .iter()
        .map(|call| format!("(try {call} (catch :default e [(.-name e) (.-message e)]))"))
        .collect();
    macro_errors_match_v8(&format!("[{}]", caught.join(" ")), None);
}
/// The pellet's cases (light-cljs-873): a cljs.core operation's error caught
/// in a `js*` try is the V8 build's Error, and uncaught it fails the macro
/// with that Error, not a TypeError.
#[cfg(light_v8)]
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_macro_core_operations_raise_cljs_core_errors() {
    for call in ["(nth [1] 10)", "(first 5)", "(assoc [] 3 1)"] {
        macro_errors_match_v8(
            &format!(
                r#"(js* "(() => {{ try {{ return ~{{}}; }} catch (e) {{ return e.name + \":\" + e.message; }} }})()" {call})"#
            ),
            None,
        );
    }
    macro_errors_match_v8(
        "(nth [1] 10)",
        Some(("Error: Index out of bounds", "HostEffect")),
    );
    // An ordinary Error instance: Error.prototype, an own non-enumerable
    // message, caught by (catch js/Error e ...).
    macro_errors_match_v8(
        r#"(try (count :k) (catch js/Error e [(instance? js/Error e) (ex-message e) (js* "Object.getPrototypeOf(~{}) === Error.prototype" e) (js* "Object.getOwnPropertyDescriptor(~{}, 'message').enumerable" e)]))"#,
        None,
    );
}
/// nth and a vector's call (light-cljs-873): the index's type, bounds as
/// the runtime finds them (a vector's and a string's index truncated, a
/// list walked) and the collection's type.
#[cfg(light_v8)]
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_macro_nth_errors_match_v8() {
    core_errors_match_v8(&[
        "(nth [1] -1)",
        "(nth [1 2] 1.5)",
        "(nth [1 2] -0.5)",
        r#"(nth [1] "a")"#,
        "(nth [1] nil)",
        "(nth [1] 4294967296)",
        "(nth '(1 2) 5)",
        "(nth '(1 2) 1.5)",
        "(nth (rest [1 2 3]) 5)",
        r#"(nth "ab" 5)"#,
        r#"(nth "ab" 1.5)"#,
        "(nth {:a 1} 0)",
        "(nth #{1} 0)",
        "(nth 5 0)",
        "(nth :k 0)",
        "(nth nil 0)",
        r#"(nth nil "a")"#,
        "(nth [1] 10 :d)",
        "(nth '(1 2) 1.5 :d)",
        "(nth 5 0 :d)",
    ]);
    core_errors_match_v8(&[
        "([1] 5)",
        "([1] -1)",
        "([1] :a)",
        "([1] nil)",
        "([1 2] 1.5)",
        "([1])",
        "([1] 0 1 2)",
    ]);
}
/// count, seq, first, rest and next of a value that is not seqable: its
/// text in the message (light-cljs-873).
#[cfg(light_v8)]
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_macro_seq_and_count_errors_match_v8() {
    core_errors_match_v8(&[
        "(count 5)",
        "(count :k)",
        "(count 'a)",
        "(count true)",
        "(count nil)",
        r#"(count "abc")"#,
        "(seq 5)",
        "(rest true)",
        "(next 'x/a)",
        "(second 5)",
        "(first :a/k)",
        "(first 1.5)",
        "(first 1e21)",
        "(first false)",
        "(first #{})",
        "(first {})",
        r#"(first "ab")"#,
    ]);
}
/// assoc and conj (light-cljs-873): a vector's key and bounds, a type that
/// does not support the operation, assoc of nil, and a map's entries.
#[cfg(light_v8)]
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_macro_assoc_and_conj_errors_match_v8() {
    core_errors_match_v8(&[
        "(assoc [] -1 1)",
        "(assoc [1] 1.5 2)",
        "(assoc [] :a 1)",
        "(assoc [] nil 1)",
        "(assoc 5 :a 1)",
        "(assoc '(1) 0 1)",
        "(assoc #{} 1 2)",
        r#"(assoc "ab" 0 1)"#,
        "(assoc nil :a 1)",
        "(assoc [1] 1 2)",
    ]);
    core_errors_match_v8(&[
        "(conj 5 1)",
        "(conj :k 1)",
        r#"(conj "ab" 1)"#,
        "(conj {:a 1} 5)",
        "(conj {:a 1} :k)",
        "(conj {:a 1} [1])",
        "(conj {:a 1} '([:b 2] 5))",
        "(conj {:a 1} '([:b 2] [5]))",
        r#"(conj {:a 1} "x")"#,
        "(conj {:a 1} [:b 2])",
        "(conj {} [1 2 3])",
        "(conj {:a 1} {:b 2 :a 3})",
        "(conj {:a 1} '([:b 2] [:c 3]))",
        "(conj {:a 1} nil)",
        "(conj #{1} 2)",
        "(conj #{1} 1)",
        "(conj (rest [1 2]) 0)",
    ]);
}
/// Atoms, volatiles and the runtime leaves that seq their source
/// (light-cljs-873): the protocol errors and deref's.
#[cfg(light_v8)]
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_macro_reference_and_leaf_errors_match_v8() {
    core_errors_match_v8(&[
        "(deref 5)",
        "(deref nil)",
        "(deref [1])",
        "(swap! 5 inc)",
        "(swap! (volatile! 1) inc)",
        "(reset! nil 1)",
        "(reset! (volatile! 1) 2)",
        "(vreset! (atom 1) 2)",
        "(compare-and-set! 5 1 2)",
        "(compare-and-set! (volatile! 1) 1 2)",
        "(swap-vals! 5 inc)",
        "(reset-vals! (volatile! 1) 2)",
        "(add-watch 5 :k inc)",
        "(remove-watch (volatile! 1) :k)",
        "(set-validator! 5 inc)",
    ]);
    core_errors_match_v8(&[
        "(mapv inc 5)",
        "(vec :k)",
        "(into [] 5)",
        "(into 5 [1])",
        "(into 5 [])",
        "(into {} [[:a 1]])",
        "(into #{} [1 1 2])",
        "(sequence 5)",
        "(get-in {:a 1} :a)",
    ]);
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
    let cases: [(&str, &str, i64, &[usize]); 10] = [
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
        // A property key's toString demands the root inside the Convert
        // frame of an assignment (light-cljs-828): the base, the converted
        // key and the assigned value survive the wait, and the key converts
        // once.
        (
            "to-primitive-key",
            "let calls = 0; const k = { toString: function () { try { calls += 1; note(1); return read('fixture/s'); } finally { rec.frame = 1; note(4); } } }; const o = {}; const r = (o[k] = 5); value = (o.x === 5 && r === 5 ? 5 : 0) + calls * 100;",
            1489105,
            &[3],
        ),
        // A constructor demands the root (light-cljs-831): its frame keeps
        // the new object and new.target across the wait, and the primitive
        // it returns afterwards is replaced by the object.
        (
            "construct",
            "function C() { try { note(1); this.v = read('fixture/a'); this.t = new.target === C; return 3; } finally { rec.frame = 1; note(4); } } const c = new C(); value = c.v + (c.t && Object.getPrototypeOf(c) === C.prototype ? 2 : 0);",
            1489042,
            &[1],
        ),
        // The Error constructors' and Error.prototype.toString's native
        // frames (light-cljs-870): a message's toString inside `new Error`,
        // a cause getter inside `new TypeError` (its receiver the options
        // object) and a name getter inside Error.prototype.toString demand
        // the root; the frame keeps the error object, the options object
        // and the receiver across the wait. Values are the V8 build's.
        (
            "error-message",
            "let calls = 0; const m = { toString: function () { try { calls += 1; note(1); return read('fixture/s'); } finally { rec.frame = 1; note(4); } } }; const e = new Error(m); value = (e.message === 'x' ? 5 : 0) + calls * 100;",
            1489105,
            &[3],
        ),
        (
            "error-cause",
            "let calls = 0; const o = { n: 2, get cause() { try { calls += 1; note(1); return read('fixture/a') + this.n; } finally { rec.frame = 1; note(4); } } }; const e = new TypeError('m', o); value = (e.message === 'm' && e instanceof TypeError ? e.cause : 0) + calls * 100;",
            1489142,
            &[1],
        ),
        (
            "error-name",
            "let calls = 0; const o = { get name() { try { calls += 1; note(1); return read('fixture/s'); } finally { rec.frame = 1; note(4); } }, message: 'm' }; value = (Error.prototype.toString.call(o) === 'x: m' ? 5 : 0) + calls * 100;",
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
    // (Object(Symbol.iterator), a step light-cljs-885 implemented, was the
    // built-in step here; Number.prototype.toString's radix 16 is one.)
    for (body, name) in [
        ("(255).toString(16)", "a-radix-other-than-10"),
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

/// A NativeBoundary names what the Goose runtime lacks (light-cljs-915;
/// source_vm.goose boundary_detail): a core function by its runtime name,
/// a built-in step, a form operation by its `__rt` name, an image member or
/// an image write, in a job and in a preparation, and the compiler's
/// diagnostic carries it. The ClojureScript macros use core functions and
/// steps Goose lacks today; when one runs, pick another the runtime lacks.
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_names_what_a_native_boundary_lacks() {
    for (macro_body, named) in [
        (
            "(map inc (take 2 x))",
            "Fault.NativeBoundary:no-core-leaf-take ",
        ),
        // `map` runs as the mapSeq leaf, which maps only a finite list.
        (
            "(map inc x)",
            "Fault.NativeBoundary:mapSeq:-no-core-leaf-map-over-a-vector ",
        ),
        ("(cons 0 x)", "Fault.NativeBoundary:form-operation-cljCons "),
        // A deftype in a macro body fails as its definition is prepared.
        (
            "(do (deftype T [a]) (.-a (T. 1)))",
            "preparation-Fault.NativeBoundary:form-operation-structType ",
        ),
    ] {
        let source = format!("(defmacro m [x] (list 'quote {macro_body})) (m [1 2 3])");
        assert!(
            crate::compile_with_macros(&source, &mut crate::MacroHost::new()).is_ok(),
            "{source}"
        );
        let session = GooseSession::start(&runner(), 1).unwrap();
        let mut host = crate::MacroHost::new();
        host.use_goose_source_macros(session.clone()).unwrap();
        let error = crate::compile_with_macros(&source, &mut host)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains(named)
                && error.contains("emitted-span=")
                && (named.starts_with("preparation-") || !error.contains("preparation-")),
            "{source}: {error}"
        );
        cleaned(&session);
    }
    for (body, named) in [
        (
            "rt => (form,env) => globalThis.__lightNativeRuntime.core.notACoreLeaf",
            "Fault.NativeBoundary:no-core-leaf-notACoreLeaf ",
        ),
        (
            "rt => (form,env) => globalThis.__lightNativeRuntime.notAMember",
            "Fault.NativeBoundary:no-rt-member-notAMember ",
        ),
        (
            "rt => (form,env) => globalThis.gooseAbsentName",
            "Fault.NativeBoundary:no-global-gooseAbsentName ",
        ),
        (
            "rt => (form,env) => { globalThis.gooseForbidden = 1; return 1; }",
            "Fault.NativeBoundary:write-to-an-image-row:-gooseForbidden ",
        ),
        (
            "rt => (form,env) => { rt.core.inc.x = 1; return 1; }",
            "Fault.NativeBoundary:write-to-an-image-row:-x ",
        ),
        (
            "rt => (form,env) => __rt.seal(form)",
            "Fault.NativeBoundary:form-operation-seal ",
        ),
        // A built-in a forwarded call reaches names itself too.
        (
            "rt => (form,env) => Reflect.apply(rt.core.mapSeq, null, [rt.core.inc, __rt.formVector(1)])",
            "Fault.NativeBoundary:mapSeq:-no-core-leaf-map-over-a-vector ",
        ),
        (
            "rt => { globalThis.__lightNativeRuntime.core.notACoreLeaf; return (form,env) => 1; }",
            "preparation-Fault.NativeBoundary:no-core-leaf-notACoreLeaf ",
        ),
    ] {
        let s = GooseSession::start(&runner(), 1).unwrap();
        let e = GooseEvaluator::new(s.clone(), &Default::default()).unwrap();
        let (f, args, _) = inputs(&e, body);
        let error = e.call(f, &args).unwrap_err();
        assert!(
            error.0.contains(named)
                && error.0.contains("emitted-span=")
                && (named.starts_with("preparation-") || !error.0.contains("preparation-")),
            "{body}: {error}"
        );
        s.close();
        cleaned(&s);
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
        let rx = send_call(&session, 1, 2, b"GFB2");
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
        let send = |id| send_call(&session, id, 1, &bytes);
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
                Event::Done(..) => break,
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
        session.send("0\n", &[]).unwrap();
        // Observe native closure before calling close: close()'s kill
        // fallback must not turn a deadlocked native shutdown into a pass.
        wait(&|| session.trace().iter().any(|s| s == "CLOSED 1"));
        assert!(!session.trace().iter().any(|s| s == "DONE 2"));
        assert_threads(&session.trace(), workers);
        session.close();
        cleaned(&session);
        // The runner's gate files, cleanup-ready from job 1's worker and
        // claim-ready from job 2's, are counted though both are gone with the
        // directory now: the count light-cljs-822's checks rely on sees files
        // that no longer exist, from any worker thread.
        assert_eq!(session.files().runner, Some(2), "{:?}", session.trace());
    }
}

/// The session directory got no file but the base image and one export per
/// prepared definition, in that order, from the compiler or the runner. Both
/// count every file as they create it (light-cljs-822), so a per-call file
/// removed again at once still fails this. After close: the runner reports
/// its count as it closes.
fn assert_only_definition_files(session: &GooseSession, definitions: u64, context: &str) {
    let files = session.files();
    let expected = std::iter::once("base.gsp".to_owned())
        .chain((1..=definitions).map(|n| format!("definition-{n}.gsp")))
        .collect::<Vec<_>>();
    assert_eq!(
        files.compiler, expected,
        "{context}: compiler-created files"
    );
    assert_eq!(files.runner, Some(0), "{context}: runner-created files");
}

/// The counts above see every file only while each side has one way to
/// create one: the adapter writes base.gsp at start and every other file
/// through `export`, and the runner writes through `create_file` alone
/// (light-cljs-822). A new write elsewhere must be routed through them.
#[test]
fn goose_session_files_have_one_counted_writer_on_each_side() {
    let adapter = include_str!("../goose_evaluator.rs");
    let adapter = &adapter[..adapter.find("#[cfg(test)]\nmod ").unwrap()];
    assert_eq!(adapter.matches("fs::write(").count(), 2);
    for other in [
        "File::create",
        "OpenOptions",
        "fs::copy",
        "fs::rename",
        "hard_link",
        "symlink",
    ] {
        assert!(!adapter.contains(other), "{other}");
    }
    let runner = include_str!("../../crates/big-boy-pants-js/src/compiler_session.goose");
    assert_eq!(
        runner.matches("write_file(").count(),
        1,
        "only in create_file"
    );
    for other in ["append_file(", "write_file_atomic(", "rename_file("] {
        assert!(!runner.contains(other), "{other}");
    }
}

/// Source-macro calls, their results and their helper answers travel in the
/// runner's pipes (light-cljs-816): a build of many calls at 1 and 4 workers,
/// some reading a data root, some calling a function root, and some reading
/// neither, creates no file in the session directory but the base image and
/// the definition exports, counted as they are created (light-cljs-822).
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_calls_create_no_per_call_files() {
    let root = std::env::temp_dir().join(format!(
        "goose-no-call-files-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("macros.cljs"), "(ns macros) (defn wrap [form] (list 'quote form)) (def shared [11 22]) (defmacro data [n] (list 'quote [n shared])) (defmacro call [n] (wrap [n n])) (defmacro plain [n] (* n 2))").unwrap();
    let mut requires = String::new();
    let mut names = Vec::new();
    for space in 0..4 {
        let calls = (0..12)
            .map(|i| {
                let n = space * 100 + i;
                let call = ["data", "call", "plain"][i % 3];
                format!("(def v{i} ({call} {n}))")
            })
            .collect::<Vec<_>>()
            .join(" ");
        std::fs::write(
            root.join(format!("batch{space}.cljs")),
            format!("(ns batch{space} (:require [macros :refer [data call plain]])) {calls}"),
        )
        .unwrap();
        requires.push_str(&format!(" [batch{space} :as b{space}]"));
        names.push(format!("b{space}/v0 b{space}/v1 b{space}/v11"));
    }
    std::fs::write(
        root.join("main.cljs"),
        format!(
            "(ns main (:require{requires})) (js/console.log (pr-str [{}]))",
            names.join(" ")
        ),
    )
    .unwrap();
    let mut builds = Vec::new();
    for workers in [1, 4] {
        let session = GooseSession::start(&runner(), workers).unwrap();
        let mut host = crate::MacroHost::new();
        host.evaluator_options.memory_limit = 512 << 20;
        host.use_goose_source_macros(session.clone()).unwrap();
        let _names = crate::generated_names::enter_namespace(0);
        let (program, _) = crate::compiler::analyze_namespace_entry_recorded(
            "main",
            std::slice::from_ref(&root),
            &mut host,
            Default::default(),
            &mut Vec::new(),
            crate::compiler::EntryRecorders {
                slots: crate::scheduler::Slots::new(4).unwrap(),
                ..Default::default()
            },
        )
        .unwrap_or_else(|e| panic!("workers={workers}: {e} trace={:?}", session.trace()));
        let stats = session.stats();
        let trace = session.trace();
        assert_eq!(stats.calls, 48, "{trace:?}");
        assert!(
            trace.iter().filter(|s| s.starts_with("NEED ")).count() >= 32,
            "the data and call macros demand their roots: {trace:?}"
        );
        session.close();
        cleaned(&session);
        assert_only_definition_files(&session, stats.definitions, &format!("workers={workers}"));
        builds.push(crate::emit_js(&program));
    }
    assert_eq!(builds[0], builds[1], "deterministic compiler output");
    for expected in ["[11, 22]", "[101, 101]"] {
        assert!(builds[0].contains(expected), "{expected}: {}", builds[0]);
    }
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_job_reclaims_past_its_margins() {
    // A macro job that allocates 150,000 nodes, five per iteration, keeping
    // a small live set (its argument forms and a vector it made first)
    // collects inside the job and completes (light-cljs-847); the JOBS line
    // counts its collections. One whose live set passes the job's
    // 100,000-node limit still ends MemoryLimit.
    let churn = "rt => (form,env) => { const keep = __rt.formVector(1, 2); let o; for (let i = 0; i < 30000; i++) o = [{}, {}, {}, {i}]; return __rt.cljCount(keep) * 100000 + o[3].i; }";
    let live = "rt => (form,env) => { const kept = []; for (let i = 0; i < 30000; i++) kept[i] = [{}, {}, {}, {i}]; return kept.length; }";
    for workers in [1, 2] {
        // Traced, so the session prints its JOBS line.
        let session = GooseSession::start_mode(&runner(), workers, Some("readiness-test")).unwrap();
        let e = GooseEvaluator::new(session.clone(), &Default::default()).unwrap();
        let (f, args, call) = inputs(&e, churn);
        for _ in 0..2 {
            let value = e.call(f, &args).unwrap();
            let form = e
                .export_form(value, &call, origin(&call), Default::default())
                .unwrap();
            assert!(
                form.data()
                    .value_eq(&Datum::new(DatumKind::Integer(229999))),
                "{form:?}"
            );
        }
        let (f, args, _) = inputs(&e, live);
        let message = e.call(f, &args).unwrap_err().0;
        assert!(message.contains("Fault.MemoryLimit"), "{message}");
        session.close();
        cleaned(&session);
        let jobs = assert_threads(&session.trace(), workers);
        assert!(jobs.reclaims >= 2, "{jobs:?}");
    }
}

#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_reclamation_keeps_helper_identities() {
    // A job reads a published helper, allocates past its node margin so its
    // collections move the rows above its floor (the helper's foreign range
    // and cell rows, the cache entry naming it, the job's own values), and
    // reads the helper again: the same identity, the same value
    // (light-cljs-847). With retention, the wait happens in place and the
    // helper's rows lie above the job's floor; with none, the job saves its
    // continuation at each wait and resumes from it.
    let body = "rt => (form,env) => { const keep = __rt.formVector(7, 8); const a = __rt.cljReadRoot(globalThis.__lightCompilerRoots, null, 'fixture/shared'); let o; for (let i = 0; i < 15000; i++) o = [{}, {}, {}, {i}]; const b = __rt.cljReadRoot(globalThis.__lightCompilerRoots, null, 'fixture/shared'); for (let i = 0; i < 15000; i++) o = [{}, {}, {}, {i}]; return a === b && __rt.cljCount(a) === 3 && __rt.formValue(__rt.cljNth(b, 2)) === 3 && __rt.formValue(__rt.cljNth(keep, 1)) === 8 && o[3].i === 14999; }";
    for retained in [None, Some(0)] {
        let mut env = vec![];
        if let Some(retained) = retained {
            env.push(("BBP_RETAINED_WORKERS", retained.to_string()));
        }
        let session = GooseSession::start_env(&runner(), 1, Some("readiness-test"), &env).unwrap();
        let e = GooseEvaluator::new(session.clone(), &Default::default()).unwrap();
        serve(&e, &[("fixture/shared", 77)], |_| {
            Ok("rt => __rt.formVector(1, 2, 3)".into())
        });
        let (f, args, call) = inputs(&e, body);
        let value = e.call(f, &args).unwrap();
        let form = e
            .export_form(value, &call, origin(&call), Default::default())
            .unwrap();
        assert!(
            form.data().value_eq(&Datum::new(DatumKind::Boolean(true))),
            "{retained:?}: {form:?}"
        );
        session.close();
        cleaned(&session);
        let jobs = assert_threads(&session.trace(), 1);
        assert!(jobs.reclaims >= 2, "{retained:?}: {jobs:?}");
        if retained == Some(0) {
            assert!(jobs.saves >= 1, "{jobs:?}");
        }
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
    send_call(session, id, key, bytes)
}
/// Send raw call `id` of definition `key` with argument graph `bytes`, in
/// its frame (light-cljs-816), and return its mailbox.
fn send_call(session: &GooseSession, id: u64, key: u64, bytes: &[u8]) -> mpsc::Receiver<Event> {
    let (tx, rx) = mpsc::channel();
    session.pending.lock().unwrap().insert(id, tx);
    session
        .send(&format!("C {id} {key} {}\n", bytes.len()), bytes)
        .unwrap();
    rx
}
/// The definition export that answers job `id`'s wait `sequence` in raw
/// fixtures: one per answer, as the compiler's helper exports are numbered.
fn helper_definition(id: u64, sequence: u64) -> u64 {
    1_000_000 + id * 1000 + sequence
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
    let definitions: [(u64, &str); 19] = [
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
        // A synchronous entry returns 42 while two async side tasks await
        // (light-cljs-814): the first asks the compiler for a root during
        // the promise-job drain, after its await, then awaits again and
        // settles with a value of its own; the second, queued behind it,
        // runs in between. Each side task rejects, unhandled, with the
        // effects it saw (two decimal digits each) unless they are in FIFO
        // order, so a lost or replayed queue entry fails the job.
        (
            18,
            "rt => (form, env) => { const log = []; const first = async () => { log[log.length] = 1; await null; log[log.length] = 4; const v = __rt.cljReadRoot(globalThis.__lightCompilerRoots, null, 'fixture/value'); log[log.length] = v; await null; let seen = 0; for (let i = 0; i < log.length; i++) seen = seen * 100 + log[i]; if (seen !== 10203044105) throw seen; return 'side'; }; const second = async () => { log[log.length] = 2; await null; let seen = 0; for (let i = 0; i < log.length; i++) seen = seen * 100 + log[i]; if (seen !== 102030441) throw seen; log[log.length] = 5; return 'other'; }; first(); second(); log[log.length] = 3; return 42; }",
        ),
        // Tagged templates (light-cljs-862): a site's template object, and
        // one in eval code, made before a root read are the same objects
        // after it, frozen and with their contents, whether the job waited
        // in place or resumed from a saved continuation (Saved.templates).
        (
            19,
            "rt => (form, env) => { \"use strict\"; const tag = s => s; const site = () => tag`k${0}v`; const h = eval(\"(() => tag`e`)\"); const a = site(), b = h(); const v = __rt.cljReadRoot(globalThis.__lightCompilerRoots, null, 'fixture/value'); let n = v; if (site() === a) n += 100; if (h() === b) n += 1000; if (a[0] === 'k' && a.raw[1] === 'v' && b[0] === 'e') n += 10000; try { a[0] = 'z'; } catch (e) { if (e instanceof TypeError && a[0] === 'k') n += 100000; } return n; }",
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
        if key == 8 || key == 9 || key == 18 || key == 19 {
            let helper = if key == 8 {
                "rt => {const result=41; return function(){return result;};}"
            } else {
                "rt => 41"
            };
            let definition = helper_definition(id, 0);
            session
                .export(
                    &session
                        .directory
                        .join(format!("definition-{definition}.gsp")),
                    helper,
                )
                .unwrap();
            let unit = if key == 8 { 8 } else { 9 };
            session
                .answer_key(id, 0, &format!("{unit} {definition}"))
                .unwrap();
        }
        let rx = submit_key(&session, &bytes, id, key);
        let result = loop {
            match rx.recv_timeout(Duration::from_secs(20)).unwrap() {
                Event::Done(_, out) => {
                    let output = bridge::Graph::decode(&out, &graph.originals, limits, &budget)
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
    let reads = plan
        .iter()
        .filter(|&&k| [8, 9, 14, 18, 19].contains(&k))
        .count() as u64;
    // Definitions 9, 18 and 19 read the same root: one production serves them.
    let productions =
        (plan.contains(&8) as u64) + (plan.iter().any(|k| [9, 18, 19].contains(k)) as u64);
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
fn goose_compiler_saved_continuation_during_promise_drain() {
    // The entry has returned and the promise-job queue is draining when an
    // async side task waits for a helper (light-cljs-814). With retention 0
    // the wait saves the continuation (Saved.queue, draining and entry) and
    // resumes it from the snapshot on a reused worker; with retention it
    // resumes in place. Either way the result is the entry's 42, not a side
    // task's value or a rejection over effects out of FIFO order, and a pure
    // job between the two runs sees the image as built.
    let expected = "Double(42.0)";
    for retained in [Some(0), None] {
        let (outcomes, jobs) = reuse_session(1, Some(16 << 20), retained, &[18, 1, 18]);
        for (key, outcome) in &outcomes {
            if *key == 18 {
                assert!(outcome.contains(expected), "{retained:?}: {outcome}");
                assert!(outcome.contains("NEED "), "{retained:?}: {outcome}");
            } else {
                assert!(outcome.contains("Double(53.0)"), "{retained:?}: {outcome}");
            }
        }
        assert_eq!(outcomes[0], outcomes[2], "{retained:?}");
        assert_eq!(
            jobs.saves,
            if retained == Some(0) { 2 } else { 0 },
            "{jobs:?}"
        );
        assert_eq!(jobs.retired, 0, "{jobs:?}");
    }
}

#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_tagged_templates_across_a_saved_continuation() {
    // Per-site template objects (light-cljs-862) survive a helper wait in
    // place and, with retention 0, a saved continuation restored on a reused
    // worker: identity, contents and frozenness of a site's object and of
    // an eval-compiled site's (restored at the same code rows).
    for retained in [Some(0), None] {
        let (outcomes, jobs) = reuse_session(1, Some(16 << 20), retained, &[19, 1, 19]);
        for (key, outcome) in &outcomes {
            let expected = if *key == 19 {
                "Double(111141.0)"
            } else {
                "Double(53.0)"
            };
            assert!(outcome.contains(expected), "{retained:?} {key}: {outcome}");
        }
        assert_eq!(outcomes[0], outcomes[2], "{retained:?}");
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
    send_call(session, id, 1, bytes)
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
    session.send("0\n", &[]).unwrap();
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
    session.send("0\n", &[]).unwrap();
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
                // Real emitted helper, with all answers sent before dispatch.
                // Concurrent waiters must still elect exactly one producer.
                let definition = helper_definition(id, 0);
                session
                    .export(
                        &session
                            .directory
                            .join(format!("definition-{definition}.gsp")),
                        if callable {
                            "rt => {const result=41; return function(){return result;};}"
                        } else {
                            "rt => 41"
                        },
                    )
                    .unwrap();
                session
                    .answer_key(id, 0, &format!("9 {definition}"))
                    .unwrap();
            }
            for id in 1..=8 {
                replies.push(send_call(&session, id, 1, &bytes));
            }
            for rx in replies {
                let bytes = loop {
                    match rx.recv_timeout(Duration::from_secs(10)).unwrap() {
                        Event::Done(_, bytes) => break bytes,
                        Event::Start | Event::Resumed | Event::Produced | Event::Need(_, _) => {}
                        other => panic!("unexpected reply: {other:?}"),
                    }
                };
                let output = bridge::Graph::decode(&bytes, &graph.originals, limits, &budget)
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
        // Answers no job waits for stay held and cannot produce a terminal
        // reply. Wake is coalesced even while the input pipe drains.
        for sequence in 0..256 {
            session.answer_key(999_999, sequence, "-1").unwrap();
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
        replies.push(send_call(&session, id, 1, &bytes));
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
    session.send("0\n", &[]).unwrap();
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

/// Source macros that call a `defn` helper of their macro namespace which
/// calls `count`, on a quoted vector, on `&env` and on the call's argument:
/// at compiler slots 1/2/4/8 on the Goose path the JavaScript is the V8
/// build's (light-cljs-821).
#[cfg(light_v8)]
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_defn_helpers_that_count_at_every_slot_count() {
    let cases = [
        (
            "(ns m.macros) (defn c [x] (count x)) (defmacro lit [] (c '[1 2 3]))",
            "(ns app.main (:require [m.macros :as m])) (def w (m/lit))",
        ),
        (
            "(ns m.macros) (defn inventory2 [e] (count e)) (defmacro whole2 [] (inventory2 &env))",
            "(ns app.main (:require [m.macros :as m])) (def w (m/whole2))",
        ),
        (
            "(ns m.macros) (defn c [x] (count x)) (defmacro arg [x] (c x))",
            "(ns app.main (:require [m.macros :as m])) (def w [(m/arg [1 2]) (m/arg (a b c d))])",
        ),
    ];
    for (macros, main) in cases {
        let files = HashMap::from([
            ("m.macros".to_owned(), macros.to_owned()),
            ("app.main".to_owned(), main.to_owned()),
        ]);
        let (reference, _, _) = pooled_build(&files, 1, None, Default::default());
        let reference = reference.unwrap();
        for slots in [1, 2, 4, 8] {
            let session = GooseSession::start(&runner(), slots).unwrap();
            let (output, _, census) =
                pooled_build(&files, slots, Some(&session), Default::default());
            assert_eq!(
                output.as_deref(),
                Ok(reference.as_str()),
                "slots={slots}: {macros}"
            );
            assert!(census.is_empty(), "slots={slots}: {census:?}");
            session.close();
            cleaned(&session);
        }
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
        (
            "(m/boom)",
            "macro m.macros/boom: Error: Index out of bounds; Goose",
        ),
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
    let definition = helper_definition(id, sequence);
    session
        .export(
            &session
                .directory
                .join(format!("definition-{definition}.gsp")),
            source,
        )
        .unwrap();
    session
        .answer_key(id, sequence, &format!("{unit} {definition}"))
        .unwrap();
}
/// Job `id`'s result (its expansion's data) or failure.
fn raw_outcome(raw: &RawInputs, rx: &mpsc::Receiver<Event>) -> Result<String, String> {
    loop {
        match rx.recv_timeout(Duration::from_secs(20)).unwrap() {
            Event::Done(_, out) => {
                let output =
                    bridge::Graph::decode(&out, &raw.graph.originals, raw.limits, &raw.budget)
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
        outcomes.push(raw_outcome(&raw, &rx));
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
                "rt.core.reset(a, rt.vectorFromArray([(function f() {}).bind(null)])); return 0;",
                "rt.core.reset(a, rt.vectorFromArray([(function () { return arguments; })(1)])); return 0;",
                "return rt.core.compareAndSet(a, 1, 5) ? rt.core.deref(a) : -1;",
                "rt.core.reset(a, rt.vectorFromArray([({ m() { return 1; } }).m])); return 0;",
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
        // A bound function (light-cljs-852) always has own `length` and
        // `name`: a cell refuses it inside a stored vector, by name.
        assert!(
            outcomes[4]
                .as_ref()
                .unwrap_err()
                .contains("Fault.NativeBoundary:reset:-cannot-share-a-bound-function"),
            "{outcomes:?}"
        );
        // An arguments object (light-cljs-854) is a mutable JavaScript object.
        assert!(
            outcomes[5]
                .as_ref()
                .unwrap_err()
                .contains("Fault.NativeBoundary:reset:-cannot-share-a-mutable-JavaScript-object"),
            "{outcomes:?}"
        );
        // None changed the cell: it still holds the faulting job's 1.
        assert_eq!(outcomes[6].as_deref(), Ok("Double(5.0)"), "{outcomes:?}");
        // A method's home object (light-cljs-888) is an edge of its closure:
        // an object literal's method reaches the literal, a mutable
        // JavaScript object, and is refused as a closure capturing it is.
        assert!(
            outcomes[7]
                .as_ref()
                .unwrap_err()
                .contains("Fault.NativeBoundary:reset:-cannot-share-a-mutable-JavaScript-object"),
            "{outcomes:?}"
        );
        assert_eq!((jobs.cells, jobs.conflicts), (1, 0), "{jobs:?}");
        assert!(
            jobs.cell_operations >= 7 && jobs.owner_ns > 0 && jobs.delay_waits == 0,
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
        let outcomes = [raw_outcome(&raw, &first), raw_outcome(&raw, &second)];
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
        let later = raw_outcome(&raw, &third);
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
        let outcomes = [raw_outcome(&raw, &first), raw_outcome(&raw, &second)];
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
        let later = raw_outcome(&raw, &third);
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

    // A thunk that rethrows a fault's error after changing its message: the
    // stored error keeps the fault's original message, which a job that
    // forces it uncaught reports (light-cljs-874).
    let changed = "rt => new rt.core.Delay(() => { try { null.foo; } catch (e) { e.message = 'changed'; throw e; } }, null)";
    for (workers, env) in delay_variants() {
        let session =
            GooseSession::start_env(&runner(), workers, Some("readiness-test"), &env).unwrap();
        let e = GooseEvaluator::new(session.clone(), &Default::default()).unwrap();
        let raw = raw_inputs(&e);
        raw_helper(&session, 1, 1, 43, changed);
        let first = raw_job(
            &session,
            &raw,
            1,
            &format!(
                "const d = {}; try {{ rt.core.force(d); return -1; }} catch (e) {{ return e.message === 'changed' ? 1 : 0; }}",
                root("fixture/changed")
            ),
        );
        let caught = raw_outcome(&raw, &first);
        raw_helper(&session, 2, 1, 43, changed);
        let second = raw_job(&session, &raw, 2, &force("fixture/changed"));
        let uncaught = raw_outcome(&raw, &second);
        // Starts, root reads (2 + 2), productions (counter, delay).
        raw_close(&session, workers, 2 + 4 + 2);
        assert_eq!(
            caught.as_deref(),
            Ok("Double(1.0)"),
            "workers={workers} {env:?}"
        );
        let message = uncaught.unwrap_err();
        assert!(
            message.ends_with(" uncaught=TypeError: cannot read property 'foo' of null"),
            "workers={workers} {env:?}: {message}"
        );
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
        let outcomes = [raw_outcome(&raw, &first), raw_outcome(&raw, &second)];
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
            outcomes.push(raw_outcome(&raw, &rx));
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
    let jobs: [(String, &[&str]); 21] = [
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
        // A stored function is shared: its producer may not write it.
        (
            "const f = () => 1; rt.core.reset(a, f); f.memo = 7; return 0;".to_owned(),
            &[],
        ),
        (
            "const f = rt.core.deref(a); return f() + (f.memo === undefined ? 10 : 0);"
                .to_owned(),
            &[],
        ),
        // A job's pending delay, alone and inside a vector, keeps its thunk.
        (
            "rt.core.reset(a, new rt.core.Delay(() => 42, null)); return 0;".to_owned(),
            &[],
        ),
        ("return rt.core.force(rt.core.deref(a));".to_owned(), &[]),
        (
            "rt.core.reset(a, rt.vectorFromArray([new rt.core.Delay(() => 43, null)])); return 0;"
                .to_owned(),
            &[],
        ),
        (
            "let d = null; rt.core.mapv((x) => { d = x; return 0; }, rt.core.deref(a)); return rt.core.force(d);"
                .to_owned(),
            &[],
        ),
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
        assert!(
            outcomes[15]
                .as_ref()
                .is_err_and(|e| e.contains("Fault.NativeBoundary")),
            "workers={workers} {env:?}: {outcomes:?}"
        );
        assert_eq!(at(16), ok(16, "Double(11.0)"), "workers={workers} {env:?}");
        assert_eq!(at(18), ok(18, "Double(42.0)"), "workers={workers} {env:?}");
        assert_eq!(at(20), ok(20, "Double(43.0)"), "workers={workers} {env:?}");
        // The counter, the box and the two delays; no swap! here met a
        // conflicting write.
        assert_eq!((line.cells, line.conflicts), (4, 0), "{line:?}");
        // The counter ends holding the last value stored, its bytes held.
        assert!(line.cell_bytes > 0, "{line:?}");
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
            outcomes.push(raw_outcome(&raw, &rx));
        }
        raw_close(&session, workers, 2);
        assert_eq!(
            outcomes,
            [Ok("Double(17.0)".to_owned()), Ok("Double(17.0)".to_owned())],
            "workers={workers} {env:?}"
        );
    }
}

/// raw_inputs for a call whose form is `kind`, spanning the first three
/// bytes of source file `name` with text `text`.
fn raw_inputs_from(e: &GooseEvaluator, name: &str, text: &str, kind: DatumKind) -> RawInputs {
    let source = Source::new(name, text);
    let call = Form::from_data(Datum::new(kind), source.span(0..3).unwrap(), Origin::Source);
    raw_inputs_of(e, call)
}
/// [`raw_inputs`] for the call form the reader reads from `text`.
fn raw_inputs_read(e: &GooseEvaluator, name: &str, text: &str) -> RawInputs {
    let source = Source::new(name, text);
    let call = crate::compiler_reader::read_form(
        &source,
        &crate::compiler_reader::NamespaceView::new("fixture.core", &source),
    )
    .unwrap()
    .unwrap();
    raw_inputs_of(e, call)
}
/// [`raw_inputs`] for `call`.
fn raw_inputs_of(e: &GooseEvaluator, call: Form) -> RawInputs {
    let mut raw = raw_inputs(e);
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
    raw
}

/// light-cljs-809: a stored function's captured forms outlive their call,
/// as a form cell value does: an expansion of another call (other data,
/// another source file and span) that returns a form an earlier expansion
/// captured gets it as a form built afresh. The owner's byte census follows
/// the values held: a 10-unit string, then a 3-unit one replacing it.
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_cell_function_forms_and_byte_census() {
    for workers in [1, 2, 4, 8] {
        let session =
            GooseSession::start_env(&runner(), workers, Some("readiness-test"), &[]).unwrap();
        let e = GooseEvaluator::new(session.clone(), &Default::default()).unwrap();
        let first = raw_inputs_from(&e, "first.cljs", "nil ;; first", DatumKind::Nil);
        let second = raw_inputs_from(&e, "second.cljs", "  true", DatumKind::Boolean(true));
        let mut outcomes = Vec::new();
        for (id, raw, body) in [
            (
                1,
                &first,
                "const saved = form; rt.core.reset(a, () => saved); return 0;",
            ),
            (2, &second, "return rt.core.deref(a)();"),
            (3, &second, "rt.core.reset(a, 'abcdefghij'); return 0;"),
        ] {
            let rx = raw_job(&session, raw, id, body);
            outcomes.push(raw_outcome(raw, &rx));
            thread::sleep(Duration::from_millis(20));
        }
        let line = raw_close(&session, workers, 7);
        assert_eq!(outcomes[0].as_deref(), Ok("Double(0.0)"), "{outcomes:?}");
        assert_eq!(outcomes[1].as_deref(), Ok("Nil"), "{outcomes:?}");
        assert_eq!(line.cell_bytes, 20, "{line:?}");
        let (outcomes, line) = cell_session(
            workers,
            &[],
            &[
                "rt.core.reset(a, 'abcdefghij'); return 0;",
                "rt.core.reset(a, 'abc'); return 0;",
            ],
        );
        assert!(outcomes.iter().all(Result::is_ok), "{outcomes:?}");
        assert_eq!((line.cells, line.cell_bytes), (1, 6), "{line:?}");
    }
}

/// Job `id`'s expansion, read against its call as the compiler reads it.
fn raw_form(raw: &RawInputs, rx: &mpsc::Receiver<Event>) -> Result<Form, String> {
    loop {
        match rx.recv_timeout(Duration::from_secs(20)).unwrap() {
            Event::Done(_, out) => {
                break bridge::Graph::decode(&out, &raw.graph.originals, raw.limits, &raw.budget)
                    .and_then(|graph| {
                        graph.expansion(
                            &raw.call,
                            origin(&raw.call),
                            &raw.graph.originals,
                            raw.limits,
                            &raw.budget,
                        )
                    })
                    .map_err(|error| error.0);
            }
            Event::Failure(message) => break Err(message),
            _ => {}
        }
    }
}

/// light-cljs-817: what a publication that captured an input form carries
/// of the form's source. A function stored in a cell (a callable
/// publication) that captured its call's form, and the form stored as a
/// cell value, carry neither the source's text nor its identity: the form
/// outlives its call (light-cljs-809), so a later expansion from another
/// file, whose call sent only that file, gets it built afresh at its own
/// call's span. Had the publication kept the first file's identity, the
/// second call's result would name a source it did not send, and fail.
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_captured_input_forms_carry_no_source_of_their_call() {
    for workers in [1, 2, 4, 8] {
        let session =
            GooseSession::start_env(&runner(), workers, Some("readiness-test"), &[]).unwrap();
        let e = GooseEvaluator::new(session.clone(), &Default::default()).unwrap();
        let first = raw_inputs_read(&e, "first.cljs", "[x \"é\" (y :k)]");
        let second = raw_inputs_read(&e, "second.cljs", ";; second\n(zz)");
        assert_ne!(first.call.span().bytes(), second.call.span().bytes());
        let mut outcomes = Vec::new();
        for (id, raw, body) in [
            (
                1,
                &first,
                "const saved = form; rt.core.reset(a, () => saved); return 0;",
            ),
            (2, &second, "return rt.core.deref(a)();"),
            (3, &first, "rt.core.reset(a, form); return 0;"),
            (4, &second, "return rt.core.deref(a);"),
            // The first call's own form, returned in its own call, is the
            // compiler's original, at its span.
            (5, &first, "return form;"),
        ] {
            let rx = raw_job(&session, raw, id, body);
            outcomes.push(raw_form(raw, &rx));
            thread::sleep(Duration::from_millis(20));
        }
        raw_close(&session, workers, 11);
        let outcomes = outcomes
            .into_iter()
            .collect::<Result<Vec<_>, _>>()
            .unwrap_or_else(|error| panic!("workers={workers}: {error}"));
        fn fresh(form: &Form, call: &Form) {
            assert_eq!(form.span().source().id(), call.span().source().id());
            assert_eq!(form.span().bytes(), call.span().bytes());
            assert!(matches!(form.node().origin, Origin::MacroExpansion(_)));
            for child in form.children() {
                fresh(&child, call);
            }
        }
        for at in [1, 3] {
            assert!(outcomes[at].data().value_eq(&first.call.data()));
            fresh(&outcomes[at], &second.call);
        }
        let own = &outcomes[4];
        assert_eq!(own.span().source().id(), first.call.span().source().id());
        assert_eq!(own.span().bytes(), first.call.span().bytes());
        assert!(matches!(own.node().origin, Origin::Source));
        let element = &own.children()[2];
        assert_eq!(
            element.span().bytes(),
            first.call.children()[2].span().bytes()
        );
    }
}

/// light-cljs-809: what a swap! on a cell costs in a macro job, for a number
/// (no graph: the fixed cost), a value encoding to about 64 bytes (a
/// one-number vector, 136 bytes of GFB) and to 64 KB (1,260 numbers),
/// against probe P18's flat-byte figures (9.6 to 9.8 and 28.8 to 33.8
/// microseconds a swap at one worker). The call's
/// source file is 100 KB, as a real namespace's is: a cell store exports
/// only what the value reaches, and the call carries the source as its
/// identity, not its text (light-cljs-817). Each swap loads
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
    let mut text = String::from("nil\n");
    while text.len() < 100_000 {
        text.push_str(";; a namespace's source text\n");
    }
    let raw = raw_inputs_from(&e, "large.cljs", &text, DatumKind::Nil);
    assert!(
        text.len() >= 100_000 && raw.bytes.len() < 1000,
        "{}",
        raw.bytes.len()
    );
    let mut id = 0;
    let mut timed = |value: &str, swaps: usize| {
        id += 1;
        let body = format!(
            "const xs = []; for (let i = 0; i < 1260; i++) xs[i] = i; const v = {value}; rt.core.reset(a, v); const g = (x) => v; for (let i = 0; i < {swaps}; i++) rt.core.swap(a, g); return 0;"
        );
        let started = Instant::now();
        let rx = raw_job(&session, &raw, id, &body);
        assert_eq!(raw_outcome(&raw, &rx).as_deref(), Ok("Double(0.0)"));
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

/// `m.macros` declares `declared` macros besides the ones `main` calls,
/// each of which adds an entry to every expansion's whole `&env` (the
/// macro inventory, `:light-cljs/macros`): `blind` reads no `&env`,
/// `or-local` reads only `(:locals &env)`, `counted` reads all of it with
/// `count` and `passed` passes it to a function.
fn environment_sources(declared: usize, main: &str) -> HashMap<String, String> {
    let mut macros = String::from("(ns m.macros)\n");
    for n in 0..declared {
        macros.push_str(&format!("(defmacro filler{n} [x] x)\n"));
    }
    macros.push_str(
        "(defn size [e] (count e))
         (defmacro blind [] 42)
         (defmacro or-local [sym fallback] (if (contains? (:locals &env) sym) sym fallback))
         (defmacro counted [] (count &env))
         (defmacro passed [] (size &env))",
    );
    HashMap::from([
        ("m.macros".to_owned(), macros),
        (
            "app.main".to_owned(),
            format!("(ns app.main (:require [m.macros :as m]))\n{main}"),
        ),
    ])
}
/// The Goose build of `files` at `slots` compiler slots, which must
/// succeed and create no other engine, and its session's statistics; in the
/// V8 build, its JavaScript must be V8's.
fn goose_environment_build(
    files: &HashMap<String, String>,
    slots: usize,
) -> (String, crate::goose_evaluator::SessionStats) {
    let session = GooseSession::start(&runner(), slots).unwrap();
    let (output, _, census) = pooled_build(files, slots, Some(&session), Default::default());
    let output = output.unwrap_or_else(|e| panic!("slots={slots}: {e}"));
    assert!(census.is_empty(), "slots={slots}: {census:?}");
    session.close();
    cleaned(&session);
    #[cfg(light_v8)]
    {
        let (reference, _, _) = pooled_build(files, 1, None, Default::default());
        assert_eq!(Ok(output.as_str()), reference.as_deref(), "slots={slots}");
    }
    (output, session.stats())
}
/// The end of a build's JavaScript, where `app.main` is, for a message.
fn tail(output: &str) -> &str {
    let mut at = output.len().saturating_sub(2000);
    while !output.is_char_boundary(at) {
        at += 1;
    }
    &output[at..]
}
/// The argument graph nodes each source-macro call of `stats` sent.
fn nodes_per_call(stats: &crate::goose_evaluator::SessionStats) -> u64 {
    assert!(stats.calls > 0, "{stats:?}");
    assert!(
        stats.argument_bytes > stats.argument_source_bytes,
        "{stats:?}"
    );
    assert!(stats.result_bytes > 0, "{stats:?}");
    stats.argument_nodes / stats.calls
}

/// A source macro that reads no `&env`, or only `(:locals &env)`, is given
/// only `:ns`, `:light-cljs/reader-context` and the entries it reads on
/// Goose, as on V8 (light-cljs-815): it expands as V8 expands it, and
/// what each call sends does not grow with the macros the compilation
/// declared. Without the Goose evaluator's extension tracking every call
/// sends the whole environment, the inventory of every declared macro.
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_source_macros_get_only_the_environment_entries_they_read() {
    let main = "(def f (let [a 5] [(m/blind) (m/or-local a 1) (m/or-local b 2)]))
(def g (let [c 3] [(m/or-local c 4) (m/blind)]))";
    for slots in [1, 4] {
        let (few_output, few) = goose_environment_build(&environment_sources(10, main), slots);
        let (many_output, many) = goose_environment_build(&environment_sources(200, main), slots);
        for output in [&few_output, &many_output] {
            let main = tail(output);
            assert!(main.contains("rt.vectorFromArray([42, l"), "{main}");
            assert!(main.contains(", 2]);"), "{main}");
            assert!(main.contains(", 42]);"), "{main}");
        }
        assert_eq!((few.calls, many.calls), (5, 5), "slots={slots}");
        assert_eq!(nodes_per_call(&few), nodes_per_call(&many), "slots={slots}");
        assert!(nodes_per_call(&many) < 200, "slots={slots}: {many:?}");
    }
}

/// A source macro that may read all of its `&env`, with `count` or by
/// passing it to a function, still gets the whole environment on Goose,
/// whose macro inventory grows with the macros declared (light-cljs-815).
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_source_macros_that_may_read_all_of_env_get_all_of_it() {
    let main = "(def counts [(m/counted) (m/passed) (let [x 1] [(m/counted) x])])";
    let (few_output, few) = goose_environment_build(&environment_sources(10, main), 1);
    let (many_output, many) = goose_environment_build(&environment_sources(200, main), 1);
    for output in [&few_output, &many_output] {
        // `&env` has the same eight entries, whatever the inventory holds.
        let main = tail(output);
        assert!(main.contains("rt.vectorFromArray([8, 8, "), "{main}");
        assert!(main.contains("rt.vectorFromArray([8, l"), "{main}");
    }
    assert_eq!((few.calls, many.calls), (3, 3));
    // Each of the 190 more declarations adds at least its name and metadata.
    assert!(
        nodes_per_call(&many) > nodes_per_call(&few) + 190 * 4,
        "{few:?} {many:?}"
    );
}

/// A registered extension of a core protocol that environment data could
/// reach, a top-level `extend-type` of `string`, keeps the whole
/// environment for every source macro on Goose too (light-cljs-433, 815).
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_a_registered_core_extension_keeps_the_whole_environment() {
    for extension in [
        "(extend-type string IHash (-hash [_] 1))",
        "(extend-protocol ICounted Symbol (-count [_] 0))",
    ] {
        let main = format!("{extension}\n(def f (let [a 5] [(m/blind) (m/or-local a 1)]))");
        let (_, few) = goose_environment_build(&environment_sources(10, &main), 1);
        let (_, many) = goose_environment_build(&environment_sources(200, &main), 1);
        assert_eq!((few.calls, many.calls), (2, 2), "{extension}");
        assert!(
            nodes_per_call(&many) > nodes_per_call(&few) + 190 * 4,
            "{extension}: {few:?} {many:?}"
        );
    }
}

/// Goose compile-time code cannot install an extension that environment
/// data could reach, or any other: the job that tries fails with an
/// uncatchable fault before it writes anything, and with it the
/// compilation, so the evaluator never notes one (light-cljs-815,
/// `GooseEvaluator::track_environment_extensions`). The same programs
/// install the extension on V8 (light-cljs-433's cases).
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_compile_time_code_installs_no_extension() {
    for body in [
        "(extend-type Symbol IEquiv (-equiv [a b] (identical? (str a) (str b))))",
        "(extend-type string IHash (-hash [_] 1))",
        "(specify! (js-obj) ICounted (-count [_] 0))",
        "(specify! (T. 1) IEquiv (-equiv [a b] true))",
        "(extend-protocol ILookup PersistentArrayMap (-lookup ([m k] 1) ([m k d] 1)))",
        "(extend-type T IEquiv (-equiv [a b] true))",
        "(extend-protocol P string (f [x] x) default (f [x] x) nil (f [x] x))",
        "(try (extend-type Symbol IEquiv (-equiv [a b] true)) (catch :default e nil))",
        "(reify ILookup (-lookup [_ k] 1))",
        "(set! (.-x js/Object) 1)",
    ] {
        let main = format!(
            "(defprotocol P (f [x])) (deftype T [v])
             (defn install [] {body}) (defmacro setup [] (install) nil) (setup)"
        );
        let files = HashMap::from([("app.main".to_owned(), format!("(ns app.main)\n{main}"))]);
        let session = GooseSession::start(&runner(), 1).unwrap();
        let (output, _, census) = pooled_build(&files, 1, Some(&session), Default::default());
        let error = output.unwrap_err();
        assert!(census.is_empty(), "{body}: {census:?}");
        assert!(
            error.contains("macro app.main/setup: Goose: ERROR"),
            "{body}: {error}"
        );
        // A reify site is a unit's symbol (`__lightUnitReify`), which a
        // helper reaching it may not publish (light-cljs-885).
        assert!(
            [
                "Fault.UnsupportedOperand",
                "Fault.NativeBoundary",
                "symbol-unshareable",
            ]
            .iter()
            .any(|fault| error.contains(fault)),
            "{body}: {error}"
        );
        session.close();
        cleaned(&session);
        #[cfg(light_v8)]
        {
            let (reference, _, _) = pooled_build(&files, 1, None, Default::default());
            assert!(reference.is_ok(), "{body}: {reference:?}");
        }
    }
}

/// A source macro that reads no `&env` still fails at its call on Goose
/// when the environment holds a macro's metadata that cannot be converted,
/// since that environment is not narrowed (`elidable_map_environment`),
/// and with the message it had with the whole environment; and corrected
/// source compiles (light-cljs-433, 815). On V8 the invalid regular
/// expression `#"["` fails and a valid one compiles; the Goose form bridge
/// carries no regular expression, so on Goose both fail alike, and the
/// correction that compiles there is metadata without one.
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_a_blind_source_macro_still_reports_an_unconvertible_environment() {
    let files = |probe: &str| {
        HashMap::from([(
            "app.main".to_owned(),
            format!(
                "(ns app.main)\n(defmacro ignored {{:probe {probe}}} [] nil)\n(defmacro blind [] 42)\n(js/console.log (blind))\n"
            ),
        )])
    };
    for probe in [r#"#"[""#, r#"#"\[""#] {
        let session = GooseSession::start(&runner(), 1).unwrap();
        let (output, _, _) = pooled_build(&files(probe), 1, Some(&session), Default::default());
        let error = output.unwrap_err();
        assert!(
            error.starts_with("app/main.cljs:4:17: macro expansion: while expanding blind: app/main.cljs:4:17: macro app.main/blind: Goose malformed or unsupported form graph"),
            "{probe}: {error}"
        );
        session.close();
        cleaned(&session);
    }
    #[cfg(light_v8)]
    {
        let (reference, _, _) = pooled_build(&files(r#"#"[""#), 1, None, Default::default());
        let reference = reference.unwrap_err();
        assert!(reference.contains("main.cljs:4:17"), "{reference}");
        assert!(reference.contains("macro app.main/blind"), "{reference}");
    }
    let (output, stats) = goose_environment_build(&files(r#""[""#), 1);
    assert!(tail(&output).contains("42"), "{}", tail(&output));
    assert_eq!(stats.calls, 1);
}

/// `source` compiled on Goose with `nodes` as the generated-node limit if
/// given: its JavaScript or its error.
fn goose_compile(source: &str, nodes: Option<usize>) -> Result<String, String> {
    let session = GooseSession::start(&runner(), 1).unwrap();
    let mut host = crate::MacroHost::new();
    if let Some(nodes) = nodes {
        host.limits.generated_nodes = nodes;
    }
    host.use_goose_source_macros(session.clone()).unwrap();
    let result = crate::compile_with_macros(source, &mut host).map_err(|e| e.to_string());
    drop(host);
    session.close();
    cleaned(&session);
    result
}

/// Data Goose cannot convert, which V8 can, still fails a call whose
/// macro is given a narrowed environment when the entries left out hold
/// it, here a character or an integer no double holds in a macro's
/// metadata, with the whole environment's error (light-cljs-815).
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_a_narrowed_environment_still_refuses_what_goose_cannot_convert() {
    for probe in ["\\a", "9007199254740993"] {
        for blind in [
            "(defmacro blind [] 42)",
            "(defmacro blind [] (count &env) 42)",
        ] {
            let source = format!("(defmacro ignored {{:probe {probe}}} [] nil)\n{blind}\n(blind)");
            let error = goose_compile(&source, None).unwrap_err();
            assert!(
                error.contains(
                    ":3:1: macro cljs.user/blind: Goose malformed or unsupported form graph"
                ),
                "{probe} {blind}: {error}"
            );
        }
    }
    let output = goose_compile(
        "(defmacro ignored {:probe \"a\"} [] nil)\n(defmacro blind [] 42)\n(def x (blind))",
        None,
    )
    .unwrap();
    assert!(output.contains("42"));
}

/// The entries a narrowed environment leaves out still take the call's
/// conversion budget, which its argument, environment, nested calls and
/// result share on Goose, so a call that fits only without them still
/// fails at its macro with the conversion limit (light-cljs-815): also
/// after an earlier call measured what leaving the macro inventory out
/// costs, which a later call charges at once only when it fits.
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_environment_narrowing_preserves_shared_conversion_limits() {
    let numbers = |count: usize| {
        (0..count)
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join(" ")
    };
    let source = |count: usize| {
        format!(
            "(defmacro ignored {{:probe [{}]}} [] nil)\n(defmacro blind [x] 42)\n(def y [(blind [1]) (blind [2])])\n(def x (blind [{}]))",
            numbers(1000),
            numbers(count)
        )
    };
    let error = goose_compile(&source(2100), Some(10000)).unwrap_err();
    assert!(
        error.contains(":4:8: macro cljs.user/blind: Goose conversion limit exceeded"),
        "{error}"
    );
    assert!(goose_compile(&source(1500), Some(10000)).is_ok());
}

/// The session's transport counters count what each job's call sent and
/// received: its argument graph's nodes, encoded bytes and source text
/// (UTF-8 bytes, none since a source travels as its identity,
/// light-cljs-817), and its result's bytes, summed over calls
/// (light-cljs-815).
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_transport_counters_count_wire_payloads() {
    let s = GooseSession::start(&runner(), 1).unwrap();
    let e = GooseEvaluator::new(s.clone(), &Default::default()).unwrap();
    let function = e.evaluate_unit("rt => (form, env) => form").unwrap();
    let source = Source::new("counted.cljs", "\"λ\"");
    let text = Datum::new(DatumKind::String(
        "λ".encode_utf16().collect::<Vec<_>>().into(),
    ));
    let form = Form::from_data(text.clone(), source.span(0..4).unwrap(), Origin::Source);
    let call = e.import_form(&form, Default::default()).unwrap();
    let env = e
        .import_data(&Datum::new(DatumKind::Nil), Default::default())
        .unwrap();
    // Two nodes, one code unit of string text, one source entry (its
    // identity and length, no text) and two roots, after the 32-byte header.
    let argument_bytes = 32 + 2 * 48 + 2 + 12 + 2 * 4;
    let mut result_bytes = 0;
    for calls in 1..=2 {
        let result = e.call(function, &[call, env]).unwrap();
        let Stored::Result(bytes, ..) = e.get(result).unwrap() else {
            panic!("not a result");
        };
        result_bytes += bytes.len() as u64;
        let stats = s.stats();
        assert_eq!(
            (
                stats.calls,
                stats.argument_nodes,
                stats.argument_bytes,
                stats.argument_source_bytes,
                stats.result_bytes
            ),
            (calls, 2 * calls, argument_bytes * calls, 0, result_bytes),
            "{stats:?}"
        );
        let returned = e
            .export_form(result, &form, origin(&form), Default::default())
            .unwrap();
        assert!(returned.data().value_eq(&text));
    }
    assert!(result_bytes > 0);
    s.close();
    cleaned(&s);
}

/// An environment that Goose's own import would refuse, here for a macro's
/// metadata string past a job's text capacity, fails a call to a macro
/// that reads none of it as with the whole environment (light-cljs-815):
/// the narrowed graph's omission record carries the string's units, which
/// Goose's import preflight counts, not the string (light-cljs-820).
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_environment_narrowing_preserves_goose_import_limits() {
    let probe = "a".repeat(1_048_577);
    let failure = |call: &str| {
        let mut files = environment_sources(0, &format!("(def x ({call}))"));
        files.get_mut("m.macros").unwrap().push_str(&format!(
            "\n(defmacro ignored {{:probe \"{probe}\"}} [] nil)"
        ));
        let session = GooseSession::start(&runner(), 1).unwrap();
        let (output, _, _) = pooled_build(&files, 1, Some(&session), Default::default());
        let error = output.unwrap_err();
        let stats = session.stats();
        session.close();
        cleaned(&session);
        (error, stats)
    };
    let (narrowed, narrowed_stats) = failure("m/blind");
    let (whole, whole_stats) = failure("m/counted");
    // The same failure of Goose's import, but for the macro's own name
    // and definition.
    let cause = |error: &str| {
        let (_, after) = error.split_once("macro m.macros/").unwrap();
        let (_, cause) = after.split_once(": ").unwrap();
        cause.split_once("; definition").unwrap().0.to_owned()
    };
    assert!(cause(&whole).contains("worker-abort"), "{whole}");
    assert_eq!(cause(&narrowed), cause(&whole));
    // Only the whole environment's call sent the metadata string.
    assert_eq!(narrowed_stats.calls, 1);
    assert!(narrowed_stats.argument_source_bytes < narrowed_stats.argument_bytes);
    assert!(narrowed_stats.argument_bytes < 10_000, "{narrowed_stats:?}");
    assert!(
        whole_stats.argument_bytes > 2 * 1_048_577,
        "{whole_stats:?}"
    );
}

/// The Goose build of `files` at one compiler slot: its JavaScript or its
/// error, and its session's statistics.
fn goose_build_at_one_slot(
    files: &HashMap<String, String>,
) -> (Result<String, String>, crate::goose_evaluator::SessionStats) {
    let session = GooseSession::start(&runner(), 1).unwrap();
    let (output, _, _) = pooled_build(files, 1, Some(&session), Default::default());
    let stats = session.stats();
    session.close();
    cleaned(&session);
    (output, stats)
}

/// A job given a narrowed environment has the heap headroom it would
/// have with the whole one (light-cljs-820): Goose reserves the rows of the
/// entries left out for the rest of the job. `fill` reads no `&env` and
/// allocates ten arrays 9,700 times when `n` is 1; `fill-counted` does the
/// same after reading all of `&env` with `count`. Their 97,000 arrays fit a job's
/// 100,000 heap nodes beside the call's own few hundred nodes, but not
/// beside an unused macro's metadata of 6,000 more: given the narrowed
/// environment, which carries the metadata only as an omission record,
/// `fill` fails as `fill-counted` given the whole environment fails. The
/// reservation ends with its job: twenty narrowed calls in one worker,
/// which together reserve 120,000 nodes, all succeed.
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_environment_narrowing_reserves_the_left_out_heap_capacity() {
    let files = |probe: usize, main: &str| {
        let mut files = environment_sources(0, main);
        let numbers = (0..probe).map(|n| n.to_string()).collect::<Vec<_>>();
        let allocate = "(loop [i 0 a nil] (if (< i (* n 9700)) (recur (inc i) #js [a #js [] #js [] #js [] #js [] #js [] #js [] #js [] #js [] #js []]) (* 10 i)))";
        files.get_mut("m.macros").unwrap().push_str(&format!(
            "\n(defmacro ignored {{:probe [{}]}} [] nil)
(defmacro fill [n] {allocate})
(defmacro fill-counted [n] (count &env) {allocate})",
            numbers.join(" ")
        ));
        files
    };
    let call = |name: &str| format!("(def x (m/{name} 1))");
    // Without the metadata both fit, and the narrowed call is small.
    for name in ["fill", "fill-counted"] {
        let (output, stats) = goose_build_at_one_slot(&files(0, &call(name)));
        let output = output.unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(tail(&output).contains("97000"), "{name}: {}", tail(&output));
        assert_eq!(stats.calls, 1, "{name}");
    }
    let (narrowed, narrowed_stats) = goose_build_at_one_slot(&files(6000, &call("fill")));
    let (whole, whole_stats) = goose_build_at_one_slot(&files(6000, &call("fill-counted")));
    let (narrowed, whole) = (narrowed.unwrap_err(), whole.unwrap_err());
    // The narrowed call sent a few hundred nodes, the whole one the
    // metadata too.
    assert!(narrowed_stats.argument_nodes < 1000, "{narrowed_stats:?}");
    assert!(
        whole_stats.argument_nodes > narrowed_stats.argument_nodes + 6000,
        "{whole_stats:?}"
    );
    // The same failure of the job, but for the macro's own name,
    // definition and code position (`fill-counted` has one more call).
    let cause = |error: &str| {
        let (_, after) = error.split_once("macro m.macros/").unwrap();
        let cause = after.split_once(": ").unwrap().1;
        let cause = cause.split("; definition").next().unwrap();
        cause
            .split(' ')
            .filter(|field| !field.starts_with("pc=") && !field.starts_with("emitted-span="))
            .collect::<Vec<_>>()
            .join(" ")
    };
    assert!(cause(&whole).contains("MemoryLimit"), "{whole}");
    assert_eq!(cause(&narrowed), cause(&whole));
    let calls = vec!["(m/fill 0)"; 20].join(" ");
    let (output, stats) = goose_build_at_one_slot(&files(6000, &format!("(def x [{calls}])")));
    output.unwrap();
    assert_eq!(stats.calls, 20);
}

/// The fixture of [`goose_compiler_pins_per_call_transport_volume`]:
/// `m.macros` declares 120 filler macros, so a call given the whole `&env`
/// carries an inventory of more than 120 macros, and the source macros
/// `blind` (reads no `&env`), `or-local` (reads only `(:locals &env)`),
/// `whole` (reads all of `&env` with `count`, and counts its macro
/// inventory), `data` and `call` (which demand the helper roots `shared`
/// and `wrap`); `app.a`, `app.b` and `app.c` call them, and `app.main`
/// requires those three.
fn transport_volume_sources() -> HashMap<String, String> {
    let mut macros = String::from("(ns m.macros)\n");
    for n in 0..120 {
        macros.push_str(&format!("(defmacro filler{n} [x] x)\n"));
    }
    macros.push_str(
        "(defn wrap [form] (list 'quote form))
         (def shared [11 22])
         (defmacro blind [] 42)
         (defmacro or-local [sym fallback] (if (contains? (:locals &env) sym) sym fallback))
         (defmacro whole [] [(count &env) (count (:light-cljs/macros &env))])
         (defmacro data [n] (list 'quote [n shared]))
         (defmacro call [n] (wrap [n n]))",
    );
    let mut files = HashMap::from([("m.macros".to_owned(), macros)]);
    for (space, offset) in [("a", 0), ("b", 100), ("c", 200)] {
        let mut body = format!("(ns app.{space} (:require [m.macros :as m]))\n");
        for i in 0..4 {
            let n = offset + i;
            body.push_str(&format!(
                "(def v{i} (let [x {n}] [(m/blind) (m/or-local x 1) (m/or-local y 2) (m/data {n}) (m/call {n})]))\n"
            ));
        }
        body.push_str("(def w (m/whole))\n");
        files.insert(format!("app.{space}"), body);
    }
    files.insert(
        "app.main".to_owned(),
        "(ns app.main (:require [app.a :as a] [app.b :as b] [app.c :as c]))
(js/console.log (pr-str [a/v0 b/v3 c/v1 a/w b/w c/w]))"
            .to_owned(),
    );
    files
}

/// The per-call transport volume the three transport changes reached is
/// pinned (light-cljs-818, after 815, 816 and 817). A fixed multi-namespace
/// build at one compiler slot and Goose worker sends exactly the argument
/// nodes and bytes, and receives exactly the result bytes, pinned below (at
/// four, the same calls and results and a few more nodes); its argument
/// graphs carry no source text
/// (sources travel as identities, 817); the session directory never holds a
/// file but the base image and the definition exports (calls travel in the
/// pipes, 816); the 60 calls that read no `&env` or only its `:locals` are
/// sent the narrowed environment, without the 120-macro inventory (815);
/// and the three calls that read all of `&env` still receive the whole
/// environment, inventory included. Counts only, no timing.
#[test]
#[ignore = "build native runner with compiler.bb"]
fn goose_compiler_pins_per_call_transport_volume() {
    let files = transport_volume_sources();
    let mut pinned = Vec::new();
    for slots in [1, 4] {
        let session = GooseSession::start(&runner(), slots).unwrap();
        let (output, _, census) = pooled_build(&files, slots, Some(&session), Default::default());
        let output = output.unwrap_or_else(|e| panic!("slots={slots}: {e} {:?}", session.trace()));
        assert!(census.is_empty(), "slots={slots}: {census:?}");
        let stats = session.stats();
        let trace = session.trace();
        session.close();
        cleaned(&session);
        #[cfg(light_v8)]
        {
            let (reference, _, _) = pooled_build(&files, 1, None, Default::default());
            assert_eq!(Ok(output.as_str()), reference.as_deref(), "slots={slots}");
        }
        // Each `whole` call got all eight entries of `&env`, the inventory
        // of m.macros's 125 macros among them.
        assert_eq!(
            output.matches("rt.vectorFromArray([8, 125])").count(),
            3,
            "slots={slots}: {output}"
        );
        assert!(
            trace.iter().filter(|s| s.starts_with("NEED ")).count() >= 2,
            "the data and call macros demand their roots: {trace:?}"
        );
        // With per-call files (816 reverted) other names are counted here,
        // even ones removed at once (light-cljs-822).
        assert_only_definition_files(&session, stats.definitions, &format!("slots={slots}"));
        // With source text in the arguments (817 reverted) this is not zero.
        assert_eq!(stats.argument_source_bytes, 0, "slots={slots}: {stats:?}");
        pinned.push((
            stats.calls,
            stats.argument_nodes,
            stats.argument_bytes,
            stats.result_bytes,
        ));
    }
    // 21 calls in each of three namespaces. With the whole environment
    // sent to every call (815 reverted), the nodes and bytes grow by the
    // 120-macro inventory about 60 times over.
    // The 60 narrowed graphs carry a 24-byte omission record each
    // (light-cljs-820).
    assert_eq!(pinned[0], (63, 8337, 516_648, 12_288), "{pinned:?}");
    // At four slots a build's graphs held 12 or 15 more nodes (56 encoded
    // bytes each) than at one, from run to run; the calls and results are
    // the same.
    let (calls, nodes, bytes, results) = pinned[1];
    assert_eq!((calls, results), (63, 12_288), "{pinned:?}");
    assert!(
        (8337..=8337 + 32).contains(&nodes) && (516_648..=516_648 + 32 * 56).contains(&bytes),
        "{pinned:?}"
    );
}
