//! Same-binary scalar-equivalent type costs, with separate definition and GC work.
use light_cljs::interpreter::{
    AnalyzedForm, Arity, Context, ContextOptions, MemoryStats, Name, NativeDefinition,
    NativeEffects, NativeMetadata, RootedValue, ValueView,
};
#[cfg(not(feature = "count-allocations"))]
use std::time::Instant;
use std::{
    env,
    error::Error,
    hint::black_box,
    sync::atomic::{AtomicUsize, Ordering::Relaxed},
};
#[cfg(feature = "count-allocations")]
#[path = "support/interpreter_allocations.rs"]
mod allocations;
#[cfg(feature = "count-allocations")]
#[global_allocator]
static ALLOCATOR: allocations::CountingAllocator = allocations::CountingAllocator;

macro_rules! fixture {
    ($name:literal) => {
        include_str!(concat!(
            "../benchmarks/fixtures/interpreter/types/",
            $name,
            ".clj"
        ))
    };
}
const SUM: usize = 499_500;
const OBJECTS: usize = 1000;
const CASES: &[(&str, &str)] = &[
    ("map", fixture!("map")),
    ("direct-record", fixture!("direct-record")),
    ("inline", fixture!("inline")),
    ("external", fixture!("external")),
    ("reify", fixture!("reify")),
    ("closure", fixture!("closure")),
    ("mixed", fixture!("mixed")),
];
static CALLBACK_COLLECTIONS: AtomicUsize = AtomicUsize::new(0);
static GC: NativeDefinition = NativeDefinition {
    metadata: NativeMetadata {
        namespace: "bench",
        name: "gc",
        arity: Arity::Exact(0),
        effects: NativeEffects::Context,
        errors: &[],
    },
    callback: |c, _| {
        c.collect_garbage();
        CALLBACK_COLLECTIONS.fetch_add(1, Relaxed);
        Ok(c.nil())
    },
};

#[derive(Default)]
struct Cost {
    ns: u128,
    allocations: usize,
    reallocations: usize,
    bytes: usize,
    live: usize,
    peak: usize,
    collections: usize,
}
fn measure<T>(f: impl FnOnce() -> T) -> (T, Cost) {
    let before = CALLBACK_COLLECTIONS.load(Relaxed);
    #[cfg(feature = "count-allocations")]
    let (value, mut cost) = {
        let (value, allocations, reallocations, bytes, live, peak) = allocations::measure(f);
        (
            value,
            Cost {
                allocations,
                reallocations,
                bytes,
                live,
                peak,
                ..Cost::default()
            },
        )
    };
    #[cfg(not(feature = "count-allocations"))]
    let (value, mut cost) = {
        let start = Instant::now();
        let value = f();
        (
            value,
            Cost {
                ns: start.elapsed().as_nanos(),
                ..Cost::default()
            },
        )
    };
    cost.collections = CALLBACK_COLLECTIONS.load(Relaxed) - before;
    (value, cost)
}
fn row(
    name: &str,
    phase: &str,
    sample: usize,
    iterations: usize,
    cost: Cost,
    m: MemoryStats,
    checksum: usize,
) {
    println!(
        "{name},{phase},{sample},{iterations},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
        cost.ns,
        cost.allocations,
        cost.reallocations,
        cost.bytes,
        cost.live,
        cost.peak,
        m.heap_values,
        m.names,
        m.sources,
        m.executable_units,
        m.namespaces,
        m.vars,
        m.roots,
        m.slots,
        cost.collections,
        checksum
    );
}
fn integer(v: RootedValue) -> usize {
    let ValueView::Integer(n) = black_box(v).inspect() else {
        panic!("expected integer")
    };
    n.try_into().unwrap()
}
fn analyzed(c: &mut Context, text: &str) -> AnalyzedForm {
    let source = c.source("types-call.clj", text);
    let form = c.reader(&source).unwrap().read_form().unwrap().unwrap();
    c.analyze_form(&form).unwrap()
}
fn live(mut m: MemoryStats) -> MemoryStats {
    m.slots = 0;
    m
}

