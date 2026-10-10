//! Equivalent map/record operations with validated values and warmed GC counts.
use light_cljs::interpreter::{Context, Name, ValueView};
use std::hint::black_box;
#[cfg(not(feature = "count-allocations"))]
use std::time::Instant;
#[cfg(feature = "count-allocations")]
#[path = "support/interpreter_allocations.rs"]
mod allocations;
#[cfg(feature = "count-allocations")]
#[global_allocator]
static ALLOCATOR: allocations::CountingAllocator = allocations::CountingAllocator;

fn main() {
    if cfg!(debug_assertions) {
        panic!("use cargo bench");
    }
    println!(
        "scenario,phase,sample,iterations,elapsed_ns,allocations,reallocations,bytes,checksum"
    );
    for scenario in ["map", "record"] {
        let mut c = Context::new();
        c.eval_string("(defrecord R [x y z])").unwrap();
        let data = c
            .eval_string(if scenario == "record" {
                "(->R 1 2 3)"
            } else {
                "{:x 1 :y 2 :z 3}"
            })
            .unwrap();
        let input_map = c.eval_string("{:x 1 :y 2 :z 3}").unwrap();
        let field = c.keyword(Name::new(None, "x"));
        for (phase, source, input, expected) in [
            (
                "construct",
                if scenario == "record" {
                    "(fn [x] (->R x 2 3))"
                } else {
                    "(fn [x] {:x x :y 2 :z 3})"
                },
                c.integer(1),
                1,
            ),
            (
                "from-map",
                if scenario == "record" {
                    "map->R"
                } else {
                    "(fn [m] (into {} m))"
                },
                input_map.clone(),
                1,
            ),
            ("lookup", "(fn [r] (:x r))", data.clone(), 1),
            ("assoc", "(fn [r] (assoc r :x 4))", data.clone(), 4),
            (
                "seq-reduce",
                "(fn [r] (reduce (fn [a e] (+ a (second e))) 0 r))",
                data.clone(),
                6,
            ),
            (
                "key",
                if scenario == "record" {
                    "(let [m {(->R 1 2 3) 7}] (fn [r] (get m r)))"
                } else {
                    "(let [m {{:x 1 :y 2 :z 3} 7}] (fn [r] (get m r)))"
                },
                data.clone(),
                7,
            ),
        ] {
            let function = c.eval_string(source).unwrap();
            let arguments = [input];
            let project = c
                .eval_string("(fn [x] (if (map? x) (into {} x) x))")
                .unwrap();
            let probe = c.invoke(&function, &arguments).unwrap();
            let actual = c.invoke(&project, std::slice::from_ref(&probe)).unwrap();
            let expected_value = c
                .eval_string(match phase {
                    "construct" | "from-map" => "{:x 1 :y 2 :z 3}",
                    "assoc" => "{:x 4 :y 2 :z 3}",
                    "lookup" => "1",
                    "seq-reduce" => "6",
                    "key" => "7",
                    _ => unreachable!(),
                })
                .unwrap();
            assert!(actual.value_eq(&expected_value));
            if matches!(phase, "construct" | "from-map" | "assoc") {
                assert_eq!(
                    matches!(probe.inspect(), ValueView::Record(_)),
                    scenario == "record"
                );
            }
            drop((project, probe, actual, expected_value));
            let mut steady = None;
            let iterations = 2_000;
            for sample in 0..=7 {
                let mut run = || {
                    let mut checksum = 0;
                    for _ in 0..iterations {
                        let result = c.invoke(&function, &arguments).unwrap();
                        let value = match result.inspect() {
                            ValueView::Map(_) | ValueView::Record(_) => {
                                result.get(&field).unwrap().unwrap()
                            }
                            _ => result,
                        };
                        let ValueView::Integer(n) = black_box(value).inspect() else {
                            panic!("invalid result")
                        };
                        checksum += n;
                    }
                    checksum
                };
                #[cfg(not(feature = "count-allocations"))]
                let (checksum, ns, allocs, reallocs, bytes) = {
                    let start = Instant::now();
                    (run(), start.elapsed().as_nanos(), 0, 0, 0)
                };
                #[cfg(feature = "count-allocations")]
                let (checksum, ns, allocs, reallocs, bytes) = {
                    let (sum, a, r, b, _, _) = allocations::measure(&mut run);
                    (sum, 0, a, r, b)
                };
                assert_eq!(checksum, iterations * expected);
                if sample > 0 {
                    println!(
                        "{scenario},{phase},{sample},{iterations},{ns},{allocs},{reallocs},{bytes},{checksum}"
                    );
                }
                let after = c.collect_garbage().after;
                if let Some(expected) = steady {
                    assert_eq!(after, expected);
                } else {
                    steady = Some(after);
                }
            }
        }
    }
    method_benchmarks();
}

