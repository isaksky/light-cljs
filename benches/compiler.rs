//! Resident phases; raw output is consumed by benchmarks/run_compiler.py.
use light_cljs::{
    ApplicationOptions, MacroHost, NamespaceInput, Program, SyntaxTree, analyze,
    analyze_namespaces, analyze_with_macros, compile, compile_namespace,
    compile_namespace_with_macros, compile_with_macros, emit_js, parse, prepare_application,
};
#[cfg(not(feature = "count-allocations"))]
use std::time::Instant;
use std::{
    env,
    error::Error,
    fs,
    hint::black_box,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};
#[cfg(feature = "count-allocations")]
#[path = "support/allocations.rs"]
mod allocations;
#[cfg(feature = "count-allocations")]
#[global_allocator]
static ALLOCATOR: allocations::CountingAllocator = allocations::CountingAllocator;

const COLLECTION: &str = include_str!("../benchmarks/fixtures/compiler/collections/value.cljs");
const MACRO_CALL: &str = "(defmacro data [] '[1 2 3]) (data)";
const DATA: &str = include_str!("../benchmarks/fixtures/compiler/collections/bench/data.cljs");
const MAIN: &str = include_str!("../benchmarks/fixtures/compiler/collections/bench/main.cljs");
const SINGLE: &str = include_str!("../benchmarks/fixtures/compiler/collections/bench/single.cljs");

fn host() -> MacroHost {
    MacroHost::new()
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

// This focused path measures primitives separately from macro and application setup.
fn application_node(js: &str) -> Vec<u8> {
    let mut child = Command::new(env::var_os("NODE").unwrap_or_else(|| "node".into()))
        .args([
            "--eval",
            "console.log(JSON.stringify(eval(require('fs').readFileSync(0,'utf8'))));",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("application benchmarks require Node");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(js.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

fn application_phases(
    name: &str,
    source: &str,
    samples: usize,
    iterations: usize,
    dump: Option<&PathBuf>,
) -> Result<(), Box<dyn Error>> {
    let tree = parse(source)?;
    let program = analyze(source, &tree)?;
    let unoptimized = prepare_application(
        &program,
        ApplicationOptions {
            optimize: false,
            exports: vec![],
        },
    );
    let expected = application_node(&unoptimized.emit_js());
    for optimize in [false, true] {
        let scenario = format!("{name}-{}", if optimize { "on" } else { "off" });
        let options = ApplicationOptions {
            optimize,
            exports: vec![],
        };
        let prepared = prepare_application(&program, options.clone());
        let js = prepared.emit_js();
        assert_eq!(application_node(&js), expected, "{scenario}");
        batch(&scenario, "prepare", samples, iterations, || {
            let application = prepare_application(black_box(&program), options.clone());
            black_box(&application);
            let stats = application.stats();
            stats.retained_globals + stats.removed_definitions + stats.specialized_calls
        });
        // The emit checksum is emitted UTF-8 bytes per iteration.
        batch(&scenario, "prepared-emit", samples, iterations, || {
            let output = prepared.emit_js();
            black_box(&output);
            output.len()
        });
        // Includes fresh process startup, JS parsing/evaluation and output validation.
        // Child allocations are not included in the optional Rust allocator counts.
        batch(
            &scenario,
            "node-startup-execute",
            samples,
            iterations,
            || {
                let output = application_node(black_box(&js));
                assert_eq!(output, expected, "{scenario}");
                output.len()
            },
        );
        if let Some(dir) = dump {
            fs::write(dir.join(format!("{scenario}.js")), js)?;
        }
    }
    Ok(())
}

fn application_benchmarks(
    samples: usize,
    iterations: usize,
    dump: Option<&PathBuf>,
) -> Result<(), Box<dyn Error>> {
    for (name, source) in primitive_sources() {
        application_phases(name, source, samples, iterations, dump)?;
    }
    application_phases("collections", COLLECTION, samples, iterations, dump)?;
    let mut source = String::from(
        "(def factor 2) (defn twice [x] (* factor x)) (defmacro doubled [x] (twice x))\n",
    );
    // Bounded generated source provides 24 ordinary helpers used only in expansion.
    for n in 0..24 {
        source.push_str(&format!("(def constant{n} {n}) (defn helper{n} [x] (+ constant{n} x)) (defmacro macro{n} [x] (helper{n} x))\n"));
    }
    source.push_str("[(doubled 21) (twice 21)");
    for n in 0..24 {
        source.push_str(&format!(" (macro{n} 1)"));
    }
    source.push(']');
    application_phases("macro-helpers", &source, samples, iterations, dump)
}

fn main() -> Result<(), Box<dyn Error>> {
    let (mut samples, mut iterations) = (9, 32);
    let (mut counted, mut primitives, mut application_only) = (false, false, false);
    let mut dump = None;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--samples" => samples = args.next().ok_or("missing samples")?.parse()?,
            "--iterations" => iterations = args.next().ok_or("missing iterations")?.parse()?,
            "--allocations" => counted = true,
            "--primitive-only" => primitives = true,
            "--application-only" => application_only = true,
            "--dump" => dump = Some(PathBuf::from(args.next().ok_or("missing dump directory")?)),
            "--bench" => {}
            "--help" | "-h" => {
                println!(
                    "compiler [--samples N>=9] [--iterations N] [--allocations] [--primitive-only] [--application-only] [--dump DIR]"
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
    if application_only {
        return application_benchmarks(samples, iterations, dump.as_ref());
    }
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
    assert_eq!(
        application_node(&compile_with_macros(MACRO_CALL, &mut reused)?),
        application_node(&expected),
    );
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
