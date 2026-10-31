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
    // Shareable once booted (light-cljs-646): the captured rest array is
    // sealed.
    (
        "storesPredicate",
        r#"  function storesPredicate(...stores) {
    __rt.seal(stores);
    return value => {
      for (let i = 0; i < stores.length; i++) if (stores[i].has(value)) return true;
      return false;
    };
  }"#,
    ),
    // Shareable once booted (light-cljs-646): the arities array is sealed
    // and the method key resolved at boot, not memoized in a `let` the
    // dispatcher assigns.
    (
        "coreMethod",
        r#"  function coreMethod(key, arities, fallback, native) {
    __rt.seal(arities);
    const resolved = protocolHooks.methodKey?.(key);
    const dispatch = function (value, ...args) {
      const arity = arities.length === 1 ? arities[0] : arguments.length;
      if (!arities.includes(arity)) return invalidArity(arguments.length);
      if (arities.length === 1) args = args.slice(0, arity - 1);
      const method = protocolHooks.builtin?.(value, resolved ?? protocolHooks.methodKey(key), arity);
      if (method != null) return method.call(value, value, ...args);
      if (native(value)) return fallback(value, ...args);
      throw new Error("No protocol method " + key + " defined for this value");
    };
    dispatchCell(key).dispatcher = dispatch;
    return dispatch;
  }"#,
    ),
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
/// sequence whose cells realize once.
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
    // As booted, `mapSeq` reaches a binding the runtime reassigned while
    // loading (`seq = behindProtocol(seq, ...)`); sealed at the end of the
    // boot, with its registries, it publishes (light-cljs-646).
    for text in [&verbatim, &swapped] {
        let f = run(&rt, &format!("return (\n{text}\n);")).unwrap();
        let error = rt.publish(&f).unwrap_err().to_string();
        assert!(
            error.contains("is a task-owned unsealed binding cell"),
            "{error}"
        );
    }
    let shared = run(&rt, &format!("return (\n{}\n);", shared_map_slice())).unwrap();
    rt.publish(&shared).unwrap();
    let out = map_twice(&rt, &shared_map_slice()).unwrap();
    assert_eq!(
        out.string().unwrap().to_string_lossy(),
        "lazy-seq 2 3 4 2 3 4 calls 3"
    );
}

