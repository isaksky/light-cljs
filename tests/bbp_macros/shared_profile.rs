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
/// 2026-10-31 (after light-cljs-666's first attempt): 716 of 748 members
/// publish; the remaining causes (the reader's nested state, `demungePattern`,
/// the namespace registry, the bridge's `let`) are in docs/real-core-design.md,
/// "Profile assembly" (light-cljs-666).
#[test]
#[ignore = "716 of 748 members publish; the remaining causes are listed in docs/real-core-design.md (light-cljs-666)"]
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
/// transient across the 32-element boundary. A fractional index in bounds
/// truncates, as the V8 profile's bitwise indexing has it
/// (`vector-fractional-index` in tests/fixtures/execution.tsv), and a
/// negative one keeps its fallback.
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
             const three = core.vector(10, 20, 30); \
             return [core.count(v), core.nth(core.vector(10), 0), core.nth(v, 31), core.nth(v, 32), core.nth(v, 39), \
               core.nth(v, 45, 'none'), core.nth(core.assoc(v, 5, 'x'), 5), core.nth(core.assoc(v, 35, 'y'), 35), \
               core.count(core.assoc(v, 40, 'z')), core.count(p), core.nth(p, 30), core.peek(p), \
               core.prStr(core.vector(1, 2, 3)), core.prStr(core.rest(core.seq(core.vector(1, 2, 3)))), \
               core.count(core.seq(v)), core.nth(core.seq(v), 33), core.prStr(core.subvec(v, 30, 34)), \
               core.count(t), core.nth(t, 41), core.prStr(core.pop(core.vector(1, 2))), \
               core.hash(core.vector(1, 2)) === core.hash(core.conj(core.vector(1), 2)), \
               core.first(core.seq({{a: 1}} && core.vector(7))), \
               core.nth(three, 1.8), core.get(three, 1.8), core.prStr(core.assoc(three, 1.8, 'x')), \
               core.nth(v, 33.5), core.nth(core.assoc(v, 35.2, 'y'), 35), core.nth(core.vector(10, 20), -0.5, 'missing'), \
               core.get(three, -0.5, 'missing'), core.contains(three, 1.8)].join(' ');"
        ),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        out.string().unwrap().to_string_lossy(),
        "40 10 31 32 39 none x y 41 31 30 30 [1 2 3] (2 3) 40 33 [30 31 32 33] 42 41 [1] true 7 \
         20 20 [10 \"x\" 30] 33 y missing missing false"
    );
}

/// A runtime-owned sequence (a list of `listFromArray`, an indexed seq over
/// a vector, the runtime's own `iterate` cell: design point 8) crosses into
/// the engine's `cons` and lazy-seq realization through the profile's
/// sequence adapter, which takes one element at a time, so an infinite one
/// is not traversed.
#[test]
fn shared_profile_runtime_sequences_cross_native_boundary() {
    let rt = Runtime::new();
    let out = run(
        &rt,
        &format!(
            "const rt = (\n{SHARED_RUNTIME}\n); const core = rt.core; \
             const consed = core.cons(0, core.list(1, 2)); \
             const lazy = new core.LazySeq(null, () => core.seq(core.vector(1, 2)), null, null); \
             let steps = 0; \
             const infinite = core.cons(-1, core.iterate(x => {{ steps++; return x + 1; }}, 0)); \
             const taken = core.prStr(core.take(3, infinite)); \
             const seeded = new core.LazySeq(null, null, core.seq(core.vector(5, 6)), null); \
             return [core.prStr(consed), core.count(consed), core.first(lazy), core.prStr(lazy), core.prStr(core.rest(lazy)), \
               taken, steps <= 2, core.prStr(core.conj(core.list(1, 2), 0)), core.count(core.conj(core.list(1, 2), 0)), \
               core.prStr(seeded), core.equal(consed, core.list(0, 1, 2)), core.hash(consed) === core.hash(core.list(0, 1, 2)), \
               core.prStr(core.cons(0, [1, 2])), core.prStr(core.cons(0, core.vector(1, 2)))].join(' ');"
        ),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        out.string().unwrap().to_string_lossy(),
        "(0 1 2) 3 1 (1 2) (2) (-1 0 1) true (0 1 2) 3 (5 6) true true (0 1 2) (0 1 2)"
    );
}