// Equal-sized warmup batches establish root, slot and collector capacities.
// Every result is dropped in the operation; one explicit collection per batch.
fn samples(
    c: &mut Context,
    name: &str,
    phase: &str,
    count: usize,
    iterations: usize,
    expected: usize,
    mut op: impl FnMut(&mut Context) -> usize,
) {
    for _ in 0..2 {
        for _ in 0..iterations {
            assert_eq!(op(c), expected, "{name}/{phase}");
        }
        c.collect_garbage();
    }
    let warmed = c.memory_stats();
    let gc_phase = format!("gc_after_{phase}");
    for sample in 1..=count {
        let (sum, cost) = measure(|| (0..iterations).map(|_| black_box(op(c))).sum::<usize>());
        assert_eq!(sum, iterations * expected);
        if phase == "invoke_gc_callback" {
            assert_eq!(cost.collections, iterations);
        }
        row(name, phase, sample, iterations, cost, c.memory_stats(), sum);
        let (stats, mut cost) = measure(|| c.collect_garbage());
        assert_eq!(
            stats.after, warmed,
            "{name}/{phase}: warmed live records/roots/slots drifted"
        );
        #[cfg(feature = "count-allocations")]
        assert_eq!(
            (cost.allocations, cost.reallocations, cost.bytes),
            (0, 0, 0),
            "{name}/{phase}: warmed GC must reuse scratch"
        );
        cost.collections = 1;
        row(
            name,
            &gc_phase,
            sample,
            1,
            cost,
            stats.after,
            stats.before.heap_values - stats.after.heap_values,
        );
    }
}

fn dispatch(count: usize, iterations: usize) {
    for (name, source) in CASES {
        let mut c = Context::new();
        let parts = c.eval_string(source).unwrap().elements().unwrap();
        assert_eq!(parts.len(), 2);
        let make = parts[0].clone();
        let method = parts[1].clone(); // Saved ordinary method value; no special dispatch API.
        drop(parts);
        let numbers: Vec<_> = (0..OBJECTS).map(|n| [c.integer(n as i64)]).collect();
        let objects: Vec<_> = numbers
            .iter()
            .map(|n| [c.invoke(&make, n).unwrap()])
            .collect();
        for (n, object) in objects.iter().enumerate() {
            assert_eq!(integer(c.invoke(&method, object).unwrap()), n);
            match *name {
                "map" => assert!(matches!(object[0].inspect(), ValueView::Map(_))),
                "reify" => assert_eq!(object[0].inspect(), ValueView::Reify),
                "closure" => assert_eq!(object[0].inspect(), ValueView::Function),
                _ => assert!(matches!(object[0].inspect(), ValueView::Record(_))),
            }
        }
        let input = c
            .vector(&objects.iter().map(|v| v[0].clone()).collect::<Vec<_>>())
            .unwrap();
        c.merge_opts(ContextOptions {
            bindings: vec![
                (Name::new(None, "benchmark-input"), input),
                (Name::new(None, "benchmark-method"), method.clone()),
            ],
            ..ContextOptions::default()
        })
        .unwrap();
        let code = analyzed(
            &mut c,
            "(reduce (fn [sum r] (+ sum (benchmark-method r))) 0 benchmark-input)",
        );
        assert_eq!(integer(c.eval_analyzed(&code).unwrap()), SUM);
        samples(&mut c, name, "invoke_1000", count, iterations, SUM, |c| {
            objects
                .iter()
                .map(|object| integer(c.invoke(&method, object).unwrap()))
                .sum()
        });
        samples(
            &mut c,
            name,
            "analyzed_sum_1000",
            count,
            iterations,
            SUM,
            |c| integer(c.eval_analyzed(&code).unwrap()),
        );
        // Reuse a saved constructor/factory; all 1000 fresh outputs are dropped.
        samples(
            &mut c,
            name,
            "construct_1000",
            count,
            iterations,
            OBJECTS,
            |c| {
                for n in &numbers {
                    black_box(c.invoke(&make, n).unwrap());
                }
                OBJECTS
            },
        );
    }
}

