//! light-cljs-645 spike (docs/real-core-design.md, "Spike"): the real
//! cljs.core's units on the Pants engine. Throwaway experiment code.

use big_boy_pants_js::frontend::compile_function;
use big_boy_pants_js::{PublishError, Rooted, RunOutcome, Runtime};
use light_cljs::runtime_manifest::{RootUse, RuntimeManifest};

/// The runtime factory of the units `roots` reach (`reached_by_uses`, as
/// calls), with the bodies of the units named in `swaps` replaced, returning
/// `returned`.
fn assemble(roots: &[&str], swaps: &[(&str, &str)], returned: &str) -> String {
    let m = RuntimeManifest::embedded();
    let roots = roots
        .iter()
        .map(|n| (m.unit_of(n).unwrap(), RootUse::Call))
        .collect::<Vec<_>>();
    let kept = m.reached_by_uses(&roots);
    let mut text = m.source()[m.prologue.clone()].to_owned();
    for (unit, kept) in m.units.iter().zip(&kept) {
        if kept.is_none() {
            continue;
        }
        match swaps.iter().find(|(name, _)| *name == unit.name) {
            Some((_, body)) => {
                text.push_str(body);
                text.push('\n');
            }
            None => text.push_str(&m.source()[unit.body_range.clone()]),
        }
    }
    text.push_str(&format!("  return {returned};\n}})()\n"));
    text
}

fn run(rt: &Runtime, body: &str) -> Result<Rooted, String> {
    let f =
        compile_function(&format!("function spike() {{ {body} }}")).map_err(|e| e.to_string())?;
    match rt.spawn(&f, &[]).unwrap().run() {
        Ok(RunOutcome::Complete(v)) => Ok(v),
        other => Err(format!("{other:?}")),
    }
}

fn describe(e: PublishError) -> String {
    e.to_string()
}

/// The spike's shared-heap profile: the representation units `map` over a
/// vector reaches, rewritten as views over engine forms (design point 3)
/// and constructors of engine values (point 2). Every other unit is the
/// runtime's own text.
const SWAPS: &[(&str, &str)] = &[
    (
        "vectors",
        r#"  const vectors = {
    has: value => __rt.formKind(value) === "vector",
    get: value => __rt.formKind(value) === "vector" ? {cnt: __rt.formCount(value)} : undefined,
    set() { throw new TypeError("shared profile: vectors.set"); }
  };"#,
    ),
    (
        "cells",
        r#"  const cells = {
    has: value => { const kind = __rt.formKind(value); return kind === "cons" || kind === "chunked-cons" || kind === "list"; },
    get(value) {
      const kind = __rt.formKind(value);
      if (kind === "list") return {kind, head: __rt.cljFirst(value), tail: __rt.cljRest(value), count: __rt.cljCount(value)};
      if (kind === "cons" || kind === "chunked-cons") return {kind: "cons", head: __rt.cljFirst(value), tail: __rt.cljRest(value)};
    },
    set() { throw new TypeError("shared profile: cells.set"); }
  };"#,
    ),
    (
        "lazyValues",
        r#"  const lazyValues = {
    has: value => __rt.formKind(value) === "lazy-seq",
    get: value => __rt.formKind(value) !== "lazy-seq" ? undefined
      : {thunk: null, get value() { return __rt.cljSeq(value); }, set value(_) {}},
    set() { throw new TypeError("shared profile: lazyValues.set"); }
  }, delays = new WeakMap();"#,
    ),
    (
        "LazySeq",
        r#"  function LazySeq(metadata, thunk, result, cachedHash) {
    const value = __rt.lazySeq(thunk);
    return metadata !== null ? __rt.cljWithMeta(value, metadata) : value;
  }"#,
    ),
    (
        "kindHooks",
        r#"  const kindHooks = {seqVector: value => __rt.cljSeq(value)};"#,
    ),
    ("emptyList", "  const emptyList = __rt.formList();"),
    (
        "cons",
        r#"  function cons(head, tail, kind = "cons") {
    return tail == null || kind === "list" && __rt.cljCount(tail) === 0 ? __rt.formList(head) : __rt.cljCons(head, tail);
  }"#,
    ),
];

