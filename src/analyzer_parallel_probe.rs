//! Manual scaling probe, not a production namespace scheduler. Independent
//! namespace IR views share the compilation catalog and native runtime. The
//! fixture has disjoint declarations and fixed macro providers; it does not
//! establish arbitrary registration/extension visibility or failure ordering.
use super::*;
use std::sync::{
    Arc, Barrier, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use std::time::{Duration, Instant};

struct Seed {
    program: Program,
    globals: crate::name_index::NameIndex,
    roots: Vec<ExprId>,
    requires: Vec<Vec<String>>,
    record_layouts: RecordLayouts,
}

#[derive(PartialEq)]
struct Published {
    code: String,
    mapping: crate::source_map::SourceMapping,
    advanced: String,
    advanced_mapping: crate::source_map::SourceMapping,
}

fn publish(program: &Program) -> Published {
    use crate::application::{ApplicationOptions, OptimizationLevel, prepare_application};
    let (code, mapping) = prepare_application(program, Default::default()).emit_js_mapped();
    let (advanced, advanced_mapping) = prepare_application(
        program,
        ApplicationOptions::new(OptimizationLevel::Advanced),
    )
    .emit_js_mapped();
    Published {
        code,
        mapping,
        advanced,
        advanced_mapping,
    }
}

fn source(analysis: &mut NamespaceAnalysis, host: &mut MacroHost, name: &str, text: &str) {
    let syntax = crate::parse(text).unwrap();
    let mut warnings = Vec::new();
    analysis
        .analyze(
            &NamespaceInput {
                name,
                source: text,
                syntax: &syntax,
                aliases: &[],
                requires: &[],
            },
            Default::default(),
            false,
            host,
            Default::default(),
            &mut warnings,
        )
        .unwrap();
    analysis.merge_all(host, &mut warnings).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
}

fn providers(ordered: bool, stateful: bool) -> (MacroHost, NamespaceAnalysis) {
    let mut host = MacroHost::new();
    host.use_native_macros();
    // Same limits as research/cljs-200k/light-cljs.edn. This probe compares
    // scheduling of the same fixture, not a different-budget compiler.
    host.limits.expansions = 10_000_000;
    host.limits.generated_nodes = 1_000_000_000;
    host.evaluator_options.timeout = Duration::from_secs(3600);
    host.evaluator_options.memory_limit = 2_147_483_648;
    host.evaluator_options.poll_budget = 100_000_000_000;
    host.evaluator_options.macro_core = crate::evaluator::MacroCore::Port;
    let mut analysis = NamespaceAnalysis::new(&mut host);
    if ordered {
        host.session_handle().unwrap().shared().lanes().enable();
    }
    for (name, path) in [
        (
            "workload.common",
            "research/cljs-200k/src/workload/common.cljs",
        ),
        ("workload.m0", "research/cljs-200k/macros/workload/m0.cljc"),
    ] {
        if ordered {
            host.session_handle()
                .unwrap()
                .shared()
                .lanes()
                .register_namespace(analysis.roots.len(), name, &[]);
        }
        let mut text = std::fs::read_to_string(path).unwrap();
        if stateful && name == "workload.m0" {
            text.push_str("\n(defonce probe-counter (atom 0))\n(defmacro probe-stamp [] (swap! probe-counter inc))\n");
        }
        source(&mut analysis, &mut host, name, &text);
    }
    (host, analysis)
}

fn analyze_worker(
    analysis: &mut NamespaceAnalysis,
    host: &mut MacroHost,
    id: usize,
    fixture: &str,
    stateful: bool,
) {
    let name = format!("parallel.probe{id}");
    let entry = if stateful {
        "[(m/probe-stamp) (run 42)]"
    } else {
        "(run 42)"
    };
    let declarations = if std::env::var_os("LIGHT_NAMESPACE_PROBE_CONSUMER").is_some() {
        format!("(defmacro published [x] (list '+ x (bridge {id} 3)))\n")
    } else {
        String::new()
    };
    let text = format!(
        "{}\n{declarations}{entry}\n",
        fixture.replace("workload.n0", &name)
    );
    let syntax = crate::parse(&text).unwrap();
    let requires: Vec<String> = vec!["workload.common".into(), "workload.m0".into()];
    let aliases: Vec<(String, String)> = vec![
        ("c".into(), requires[0].clone()),
        ("m".into(), requires[1].clone()),
    ];
    host.set_imports(
        &name,
        aliases
            .iter()
            .map(|(alias, namespace)| crate::macro_host::MacroImport {
                namespace: namespace.clone(),
                alias: Some(alias.clone()),
                refers: vec![],
            })
            .collect(),
    )
    .unwrap();
    let mut warnings = Vec::new();
    analysis
        .analyze(
            &NamespaceInput {
                name: &name,
                source: &text,
                syntax: &syntax,
                aliases: &aliases,
                requires: &requires,
            },
            Default::default(),
            true,
            host,
            Default::default(),
            &mut warnings,
        )
        .unwrap();
    analysis.merge_all(host, &mut warnings).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
}

fn finish_values(analysis: NamespaceAnalysis, host: &MacroHost, prefix_sources: usize) -> Program {
    let entry = analysis.roots.len() - 1;
    let prefix_roots = analysis.roots[..prefix_sources].to_vec();
    let values = analysis.roots[prefix_sources..].to_vec();
    let mut program = analysis.finish(entry, host).unwrap();
    let vector = ExprId(program.exprs.len());
    program.exprs.push(Expr {
        kind: ExprKind::Vector(values),
        span: 0..0,
        context: ExprContext::Return,
        source_index: entry,
    });
    let mut roots = prefix_roots;
    roots.push(vector);
    program.exprs[program.root.0].kind = ExprKind::Do(roots);
    program
}

fn analyze_consumer(
    analysis: &mut NamespaceAnalysis,
    host: &mut MacroHost,
    count: usize,
    ordered: bool,
    stateful: bool,
) {
    let name = "parallel.consumer";
    let mut requires: Vec<_> = (0..count).map(|id| format!("parallel.probe{id}")).collect();
    requires.push("workload.m0".into());
    let mut aliases: Vec<_> = (0..count)
        .map(|id| (format!("n{id}"), requires[id].clone()))
        .collect();
    aliases.push(("m".into(), "workload.m0".into()));
    host.set_imports(
        name,
        aliases
            .iter()
            .map(|(alias, namespace)| crate::macro_host::MacroImport {
                namespace: namespace.clone(),
                alias: Some(alias.clone()),
                refers: vec![],
            })
            .collect(),
    )
    .unwrap();
    if ordered {
        host.session_handle()
            .unwrap()
            .shared()
            .lanes()
            .register_namespace(analysis.roots.len(), name, &requires);
    }
    let calls = (0..count)
        .map(|id| {
            format!("(n{id}/bridge 42 3) (n{id}/probe 42) (n{id}/score (n{id}/->Weighted 3 2) 7) (n{id}/published 100)")
        })
        .collect::<Vec<_>>();
    // Keep each synthetic macro within the ordinary engine's unit-size
    // limit while still demanding every writer in compile-time execution.
    let declarations = calls
        .chunks(8)
        .enumerate()
        .map(|(id, chunk)| format!("(defmacro compiled{id} [] (+ {}))", chunk.join(" ")))
        .collect::<Vec<_>>()
        .join("\n");
    let compiled = (0..calls.len().div_ceil(8))
        .map(|id| format!("(compiled{id})"))
        .collect::<Vec<_>>()
        .join(" ");
    let calls = calls.join(" ");
    let kernels = (0..count)
        .map(|id| format!("(n{id}/step0 42)"))
        .collect::<Vec<_>>()
        .join(" ");
    let imports = aliases
        .iter()
        .map(|(alias, namespace)| format!("[{namespace} :as {alias}]"))
        .collect::<Vec<_>>()
        .join(" ");
    let stamp = if stateful { "(m/probe-stamp) " } else { "" };
    let text = format!(
        "(ns {name} (:require {imports}))\n{declarations}\n(defn runtime-summary [] (+ {calls} {kernels}))\n[{stamp}(+ {compiled}) (runtime-summary)]\n"
    );
    let syntax = crate::parse(&text).unwrap();
    let mut warnings = Vec::new();
    analysis
        .analyze(
            &NamespaceInput {
                name,
                source: &text,
                syntax: &syntax,
                aliases: &aliases,
                requires: &requires,
            },
            Default::default(),
            true,
            host,
            Default::default(),
            &mut warnings,
        )
        .unwrap();
    analysis.merge_all(host, &mut warnings).unwrap();
    assert!(warnings.is_empty(), "consumer warnings: {warnings:?}");
}

fn serial_reference(count: usize, fixture: &str, ordered: bool, stateful: bool) -> Published {
    let (mut host, mut analysis) = providers(ordered, stateful);
    let prefix_sources = analysis.roots.len();
    for id in 0..count {
        if ordered {
            host.session_handle()
                .unwrap()
                .shared()
                .lanes()
                .register_namespace(
                    prefix_sources + id,
                    &format!("parallel.probe{id}"),
                    &["workload.common".into(), "workload.m0".into()],
                );
        }
        analyze_worker(&mut analysis, &mut host, id, fixture, stateful);
    }
    if std::env::var_os("LIGHT_NAMESPACE_PROBE_CONSUMER").is_some() {
        analyze_consumer(&mut analysis, &mut host, count, ordered, stateful);
    }
    if std::env::var_os("LIGHT_NAMESPACE_PROBE_VISIBILITY").is_some() {
        verify_native_catalog(&host, count);
    }
    publish(&finish_values(analysis, &host, prefix_sources))
}

fn verify_native_catalog(host: &MacroHost, count: usize) {
    let engine = &host.engine.bootstrap.as_ref().unwrap().evaluator;
    for id in 0..count {
        // Full application execution is checked in Node. These demand a
        // normal multi-arity function and a macro-generated function from
        // the published native catalog, including their provider roots.
        // The latter's expected result includes defkernel's 31*x term.
        for (name, arguments, expected) in [("bridge", "42,3", 93.0), ("step0", "42", 2725.0)] {
            let value = engine.evaluate_unit(&format!(
                "rt => globalThis.__lightCompilerRoots['parallel.probe{id}/{name}']({arguments})"
            )).unwrap();
            let data = engine.export_data(value, Default::default()).unwrap();
            engine.release(value).unwrap();
            assert!(
                matches!(data.kind(), crate::compiler_data::DatumKind::Double(n) if *n == expected),
                "published namespace {id}/{name}: {data:?}"
            );
        }
    }
}

fn run(
    workers: usize,
    count: usize,
    fixture: &str,
    ordered: bool,
    stateful: bool,
) -> (Duration, Vec<String>, Published) {
    let setup = Instant::now();
    let (host, analysis) = providers(ordered, stateful);
    let seed = Arc::new(Seed {
        program: analysis.program,
        globals: analysis.globals,
        roots: analysis.roots,
        requires: analysis.requires,
        record_layouts: analysis.record_layouts,
    });
    let shared = host.session_handle().unwrap().shared().clone();
    let journal = host.session_handle().unwrap().namespace_prefix();
    let environment = host.environment();
    let runtime = shared
        .native_runtime(environment.evaluator_options())
        .unwrap();
    let reservations = (0..count)
        .map(|id| {
            Mutex::new(ordered.then(|| {
                let source = id + seed.roots.len();
                shared.lanes().register_namespace(
                    source,
                    &format!("parallel.probe{id}"),
                    &["workload.common".into(), "workload.m0".into()],
                );
                shared.lanes().reserve_pass(Some(source))
            }))
        })
        .collect::<Vec<_>>();
    let next = AtomicUsize::new(0);
    let active = AtomicUsize::new(0);
    let peak = AtomicUsize::new(0);
    let ready = Barrier::new(workers + 1);
    let start = Barrier::new(workers + 1);
    let (elapsed, mut results) = std::thread::scope(|scope| {
        let handles = (0..workers)
            .map(|slot| {
                let (environment, shared, runtime, seed) = (
                    environment.clone(),
                    shared.clone(),
                    runtime.clone(),
                    seed.clone(),
                );
                let (ready, start, next, active, peak) = (&ready, &start, &next, &active, &peak);
                let reservations = &reservations;
                let journal = &journal;
                std::thread::Builder::new()
                    .name(format!("namespace-probe-{workers}-{slot}"))
                    .spawn_scoped(scope, move || {
                        ready.wait();
                        start.wait();
                        let mut results = Vec::new();
                        loop {
                            let id = next.fetch_add(1, Ordering::Relaxed);
                            if id >= count {
                                break;
                            }
                            // A writer gets the same fixed provider prefix,
                            // never an earlier job's or another writer's suffix.
                            // Creating this task is included in analysis time.
                            let evaluator = std::rc::Rc::new(runtime.evaluator().unwrap());
                            let mut host = MacroHost::bind(environment.clone(), Some(evaluator)).unwrap();
                            let session = crate::compilation_session::CompilationSession::namespace_probe(
                                shared.clone(), journal,
                            );
                            host.engine.session = Some(session.clone());
                            session.attach(host.engine.bootstrap.as_ref().unwrap(), &host).unwrap();
                            host.tables.bootstrap_seed = false;
                            if let Some(reservation) = reservations[id].lock().unwrap().take() {
                                session.reserve_probe_namespace(
                                    seed.roots.len(),
                                    seed.roots.len() + id,
                                    reservation,
                                );
                            }
                            // Correctness stress only: later namespaces can
                            // reach their stamp while the first is still queued.
                            // Stateful runs must not be used as performance data.
                            if stateful && id == 0 && workers > 1 {
                                std::thread::sleep(Duration::from_millis(100));
                            }
                            let busy = active.fetch_add(1, Ordering::SeqCst) + 1;
                            peak.fetch_max(busy, Ordering::SeqCst);
                            let mut analysis = NamespaceAnalysis::in_compilation();
                            // This is the build's worker budget. No unit pool
                            // is started: each writer executes its own units.
                            // ONE would force all those units onto a globally
                            // serial lane even when other writers are running.
                            analysis.unit_slots = crate::scheduler::Slots::new(workers).unwrap();
                            analysis.program = seed.program.clone();
                            analysis.globals = seed.globals.clone();
                            analysis.roots = seed.roots.clone();
                            analysis.requires = seed.requires.clone();
                            analysis.record_layouts = seed.record_layouts.clone();
                            analyze_worker(&mut analysis, &mut host, id, fixture, stateful);
                            if std::env::var_os("LIGHT_NAMESPACE_PROBE_VISIBILITY").is_some() {
                                let engine = &host.engine.bootstrap.as_ref().unwrap().evaluator;
                                let source = format!("rt => Object.keys(globalThis.__lightCompilerRoots).filter(n => n.startsWith('parallel.probe') && !n.startsWith('parallel.probe{id}/')).length");
                                let value = engine.evaluate_unit(&source).unwrap();
                                let data = engine.export_data(value, Default::default()).unwrap();
                                engine.release(value).unwrap();
                                assert!(matches!(data.kind(), crate::compiler_data::DatumKind::Double(n) if *n == 0.0), "writer {id} sees another namespace: {data:?}");
                            }
                            let program = analysis.finish(2, &host).unwrap();
                            results.push((id, program, session.namespace_fragment(), host.namespace_metadata(&format!("parallel.probe{id}"))));
                            active.fetch_sub(1, Ordering::SeqCst);
                        }
                        results
                    })
                    .unwrap()
            })
            .collect::<Vec<_>>();
        ready.wait();
        let setup = setup.elapsed();
        let started = Instant::now();
        start.wait();
        let results = handles
            .into_iter()
            .flat_map(|h| h.join().unwrap())
            .collect::<Vec<_>>();
        let elapsed = started.elapsed();
        eprintln!(
            "namespace-probe workers={workers} namespaces={count} setup-ms={:.3} analysis-ms={:.3} peak-active={}",
            setup.as_secs_f64() * 1000.0,
            elapsed.as_secs_f64() * 1000.0,
            peak.load(Ordering::Relaxed)
        );
        (elapsed, results)
    });
    assert_eq!(results.len(), count);
    if ordered {
        let lanes = shared.lanes().counts();
        eprintln!("namespace-probe workers={workers} lanes={lanes:?}");
        if stateful && workers > 1 && count > 1 {
            assert!(
                lanes.waits > 0,
                "the delayed earlier namespace must order later effects"
            );
        }
    }
    if workers > 1 {
        assert!(peak.load(Ordering::Relaxed) > 1);
    }
    results.sort_by_key(|(id, _, _, _)| *id);
    let uses = host.session_handle().unwrap().compilation_uses();
    for id in 0..count {
        assert!(
            uses.iter()
                .any(|unit| unit.name == format!("parallel.probe{id}/run") && unit.analyses == 1),
            "namespace {id} must register and analyze its ordinary deferred run unit"
        );
    }
    let publication = Instant::now();
    let mut published_journal = journal;
    let mut joined = NamespaceAnalysis::in_compilation();
    joined.program = seed.program.clone();
    joined.globals = seed.globals.clone();
    joined.roots = seed.roots.clone();
    joined.requires = seed.requires.clone();
    let prefix = crate::program_fragment::SharedPrefix::same(&seed.program);
    for (id, program, registrations, _) in &results {
        published_journal.append_namespace(registrations.clone());
        let mut fragment = program.clone();
        // finish() closed this isolated view over the shared provider roots.
        // Publish only its own source root; providers must execute once.
        let ExprKind::Do(roots) = &fragment.exprs[fragment.root.0].kind else {
            panic!("a finished namespace has a source-root sequence")
        };
        let own = *roots.last().unwrap();
        assert_eq!(fragment.root.0 + 1, fragment.exprs.len());
        fragment.exprs.truncate(fragment.root.0);
        fragment.root = own;
        let source = seed.roots.len() + id;
        for index in seed.program.exprs.len()..fragment.exprs.len() {
            let expr = &mut fragment.exprs[index];
            if expr.source_index == seed.roots.len() {
                expr.source_index = source;
            }
        }
        for index in seed.program.locals.len()..fragment.locals.len() {
            let local = &mut fragment.locals[index];
            if local.source_index == seed.roots.len() {
                local.source_index = source;
            }
        }
        for index in seed.program.globals.len()..fragment.globals.len() {
            let global = &mut fragment.globals[index];
            if global.source_index == seed.roots.len() {
                global.source_index = source;
            }
        }
        let root = crate::program_fragment::append(
            &mut joined.program,
            &fragment,
            &prefix,
            false,
            Some(&format!("parallel.probe{id}")),
            Some(&mut joined.globals),
        );
        joined.roots.push(root);
        joined
            .requires
            .push(vec!["workload.common".into(), "workload.m0".into()]);
    }
    let publication = publication.elapsed();
    eprintln!(
        "namespace-probe workers={workers} publication-ms={:.3}",
        publication.as_secs_f64() * 1000.0
    );
    let mut consumer_host = None;
    if std::env::var_os("LIGHT_NAMESPACE_PROBE_VISIBILITY").is_some()
        || std::env::var_os("LIGHT_NAMESPACE_PROBE_CONSUMER").is_some()
    {
        let evaluator = std::rc::Rc::new(runtime.evaluator().unwrap());
        let mut consumer = MacroHost::bind(environment, Some(evaluator)).unwrap();
        let session = crate::compilation_session::CompilationSession::namespace_probe(
            shared,
            &published_journal,
        );
        consumer.engine.session = Some(session.clone());
        session
            .attach(consumer.engine.bootstrap.as_ref().unwrap(), &consumer)
            .unwrap();
        consumer.tables.bootstrap_seed = false;
        for (_, _, _, metadata) in &results {
            consumer.publish_namespace_metadata(metadata).unwrap();
        }
        if std::env::var_os("LIGHT_NAMESPACE_PROBE_VISIBILITY").is_some() {
            verify_native_catalog(&consumer, count);
        }
        if std::env::var_os("LIGHT_NAMESPACE_PROBE_CONSUMER").is_some() {
            let started = Instant::now();
            analyze_consumer(&mut joined, &mut consumer, count, ordered, stateful);
            eprintln!(
                "namespace-probe workers={workers} consumer-ms={:.3}",
                started.elapsed().as_secs_f64() * 1000.0
            );
        }
        consumer_host = Some(consumer);
    }
    let program = finish_values(
        joined,
        consumer_host.as_ref().unwrap_or(&host),
        seed.roots.len(),
    );
    let emission = Instant::now();
    let combined = publish(&program);
    eprintln!(
        "namespace-probe workers={workers} combined-emission-ms={:.3}",
        emission.elapsed().as_secs_f64() * 1000.0
    );
    // Application emission is outside the measured analysis interval, on
    // the coordinator. Every parallel result must match its serial bytes.
    let output = results
        .into_iter()
        .map(|(_, program, _, _)| crate::emit_js(&program))
        .collect();
    (elapsed, output, combined)
}

#[test]
#[ignore = "manual shared-runtime namespace scaling probe; requires research/cljs-200k"]
fn independent_namespace_scaling() {
    crate::wait_timeline::start_from_environment();
    let fixture = std::fs::read_to_string("research/cljs-200k/src/workload/n0.cljs").unwrap();
    let count = std::env::var("LIGHT_NAMESPACE_PROBE_COUNT").map_or(32, |n| n.parse().unwrap());
    let ordered = std::env::var_os("LIGHT_NAMESPACE_PROBE_LANES").is_some();
    let stateful = std::env::var_os("LIGHT_NAMESPACE_PROBE_STATEFUL").is_some();
    assert!(
        !stateful || ordered,
        "stateful stress requires effect lanes"
    );
    eprintln!("namespace-probe effect-lanes={ordered} stateful={stateful}");
    let reference = std::env::var_os("LIGHT_NAMESPACE_PROBE_CONSUMER").map(|_| {
        eprintln!("namespace-probe checking ordinary dependent-namespace reference");
        serial_reference(count, &fixture, ordered, stateful)
    });
    // Warm process-wide first-party compiler data before comparing fresh
    // runtimes. Each measured mode still compiles its own providers.
    let _ = run(1, 1, &fixture, ordered, stateful);
    let (serial, expected, combined) = run(1, count, &fixture, ordered, stateful);
    let reference =
        reference.unwrap_or_else(|| serial_reference(count, &fixture, ordered, stateful));
    if let Some(directory) = std::env::var_os("LIGHT_NAMESPACE_PROBE_OUTPUT") {
        let directory = std::path::PathBuf::from(directory);
        std::fs::create_dir_all(&directory).unwrap();
        for (id, javascript) in expected.iter().enumerate() {
            std::fs::write(directory.join(format!("{id}.js")), javascript).unwrap();
        }
        std::fs::write(directory.join("combined.js"), &combined.code).unwrap();
        std::fs::write(directory.join("reference.js"), &reference.code).unwrap();
        std::fs::write(directory.join("combined-advanced.js"), &combined.advanced).unwrap();
        std::fs::write(directory.join("reference-advanced.js"), &reference.advanced).unwrap();
    }
    assert!(
        combined == reference,
        "fragment publication differs from ordinary serial compilation: code={} map={} advanced={} advanced-map={}",
        combined.code == reference.code,
        combined.mapping == reference.mapping,
        combined.advanced == reference.advanced,
        combined.advanced_mapping == reference.advanced_mapping
    );
    for workers in [2, 4, 8] {
        let (elapsed, actual, joined) = run(workers, count, &fixture, ordered, stateful);
        assert!(
            joined == combined,
            "{workers}-worker combined output changed"
        );
        assert!(
            actual == expected,
            "{workers}-worker output changed at namespace {:?}",
            actual.iter().zip(&expected).position(|(a, b)| a != b)
        );
        eprintln!(
            "namespace-probe workers={workers} speedup={:.3} byte-identical=true",
            serial.as_secs_f64() / elapsed.as_secs_f64()
        );
    }
    crate::wait_timeline::finish().unwrap();
}