/// Boots `factory` (an expression whose value is the object of exports) in
/// one task and publishes each of the named members separately, `names`
/// being a JavaScript expression over `rt` that lists `[label, value]`
/// pairs. Returns, per cause (the rejected path with indices and property
/// names elided, and the kind found there), the labels it rejects, and the
/// count that publish.
fn publication_walk(
    factory: &str,
    seal: &str,
    pairs: &str,
) -> (usize, Vec<(String, Vec<String>, String)>) {
    let rt = Runtime::new();
    let booted = run(
        &rt,
        &format!(
            "const rt = (\n{factory}\n); {seal} const pairs = {pairs}; \
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
/// member of the `core` hub published on its own, first as booted
/// (`full`), then after the boot task seals the bindings the runtime
/// reassigned while loading (`full-sealed`, `__rt.sealBindings`,
/// light-cljs-646). Printed for `benchmarks/real-core-seam.bb`; asserts
/// nothing about the counts.
#[test]
#[ignore = "run by benchmarks/real-core-seam.bb"]
fn real_core_publication_walk() {
    let members = "[...Object.keys(rt).filter(k => k !== 'core').map(k => ['rt.' + k, rt[k]]), \
          ...Object.keys(rt.core).map(k => ['core.' + k, rt.core[k]])]";
    for (walk, seal) in [("full", ""), ("full-sealed", "__rt.sealBindings(rt);")] {
        let (shareable, causes) = publication_walk(
            light_cljs::runtime_manifest::COMPILER_RUNTIME,
            seal,
            members,
        );
        print_walk(walk, shareable, &causes);
    }
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
    let (shareable, causes) =
        publication_walk(&factory, "__rt.sealBindings(rt);", "Object.entries(rt)");
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
/// own core. The helper boots the assembled units in its initializer and
/// seals them ([`shared_map_slice`]); on the V8 build, the QuickJS
/// reference and in the emitted program it is `cljs.core/map`. Acceptance
/// 2: the booted `map` publishes, so the macro tasks on two threads share
/// the one helper value instead of booting a copy each.
#[test]
fn real_core_map_runs_in_a_pants_macro() {
    use light_cljs::compiler::{EntryRecorders, analyze_namespace_entry_recorded};
    let slice = shared_map_slice();
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
    // The booted, sealed `map` publishes (light-cljs-646): one helper
    // value is shared by the tasks that expand `m`.
    assert!(helpers.published >= 1, "{helpers:?}");
    assert_eq!(helpers.per_task, 0, "{helpers:?}");
    assert_eq!(pants, build(true, 1, 0).0);
    #[cfg(any(feature = "quickjs", feature = "v8-macros"))]
    assert_eq!(pants, build(false, 2, 0).0);
}

/// Why no closure of the runtime published before light-cljs-646: the
/// engine kept a binding in a task-owned cell when a closure captures it
/// before its initialization (a function declaration called before it, a
/// later `const`: every mutually recursive pair of units) or when it is
/// reassigned (the load-time wrappers, `seq = behindProtocol(seq, ...)`).
/// Now a binding nothing assigns is fixed once initialized and publishes,
/// and one its own function reassigns publishes once the boot task seals
/// it (`__rt.sealBindings`).
#[test]
fn closures_over_forward_or_reassigned_bindings_publish_once_fixed_or_sealed() {
    let rt = Runtime::new();
    for (body, needs_seal) in [
        ("const a = 1; const r = () => a;", false),
        ("let g = () => 1; const r = () => g();", false),
        (
            "function f() { return g(); } function g() { return 1; } const r = f;",
            false,
        ),
        (
            "const f = () => g(); const g = () => 1; const r = f;",
            false,
        ),
        (
            "let g = () => 1; g = (h => () => h())(g); const r = () => g();",
            true,
        ),
        (
            "function g() { return 1; } g = (h => () => h())(g); const r = () => g();",
            true,
        ),
    ] {
        let f = run(&rt, &format!("{body} return r;")).unwrap();
        match rt.publish(&f) {
            Ok(()) => assert!(!needs_seal, "{body}"),
            Err(e) => {
                assert!(needs_seal, "{body}: {e}");
                assert!(
                    describe(e).contains("capture(0) is a task-owned unsealed binding cell"),
                    "{body}"
                );
            }
        }
        let sealed = run(&rt, &format!("{body} __rt.sealBindings(r); return r;")).unwrap();
        rt.publish(&sealed)
            .unwrap_or_else(|e| panic!("{body}: {e}"));
    }
}

/// The units `map` reaches with [`SWAPS`], sealed at the end of their boot
/// for sharing (light-cljs-646): the boot-time registries, sentinels and
/// prototypes the slice declares are sealed in place (`__rt.seal`, deeply
/// through the arrays and objects that hold them), then the bindings it
/// reassigned while loading (`__rt.sealBindings`). The value is `mapSeq`.
/// Throwaway profile code: light-cljs-647 assembles the real profile.
fn shared_map_slice() -> String {
    let m = RuntimeManifest::embedded();
    let kept = m.reached_by_uses(&[(m.unit_of("mapSeq").unwrap(), RootUse::Call)]);
    let bindings = m
        .units
        .iter()
        .zip(&kept)
        .filter(|(_, k)| k.is_some())
        .flat_map(|(u, _)| u.declares.iter().copied())
        .collect::<Vec<_>>()
        .join(", ");
    let seal = format!(
        "((seal, bindings) => (bindings.forEach(x => seal(x)), __rt.sealBindings(bindings), mapSeq))(\
           function seal(x, seen = new Set()) {{\
             if (x === null || (typeof x !== 'object' && typeof x !== 'function') || seen.has(x)) return;\
             seen.add(x);\
             for (const k of Reflect.ownKeys(x)) {{\
               const d = Object.getOwnPropertyDescriptor(x, k);\
               if ('value' in d) seal(d.value, seen); else {{ seal(d.get, seen); seal(d.set, seen); }}\
             }}\
             const proto = Object.getPrototypeOf(x);\
             if (proto !== null && proto !== Object.prototype && proto !== Array.prototype && proto !== Function.prototype) seal(proto, seen);\
             try {{ __rt.seal(x); }} catch (e) {{ if (!(e instanceof TypeError)) throw e; }}\
           }}, [{bindings}])"
    );
    // The slice's other brand stores (WeakMaps and WeakSets: keywords,
    // symbols, metadata, map entries, ...) never hold an engine value on
    // the path of `map` over a vector; they become one sealed, empty,
    // read-only store.
    let text = assemble(&["mapSeq"], SWAPS, &seal);
    let prologue = m.source()[m.prologue.clone()].len();
    format!(
        "{}  const sharedEmptyStore = __rt.seal({{has: () => false, get: () => undefined, \
         delete: () => false, set: () => {{ throw new TypeError('shared profile: read-only store'); }}, \
         add: () => {{ throw new TypeError('shared profile: read-only store'); }}}});\n{}",
        &text[..prologue],
        text[prologue..]
            .replace("new WeakMap()", "sharedEmptyStore")
            .replace("new WeakSet()", "sharedEmptyStore")
    )
}

/// Counts one task's calls of the mapped function (`__rt.effect(1, …)`);
/// its first call waits at a barrier shared with the other task, so both
/// tasks are inside the one shared `map` at the same time.
#[derive(Clone)]
struct MappedCalls {
    calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    together: std::sync::Arc<std::sync::Barrier>,
}

impl big_boy_pants_js::Host for MappedCalls {
    fn effect(
        &mut self,
        call: big_boy_pants_js::HostCall,
    ) -> Result<big_boy_pants_js::Primitive, String> {
        assert_eq!(call.op, 1);
        if self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
            self.together.wait();
        }
        Ok(big_boy_pants_js::Primitive::Undefined)
    }
}

/// Acceptance 2 of the spike, on the engine directly: the real core's
/// `map` slice ([`shared_map_slice`]) boots once in one task and its
/// `mapSeq` publishes. Two tasks on two threads call that one shared
/// function at the same time (a barrier holds each inside its first call
/// of the mapped function until both are there) and each walks its lazy
/// result twice: the mapped function runs once per element in each task.
/// A lazy result itself does not publish: the real `mapSeq`'s thunk
/// captures its rest-argument array (`colls`), which is task-owned.
#[test]
fn one_shared_real_core_map_runs_on_two_threads_and_realizes_once() {
    use big_boy_pants_js::Budget;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Barrier};
    let rt = Runtime::new();
    let slice = shared_map_slice();
    let map = run(&rt, &format!("return (\n{slice}\n);")).unwrap();
    rt.publish(&map).unwrap_or_else(|e| panic!("{e}"));
    let use_map = compile_function(
        "function useMap(mapSeq, publish) { \
           const xs = mapSeq(x => (__rt.effect(1, x), x + 1), __rt.formVector(1, 2, 3)); \
           if (publish) return xs; \
           const out = []; \
           for (let pass = 0; pass < 2; pass++) \
             for (let s = __rt.cljSeq(xs); s != null; s = __rt.cljNext(s)) out.push(__rt.cljFirst(s)); \
           return __rt.formKind(xs) + ' ' + out.join(' '); }",
    )
    .unwrap();
    let together = Arc::new(Barrier::new(2));
    let walk = rt.root(big_boy_pants_js::Primitive::Bool(false));
    let tasks = [0, 1].map(|_| rt.spawn(&use_map, &[&map, &walk]).unwrap());
    let results = std::thread::scope(|s| {
        let handles = tasks
            .iter()
            .map(|task| {
                let mut host = MappedCalls {
                    calls: Arc::new(AtomicUsize::new(0)),
                    together: together.clone(),
                };
                s.spawn(move || {
                    let outcome = task.run_with(Budget::UNLIMITED, &mut host).unwrap();
                    let RunOutcome::Complete(v) = outcome else {
                        panic!("{outcome:?}")
                    };
                    (
                        v.string().unwrap().to_string_lossy(),
                        host.calls.load(Ordering::SeqCst),
                    )
                })
            })
            .collect::<Vec<_>>();
        handles
            .into_iter()
            .map(|h| h.join().unwrap())
            .collect::<Vec<_>>()
    });
    let walked = ("lazy-seq 2 3 4 2 3 4".to_owned(), 3);
    assert_eq!(results, [walked.clone(), walked]);
    // The lazy result of the shared `map` stays task-owned.
    let publish = rt.root(big_boy_pants_js::Primitive::Bool(true));
    let xs = match rt
        .spawn(&use_map, &[&map, &publish])
        .unwrap()
        .run()
        .unwrap()
    {
        RunOutcome::Complete(v) => v,
        other => panic!("{other:?}"),
    };
    let error = rt.publish(&xs).unwrap_err().to_string();
    assert!(
        error.contains("value.thunk.capture(0) is a task-owned array"),
        "{error}"
    );
}
