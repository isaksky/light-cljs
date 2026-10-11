//! End-to-end session costs. Run `cargo bench --bench evaluator -- --iterations 20`.
//! Each sample validates results; timings include result release and session lifecycle costs.
use light_cljs::{EvalOptions, EvalSession, evaluator::EvaluatorOptions};
use std::{hint::black_box, time::Instant};

fn value(session: &mut EvalSession, source: &str) {
    let result = session.eval_string(black_box(source)).unwrap();
    assert_eq!(session.number(result.value).unwrap(), 5050.0);
    session.release(result.value).unwrap();
}

fn measure(label: &str, iterations: usize, mut run: impl FnMut()) {
    run();
    let start = Instant::now();
    for _ in 0..iterations {
        run();
    }
    println!("{label}\t{iterations}\t{}", start.elapsed().as_nanos());
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let iterations = args
        .windows(2)
        .find(|a| a[0] == "--iterations")
        .map(|a| a[1].parse::<usize>().expect("positive iteration count"))
        .unwrap_or(20);
    assert!(iterations > 0);
    const SOURCE: &str = "(loop [i 100 sum 0] (if (zero? i) sum (recur (dec i) (+ sum i))))";
    println!("phase\titerations\tnanoseconds");
    measure("fresh-session", iterations, || {
        value(
            &mut EvalSession::new(EvalOptions::default()).unwrap(),
            SOURCE,
        );
    });
    let mut reused = EvalSession::new(EvalOptions::default()).unwrap();
    measure("reused-session", iterations, || value(&mut reused, SOURCE));
    measure("bounded-loop-reset-recovery", iterations, || {
        let mut session = EvalSession::new(EvalOptions {
            engine: EvaluatorOptions {
                poll_budget: 2000,
                ..Default::default()
            },
            ..Default::default()
        })
        .unwrap();
        let failure = session.eval_string("(loop [] (recur))").unwrap_err();
        assert!(failure.to_string().contains("poll budget"), "{failure}");
        assert!(
            session.eval_string("42").is_err(),
            "limit must remain sticky"
        );
        session.reset().unwrap();
        value(&mut session, SOURCE);
    });
}