fn record_operations(count: usize, iterations: usize) {
    for kind in ["map", "record"] {
        let mut c = Context::new();
        c.eval_string("(defrecord R [n y]) (defrecord S [n y])")
            .unwrap();
        // Nominal inequality and distinct nominal keys are correctness checks only.
        assert_eq!(
            c.eval_string(
                "(let [r (->R 7 8) s (->S 7 8) m {:n 7 :y 8}]
            (and (not= r m) (not= m r) (not= r s)
                 (= [(get {r 1 s 2 m 3} r) (get {r 1 s 2 m 3} s) (get {r 1 s 2 m 3} m)] [1 2 3])))"
            )
            .unwrap()
            .inspect(),
            ValueView::Boolean(true)
        );
        let input = c
            .eval_string(if kind == "record" {
                "(assoc (->R 7 8) :extra 9)"
            } else {
                "{:n 7 :y 8 :extra 9}"
            })
            .unwrap();
        let map = c.eval_string("{:n 7 :y 8 :extra 9}").unwrap();
        let field = c.keyword(Name::new(None, "n"));
        for (phase, source, expected, record_result) in [
            ("lookup", "(fn [r] (:n r))", "7", false),
            (
                "assoc_basis",
                "(fn [r] (assoc r :n 10))",
                "{:n 10 :y 8 :extra 9}",
                true,
            ),
            (
                "assoc_extra",
                "(fn [r] (assoc r :extra 10))",
                "{:n 7 :y 8 :extra 10}",
                true,
            ),
            (
                "dissoc_extra",
                "(fn [r] (dissoc r :extra))",
                "{:n 7 :y 8}",
                true,
            ),
            (
                "dissoc_basis",
                "(fn [r] (dissoc r :y))",
                "{:n 7 :extra 9}",
                false,
            ),
            (
                "from_map",
                if kind == "record" {
                    "map->R"
                } else {
                    "(fn [m] (into {} m))"
                },
                "{:n 7 :y 8 :extra 9}",
                true,
            ),
            (
                "key_lookup",
                if kind == "record" {
                    "(let [m {(assoc (->R 7 8) :extra 9) 7}] (fn [r] (get m r)))"
                } else {
                    "(let [m {{:n 7 :y 8 :extra 9} 7}] (fn [r] (get m r)))"
                },
                "7",
                false,
            ),
        ] {
            let f = c.eval_string(source).unwrap();
            let args = [if phase == "from_map" {
                map.clone()
            } else {
                input.clone()
            }];
            let expected_value = c.eval_string(expected).unwrap();
            let project = c
                .eval_string("(fn [v] (if (map? v) (into {} v) v))")
                .unwrap();
            let probe = c.invoke(&f, &args).unwrap();
            let actual = c.invoke(&project, std::slice::from_ref(&probe)).unwrap();
            assert!(actual.value_eq(&expected_value), "{kind}/{phase}");
            if matches!(actual.inspect(), ValueView::Map(_)) {
                assert_eq!(
                    matches!(probe.inspect(), ValueView::Record(_)),
                    kind == "record" && record_result
                );
                assert!(matches!(
                    probe.inspect(),
                    ValueView::Map(_) | ValueView::Record(_)
                ));
            } else {
                assert_eq!(probe.inspect(), ValueView::Integer(7));
            }
            drop((project, probe, actual, expected_value));
            let checksum = if phase == "assoc_basis" { 10 } else { 7 };
            samples(&mut c, kind, phase, count, iterations, checksum, |c| {
                let result = black_box(c.invoke(&f, &args).unwrap());
                integer(
                    if matches!(result.inspect(), ValueView::Map(_) | ValueView::Record(_)) {
                        result.get(&field).unwrap().unwrap()
                    } else {
                        result
                    },
                )
            });
        }
    }
}

fn definitions(count: usize, iterations: usize) {
    for (name, text, check) in [
        (
            "protocol",
            "(defprotocol P (p [r]))",
            "(satisfies? P (reify P (p [_] 7)))",
        ),
        ("record-data", "(defrecord R [n])", "(= 7 (:n (->R 7)))"),
        (
            "record-inline",
            fixture!("record-definition"),
            "(= 7 (p (->R 7)))",
        ),
        (
            "record-extension",
            "(extend-type R P (p [r] (:n r)))",
            "(= 7 (p (->R 7)))",
        ),
    ] {
        let mut c = Context::new();
        c.eval_string("(defprotocol P (p [r])) (defrecord R [n])")
            .unwrap();
        c.eval_string(text).unwrap();
        assert_eq!(
            c.eval_string(check).unwrap().inspect(),
            ValueView::Boolean(true)
        );
        let source = c.source("types-definition.clj", text);
        let form = c.reader(&source).unwrap().read_form().unwrap().unwrap();
        let quoted = c.eval_string(&format!("'{text}")).unwrap();
        let code = c.analyze_form(&form).unwrap();
        let nodes = code.expression_count();
        samples(&mut c, name, "macroexpand", count, iterations, 1, |c| {
            black_box(c.macroexpand(&quoted, None).unwrap());
            1
        });
        samples(&mut c, name, "analyze", count, iterations, nodes, |c| {
            black_box(c.analyze_form(&form).unwrap()).expression_count()
        });
        samples(&mut c, name, "eval_definition", count, iterations, 1, |c| {
            black_box(c.eval_analyzed(&code).unwrap());
            1
        });
        samples(
            &mut c,
            name,
            "read_analyze_eval",
            count,
            iterations,
            1,
            |c| {
                black_box(c.eval_source(&source).unwrap());
                1
            },
        );
        assert_eq!(
            c.eval_string(check).unwrap().inspect(),
            ValueView::Boolean(true)
        );
    }
}