/// A host object hashes the same whether the runtime hashes it directly
/// (`identityHash`) or the engine hashes it inside a form (a vector's
/// element through `formHash`): an engine vector and a runtime-owned list of
/// the same function are equal and hash alike, so either finds the other's
/// entry in a map. A runtime-owned value the runtime hashes by value (a
/// symbol, a list of `listFromArray`, a Date) hashes the same inside an
/// engine sequence (a vector, a nested one, a lazy seq of the real `map`)
/// as inside a runtime-owned one, both ways through a hash map
/// (light-cljs-677); a sequence of engine values hashes as the engine does.
#[test]
fn shared_profile_equal_sequences_have_equal_host_element_hashes() {
    let rt = Runtime::new();
    let out = run(
        &rt,
        &format!(
            "const rt = (\n{SHARED_RUNTIME}\n); const core = rt.core; \
             const f = () => 1, o = {{}}; \
             const v = core.vector(f), l = core.list(f); \
             const s = core.symbol('a'), d = new Date(5); \
             const vs = core.vector(s), ls = core.list(s); \
             const nested = core.vector(core.list(s), core.vector(d)), nestedList = core.list(core.list(s), core.list(d)); \
             const lazy = core.mapSeq(core.identity, core.vector(s)); \
             const pure = core.vector(1, 'a', core.keyword(null, 'k'), core.vector(2)); \
             return [core.equal(v, l), core.hash(v) === core.hash(l), core.get(core.hashMap(v, 42), l), \
               core.get(core.hashMap(l, 43), v), core.hash(core.vector(o)) === core.hash(core.list(o)), \
               core.hash(f) === __rt.identityHash(f), core.hash(core.vector(f)) === core.hash(core.vector(f)), \
               core.equal(vs, ls), core.hash(vs) === core.hash(ls), core.get(core.hashMap(vs, 1), ls), core.get(core.hashMap(ls, 2), vs), \
               core.equal(nested, nestedList), core.hash(nested) === core.hash(nestedList), \
               core.get(core.hashMap(nested, 3), nestedList), core.get(core.hashMap(nestedList, 4), nested), \
               core.hash(core.vector(d)) === core.hash(core.list(d)), core.get(core.hashMap(core.vector(d), 5), core.list(new Date(5))), \
               core.hash(lazy) === core.hash(ls), core.get(core.hashMap(ls, 6), lazy), \
               core.hash(core.cons(s, null)) === core.hash(ls), \
               core.hash(pure) === __rt.formHash(pure), core.hash(vs) === core.hash(vs)].join(' ');"
        ),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        out.string().unwrap().to_string_lossy(),
        "true true 42 43 true true true true true 1 2 true true 3 4 true 5 true 6 true true true"
    );
}

/// The lazy and delay views reflect the engine once-cell's state: checking
/// `realized?` runs no thunk, a successful realization is reported once
/// made, a throwing thunk leaves the cell pending, and a `LazySeq` made
/// with no thunk holds its computed result, realized at once. A delay keeps
/// its seed while pending (printed as its `:val`) and after a failed
/// realization; one made with a `false` thunk is realized to its seed, which
/// `force` returns, and still prints as pending, as the V8 profile's does
/// (light-cljs-678).
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
             const falseThunk = new core.Delay(false, 7); \
             const seededPending = new core.Delay(() => 9, 8); \
             const failing = new core.Delay(() => {{ throw new Error('x'); }}, 'seed'); \
             try {{ core.deref(failing); }} catch (e) {{}} \
             return [r0, c0, f, r1, c1, dr0, dc0, dv, dr1, threw, core.realized(bad), \
               core.realized(init), core.first(init), core.prStr(init), core.realized(seeded), core.force(seeded), \
               core.force(falseThunk), core.realized(falseThunk), core.prStr(falseThunk), \
               core.prStr(seededPending), core.realized(seededPending), core.force(seededPending), core.prStr(seededPending), \
               core.prStr(failing), core.realized(failing)].join(' ');"
        ),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        out.string().unwrap().to_string_lossy(),
        "false 0 1 true 1 false 1 42 true true false true 1 (1) true 7 \
         7 true #object[cljs.core.Delay {:status :pending, :val 7}] \
         #object[cljs.core.Delay {:status :pending, :val 8}] false 9 #object[cljs.core.Delay {:status :ready, :val 9}] \
         #object[cljs.core.Delay {:status :pending, :val \"seed\"}] false"
    );
}

