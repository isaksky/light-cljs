//! The shared-heap profile of the real cljs.core (`src/js_shared_profile.js`,
//! assembled by build.rs into `SHARED_RUNTIME`; docs/real-core-design.md,
//! section 1; light-cljs-647): it boots in a boot task and publishes.

use big_boy_pants_js::frontend::compile_function;
use big_boy_pants_js::{HeapPolicy, PublishError, Rooted, RunOutcome, Runtime};
use light_cljs::runtime_manifest::SHARED_RUNTIME;

fn run(rt: &Runtime, body: &str) -> Result<Rooted, String> {
    let f =
        compile_function(&format!("function boot() {{ {body} }}")).map_err(|e| e.to_string())?;
    match rt.spawn(&f, &[]).unwrap().run() {
        Ok(RunOutcome::Complete(v)) => Ok(v),
        other => Err(format!("{other:?}")),
    }
}

/// Boots the profile in one task: its value is the sealed export object.
fn boot(rt: &Runtime) -> Rooted {
    run(rt, &format!("return (\n{SHARED_RUNTIME}\n);")).unwrap_or_else(|e| panic!("boot: {e}"))
}

/// The writes the boot made to a brand store's private fallback (`sharedView`
/// in the profile): the store and its first writer.
fn boot_store_writes(rt: &Runtime) -> String {
    let calls = run(
        rt,
        &format!(
            "globalThis.__lightSharedSetTrace = new Map(); \
             const rt = (\n{SHARED_RUNTIME}\n); \
             const out = []; \
             for (const [name, stack] of globalThis.__lightSharedSetTrace) {{ const lines = stack.split('\\n'); out.push(name + ' ' + (lines[2] ?? lines[1] ?? lines[0])); }} \
             return out.join('\\n');"
        ),
    )
    .unwrap_or_else(|e| panic!("boot: {e}"));
    calls.string().unwrap().to_string_lossy()
}

