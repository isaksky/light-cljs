//! Bounded final experiment. Invoked only by benchmark.bb, never ordinary tests.
use super::*;
use crate::evaluator::{EvaluatorOptions, MacroEvaluator};
use std::time::Instant;

#[cfg(feature = "count-allocations")]
#[path = "../../../../benches/support/allocations.rs"]
mod allocations;
#[cfg(feature = "count-allocations")]
#[global_allocator]
static ALLOCATOR: allocations::CountingAllocator = allocations::CountingAllocator;

pub(super) fn compare(a: &Form, b: &Form) {
    assert!(a.data().value_eq(&b.data()));
    let (mut sa, mut sb, mut da, mut db) = (
        Sources::default(),
        Sources::default(),
        Data::default(),
        Data::default(),
    );
    assert_eq!(forms(a, &mut sa, &mut da), forms(b, &mut sb, &mut db));
    assert_eq!(da.text(), db.text());
    assert_eq!(sa.text(), sb.text());
    compare_origins(a, b);
}
pub(super) fn compare_origins(a: &Form, b: &Form) {
    assert_eq!(a.expansions().len(), b.expansions().len());
    let span = |s: &crate::compiler_data::SourceSpan| {
        (
            s.source().name().to_owned(),
            s.source().text().to_owned(),
            s.bytes(),
        )
    };
    for (a, b) in a.expansions().iter().zip(b.expansions()) {
        assert_eq!(a.macro_name, b.macro_name);
        assert_eq!(span(&a.call), span(&b.call));
        assert_eq!(
            a.definition.as_ref().map(span),
            b.definition.as_ref().map(span)
        );
        assert_eq!(a.helpers, b.helpers);
    }
}

#[test]
fn complete_form_comparison_rejects_scalar_changes() {
    use crate::compiler_data::{DatumKind, FormKind, FormNode, Origin, Source};
    let source = Source::new("scalar.cljs", "1");
    let make = |datum| {
        Form::from_parts(
            vec![FormNode {
                kind: FormKind::Scalar,
                metadata: None,
                compiler_payload: None,
                origin: Origin::Source,
            }],
            vec![datum],
            vec![source.span(0..1).unwrap()],
            crate::compiler_data::FormId(0),
        )
    };
    let integer = make(Datum::new(DatumKind::Integer(1)));
    let double = make(Datum::new(DatumKind::Double(1.0)));
    let metadata =
        make(Datum::new(DatumKind::Integer(1)).with_metadata(Datum::new(DatumKind::Boolean(true))));
    compare(&integer, &integer);
    assert!(std::panic::catch_unwind(|| compare(&integer, &double)).is_err());
    assert!(std::panic::catch_unwind(|| compare(&integer, &metadata)).is_err());
}

#[test]
fn complete_form_comparison_rejects_expansion_changes() {
    use crate::compiler_data::{DatumKind, Origin, Source};
    let source = Source::new("origin.cljs", "12");
    let input = Form::from_data(
        Datum::new(DatumKind::Integer(1)),
        source.span(0..1).unwrap(),
        Origin::Source,
    );
    let origin = ExpansionOrigin {
        macro_name: Name::new(Some("fixture"), "macro"),
        call: input.span(),
        definition: Some(source.span(0..1).unwrap()),
        helpers: vec![Name::new(Some("fixture"), "helper")],
    };
    let make = |origin| {
        Form::from_bridge_expansion(
            input.data(),
            &input,
            origin,
            &std::collections::HashMap::<usize, Form>::new(),
        )
    };
    let expected = make(origin.clone());
    compare(&expected, &make(origin.clone()));
    let mut changed = origin.clone();
    changed.definition = Some(source.span(1..2).unwrap());
    assert!(std::panic::catch_unwind(|| compare(&expected, &make(changed))).is_err());
    let mut changed = origin;
    changed.helpers.clear();
    assert!(std::panic::catch_unwind(|| compare(&expected, &make(changed))).is_err());
}