/// `mapSeq(f, [1 2 3])` over an engine vector, walked twice with the
/// engine's own `seq`/`first`/`next`, as `kind items... calls n`.
fn map_twice(rt: &Runtime, text: &str) -> Result<Rooted, String> {
    run(
        rt,
        &format!(
            "const mapSeq = (\n{text}\n); let calls = 0; \
             const xs = mapSeq(x => (calls++, x + 1), __rt.formVector(1, 2, 3)); \
             const kind = __rt.formKind(xs), items = []; \
             for (let s = __rt.cljSeq(xs); s != null; s = __rt.cljNext(s)) items.push(__rt.cljFirst(s)); \
             for (let s = __rt.cljSeq(xs); s != null; s = __rt.cljNext(s)) items.push(__rt.cljFirst(s)); \
             return kind + ' ' + items.join(' ') + ' calls ' + calls;"
        ),
    )
}

/// The verbatim units cannot take the seq of an engine vector (it has no
/// brand they know and no iterator); with [`SWAPS`] the runtime's own
/// `mapSeq`, `seq`, `first`, `rest` and `next` build an engine lazy
/// sequence whose cells realize once. Neither assembled `mapSeq`
/// publishes: its closure reaches binding cells.
#[test]
fn real_core_map_over_an_engine_vector_in_one_task() {
    let rt = Runtime::new();
    let verbatim = assemble(&["mapSeq"], &[], "mapSeq");
    // Its `seq` reaches `throw new Error(String(value) + " is not
    // ISeqable")`, and an engine vector has no string conversion either.
    let failed = run(
        &rt,
        &format!(
            "const mapSeq = (\n{verbatim}\n); \
             return Array.from(mapSeq(x => x + 1, __rt.formVector(1, 2, 3))).join(' ');"
        ),
    )
    .unwrap_err();
    assert!(
        failed.contains("cannot convert object to primitive value"),
        "{failed}"
    );
    let swapped = assemble(&["mapSeq"], SWAPS, "mapSeq");
    let out = map_twice(&rt, &swapped).unwrap();
    assert_eq!(
        out.string().unwrap().to_string_lossy(),
        "lazy-seq 2 3 4 2 3 4 calls 3"
    );
    for text in [&verbatim, &swapped] {
        let f = run(&rt, &format!("return (\n{text}\n);")).unwrap();
        let error = rt.publish(&f).unwrap_err().to_string();
        assert!(
            error.contains("value.capture(2) is a task-owned binding cell"),
            "{error}"
        );
    }
}

/// Boots `factory` (an expression whose value is the object of exports) in
/// one task and publishes each of the named members separately, `names`
/// being a JavaScript expression over `rt` that lists `[label, value]`
/// pairs. Returns, per cause (the rejected path with indices and property
/// names elided, and the kind found there), the labels it rejects, and the
/// count that publish.
fn publication_walk(factory: &str, pairs: &str) -> (usize, Vec<(String, Vec<String>, String)>) {
    let rt = Runtime::new();
    let booted = run(
        &rt,
        &format!(
            "const rt = (\n{factory}\n); const pairs = {pairs}; \
             let labels = __rt.formVector(), values = __rt.formVector(); \
             for (const [l, v] of pairs) {{ labels = __rt.formConj(labels, l); values = __rt.formConj(values, v); }} \
             return __rt.formVector(labels, values);"
        ),
    )
    .unwrap();
    let (labels, values) = rt.scope(|s| {
        let booted = s.local(&booted).unwrap();
        let labels = s.form_nth(booted, 0).unwrap();
        let values = s.form_nth(booted, 1).unwrap();
        let n = s.form_count(labels).unwrap();
        (
            (0..n)
                .map(|i| {
                    s.string(s.form_nth(labels, i).unwrap())
                        .unwrap()
                        .to_string_lossy()
                })
                .collect::<Vec<_>>(),
            (0..n)
                .map(|i| s.persist(s.form_nth(values, i).unwrap()))
                .collect::<Vec<_>>(),
        )
    });
    let mut shareable = 0;
    let mut causes: Vec<(String, Vec<String>, String)> = Vec::new();
    for (label, value) in labels.into_iter().zip(&values) {
        match rt.publish(value) {
            Ok(()) => shareable += 1,
            Err(PublishError::NotShareable { path, found, .. }) => {
                use big_boy_pants_js::PathStep;
                // A cause is the member's first edge (a closure's capture, a
                // function's own-property record, or the member is itself an
                // ordinary object or array) and the kind of the rejected
                // object at the path's end.
                let edge = match path.steps().first() {
                    Some(PathStep::Capture(_)) => "closure capture",
                    Some(PathStep::FunctionProperties) => "function own properties",
                    Some(PathStep::HomeObject) => "method home object",
                    None | Some(PathStep::Property(_) | PathStep::Element(_)) => {
                        "member is an ordinary object"
                    }
                    Some(_) => "form element",
                };
                let cause = format!("{edge} -> {found}");
                match causes.iter_mut().find(|(c, _, _)| *c == cause) {
                    Some((_, labels, _)) => labels.push(label),
                    None => causes.push((cause, vec![label], path.to_string())),
                }
            }
            Err(other) => panic!("{label}: {other}"),
        }
    }
    causes.sort_by_key(|(_, labels, _)| std::cmp::Reverse(labels.len()));
    (shareable, causes)
}

