//! Supplemental same-source harness for baseline/current libraries. No new API
//! is needed: rustc can link this exact harness against either saved rlib.
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

const CODE: &str = "(let* [x 20 f (fn* [n] (+ x n))] (f 22))";
const ITERATIONS: usize = 256;
fn main() {
    println!("phase,sample,iterations,elapsed_ns,allocations,reallocations,bytes,checksum");
    for phase in ["cold_analysis", "warm_analysis"] {
        let mut c = Context::new();
        let source = c.source("analysis-reuse.clj", CODE);
        let forms: Vec<_> = (0..ITERATIONS)
            .map(|_| c.reader(&source).unwrap().read_form().unwrap().unwrap())
            .collect();
        let code = c.analyze_form(&forms[0]).unwrap();
        assert_eq!(
            c.eval_analyzed(&code).unwrap().inspect(),
            ValueView::Integer(42)
        );
        for sample in 1..=9 {
            c.collect_garbage();
            let run = || {
                let mut checksum = 0;
                for i in 0..ITERATIONS {
                    let form = &forms[if phase == "cold_analysis" { i } else { 0 }];
                    checksum += black_box(c.analyze_form(form).unwrap()).expression_count();
                }
                checksum
            };
            #[cfg(not(feature = "count-allocations"))]
            let (checksum, ns, allocs, reallocs, bytes) = {
                let started = Instant::now();
                let mut run = run;
                (run(), started.elapsed().as_nanos(), 0, 0, 0)
            };
            #[cfg(feature = "count-allocations")]
            let (checksum, ns, allocs, reallocs, bytes) = {
                let (checksum, allocs, reallocs, bytes, _, _) = allocations::measure(run);
                (checksum, 0, allocs, reallocs, bytes)
            };
            assert_eq!(checksum, ITERATIONS * code.expression_count());
            println!("{phase},{sample},{ITERATIONS},{ns},{allocs},{reallocs},{bytes},{checksum}");
        }
    }
}