fn lifetimes(count: usize, iterations: usize) {
    for keep in [false, true] {
        let mut c = Context::new();
        c.eval_string("(defprotocol P (p [r]))").unwrap();
        let text = fixture!("captured-definition");
        c.eval_string(text).unwrap();
        // Prime all Vars used by the later validation before remembering the baseline.
        assert_eq!(integer(c.eval_string("(p (->R 7))").unwrap()), 7);
        c.collect_garbage();
        let baseline = live(c.memory_stats());
        let retained = if keep {
            Some(c.eval_string("[(->R 7) R]").unwrap().elements().unwrap())
        } else {
            None
        };
        samples(
            &mut c,
            if keep {
                "retained-generation"
            } else {
                "released-generation"
            },
            "redefine",
            count,
            iterations,
            1,
            |c| {
                black_box(c.eval_string(text).unwrap());
                1
            },
        );
        assert_eq!(integer(c.eval_string("(p (->R 7))").unwrap()), 7);
        if let Some(values) = &retained {
            let method = c.eval_string("p").unwrap();
            assert_eq!(integer(c.invoke(&method, &values[..1]).unwrap()), 7);
            assert!(matches!(values[1].inspect(), ValueView::RecordType(_)));
        }
        drop(retained);
        c.collect_garbage();
        assert_eq!(
            live(c.memory_stats()),
            baseline,
            "old generation must be released"
        );
        // Nine independent retained/released transitions; definitions and the
        // first collection are outside this release-only measured region.
        for sample in 1..=count {
            let retained = if keep {
                Some(c.eval_string("[(->R 7) R]").unwrap().elements().unwrap())
            } else {
                None
            };
            c.eval_string(text).unwrap();
            c.collect_garbage();
            let (stats, mut cost) = measure(|| {
                drop(retained);
                c.collect_garbage()
            });
            cost.collections = 1;
            assert_eq!(
                live(stats.after),
                baseline,
                "released old descriptor/capture remains rooted"
            );
            row(
                if keep {
                    "retained-generation"
                } else {
                    "released-generation"
                },
                "release_final",
                sample,
                1,
                cost,
                stats.after,
                stats.before.heap_values - stats.after.heap_values,
            );
        }
    }
    for (name, text, failure) in [
        ("failed-generation", fixture!("failed-definition"), true),
        ("replaced-capture", fixture!("captured-extension"), false),
    ] {
        let mut c = Context::new();
        c.eval_string("(defprotocol P (p [r])) (defrecord R [n] P (p [_] n))")
            .unwrap();
        let object = c.eval_string("(->R 7)").unwrap();
        let method = c.eval_string("p").unwrap();
        samples(&mut c, name, "redefine", count, iterations, 1, |c| {
            let result = black_box(c.eval_string(text));
            assert_eq!(result.is_err(), failure, "unexpected definition outcome");
            1
        });
        assert_eq!(integer(c.invoke(&method, &[object]).unwrap()), 7);
    }
    for name in ["inline", "external", "reify"] {
        let mut c = Context::new();
        c.register_native(&GC);
        c.eval_string("(defprotocol P (p [r]))").unwrap();
        let object = c
            .eval_string(match name {
                "inline" => "(defrecord R [n] P (p [_] (bench/gc) n)) (->R 7)",
                "external" => {
                    "(defrecord R [n]) (extend-type R P (p [r] (bench/gc) (:n r))) (->R 7)"
                }
                _ => "(let [n 7] (reify P (p [_] (bench/gc) n)))",
            })
            .unwrap();
        let method = c.eval_string("p").unwrap();
        samples(
            &mut c,
            name,
            "invoke_gc_callback",
            count,
            iterations,
            7,
            |c| integer(c.invoke(&method, std::slice::from_ref(&object)).unwrap()),
        );
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    if cfg!(debug_assertions) {
        return Err("use cargo bench or a release build".into());
    }
    let (mut count, mut iterations, mut counts) = (9, 32, false);
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--samples" => count = args.next().ok_or("missing samples")?.parse()?,
            "--iterations" => iterations = args.next().ok_or("missing iterations")?.parse()?,
            "--allocations" => counts = true,
            "--bench" => {}
            "--help" | "-h" => {
                eprintln!("interpreter_types [--samples N] [--iterations N] [--allocations]");
                return Ok(());
            }
            _ => return Err(format!("unknown option {arg}").into()),
        }
    }
    if count == 0 || iterations == 0 {
        return Err("samples/iterations must be positive".into());
    }
    if counts != cfg!(feature = "count-allocations") {
        return Err("allocation runs require count-allocations and --allocations".into());
    }
    eprintln!(
        "samples={count} iterations={iterations} allocations={counts}; two warmup batches; GC once per batch plus explicitly named callbacks; full type/content validation before timing"
    );
    println!(
        "scenario,phase,sample,iterations,elapsed_ns,allocations,reallocations,requested_bytes,live_requested_bytes,peak_requested_bytes,heap_values,names,sources,executable_units,namespaces,vars,roots,slots,gc_collections,checksum"
    );
    dispatch(count, iterations);
    record_operations(count, iterations);
    definitions(count, iterations);
    lifetimes(count, iterations);
    Ok(())
}