/// The publication walk of a naive boot of the whole compile-time runtime
/// (`COMPILER_RUNTIME`, as the V8 build boots it): every export and every
/// member of the `core` hub published on its own. Printed for
/// `benchmarks/real-core-seam.bb`; asserts nothing about the counts.
#[test]
#[ignore = "run by benchmarks/real-core-seam.bb"]
fn real_core_publication_walk() {
    let (shareable, causes) = publication_walk(
        light_cljs::runtime_manifest::COMPILER_RUNTIME,
        "[...Object.keys(rt).filter(k => k !== 'core').map(k => ['rt.' + k, rt[k]]), \
          ...Object.keys(rt.core).map(k => ['core.' + k, rt.core[k]])]",
    );
    print_walk("full", shareable, &causes);
}

/// The same walk over the units `map` reaches, with the spike's swapped
/// representation units ([`SWAPS`]): every top-level binding on its own.
#[test]
#[ignore = "run by benchmarks/real-core-seam.bb"]
fn real_core_map_slice_publication_walk() {
    let m = RuntimeManifest::embedded();
    let kept = m.reached_by_uses(&[(m.unit_of("mapSeq").unwrap(), RootUse::Call)]);
    let bindings = m
        .units
        .iter()
        .zip(&kept)
        .filter(|(_, k)| k.is_some())
        .flat_map(|(u, _)| u.declares.iter().copied())
        .collect::<Vec<_>>();
    let factory = assemble(&["mapSeq"], SWAPS, &format!("{{{}}}", bindings.join(", ")));
    let (shareable, causes) = publication_walk(&factory, "Object.entries(rt)");
    print_walk("map-slice", shareable, &causes);
}

fn print_walk(walk: &str, shareable: usize, causes: &[(String, Vec<String>, String)]) {
    let total = shareable + causes.iter().map(|(_, l, _)| l.len()).sum::<usize>();
    println!("SEAM {walk} members {total} shareable {shareable}");
    for (cause, labels, first) in causes {
        println!(
            "SEAM cause {} | {} | first {} ({}) | e.g. {}",
            labels.len(),
            cause,
            labels[0],
            first,
            labels
                .iter()
                .skip(1)
                .take(6)
                .cloned()
                .collect::<Vec<_>>()
                .join(" ")
        );
    }
}

