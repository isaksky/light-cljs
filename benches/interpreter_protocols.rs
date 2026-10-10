//! Validated reify construction and invocation costs versus captured closures.
use light_cljs::interpreter::{Context, ValueView};
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
    for scenario in ["closure", "reify"] {
        let mut c = Context::new();
        c.eval_string("(defprotocol P (p [x y]))").unwrap();
        let make = c
            .eval_string(if scenario == "closure" {
                "(fn [n] (fn [_ y] (+ n y)))"
            } else {
                "(fn [n] (reify P (p [_ y] (+ n y))))"
            })
            .unwrap();
        let arg = [c.integer(7)];
        let object = c.invoke(&make, &arg).unwrap();
        let method = if scenario == "closure" {
            object.clone()
        } else {
            c.eval_string("p").unwrap()
        };
        let arguments = [object.clone(), c.integer(3)];
        assert_eq!(
            c.invoke(&method, &arguments).unwrap().inspect(),
            ValueView::Integer(10)
        );
        for phase in ["construct", "invoke"] {
            let iterations = if phase == "construct" { 2_000 } else { 20_000 };
            let mut steady = None;
            // Warm root/arena/collector capacities with an equal-sized batch.
            for sample in 0..=7 {
                let mut run = || {
                    let mut checksum = 0;
                    for _ in 0..iterations {
                        let value = if phase == "construct" {
                            c.invoke(&make, &arg).unwrap()
                        } else {
                            c.invoke(&method, &arguments).unwrap()
                        };
                        checksum += match black_box(value).inspect() {
                            ValueView::Integer(n) => n,
                            ValueView::Function | ValueView::Reify => 1,
                            other => panic!("invalid result: {other:?}"),
                        };
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
                assert_eq!(
                    checksum,
                    iterations * if phase == "construct" { 1 } else { 10 }
                );
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
}