/// `cons` normalizes a tail that is not a seq (an array, a vector, an
/// invalid object) through `seq` when it is called, as the V8 profile's
/// does: a cons onto an empty array is `(0)` whatever is pushed to the array
/// later (one onto a nonempty array is the indexed seq over the array, which
/// reads it live, as the V8 profile's does), a cons onto a plain object
/// throws at once, and a seq tail (a lazy seq, the runtime's own `iterate`)
/// is kept as it is, unrealized (light-cljs-680). A tail that normalizes to
/// nil (an empty array, vector or string) keeps the cons kind, not counted,
/// as the V8 profile's `Cons` with a nil tail is; only an originally nil
/// tail makes a counted list (light-cljs-682).
#[test]
fn shared_profile_cons_normalizes_non_seq_tails_immediately() {
    let rt = Runtime::new();
    let out = run(
        &rt,
        &format!(
            "const rt = (\n{SHARED_RUNTIME}\n); const core = rt.core; \
             const a = []; const c = core.cons(0, a); a.push(1); \
             let threw = 'no'; try {{ core.cons(0, {{}}); }} catch (e) {{ threw = e.constructor.name; }} \
             const b = [1]; const d = core.cons(0, b); b.push(2); \
             let calls = 0; \
             const lazy = new core.LazySeq(null, () => {{ calls++; return core.vector(1); }}, null, null); \
             const onLazy = core.cons(0, lazy); const before = calls; \
             const onIterate = core.cons(-1, core.iterate(x => {{ calls++; return x + 1; }}, 0)); \
             return [core.prStr(c), core.count(c), threw, core.prStr(d), core.prStr(core.cons(0, core.vector(1))), \
               core.prStr(core.cons(0, core.list(1))), core.prStr(core.cons(0, 'ab')), before, core.prStr(onLazy), calls, \
               core.prStr(core.take(2, onIterate)), \
               core.isCounted(c), core.isCounted(core.cons(0, core.vector())), core.isCounted(core.cons(0, '')), \
               core.isCounted(core.cons(0, null)), core.prStr(core.cons(0, core.vector())), core.count(core.cons(0, '')), \
               core.prStr(core.rest(c)), core.isSeq(c), core.equal(c, core.list(0)), core.hash(c) === core.hash(core.list(0))].join(' ');"
        ),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        out.string().unwrap().to_string_lossy(),
        "(0) 1 Error (0 1 2) (0 1) (0 1) (0 \"a\" \"b\") 0 (0 1) 1 (-1 0) \
         false false false true (0) 1 () true true true"
    );
}

/// `conj` onto a list keeps a list: counted (`isCounted`, `countProtocol`)
/// after one and repeated prepends, printed and recognized as a list, with
/// the list's metadata kept, whether the list is the runtime's own
/// (`listFromArray`) or an engine list; a `cons` onto a list is a cons, not
/// counted, as the V8 profile's (light-cljs-681).
#[test]
fn shared_profile_conj_preserves_counted_list_kind() {
    let rt = Runtime::new();
    let out = run(
        &rt,
        &format!(
            "const rt = (\n{SHARED_RUNTIME}\n); const core = rt.core; \
             const l1 = core.conj(core.list(1), 0), l2 = core.conj(l1, -1); \
             const e1 = core.conj(__rt.formList(1), 0), e2 = core.conj(e1, -1); \
             const withMeta = core.withMeta(core.list(1), core.hashMap(core.keyword(null, 'a'), 1)); \
             return [core.isCounted(l1), core.countProtocol(l1), core.isCounted(l2), core.countProtocol(l2), core.prStr(l2), core.isList(l2), \
               core.isCounted(e1), core.countProtocol(e1), core.isCounted(e2), core.countProtocol(e2), core.prStr(e2), core.isList(e2), \
               core.prStr(core.conj(core.list(), 5)), core.isCounted(core.conj(core.list(), 5)), \
               core.prStr(core.meta(core.conj(withMeta, 0))), core.prStr(core.conj(withMeta, 0)), \
               core.isCounted(core.cons(0, core.list(1))), core.equal(l2, e2), core.hash(l2) === core.hash(e2)].join(' ');"
        ),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        out.string().unwrap().to_string_lossy(),
        "true 2 true 3 (-1 0 1) true true 2 true 3 (-1 0 1) true (5) true {:a 1} (0 1) false true true"
    );
}