/// Acceptance 1: the real core's `map` (the `mapSeq` unit and the 151 units
/// it reaches, with [`SWAPS`]) runs in a Pants-build macro over a vector the
/// compiled macro built as an engine form, and the program is
/// byte-identical to the reference build's, where `map` is the reference's
/// own core. The helper boots the assembled units in its initializer; on
/// the V8 build, the QuickJS reference and in the emitted program it is
/// `cljs.core/map`. Records why the helper cannot publish yet (acceptance
/// 2): it initializes once per consuming task.
#[test]
fn real_core_map_runs_in_a_pants_macro() {
    use light_cljs::compiler::{EntryRecorders, analyze_namespace_entry_recorded};
    let slice = assemble(&["mapSeq"], SWAPS, "mapSeq");
    assert!(!slice.contains("~{"), "js* would interpolate the slice");
    let escaped = slice.replace('\\', "\\\\").replace('"', "\\\"");
    let root = super::temp_dir::TempDir::new("real-core-map");
    for (file, source) in [
        (
            "parts.cljs".to_owned(),
            format!(
                "(ns parts (:refer-clojure :exclude [map]))
                 (def map (if (exists? js/globalThis.__lightNativeRuntime) (js* \"(\n{escaped}\n)\") cljs.core/map))
                 (defmacro m [] (vec (map inc [1 2 3])))
                 (defmacro which [] (str (.-name map) \" \" (js* \"__rt.formKind(~{{}})\" (map inc [1]))))"
            ),
        ),
        (
            "probe.cljs".to_owned(),
            "(ns probe (:require [parts :refer [which]])) (js/console.log (which))".to_owned(),
        ),
        (
            "a.cljs".to_owned(),
            "(ns a (:require [parts :refer [m]])) (def value (m))".to_owned(),
        ),
        (
            "b.cljs".to_owned(),
            "(ns b (:require [parts :refer [m]])) (def value (m))".to_owned(),
        ),
        (
            "app.cljs".to_owned(),
            "(ns app (:require [a] [b])) (js/console.log (pr-str [a/value b/value]))".to_owned(),
        ),
    ] {
        std::fs::write(root.join(&file), source).unwrap();
    }
    let build_entry = |entry: &str, native: bool, jobs: usize, seed: u64| {
        let mut host = light_cljs::MacroHost::new();
        if native {
            host.use_native_macros();
        }
        let mut stats = light_cljs::CompileStats::default();
        let (program, _) = analyze_namespace_entry_recorded(
            entry,
            &[root.path().to_path_buf()],
            &mut host,
            Default::default(),
            &mut Vec::new(),
            EntryRecorders {
                stats: Some(&mut stats),
                slots: light_cljs::scheduler::Slots::new(jobs).unwrap(),
                #[cfg(any(debug_assertions, feature = "scheduling-stress"))]
                perturbation: native.then_some(light_cljs::scheduler::Perturbation {
                    seed,
                    limits: Some(light_cljs::scheduler::Limits {
                        batch_jobs: 1,
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            },
        )
        .unwrap_or_else(|e| panic!("native {native}, jobs {jobs}, seed {seed}: {e}"));
        let _ = seed;
        (light_cljs::emit_js(&program), stats.native_helpers)
    };
    let build = |native: bool, jobs: usize, seed: u64| build_entry("app", native, jobs, seed);
    // The helper is the real core's `mapSeq`, and its result an engine lazy
    // sequence.
    assert_eq!(
        super::program_output(&build_entry("probe", true, 1, 0).0),
        "mapSeq lazy-seq\n"
    );
    let (pants, helpers) = build(true, 2, 1);
    assert_eq!(super::program_output(&pants), "[[2 3 4] [2 3 4]]\n");
    // The booted `map` closes over binding cells and the runtime's
    // registries, so it does not publish: each task that expands `m`
    // boots its own copy (light-cljs-646 makes it publish).
    assert_eq!(helpers.published, 0, "{helpers:?}");
    assert!(helpers.per_task >= 1, "{helpers:?}");
    assert_eq!(pants, build(true, 1, 0).0);
    #[cfg(any(feature = "quickjs", feature = "v8-macros"))]
    assert_eq!(pants, build(false, 2, 0).0);
}

/// Why no closure of the runtime publishes today: the engine keeps a
/// binding in a task-owned cell when a closure captures it before its
/// initialization (a function declaration called before it, a later
/// `const`: every mutually recursive pair of units) or when it is
/// reassigned (the load-time wrappers, `seq = behindProtocol(seq, ...)`).
/// Only a binding initialized before the closure and never assigned is
/// captured as its value. light-cljs-646 changes the rejected rows.
#[test]
fn closures_over_forward_or_reassigned_bindings_do_not_publish() {
    let rt = Runtime::new();
    for (body, publishes) in [
        ("const a = 1; return () => a;", true),
        ("let g = () => 1; return () => g();", true),
        (
            "function f() { return g(); } function g() { return 1; } return f;",
            false,
        ),
        ("const f = () => g(); const g = () => 1; return f;", false),
        (
            "let g = () => 1; g = (h => () => h())(g); return () => g();",
            false,
        ),
        (
            "function g() { return 1; } g = (h => () => h())(g); return () => g();",
            false,
        ),
    ] {
        let f = run(&rt, body).unwrap();
        match rt.publish(&f) {
            Ok(()) => assert!(publishes, "{body}"),
            Err(e) => {
                assert!(!publishes, "{body}: {e}");
                assert!(
                    describe(e).contains("capture(0) is a task-owned binding cell"),
                    "{body}"
                );
            }
        }
    }
}