struct BatchClock {
    previous: Instant,
    expected: Vec<usize>,
    completed: Vec<usize>,
    rows: String,
}
static BATCH_CLOCK: std::sync::Mutex<Option<BatchClock>> = std::sync::Mutex::new(None);

// The existing test-only expansion hook observes real compiler calls, including
// calls on worker threads. Namespace dependencies order the two batches.
pub(super) fn record_batch(call: &Form, origin: &ExpansionOrigin) {
    if origin.macro_name.name() != "work".encode_utf16().collect::<Vec<_>>() {
        return;
    }
    let span = call.span();
    let name = span.source().name();
    let batch = if name.contains("wave0_") {
        0
    } else if name.ends_with("join.cljs") {
        1
    } else if name.contains("wave1_") {
        2
    } else if name.ends_with("batch0.cljs") {
        0
    } else if name.ends_with("batch1.cljs") {
        1
    } else {
        return;
    };
    let mut clock = BATCH_CLOCK.lock().unwrap();
    if let Some(c) = clock.as_mut() {
        c.completed[batch] += 1;
        if c.completed[batch] == c.expected[batch] {
            assert!((0..batch).all(|i| c.completed[i] == c.expected[i]));
            let now = Instant::now();
            c.rows.push_str(&format!(
                "{{:phase :batch :batch {batch} :wall-ns {} :checked {}}}\n",
                now.duration_since(c.previous).as_nanos(),
                c.expected[batch]
            ));
            c.previous = now;
        }
    }
}
fn evaluator(
    engine: &str,
    options: &EvaluatorOptions,
    pants: &Option<crate::bbp_evaluator::BbpRuntime>,
    goose: &Option<std::sync::Arc<crate::goose_evaluator::GooseSession>>,
) -> Box<dyn MacroEvaluator> {
    match engine {
        "v8" => Box::new(crate::v8_evaluator::V8Evaluator::new(options.clone()).unwrap()),
        "pants" => Box::new(pants.as_ref().unwrap().evaluator().unwrap()),
        "goose" => Box::new(
            crate::goose_evaluator::GooseEvaluator::new(goose.as_ref().unwrap().clone(), options)
                .unwrap(),
        ),
        _ => panic!("unknown engine"),
    }
}
fn invoke(
    e: &dyn MacroEvaluator,
    function: crate::evaluator::ValueId,
    request: &Request,
) -> (Form, [u128; 4]) {
    let limits = Default::default();
    let begin = Instant::now();
    let form = e.import_macro_form(&request.call, limits).unwrap();
    let env = e.import_environment(&request.environment, limits).unwrap();
    let mut params = vec![form, env];
    params.extend(e.macro_arguments(form, &request.call, limits).unwrap());
    let imported = begin.elapsed().as_nanos();
    let begin = Instant::now();
    let value = e.call(function, &params).unwrap();
    let invoked = begin.elapsed().as_nanos();
    let begin = Instant::now();
    let output = e
        .export_form(value, &request.call, request.origin.clone(), limits)
        .unwrap();
    let exported = begin.elapsed().as_nanos();
    let begin = Instant::now();
    for id in params.into_iter().chain([value]) {
        e.release(id).unwrap();
    }
    (
        output,
        [imported, invoked, exported, begin.elapsed().as_nanos()],
    )
}
fn stats(session: &crate::goose_evaluator::GooseSession) -> String {
    let s = session.stats();
    format!(
        "{{:subprocesses {} :initializations {} :jobs {} :exports {} :definitions {} :preparations {} :export-ns {} :bridge-bytes {} :spawn-ns {} :rust-bridge-ns {} :base-load-ns {} :boot-ns {} :snapshot-ns {} :job-load-ns {} :goose-bridge-ns {} :trace {:?}}}",
        s.subprocesses,
        s.initializations,
        s.jobs,
        s.exports,
        s.definitions,
        s.preparations,
        s.export_ns,
        s.bridge_bytes,
        s.spawn_ns,
        s.rust_bridge_ns,
        s.goose_base_load_ns,
        s.goose_boot_ns,
        s.goose_snapshot_ns,
        s.goose_job_load_ns,
        s.goose_bridge_ns,
        session.trace()
    )
}
fn compile(
    directory: &Path,
    engine: &str,
    workers: usize,
    options: &EvaluatorOptions,
    runner: &Path,
    rows: &mut String,
    expected_jobs: usize,
) -> String {
    let setup = Instant::now();
    let mut host = MacroHost::new();
    host.evaluator_options = options.clone();
    if engine == "pants" {
        host.use_native_macros();
    }
    let session = (engine == "goose")
        .then(|| crate::goose_evaluator::GooseSession::start(runner, workers).unwrap());
    if let Some(s) = &session {
        host.use_goose_source_macros(s.clone()).unwrap();
    }
    rows.push_str(&format!(
        "{{:phase :runtime :ns {}}}\n",
        setup.elapsed().as_nanos()
    ));
    let instrumented = std::env::var_os("GOOSE_BENCH_INSTRUMENT").is_some();
    if instrumented {
        crate::wait_timeline::start_from_environment();
    }
    let begun = Instant::now();
    let (program, _) = crate::compiler::analyze_namespace_entry_recorded(
        "main",
        &[directory.to_path_buf()],
        &mut host,
        Default::default(),
        &mut Vec::new(),
        crate::compiler::EntryRecorders {
            slots: crate::scheduler::Slots::new(workers).unwrap(),
            ..Default::default()
        },
    )
    .unwrap();
    let analyzed = begun.elapsed().as_nanos();
    let begun = Instant::now();
    let js = crate::emit_js(&program);
    let emitted = begun.elapsed().as_nanos();
    if let Some(s) = &session {
        assert_eq!(s.stats().initializations, 1);
        assert_eq!(s.stats().jobs as usize, expected_jobs);
        assert_eq!(s.stats().definitions, 2, "one macro plus shared helper");
        assert_eq!(s.stats().exports, 2);
        assert_eq!(s.stats().preparations, 1);
        assert_eq!(
            s.trace()
                .iter()
                .filter(|line| line.starts_with("PRODUCED "))
                .count(),
            1,
            "one compiler-owned shared helper producer per fresh compilation"
        );
        rows.push_str(&format!("{{:phase :adapter :stats {}}}\n", stats(s)));
    }
    let begun = Instant::now();
    if let Some(s) = &session {
        s.close();
    }
    let closed = begun.elapsed().as_nanos();
    if let Some(s) = &session {
        rows.push_str(&format!("{{:phase :adapter-closed :stats {}}}\n", stats(s)));
    }
    if instrumented {
        crate::wait_timeline::finish().unwrap();
    }
    drop(host);
    drop(program);
    rows.push_str(&format!(
        "{{:phase :compiler :analyze-ns {analyzed} :emit-ns {emitted} :session-close-ns {closed} :cleanup-ns {}}}\n",
        begun.elapsed().as_nanos()
    ));
    js
}
fn run() {
    let directory = PathBuf::from(std::env::var_os("GOOSE_BENCH_DIRECTORY").unwrap());
    let engine = std::env::var("GOOSE_BENCH_ENGINE").unwrap();
    let scope = std::env::var("GOOSE_BENCH_SCOPE").unwrap();
    let control = std::env::var("GOOSE_BENCH_CONTROL").unwrap();
    let workers: usize = std::env::var("GOOSE_BENCH_WORKERS")
        .unwrap()
        .parse()
        .unwrap();
    assert!([1, 2, 4, 8].contains(&workers));
    assert!(engine != "v8" || workers == 1);
    let runner = PathBuf::from(std::env::var_os("GOOSE_COMPILER_RUNNER").unwrap());
    let source = std::fs::read_to_string(directory.join("input.cljs")).unwrap();
    if engine == "storage" {
        let base = big_boy_pants_js::frontend::goose::export_source("goose-base.js", "rt => null")
            .unwrap();
        std::fs::write(directory.join("base.gsp"), base.bytes).unwrap();
        let result = std::process::Command::new(&runner)
            .arg(&directory)
            .args(["1", "profile"])
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        std::fs::write(directory.join("storage.edn"), result.stdout).unwrap();
        return;
    }
    let options = EvaluatorOptions {
        memory_limit: 1024 << 20,
        timeout: std::time::Duration::from_secs(120),
        ..Default::default()
    };
    let overall = Instant::now();
    let _names = crate::generated_names::enter_namespace(0);
    let mut rows = String::new();
    if scope == "compiler" {
        let fanout: usize = std::env::var("GOOSE_BENCH_FANOUT")
            .unwrap()
            .parse()
            .unwrap();
        let mut js = String::new();
        if control == "rebuild" {
            for batch in 0..2 {
                let begun = Instant::now();
                for index in 0..fanout {
                    let job = directory.join(format!("job-{index}"));
                    let result = compile(&job, &engine, 1, &options, &runner, &mut rows, 1);
                    js.push_str(&format!("(function(){{\n{result}\n}})();\n"));
                }
                rows.push_str(&format!(
                    "{{:phase :batch :batch {batch} :wall-ns {} :checked {fanout}}}\n",
                    begun.elapsed().as_nanos()
                ));
            }
        } else {
            let expected = std::env::var("GOOSE_BENCH_PHASE_COUNTS")
                .map(|v| {
                    v.split(',')
                        .map(|s| s.parse::<usize>().unwrap())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_else(|_| vec![fanout; 2]);
            *BATCH_CLOCK.lock().unwrap() = Some(BatchClock {
                previous: overall,
                expected: expected.clone(),
                completed: vec![0; expected.len()],
                rows: String::new(),
            });
            js = compile(
                &directory,
                &engine,
                workers,
                &options,
                &runner,
                &mut rows,
                expected.iter().sum(),
            );
            let clock = BATCH_CLOCK.lock().unwrap().take().unwrap();
            assert_eq!(clock.completed, expected);
            rows.push_str(&clock.rows);
        }
        std::fs::write(directory.join("actual.js"), js).unwrap();
    } else {
        assert_eq!(scope, "vm");
        // Fresh actual compiler emission/independent expected forms each process.
        // Preparation is included in process/overall wall, separately identified.
        let begun = Instant::now();
        let mut host = MacroHost::new();
        REQUESTS.with_borrow_mut(|r| *r = Some(Vec::new()));
        crate::analyzer::analyze_named_with_macro_warnings(
            "input.cljs",
            &source,
            &crate::parse(&source).unwrap(),
            &mut host,
            Default::default(),
            &mut Vec::new(),
        )
        .unwrap();
        let requests = REQUESTS
            .with_borrow_mut(|r| r.take().unwrap())
            .into_iter()
            .filter(|r| {
                r.origin
                    .macro_name
                    .namespace()
                    .is_some_and(|ns| ns == "cljs.user".encode_utf16().collect::<Vec<_>>())
            })
            .collect::<Vec<_>>();
        assert!(!requests.is_empty());
        // The Pants/Goose build emits native form intrinsics; V8 emits the
        // canonical JS runtime calls. Keep the independent V8 expected forms.
        let factory = {
            drop(host);
            host = MacroHost::new();
            if engine != "v8" {
                host.use_native_macros();
            }
            REQUESTS.with_borrow_mut(|r| *r = Some(Vec::new()));
            crate::analyzer::analyze_named_with_macro_warnings(
                "input.cljs",
                &source,
                &crate::parse(&source).unwrap(),
                &mut host,
                Default::default(),
                &mut Vec::new(),
            )
            .unwrap();
            let native = REQUESTS
                .with_borrow_mut(|r| r.take().unwrap())
                .into_iter()
                .filter(|r| {
                    r.origin
                        .macro_name
                        .namespace()
                        .is_some_and(|ns| ns == "cljs.user".encode_utf16().collect::<Vec<_>>())
                })
                .collect::<Vec<_>>();
            assert_eq!(native.len(), requests.len());
            for (a, b) in native.iter().zip(&requests) {
                compare(&a.expanded, &b.expanded);
            }
            let MacroFunction::Ready(_, Some(code)) =
                &host.tables.javascript_macros["cljs.user/work"].function
            else {
                panic!("native source factory required")
            };
            format!("rt => rt.core.macroReaderFunction(({})(rt))", code.unit)
        };
        drop(host);
        rows.push_str(&format!(
            "{{:phase :prepare :ns {} :jobs {} :source-bytes {}}}\n",
            begun.elapsed().as_nanos(),
            requests.len(),
            source.len()
        ));
        let setup = Instant::now();
        let pants = (engine == "pants" && control != "rebuild")
            .then(|| crate::bbp_evaluator::BbpRuntime::new(&options).unwrap());
        let goose = (engine == "goose" && control != "rebuild")
            .then(|| crate::goose_evaluator::GooseSession::start(&runner, workers).unwrap());
        let serial = control == "serial" || (engine == "v8" && control != "rebuild");
        let retained = serial.then(|| evaluator(&engine, &options, &pants, &goose));
        let retained_function = retained
            .as_ref()
            .map(|e| e.evaluate_unit(&factory).unwrap());
        rows.push_str(&format!(
            "{{:phase :runtime :ns {}}}\n",
            setup.elapsed().as_nanos()
        ));
        // Batch 0 is cold; batch 1 uses the same runtime/session. External warmup
        // is a whole separate process. All result/handle cleanup is included.
        for batch in 0..2 {
            let begun = Instant::now();
            let results = if let (Some(e), Some(f)) = (&retained, retained_function) {
                requests
                    .iter()
                    .enumerate()
                    .map(|(i, r)| {
                        let (form, cost) = invoke(e.as_ref(), f, r);
                        (i, form, cost)
                    })
                    .collect::<Vec<_>>()
            } else {
                std::thread::scope(|scope| {
                    let handles =
                        (0..workers)
                            .map(|worker| {
                                let (
                                    engine,
                                    control,
                                    factory,
                                    requests,
                                    pants,
                                    goose,
                                    options,
                                    runner,
                                ) = (
                                    &engine, &control, &factory, &requests, &pants, &goose,
                                    &options, &runner,
                                );
                                scope.spawn(move || {
                                    let mut output = Vec::new();
                                    let serial = control == "serial"
                                        || (engine == "v8" && control != "rebuild");
                                    let retained =
                                        serial.then(|| evaluator(engine, options, pants, goose));
                                    let function = retained
                                        .as_ref()
                                        .map(|e| e.evaluate_unit(factory).unwrap());
                                    for (index, request) in requests
                                        .iter()
                                        .enumerate()
                                        .filter(|(i, _)| i % workers == worker)
                                    {
                                        let fresh_pants = (engine == "pants"
                                            && control == "rebuild")
                                            .then(|| {
                                                crate::bbp_evaluator::BbpRuntime::new(options)
                                                    .unwrap()
                                            });
                                        let fresh_goose = (engine == "goose"
                                            && control == "rebuild")
                                            .then(|| {
                                                crate::goose_evaluator::GooseSession::start(
                                                    runner, 1,
                                                )
                                                .unwrap()
                                            });
                                        let task = (!serial).then(|| {
                                            evaluator(
                                                engine,
                                                options,
                                                if fresh_pants.is_some() {
                                                    &fresh_pants
                                                } else {
                                                    pants
                                                },
                                                if fresh_goose.is_some() {
                                                    &fresh_goose
                                                } else {
                                                    goose
                                                },
                                            )
                                        });
                                        let e = retained.as_ref().or(task.as_ref()).unwrap();
                                        let f = function
                                            .unwrap_or_else(|| e.evaluate_unit(factory).unwrap());
                                        let (form, costs) = invoke(e.as_ref(), f, request);
                                        if !serial {
                                            e.release(f).unwrap();
                                        }
                                        drop(task);
                                        if let Some(s) = fresh_goose {
                                            #[cfg(feature = "count-allocations")]
                                            {
                                                let destination = PathBuf::from(
                                                    std::env::var_os("GOOSE_BENCH_DIRECTORY")
                                                        .unwrap(),
                                                );
                                                std::fs::write(
                                                    destination.join(format!(
                                                        "rebuild-{batch}-{index}.edn"
                                                    )),
                                                    stats(&s),
                                                )
                                                .unwrap();
                                            }
                                            s.close();
                                        }
                                        output.push((index, form, costs));
                                    }
                                    if let (Some(e), Some(f)) = (&retained, function) {
                                        e.release(f).unwrap();
                                    }
                                    output
                                })
                            })
                            .collect::<Vec<_>>();
                    handles
                        .into_iter()
                        .flat_map(|h| h.join().unwrap())
                        .collect::<Vec<_>>()
                })
            };
            let wall = begun.elapsed().as_nanos();
            let mut costs = [0u128; 4];
            for (index, form, c) in results {
                compare(&requests[index].expanded, &form);
                for i in 0..4 {
                    costs[i] += c[i];
                }
            }
            rows.push_str(&format!("{{:phase :batch :batch {batch} :wall-ns {wall} :import-sum-ns {} :invoke-sum-ns {} :export-sum-ns {} :release-sum-ns {} :checked {}}}\n",costs[0],costs[1],costs[2],costs[3],requests.len()));
        }
        let evaluator_cleanup = Instant::now();
        if let (Some(e), Some(f)) = (&retained, retained_function) {
            e.release(f).unwrap();
        }
        drop(retained);
        rows.push_str(&format!(
            "{{:phase :evaluator-dispose :ns {}}}\n",
            evaluator_cleanup.elapsed().as_nanos()
        ));
        if let Some(s) = &goose {
            assert_eq!(s.stats().initializations, 1);
            rows.push_str(&format!("{{:phase :adapter :stats {}}}\n", stats(s)));
        }
        #[cfg(feature = "count-allocations")]
        if let Some(p) = &pants {
            let c = p.helper_counts();
            let h = c.heap.unwrap();
            rows.push_str(&format!("{{:phase :pants-storage :managed-bytes {} :peak-managed-bytes {} :admitted-bytes {} :contexts {} :steps {} :waits {}}}\n",h.managed_bytes,h.peak_managed_bytes,h.admitted_bytes,c.contexts,c.steps,c.waits));
        }
        let begun = Instant::now();
        if let Some(s) = goose {
            s.close();
        }
        drop(pants);
        drop(requests);
        drop(factory);
        rows.push_str(&format!(
            "{{:phase :dispose :ns {}}}\n",
            begun.elapsed().as_nanos()
        ));
    }
    rows.push_str(&format!(
        "{{:phase :overall :ns {}}}\n",
        overall.elapsed().as_nanos()
    ));
    std::fs::write(directory.join("rows.edn"), rows).unwrap();
}
#[test]
#[ignore = "quiet final experiment: benchmark.bb"]
fn final_benchmark() {
    #[cfg(feature = "count-allocations")]
    {
        let (_, allocs, reallocs, bytes) = allocations::measure(run);
        let directory = PathBuf::from(std::env::var_os("GOOSE_BENCH_DIRECTORY").unwrap());
        std::fs::write(
            directory.join("allocations.edn"),
            format!(
                "{{:allocations {allocs} :reallocations {reallocs} :requested-bytes {bytes}}}\n"
            ),
        )
        .unwrap();
    }
    #[cfg(not(feature = "count-allocations"))]
    run();
}