fn method_benchmarks() {
    for scenario in [
        "direct-field",
        "record-method",
        "extended-method",
        "reify-method",
    ] {
        let mut c = Context::new();
        c.eval_string("(defprotocol P (p [r])) (defrecord R [x] P (p [_] x))")
            .unwrap();
        if scenario == "extended-method" {
            c.eval_string("(extend-type R P (p [r] (:x r)))").unwrap();
        }
        let make = c
            .eval_string(match scenario {
                "direct-field" => "->R",
                "record-method" => "->R",
                "extended-method" => "->R",
                _ => "(fn [x] (reify P (p [_] x)))",
            })
            .unwrap();
        let method = c
            .eval_string(if scenario == "direct-field" {
                "(fn [r] (:x r))"
            } else {
                "p"
            })
            .unwrap();
        let object = c.invoke(&make, &[c.integer(7)]).unwrap();
        for phase in ["invoke", "construct-method"] {
            let (function, argument) = if phase == "invoke" {
                (&method, object.clone())
            } else {
                (&make, c.integer(7))
            };
            let arguments = [argument];
            let mut baseline = None;
            for sample in 0..=7 {
                let iterations = 2_000;
                let mut run = || {
                    for _ in 0..iterations {
                        let value = c.invoke(function, &arguments).unwrap();
                        if phase == "invoke" {
                            assert_eq!(black_box(value).inspect(), ValueView::Integer(7));
                        } else {
                            black_box(value);
                        }
                    }
                };
                #[cfg(not(feature = "count-allocations"))]
                let (ns, allocs, reallocs, bytes) = {
                    let start = Instant::now();
                    run();
                    (start.elapsed().as_nanos(), 0, 0, 0)
                };
                #[cfg(feature = "count-allocations")]
                let (ns, allocs, reallocs, bytes) = {
                    let (_, a, r, b, _, _) = allocations::measure(&mut run);
                    (0, a, r, b)
                };
                // Validate constructed values by dispatch outside the timed loop.
                if phase != "invoke" {
                    let value = c.invoke(function, &arguments).unwrap();
                    assert_eq!(
                        c.invoke(&method, &[value]).unwrap().inspect(),
                        ValueView::Integer(7)
                    );
                }
                if sample > 0 {
                    println!(
                        "{scenario},{phase},{sample},{iterations},{ns},{allocs},{reallocs},{bytes},14000"
                    );
                }
                let after = c.collect_garbage().after;
                if let Some(expected) = baseline {
                    assert_eq!(after, expected);
                } else {
                    baseline = Some(after);
                }
            }
        }
    }
    for scenario in [
        "record-data-definition",
        "record-method-definition",
        "record-extension",
    ] {
        let mut c = Context::new();
        c.eval_string("(defprotocol P (p [r]))").unwrap();
        if scenario == "record-extension" {
            c.eval_string("(defrecord R [x])").unwrap();
        }
        let source = if scenario == "record-data-definition" {
            "(defrecord R [x])"
        } else if scenario == "record-extension" {
            "(let [n 3] (extend-type R P (p [r] (+ (:x r) n))))"
        } else {
            "(let [n 3] (defrecord R [x] P (p [_] (+ x n))))"
        };
        let mut baseline = None;
        for sample in 0..=7 {
            let iterations = 100;
            let mut run = || {
                for _ in 0..iterations {
                    let value = black_box(c.eval_string(source).unwrap()).inspect();
                    if scenario == "record-extension" {
                        assert_eq!(value, ValueView::Nil);
                    } else {
                        assert!(matches!(value, ValueView::RecordType(_)));
                    }
                }
            };
            #[cfg(not(feature = "count-allocations"))]
            let (ns, allocs, reallocs, bytes) = {
                let start = Instant::now();
                run();
                (start.elapsed().as_nanos(), 0, 0, 0)
            };
            #[cfg(feature = "count-allocations")]
            let (ns, allocs, reallocs, bytes) = {
                let (_, a, r, b, _, _) = allocations::measure(&mut run);
                (0, a, r, b)
            };
            assert_eq!(
                c.eval_string(if scenario == "record-data-definition" {
                    "(:x (->R 7))"
                } else {
                    "(p (->R 4))"
                })
                .unwrap()
                .inspect(),
                ValueView::Integer(7)
            );
            if sample > 0 {
                println!(
                    "{scenario},define,{sample},{iterations},{ns},{allocs},{reallocs},{bytes},700"
                );
            }
            let after = c.collect_garbage().after;
            let live = (
                after.heap_values,
                after.names,
                after.vars,
                after.executable_units,
                after.sources,
            );
            if let Some(expected) = baseline {
                assert_eq!(live, expected);
            } else {
                baseline = Some(live);
            }
        }
    }
}