/// Every export and every member of the `core` hub published on its own,
/// grouped by cause: the member's first edge and the kind at the path's end,
/// with the labels it rejects and the first path. The count that publish
/// comes first.
fn publication_walk(rt: &Runtime) -> (usize, Vec<(String, Vec<String>, String)>) {
    let booted = run(
        rt,
        &format!(
            "const rt = (\n{SHARED_RUNTIME}\n); \
             const pairs = [...Object.keys(rt).filter(k => k !== 'core').map(k => ['rt.' + k, rt[k]]), \
               ...Object.keys(rt.core).map(k => ['core.' + k, rt.core[k]])]; \
             let labels = __rt.formVector(), values = __rt.formVector(); \
             for (const [l, v] of pairs) {{ labels = __rt.formConj(labels, l); values = __rt.formConj(values, v); }} \
             return __rt.formVector(labels, values);"
        ),
    )
    .unwrap_or_else(|e| panic!("boot: {e}"));
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

fn walk_report(shareable: usize, causes: &[(String, Vec<String>, String)]) -> String {
    let total = shareable + causes.iter().map(|(_, l, _)| l.len()).sum::<usize>();
    let mut out = format!("members {total} shareable {shareable}\n");
    for (cause, labels, first) in causes {
        out.push_str(&format!(
            "  {} | {} | first {} ({}) | e.g. {}\n",
            labels.len(),
            cause,
            labels[0],
            first,
            labels
                .iter()
                .skip(1)
                .take(8)
                .cloned()
                .collect::<Vec<_>>()
                .join(" ")
        ));
    }
    out
}

/// Acceptance 4 of light-cljs-647: the assembled profile boots in a boot
/// task and `Runtime::publish` accepts the result with zero rejected paths.
/// On failure the first rejected path is printed, then the walk of every
/// member by cause and the top-level bindings a seal cannot take. State on
/// 2026-10-31: 268 of 748 members publish; the remaining causes are in
/// docs/real-core-design.md, "Profile assembly" (light-cljs-666).
#[test]
#[ignore = "268 of 748 members publish; the remaining causes are listed in docs/real-core-design.md (light-cljs-666)"]
fn shared_profile_boots_and_publishes_with_zero_rejected_paths() {
    let rt = Runtime::new();
    let booted = boot(&rt);
    if let Err(error) = rt.publish(&booted) {
        let (shareable, causes) = publication_walk(&rt);
        panic!(
            "the booted shared-heap profile does not publish\nfirst rejected path: {error}\n{}\nboot writes to store fallbacks:\n{}",
            walk_report(shareable, &causes),
            boot_store_writes(&rt)
        );
    }
}

/// A task's cells (`sharedTaskCells` in the profile) own the state the
/// runtime assigns per task, so an assignment lands on the task's object
/// where an inherited property of the sealed hub would throw: the evaluator
/// services, the gensym counter and names, and a temporary print setting.
#[test]
fn shared_profile_task_cells_are_writable() {
    let rt = Runtime::new();
    let out = run(
        &rt,
        &format!(
            "const rt = (\n{SHARED_RUNTIME}\n); const core = rt.core; \
             const cells = globalThis.__lightSharedCells; \
             const expand = form => form; rt.setMacroexpandServices({{macroexpand: expand}}); \
             const a = core.name(core.gensym()), b = core.name(core.gensym()); \
             const before = core.prStr('a'); \
             cells.printReadably = false; const plain = core.prStr('a'); cells.printReadably = true; \
             return [cells.macroexpandServices.macroexpand === expand, a !== b, cells.gensymCounter, \
               before, plain, core.prStr('a'), Object.isFrozen(core), typeof core.sharedTaskCells().gensymCounter].join(' ');"
        ),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        out.string().unwrap().to_string_lossy(),
        "true true 2 \"a\" a \"a\" true number"
    );
}

/// A task's first cells (`core.sharedTaskCells`, which every task
/// light-cljs-648 starts calls before it reads the runtime) hold the boot's
/// defaults, not the calling task's state: a counter the previous cells
/// advanced starts over, and the factory needs no cells to exist.
#[test]
fn shared_profile_task_cells_initialize_without_existing_cells() {
    let rt = Runtime::new();
    let out = run(
        &rt,
        &format!(
            "const rt = (\n{SHARED_RUNTIME}\n); const core = rt.core; \
             const make = core.sharedTaskCells; \
             core.gensym(); core.gensym(); \
             const advanced = globalThis.__lightSharedCells.gensymCounter; \
             const next = make(); \
             globalThis.__lightSharedCells = undefined; \
             let first; try {{ first = make(); }} catch (e) {{ return 'threw ' + e.constructor.name; }} \
             globalThis.__lightSharedCells = first; \
             return [advanced, next.gensymCounter, first.gensymCounter, first.printReadably, first.tapLimit, \
               Object.getPrototypeOf(first) === Object.getPrototypeOf(next), core.name(core.gensym())].join(' ');"
        ),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        out.string().unwrap().to_string_lossy(),
        "2 0 0 true 1024 true G__1"
    );
}

/// The vector storage contract of the engine view: the verbatim operation
/// units read `cnt` and a small vector's `tail`, and the swapped storage
/// units answer `nth`, `seq`, `conj`, `assoc`, `pop`, `subvec` and a
/// transient across the 32-element boundary.
#[test]
fn shared_profile_vector_operations() {
    let rt = Runtime::new();
    let out = run(
        &rt,
        &format!(
            "const rt = (\n{SHARED_RUNTIME}\n); const core = rt.core; \
             let v = core.vector(); for (let i = 0; i < 40; i++) v = core.conj(v, i); \
             let p = v; for (let i = 0; i < 9; i++) p = core.pop(p); \
             const t = core.persistentBang(core.conjBang(core.conjBang(core.transient(v), 40), 41)); \
             return [core.count(v), core.nth(core.vector(10), 0), core.nth(v, 31), core.nth(v, 32), core.nth(v, 39), \
               core.nth(v, 45, 'none'), core.nth(core.assoc(v, 5, 'x'), 5), core.nth(core.assoc(v, 35, 'y'), 35), \
               core.count(core.assoc(v, 40, 'z')), core.count(p), core.nth(p, 30), core.peek(p), \
               core.prStr(core.vector(1, 2, 3)), core.prStr(core.rest(core.seq(core.vector(1, 2, 3)))), \
               core.count(core.seq(v)), core.nth(core.seq(v), 33), core.prStr(core.subvec(v, 30, 34)), \
               core.count(t), core.nth(t, 41), core.prStr(core.pop(core.vector(1, 2))), \
               core.hash(core.vector(1, 2)) === core.hash(core.conj(core.vector(1), 2)), \
               core.first(core.seq({{a: 1}} && core.vector(7)))].join(' ');"
        ),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        out.string().unwrap().to_string_lossy(),
        "40 10 31 32 39 none x y 41 31 30 30 [1 2 3] (2 3) 40 33 [30 31 32 33] 42 41 [1] true 7"
    );
}

/// The lazy and delay views reflect the engine once-cell's state: checking
/// `realized?` runs no thunk, a successful realization is reported once
/// made, a throwing thunk leaves the cell pending, and a `LazySeq` made
/// with no thunk holds its computed result, realized at once.
#[test]
fn shared_profile_realization_state() {
    let rt = Runtime::new();
    let out = run(
        &rt,
        &format!(
            "const rt = (\n{SHARED_RUNTIME}\n); const core = rt.core; \
             let calls = 0; \
             const pending = new core.LazySeq(null, () => {{ calls++; return core.vector(1, 2); }}, null, null); \
             const r0 = core.realized(pending), c0 = calls; \
             const f = core.first(pending), r1 = core.realized(pending), c1 = calls; \
             const d = new core.Delay(() => {{ calls++; return 42; }}, null); \
             const dr0 = core.realized(d), dc0 = calls; \
             const dv = core.deref(d), dr1 = core.realized(d); \
             const bad = new core.Delay(() => {{ throw new Error('boom'); }}, null); \
             let threw = false; try {{ core.deref(bad); }} catch (e) {{ threw = e.message === 'boom'; }} \
             const init = new core.LazySeq(null, null, __rt.formList(1), null); \
             const seeded = new core.Delay(null, 7); \
             return [r0, c0, f, r1, c1, dr0, dc0, dv, dr1, threw, core.realized(bad), \
               core.realized(init), core.first(init), core.prStr(init), core.realized(seeded), core.force(seeded)].join(' ');"
        ),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        out.string().unwrap().to_string_lossy(),
        "false 0 1 true 1 false 1 42 true true false true 1 (1) true 7"
    );
}

/// Keywords are engine keyword forms: one the engine made and one the
/// runtime's `keyword` makes are recognized by `keyword?`, named, printed,
/// equal and hashed alike through the real units.
#[test]
fn shared_profile_keywords_are_engine_keywords() {
    let rt = Runtime::new();
    let out = run(
        &rt,
        &format!(
            "const rt = (\n{SHARED_RUNTIME}\n); const core = rt.core; \
             const engine = __rt.formKeyword('ns', 'a'), made = core.keyword('ns', 'a'), plain = rt.keyword(null, 'b'); \
             return [core.isKeyword(engine), core.isKeyword(made), core.isKeyword('a'), \
               core.name(engine), core.namespace(made), String(core.namespace(plain)), \
               core.prStr(engine), core.prStr(plain), core.get(rt.arrayMapFromArray([engine, 1]), made), core.hash(engine) === core.hash(made), \
               core.isKeyword(core.keyword(core.symbol('c', 'd'))), core.prStr(core.keyword('e'))].join(' ');"
        ),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        out.string().unwrap().to_string_lossy(),
        "true true false a ns null :ns/a :b 1 true true :e"
    );
}

/// The identity hashes of host objects are the engine's (`identityHash`),
/// not a counter and WeakMap of the task: `hash` of a function is stable,
/// distinct between functions and the engine's number, and the verbatim
/// unit's counter is never reached. tests/sealing.rs in the engine covers
/// the same number in two tasks and no reuse after collection.
#[test]
fn shared_profile_identity_hashes_are_the_engines() {
    let rt = Runtime::new();
    let out = run(
        &rt,
        &format!(
            "const rt = (\n{SHARED_RUNTIME}\n); const core = rt.core; \
             const f = () => 1, g = () => 2; \
             return [core.hash(f) === core.hash(f), core.hash(f) === __rt.identityHash(f), \
               core.hash(f) !== core.hash(g), Number.isInteger(core.hash(g)), \
               globalThis.__lightSharedCells.identityCounter].join(' ');"
        ),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        out.string().unwrap().to_string_lossy(),
        "true true true true 0"
    );
}

/// A write to a view's private fallback (a runtime-owned seq kind, design
/// point 8) keeps nothing unless the task asked for the migration trace,
/// and then one stack per view.
#[test]
fn shared_profile_fallback_diagnostics_are_bounded() {
    let rt = Runtime::new();
    let out = run(
        &rt,
        &format!(
            "const rt = (\n{SHARED_RUNTIME}\n); const core = rt.core; \
             let n = 0; for (let i = 0; i < 2000; i++) n += core.count(core.repeat(2, 'x')); \
             const untraced = typeof globalThis.__lightSharedSetCalls + ' ' + typeof globalThis.__lightSharedSetTrace; \
             const trace = globalThis.__lightSharedSetTrace = new Map(); \
             for (let i = 0; i < 2000; i++) n += core.count(core.repeat(2, 'x')); \
             return [n, untraced, trace.size, [...trace.keys()].join(',')].join(' ');"
        ),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        out.string().unwrap().to_string_lossy(),
        "8000 undefined undefined 1 cells"
    );
}

/// The profile is to boot under the 4 MiB starting heap of the evaluator's
/// tests (acceptance 4 of light-cljs-647), with the method-key memos filled
/// at boot and the tables built on demand (gensym names, protocol slots)
/// first used after it. Measured on 2026-10-31 (`Runtime::new`, this
/// boot): peak 53 MB of managed bytes, 50 MB live after the boot; without
/// the seal walk 46 MB peak and 39 MB live; under a hard limit of 32 MiB or
/// less the boot fails with `HeapLimit`. The budget is open until that
/// footprint is understood (light-cljs-666 hand-back).
#[test]
#[ignore = "the shared-heap profile's boot peaks at 53 MB of managed bytes; the 4 MiB budget is open (light-cljs-666)"]
fn shared_profile_boots_with_four_mib_heap() {
    let rt = Runtime::with_heap_policy(
        Default::default(),
        HeapPolicy {
            hard_limit_bytes: Some(4 << 20),
            ..Default::default()
        },
    );
    let out = run(
        &rt,
        &format!(
            "const rt = (\n{SHARED_RUNTIME}\n); const core = rt.core; \
             return [core.name(core.gensym()).startsWith('G__'), core.prStr(core.vector(core.keyword(null, 'a'), 1)), \
               core.reduce(core.add, 0, core.vector(1, 2, 3)), core.satisfies(core.ICounted, core.vector())].join(' ');"
        ),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        out.string().unwrap().to_string_lossy(),
        "true [:a 1] 6 true"
    );
    let stats = rt.heap_stats();
    assert!(
        stats.peak_managed_bytes <= 4 << 20,
        "peak managed bytes {}",
        stats.peak_managed_bytes
    );
}

/// The writes the boot makes to the brand stores' private fallbacks, for
/// the migration list.
#[test]
#[ignore = "run by benchmarks/real-core-seam.bb"]
fn shared_profile_publication_walk_store_writes() {
    let rt = Runtime::new();
    for line in boot_store_writes(&rt).lines() {
        println!("SEAM store-write {line}");
    }
}

/// The walk of the booted profile's members by cause, for
/// `bb benchmarks/real-core-seam.bb` and the migration list.
#[test]
#[ignore = "run by benchmarks/real-core-seam.bb"]
fn shared_profile_publication_walk() {
    let rt = Runtime::new();
    let (shareable, causes) = publication_walk(&rt);
    for line in walk_report(shareable, &causes).lines() {
        println!("SEAM shared {line}");
    }
}

/// The publication path of the members named in `SHARED_MEMBERS` (space
/// separated, `rt.X` or `core.X`), for finding what a path's steps are.
#[test]
#[ignore = "diagnostic: SHARED_MEMBERS=\"core.gensym rt.typeMasks\""]
fn shared_profile_publication_path_of() {
    let rt = Runtime::new();
    let members = std::env::var("SHARED_MEMBERS").unwrap_or_default();
    for member in members.split_whitespace() {
        let (owner, name) = member.split_once('.').unwrap();
        let value = run(
            &rt,
            &format!(
                "const rt = (\n{SHARED_RUNTIME}\n); return {}['{name}'];",
                if owner == "core" { "rt.core" } else { "rt" }
            ),
        )
        .unwrap_or_else(|e| panic!("boot: {e}"));
        println!(
            "PATH {member}: {}",
            match rt.publish(&value) {
                Ok(()) => "publishes".to_string(),
                Err(e) => e.to_string(),
            }
        );
    }
}
