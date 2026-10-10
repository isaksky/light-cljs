//! Resident phases; raw output is consumed by benchmarks/run_compiler.py.
use light_cljs::{
    MacroHost, MacroImport, NamespaceInput, Program, SyntaxTree, analyze, analyze_namespaces,
    analyze_with_macros, compile, compile_namespace, compile_namespace_with_macros,
    compile_with_macros, emit_js, interpreter::Name, parse,
};
#[cfg(not(feature = "count-allocations"))]
use std::time::Instant;
use std::{env, error::Error, fs, hint::black_box, path::PathBuf};
#[cfg(feature = "count-allocations")]
#[path = "support/allocations.rs"]
mod allocations;
#[cfg(feature = "count-allocations")]
#[global_allocator]
static ALLOCATOR: allocations::CountingAllocator = allocations::CountingAllocator;

const COLLECTION: &str = include_str!("../benchmarks/fixtures/compiler/collections/value.cljs");
const MACRO: &str = include_str!("../benchmarks/fixtures/compiler/collections/bench/macros.clj");
const MACRO_CALL: &str = "(m/data)";
const DATA: &str = include_str!("../benchmarks/fixtures/compiler/collections/bench/data.cljs");
const MAIN: &str = include_str!("../benchmarks/fixtures/compiler/collections/bench/main.cljs");
const SINGLE: &str = include_str!("../benchmarks/fixtures/compiler/collections/bench/single.cljs");

fn host() -> MacroHost {
    let mut host = MacroHost::new();
    host.context_mut().eval_string(MACRO).unwrap();
    host.set_imports(
        "cljs.user",
        vec![MacroImport {
            namespace: "bench.macros".into(),
            alias: Some("m".into()),
            refers: vec![],
        }],
    )
    .unwrap();
    host
}

fn batch(name: &str, phase: &str, samples: usize, iterations: usize, mut f: impl FnMut() -> usize) {
    // Fixed two full warmup batches, outside instrumentation and reported samples.
    for _ in 0..2 {
        for _ in 0..iterations {
            black_box(f());
        }
    }
    let expected = f();
    for sample in 1..=samples {
        let run = || {
            let mut checksum = 0usize;
            for _ in 0..iterations {
                checksum = checksum.wrapping_add(black_box(f()));
            }
            black_box(checksum)
        };
        #[cfg(feature = "count-allocations")]
        let (checksum, allocs, reallocs, bytes) = allocations::measure(run);
        #[cfg(feature = "count-allocations")]
        let elapsed = 0;
        #[cfg(not(feature = "count-allocations"))]
        let (checksum, elapsed, allocs, reallocs, bytes) = {
            let start = Instant::now();
            let mut run = run;
            let checksum = run();
            (checksum, start.elapsed().as_nanos(), 0, 0, 0)
        };
        assert_eq!(checksum, expected.wrapping_mul(iterations));
        println!(
            "{name},{phase},{sample},{iterations},{elapsed},{allocs},{reallocs},{bytes},{checksum}"
        );
    }
}

fn primitive_sources() -> Vec<(&'static str, &'static str)> {
    let cases = include_str!("../tests/fixtures/execution.tsv");
    ["integer", "nested-closure", "js-nested-method"]
        .into_iter()
        .map(|name| {
            let row = cases
                .lines()
                .find(|row| row.split('\t').next() == Some(name))
                .unwrap();
            (name, row.split('\t').nth(1).unwrap())
        })
        .collect()
}

fn phases(name: &str, source: &str, samples: usize, iterations: usize) {
    let tree = parse(source).unwrap();
    let program = analyze(source, &tree).unwrap();
    let js = emit_js(&program);
    assert_eq!(compile(source).unwrap(), js);
    batch(name, "parse", samples, iterations, || {
        let tree = parse(black_box(source)).unwrap();
        black_box(&tree);
        tree.nodes.len()
    });
    batch(name, "analyze", samples, iterations, || {
        let program = analyze(black_box(source), black_box(&tree)).unwrap();
        black_box(&program);
        program.exprs.len()
    });
    batch(name, "emit", samples, iterations, || {
        let js = emit_js(black_box(&program));
        black_box(&js);
        js.len()
    });
    batch(name, "total", samples, iterations, || {
        let js = compile(black_box(source)).unwrap();
        black_box(&js);
        js.len()
    });
}