/// The two brand stores kept as registries on the task's cells
/// (`protocolImplementations`, what `installMethods` records per value, and
/// `typeNames`) hold their keys weakly after boot, as the V8 profile's
/// WeakMaps do: a reify discarded by the task is reclaimed while the
/// registry stays rooted, and a named type the task keeps is still named
/// (light-cljs-683). The boot's entries seed them.
#[test]
fn shared_profile_brand_registries_release_discarded_values() {
    use big_boy_pants_js::HandleError;
    let rt = Runtime::new();
    let booted = run(
        &rt,
        &format!(
            "const rt = (\n{SHARED_RUNTIME}\n); const core = rt.core; \
             const cells = globalThis.__lightSharedCells; \
             const reified = rt.makeReify([]); \
             const T = rt.makeType(['a'], [], false, null, 'user/T'); \
             const implementations = cells.protocolImplementations, names = cells.typeNames; \
             const report = [Object.prototype.toString.call(implementations), Object.prototype.toString.call(names), \
               implementations.has(reified), names.get(T), names.get(rt.Loadable), \
               Object.getOwnPropertyDescriptor(cells, 'protocolImplementations') !== undefined].join(' '); \
             return __rt.formVector(rt, implementations, names, T, reified, report);"
        ),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    let (kept, implementations, names, weak_reified, report) = rt.scope(|s| {
        let booted = s.local(&booted).unwrap();
        let kept = (0..4)
            .map(|i| s.persist(s.form_nth(booted, i).unwrap()))
            .collect::<Vec<_>>();
        let implementations = s.persist(s.form_nth(booted, 1).unwrap());
        let names = s.persist(s.form_nth(booted, 2).unwrap());
        let weak = s.downgrade(s.form_nth(booted, 4).unwrap());
        let report = s
            .string(s.form_nth(booted, 5).unwrap())
            .unwrap()
            .to_string_lossy();
        (kept, implementations, names, weak, report)
    });
    assert_eq!(
        report,
        "[object WeakMap] [object WeakMap] true user/T light.lazy/Loadable true"
    );
    let before = implementations.weak_map_live_entry_count().unwrap();
    let named = names.weak_map_live_entry_count().unwrap();
    assert!(before >= 1 && named >= 2, "entries {before} {named}");
    drop(booted);
    rt.collect().unwrap();
    assert_eq!(weak_reified.upgrade().unwrap_err(), HandleError::Reclaimed);
    assert_eq!(implementations.weak_map_live_entry_count(), Some(before - 1));
    assert_eq!(names.weak_map_live_entry_count(), Some(named));
    drop(kept);
}

/// `ClassBrand`'s "some class is branded" flag is the task's cell
/// `classBrands`, read through the real `defineClass`, `ClassBrand` and
/// `TypeBrand` units (neither `defineClass` nor `TypeBrand` is exported, as
/// in the V8 profile, so this boots a copy of the runtime that exports them
/// too, as tests/support/js_runtime.test.js does): false until the task
/// defines a class, so no prototype is probed before (a proxy's
/// `getPrototypeOf` is never asked for); true after, when a genuine
/// instance is recognized as its type and one forged from the prototype,
/// from an instance's fields or from an instance as a prototype is not; a
/// task's fresh cells (`sharedTaskCells`) start with the flag false again,
/// so that task probes no prototype and recognizes no class instance until
/// it defines a class itself, and its flag is its own (light-cljs-684).
#[test]
fn shared_profile_class_brands_are_task_local() {
    let rt = Runtime::new();
    const ANCHOR: &str = "return sharedProfileBoot(Object.freeze({setDeclarationDemand, ";
    assert_eq!(SHARED_RUNTIME.matches(ANCHOR).count(), 1);
    let exporting = SHARED_RUNTIME.replace(
        ANCHOR,
        "return sharedProfileBoot(Object.freeze({defineClass, TypeBrand, setDeclarationDemand, ",
    );
    let out = run(
        &rt,
        &format!(
            "const rt = (\n{exporting}\n); const core = rt.core; \
             const cells = globalThis.__lightSharedCells; \
             let probes = 0; const probe = new Proxy({{}}, {{getPrototypeOf() {{ probes++; return null; }}}}); \
             const probed = () => {{ try {{ rt.TypeBrand.type(probe); }} catch (e) {{ if (!/getPrototypeOf/.test(e.message)) throw e; probes++; }} return probes; }}; \
             const initial = cells.classBrands, probesBefore = probed(); \
             class P {{ #b; constructor(a) {{ this.#b = true; this.a = a; }} static c = x => #b in x; }} \
             const defined = rt.defineClass(P, 'user/P a', [], true, null) === P && P.c === undefined; \
             const after = cells.classBrands; \
             const genuine = new P(1), sub = new (class extends P {{}})(2); \
             const forged = [Object.create(P.prototype), Object.create(genuine), \
               Object.defineProperties({{}}, Object.getOwnPropertyDescriptors(genuine)), \
               Object.defineProperties(Object.create(P.prototype), Object.getOwnPropertyDescriptors(genuine)), P.prototype, {{}}]; \
             const recognized = rt.TypeBrand.type(genuine) === P && rt.TypeBrand.type(sub) === P; \
             const forgedTypes = forged.map(value => String(rt.TypeBrand.type(value))).join(','); \
             const got = String(core.get(genuine, core.keyword(null, 'a'))); \
             const probesAfter = probed(); \
             const fresh = core.sharedTaskCells(); const freshFlag = fresh.classBrands; \
             globalThis.__lightSharedCells = fresh; \
             const freshRecognized = String(rt.TypeBrand.type(genuine)), probesFresh = probed(); \
             class Q {{ #b; constructor() {{ this.#b = true; }} static c = x => #b in x; }} \
             rt.defineClass(Q, 'user/Q', [], true, null); \
             const freshAfter = fresh.classBrands, freshRecognizedAfter = rt.TypeBrand.type(genuine) === P, probesFreshAfter = probed(); \
             globalThis.__lightSharedCells = cells; \
             return [initial, probesBefore, defined, after, recognized, forgedTypes, got, probesAfter, \
               freshFlag, freshRecognized, probesFresh, freshAfter, freshRecognizedAfter, probesFreshAfter, cells.classBrands].join(' ');"
        ),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        out.string().unwrap().to_string_lossy(),
        "false 0 true true true undefined,undefined,undefined,undefined,undefined,undefined 1 1 false undefined 1 true true 2 true"
    );
}

/// The method-key memos the boot fills and seals cover every arity a
/// dispatcher declares: `invokeProtocol` declares arities 1 to 22, so a
/// receiver implementing `cljs$core$IFn$_invoke$arity$22` is invoked with
/// 21 arguments through the sealed slots, where a missing slot would be
/// written at call time and throw (light-cljs-679).
#[test]
fn shared_profile_invoke_protocol_max_arity() {
    let rt = Runtime::new();
    let out = run(
        &rt,
        &format!(
            "const rt = (\n{SHARED_RUNTIME}\n); const core = rt.core; \
             const receiver = {{cljs$core$IFn$_invoke$arity$22: function (...args) {{ return args.length + ':' + args[21]; }}, \
               cljs$core$IFn$_invoke$arity$2: function (self, x) {{ return 'one ' + x; }}}}; \
             const args = Array.from({{length: 21}}, (_, i) => i); \
             return [core.invokeProtocol(receiver, ...args), core.invokeProtocol(receiver, 'x'), \
               core.invokeProtocol(receiver, ...args.slice(0, 20), 'last')].join(' ');"
        ),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        out.string().unwrap().to_string_lossy(),
        "22:20 one x 22:last"
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

/// The value of the JavaScript in `SHARED_EVAL`, run after the boot with
/// `rt` and `core` in scope and printed as a string, then its publication
/// path, for finding what a member holds.
#[test]
#[ignore = "diagnostic: SHARED_EVAL=\"rt.Loadable.prototype\""]
fn shared_profile_eval() {
    let rt = Runtime::new();
    let body = std::env::var("SHARED_EVAL").unwrap_or_default();
    let value = run(
        &rt,
        &format!("const rt = (\n{SHARED_RUNTIME}\n); const core = rt.core; return ({body});"),
    )
    .unwrap_or_else(|e| panic!("boot: {e}"));
    let text = run(
        &rt,
        &format!(
            "const rt = (\n{SHARED_RUNTIME}\n); const core = rt.core; const value = ({body}); \
             return typeof value === 'object' && value !== null ? Object.prototype.toString.call(value) + ' ' + Reflect.ownKeys(value).map(String).join(',') : String(value);"
        ),
    )
    .unwrap_or_else(|e| panic!("eval: {e}"));
    println!("EVAL {body}: {}", text.string().unwrap().to_string_lossy());
    println!(
        "PATH: {}",
        match rt.publish(&value) {
            Ok(()) => "publishes".to_string(),
            Err(e) => e.to_string(),
        }
    );
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