fn namespace_program(trees: &[SyntaxTree], combined: bool) -> Program {
    let aliases = vec![("d".into(), "bench.data".into())];
    let requires = vec!["bench.data".into()];
    if combined {
        analyze_namespaces(&[
            NamespaceInput {
                name: "bench.data",
                source: DATA,
                syntax: &trees[0],
                aliases: &[],
                requires: &[],
            },
            NamespaceInput {
                name: "bench.main",
                source: MAIN,
                syntax: &trees[1],
                aliases: &aliases,
                requires: &requires,
            },
        ])
        .unwrap()
    } else {
        analyze_namespaces(&[NamespaceInput {
            name: "bench.single",
            source: SINGLE,
            syntax: &trees[0],
            aliases: &[],
            requires: &[],
        }])
        .unwrap()
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let (mut samples, mut iterations) = (9, 32);
    let (mut counted, mut primitives) = (false, false);
    let mut dump = None;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--samples" => samples = args.next().ok_or("missing samples")?.parse()?,
            "--iterations" => iterations = args.next().ok_or("missing iterations")?.parse()?,
            "--allocations" => counted = true,
            "--primitive-only" => primitives = true,
            "--dump" => dump = Some(PathBuf::from(args.next().ok_or("missing dump directory")?)),
            "--bench" => {}
            "--help" | "-h" => {
                println!(
                    "compiler [--samples N>=9] [--iterations N] [--allocations] [--primitive-only] [--dump DIR]"
                );
                return Ok(());
            }
            _ => return Err(format!("unknown argument: {arg}").into()),
        }
    }
    if cfg!(debug_assertions)
        || samples < 9
        || iterations == 0
        || counted != cfg!(feature = "count-allocations")
    {
        return Err("require release build, >=9 samples, positive iterations, and --allocations iff count-allocations enabled".into());
    }
    if let Some(dir) = &dump {
        fs::create_dir_all(dir)?;
    }
    println!(
        "scenario,phase,sample,iterations,elapsed_ns,allocations,reallocations,requested_bytes,checksum"
    );
    for (name, source) in primitive_sources() {
        phases(name, source, samples, iterations);
        if let Some(dir) = &dump {
            fs::write(dir.join(format!("{name}.js")), compile(source)?)?;
        }
    }
    if primitives {
        return Ok(());
    }
    for (name, source) in [
        ("empty", ""),
        ("collections", COLLECTION),
        ("expanded-macro", "[1 2 3]"),
    ] {
        phases(name, source, samples, iterations);
        if let Some(dir) = &dump {
            fs::write(dir.join(format!("{name}.js")), compile(source)?)?;
        }
    }
    let tree = parse(MACRO_CALL).unwrap();
    let mut reused = host();
    let expected = compile("[1 2 3]")?;
    assert_eq!(compile_with_macros(MACRO_CALL, &mut reused)?, expected);
    batch("macro", "parse", samples, iterations, || {
        let t = parse(black_box(MACRO_CALL)).unwrap();
        black_box(&t);
        t.nodes.len()
    });
    batch("macro", "host-setup", samples, iterations, || {
        black_box(host());
        1
    });
    batch("macro", "analyze-reused-host", samples, iterations, || {
        let p = analyze_with_macros(black_box(MACRO_CALL), &tree, &mut reused).unwrap();
        black_box(&p);
        p.exprs.len()
    });
    batch("macro", "analyze-fresh-host", samples, iterations, || {
        let p = analyze_with_macros(black_box(MACRO_CALL), &tree, &mut host()).unwrap();
        black_box(&p);
        p.exprs.len()
    });
    let expanded = analyze_with_macros(MACRO_CALL, &tree, &mut reused)?;
    batch("macro", "emit", samples, iterations, || {
        let js = emit_js(&expanded);
        black_box(&js);
        js.len()
    });
    for fresh in [false, true] {
        batch(
            "macro",
            if fresh {
                "total-fresh-host"
            } else {
                "total-reused-host"
            },
            samples,
            iterations,
            || {
                let js = if fresh {
                    compile_with_macros(MACRO_CALL, &mut host())
                } else {
                    compile_with_macros(MACRO_CALL, &mut reused)
                }
                .unwrap();
                black_box(&js);
                js.len()
            },
        );
    }
    // Isolate the public expansion call: prepared owned form, resolved Var and
    // nil environment (this macro does not inspect &env). Excludes compiler
    // import/lowering, lookup, source parsing, host construction and macro load.
    let c = reused.context_mut();
    let src = c.source("macro-call.cljs", MACRO_CALL);
    let form = c.reader(&src)?.read_form()?.unwrap();
    let var = c
        .find_interned_var(&Name::new(None, "bench.macros"), &Name::new(None, "data"))?
        .unwrap();
    let env = c.nil();
    batch("macro", "expand-resolved", samples, iterations, || {
        let expanded = c.expand_macro_call(&form, &var, &env).unwrap();
        black_box(&expanded);
        expanded.nodes().len()
    });
    let root =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("benchmarks/fixtures/compiler/collections");
    for (name, entry, sources) in [
        ("single", "bench.single", vec![SINGLE]),
        ("combined", "bench.main", vec![DATA, MAIN]),
    ] {
        let combined = sources.len() == 2;
        let trees: Vec<_> = sources.iter().map(|s| parse(s).unwrap()).collect();
        let p = namespace_program(&trees, combined);
        assert_eq!(
            emit_js(&p),
            compile_namespace(entry, std::slice::from_ref(&root))?
        );
        batch(name, "parse", samples, iterations, || {
            let ts: Vec<_> = sources
                .iter()
                .map(|s| parse(black_box(s)).unwrap())
                .collect();
            black_box(&ts);
            ts.iter().map(|t| t.nodes.len()).sum()
        });
        batch(name, "analyze", samples, iterations, || {
            let p = namespace_program(&trees, combined);
            black_box(&p);
            p.exprs.len()
        });
        batch(name, "emit", samples, iterations, || {
            let js = emit_js(&p);
            black_box(&js);
            js.len()
        });
        batch(name, "total-resident", samples, iterations, || {
            let trees: Vec<_> = sources
                .iter()
                .map(|s| parse(black_box(s)).unwrap())
                .collect();
            let js = emit_js(&namespace_program(&trees, combined));
            black_box(&js);
            js.len()
        });
    }
    for entry in [
        "bench.single",
        "bench.main",
        "bench.macro",
        "bench.macro-main",
    ] {
        let mut reused = MacroHost::new();
        let expected = compile_namespace(entry, std::slice::from_ref(&root))?;
        assert_eq!(
            compile_namespace_with_macros(entry, std::slice::from_ref(&root), &mut reused)?,
            expected
        );
        for fresh in [false, true] {
            batch(
                entry,
                if fresh {
                    "filesystem-fresh-host"
                } else {
                    "filesystem-reused-host"
                },
                samples,
                iterations,
                || {
                    let js = if fresh {
                        compile_namespace(entry, std::slice::from_ref(&root))
                    } else {
                        compile_namespace_with_macros(
                            entry,
                            std::slice::from_ref(&root),
                            &mut reused,
                        )
                    }
                    .unwrap();
                    black_box(&js);
                    js.len()
                },
            );
        }
        if let Some(dir) = &dump {
            fs::write(dir.join(format!("{entry}.js")), expected)?;
        }
    }
    Ok(())
}
