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
/// 2026-10-31 (after light-cljs-666's third attempt, the symbols view): 739
/// of 748 members publish; the one remaining cause is the V8 form bridge
/// (`compilerForms` and, through `readerJsLiterals`, the eight reader entry
/// points; what the Pants build boots of the bridge is light-cljs-648's,
/// with the options in docs/real-core-design.md, "Profile assembly").
#[test]
#[ignore = "739 of 748 members publish; the V8 form bridge (light-cljs-648) remains, see docs/real-core-design.md"]
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
    assert_eq!(
        implementations.weak_map_live_entry_count(),
        Some(before - 1)
    );
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

/// The reader's own state (the declarations at the top of the `reader`
/// unit's function: the stream and reader-map registries, the tag tables,
/// the default tag and the read depth) and the namespace registry live on
/// the task's cells (`SHARED_READER_STATE` and `namespaces` in build.rs):
/// a tag parser, a default tag parser, a stream and a namespace registered
/// by one task's cells are unknown to a task's fresh cells, which still
/// have the boot's builtin tags and namespaces, and the read depth is back
/// at zero after a read (light-cljs-666).
#[test]
fn shared_profile_reader_state_is_task_local() {
    let rt = Runtime::new();
    let out = run(
        &rt,
        &format!(
            "const rt = (\n{SHARED_RUNTIME}\n); const core = rt.core; \
             const cells = globalThis.__lightSharedCells; \
             const attempt = f => {{ try {{ return f(); }} catch (e) {{ return e.constructor.name + ': ' + e.message.slice(0, 24); }} }}; \
             const previous = core.readerRegisterTagParser(core.symbol('my', 'tag'), v => core.vector('tagged', v)); \
             const tagged = core.prStr(core.readString('#my/tag 1')); \
             const builtins = core.count(core.readerInitialTagTable()), queue = core.prStr(core.readString('#queue [1 2]')); \
             const stream = core.readerStringReader('a\\nb'); const line = core.readerReadLine(stream); \
             core.readerRegisterDefaultTagParser((tag, v) => core.vector('default', tag, v)); \
             const viaDefault = core.prStr(core.readString('#other/tag 2')); \
             const ns = core.createNs(core.symbol(null, 'my.ns')); const all = core.count(core.allNs()); \
             const depth = cells.activeDepth; \
             const fresh = core.sharedTaskCells(); globalThis.__lightSharedCells = fresh; \
             const freshQueue = core.prStr(core.readString('#queue [3]')); \
             const freshTagged = attempt(() => core.prStr(core.readString('#my/tag 1'))); \
             const freshDefault = fresh.defaultTag === null, freshLine = attempt(() => core.readerReadLine(stream)); \
             const freshNs = String(core.findNs(core.symbol(null, 'my.ns'))), freshCore = String(core.findNs(core.symbol(null, 'cljs.core'))); \
             const freshAll = core.count(core.allNs()), freshDepth = fresh.activeDepth; \
             globalThis.__lightSharedCells = cells; \
             const still = core.prStr(core.readString('#my/tag 3')), stillNs = core.findNs(core.symbol(null, 'my.ns')) === ns, stillLine = core.readerReadLine(stream); \
             return [String(previous), tagged, builtins, queue, line, viaDefault, all, depth, freshQueue, freshTagged, freshDefault, freshLine, \
               freshNs, freshCore, freshAll, freshDepth, still, stillNs, stillLine].join(' ');"
        ),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        out.string().unwrap().to_string_lossy(),
        "null [\"tagged\" 1] 4 #queue [1 2] a [\"default\" other/tag 2] 3 0 #queue [3] Error: No reader function for t true TypeError: Expected input stream \
         null cljs.core 2 0 [\"tagged\" 3] true b"
    );
}

/// `demunge` keeps no regex object: `demungePattern` is the pattern's
/// source and the global regex is built per call (a regex object is exotic
/// data a seal cannot take; light-cljs-666).
#[test]
fn shared_profile_demunge_builds_its_regex_per_call() {
    let rt = Runtime::new();
    let out = run(
        &rt,
        &format!(
            "const rt = (\n{SHARED_RUNTIME}\n); const core = rt.core; \
             return [core.demunge('a_DOT_b_STAR_c'), core.demunge('ns$name'), core.demunge('x$'), core.demunge('_DOT__DOT_'), \
               core.prStr(core.demunge(core.symbol(null, 'a_PLUS_b'))), core.demunge('a_SLASH_b'), core.demunge('a_b')].join(' ');"
        ),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    // The same answers as the V8 profile's under Node (`_DOT_` is not in the
    // munge table, so its `_` demunges to `-`).
    assert_eq!(
        out.string().unwrap().to_string_lossy(),
        "a-DOT-b*c ns/name x .. a+b a/b a-b"
    );
}

/// The members light-cljs-666's second attempt made shareable publish on
/// their own (a regression here is a rejected publication, which the
/// in-task tests above would miss): `demunge` without its regex object, the
/// reader's functions over the task's cells, and the namespace registry's.
/// A task spawned separately, on another thread, with its own first cells,
/// then calls the published functions: `demunge` answers as in the boot
/// task, a stream it makes reads line by line, and a namespace it creates
/// is its own while the boot's `cljs.core` is shared (light-cljs-688).
#[test]
fn shared_profile_newly_shareable_members_publish() {
    const MEMBERS: [&str; 10] = [
        "demunge",
        "munge",
        "readerStringReader",
        "readerReadLine",
        "readerRegisterTagParser",
        "readerRegisterDefaultTagParser",
        "createNs",
        "findNs",
        "allNs",
        "nsName",
    ];
    // Published too, for the other task: `symbol`, `str` and `count`, to
    // make and print a namespace name and count the registry. `sharedTaskCells`, which the
    // task needs for its first cells, captures the hub, whose publication
    // waits for the V8 form bridge's (light-cljs-648; the reader entry
    // points reach it through `readerJsLiterals`), so it is handed to the
    // task unpublished, as `Runtime::spawn` allows.
    const HELPERS: [&str; 4] = ["symbol", "str", "count", "sharedTaskCells"];
    let rt = Runtime::new();
    let members = run(
        &rt,
        &format!(
            "const rt = (\n{SHARED_RUNTIME}\n); const core = rt.core; \
             return __rt.formVector({});",
            MEMBERS
                .iter()
                .chain(HELPERS.iter())
                .map(|m| format!("core.{m}"))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    )
    .unwrap_or_else(|e| panic!("boot: {e}"));
    let values = rt.scope(|s| {
        let members = s.local(&members).unwrap();
        (0..MEMBERS.len() + HELPERS.len())
            .map(|i| s.persist(s.form_nth(members, i).unwrap()))
            .collect::<Vec<_>>()
    });
    for (name, value) in MEMBERS.iter().chain(HELPERS[..3].iter()).zip(&values) {
        rt.publish(value)
            .unwrap_or_else(|e| panic!("core.{name} does not publish: {e}"));
    }
    let call = compile_function(
        "function call(demunge, munge, stringReader, readLine, registerTag, registerDefault, \
                       createNs, findNs, allNs, nsName, symbol, str, count, sharedTaskCells) { \
           globalThis.__lightSharedCells = sharedTaskCells(); \
           const stream = stringReader('first\\nsecond'); \
           const made = createNs(symbol(null, 'other.ns')); \
           const previous = registerTag(symbol('my', 'tag'), v => v); \
           return [demunge('a_DOT_b_STAR_c'), demunge('ns$name'), str(demunge(symbol(null, 'a_PLUS_b'))), munge('a-b'), \
             readLine(stream), readLine(stream), String(readLine(stream)), \
             str(nsName(findNs(symbol(null, 'cljs.core')))), findNs(symbol(null, 'other.ns')) === made, count(allNs()), \
             String(previous)].join(' '); }",
    )
    .unwrap();
    let args = values.iter().collect::<Vec<_>>();
    let task = rt.spawn(&call, &args).unwrap();
    let out = std::thread::scope(|s| {
        s.spawn(|| match task.run().unwrap() {
            RunOutcome::Complete(v) => v.string().unwrap().to_string_lossy(),
            other => panic!("{other:?}"),
        })
        .join()
        .unwrap()
    });
    assert_eq!(
        out,
        "a-DOT-b*c ns/name a+b a_b first second null cljs.core true 3 null"
    );
    // The namespace the other task created is unknown to a task with the
    // boot's cells.
    let boot_view = run(
        &rt,
        &format!(
            "const rt = (\n{SHARED_RUNTIME}\n); const core = rt.core; \
             return [String(core.findNs(core.symbol(null, 'other.ns'))), core.count(core.allNs())].join(' ');"
        ),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(boot_view.string().unwrap().to_string_lossy(), "null 2");
}

/// The per-task registries of `sharedTaskDefaults` (the lazy getters: the
/// gensym and tap state, the protocol-slot and hash memos, the dispatch
/// registries, the brand registries, the namespace registry and the
/// reader's state) are reached from the published hub, so they publish with
/// it (light-cljs-648 found every getter rejected through the namespaces'
/// runtime-built symbols; light-cljs-688). A task spawned separately reads
/// every getter through its first cells: each answers the task's own
/// registry, defined on the task's cells, the same object on a second read,
/// and the defaults still publish afterwards, since no read wrote through
/// them. One cause stays, light-cljs-648's: the builtin `js` tag parser,
/// held by the tag tables' boot snapshots, reaches the V8 form bridge
/// (`jsLiteral`, `readerJsLiterals.array`), so this boots a copy of the
/// runtime without that one registration, as the hub's publication
/// (`sharedTaskCells` captures the hub) waits for the same decision.
#[test]
fn shared_profile_registry_getters_keep_the_profile_published() {
    let rt = Runtime::new();
    const JS_TAG: &str = "  globalThis.__lightSharedCells.builtinTags.set('js', value => jsLiteral(builtinLocation,value));\n";
    assert_eq!(SHARED_RUNTIME.matches(JS_TAG).count(), 1);
    let without_js_tag = SHARED_RUNTIME.replace(JS_TAG, "");
    let booted = run(
        &rt,
        &format!(
            "const rt = (\n{without_js_tag}\n); const core = rt.core; \
             return __rt.formVector(Object.getPrototypeOf(core), core.sharedTaskCells, core.currentNs);"
        ),
    )
    .unwrap_or_else(|e| panic!("boot: {e}"));
    let [defaults, make_cells, current_ns] = rt.scope(|s| {
        let booted = s.local(&booted).unwrap();
        [0, 1, 2].map(|i| s.persist(s.form_nth(booted, i).unwrap()))
    });
    for (name, value) in [
        ("the task defaults", &defaults),
        ("core.currentNs", &current_ns),
    ] {
        rt.publish(value)
            .unwrap_or_else(|e| panic!("{name} does not publish: {e}"));
    }
    let read = compile_function(
        "function read(sharedTaskCells, defaults) { \
           const cells = sharedTaskCells(); \
           const names = Object.getOwnPropertyNames(defaults).filter(n => Object.getOwnPropertyDescriptor(defaults, n).get !== undefined); \
           const counted = ['gensymScopes', 'generatedNames', 'tapListeners', 'protocolSlots', 'namespaces', 'tags', 'builtinTags']; \
           const out = names.map(n => { \
             const v = cells[n], own = Object.getOwnPropertyDescriptor(cells, n) !== undefined; \
             const kind = Object.prototype.toString.call(v).slice(8, -1); \
             const size = v.size === undefined ? '' : counted.includes(n) ? v.size : v.size > 0 ? '+' : '0'; \
             return n + ':' + kind + size + (own ? '' : '!') + (cells[n] === v ? '' : '?'); }); \
           const untouched = names.every(n => Object.getOwnPropertyDescriptor(defaults, n).get !== undefined); \
           return out.join(' ') + ' ' + untouched; }",
    )
    .unwrap();
    let task = rt.spawn(&read, &[&make_cells, &defaults]).unwrap();
    let out = std::thread::scope(|s| {
        s.spawn(|| match task.run().unwrap() {
            RunOutcome::Complete(v) => v.string().unwrap().to_string_lossy(),
            other => panic!("{other:?}"),
        })
        .join()
        .unwrap()
    });
    assert_eq!(
        out,
        "gensymScopes:Map0 generatedNames:Set0 tapListeners:Set0 tapQueue:Array protocolSlots:Map0 formHashes:WeakMap \
         coreDispatchers:Map+ nativeMethods:Map0 protocolValues:Map0 protocolFunctions:Map0 protocolVariables:Map0 \
         protocolGetters:Map0 reifySites:Map0 protocolImplementations:WeakMap typeNames:WeakMap namespaces:Map2 \
         streams:WeakMap readerMaps:WeakMap tags:Map3 builtinTags:Map3 true"
    );
    rt.publish(&defaults)
        .unwrap_or_else(|e| panic!("the task defaults do not publish after the reads: {e}"));
}

/// Symbols are engine symbol forms (light-cljs-666, acceptance 4): one the
/// engine made and one the runtime's `symbol` makes are recognized by
/// `symbol?`, named, printed, stringified, equal, hashed and compared alike
/// through the real units, and the namespace registry's names are engine
/// symbols, so `currentNs` and the namespaces publish. Metadata is the
/// engine's on an engine symbol; a symbol given runtime-built metadata (a
/// map of the verbatim `maps`, until the maps view) is a runtime-owned
/// symbol that reads alike, and an engine symbol again once its metadata is
/// engine-built or removed. An unqualified name holding `/`, which no engine
/// form spells, is a runtime-owned symbol too.
#[test]
fn shared_profile_symbols_are_engine_symbols() {
    let rt = Runtime::new();
    let out = run(
        &rt,
        &format!(
            "const rt = (\n{SHARED_RUNTIME}\n); const core = rt.core; \
             const engine = __rt.formSymbol('ns', 'a'), made = core.symbol('ns', 'a'), plain = rt.symbol(null, 'b'); \
             const m = rt.arrayMapFromArray([core.keyword(null, 'a'), 1]), withM = core.withMeta(made, m), back = core.withMeta(withM, null); \
             const fm = __rt.formWithMeta(engine, __rt.formMap(__rt.formKeyword(null, 'b'), 2)); \
             const em = core.withMeta(withM, __rt.formMap(__rt.formKeyword(null, 'c'), 3)); \
             const slash = core.symbol(null, 'a/b'), pos = new core.Symbol('ns', 'c', 'ns/c', null, null); \
             const coreNs = core.findNs(core.symbol(null, 'cljs.core')); \
             return [core.isSymbol(engine), core.isSymbol(made), core.isSymbol('a'), __rt.formKind(made), __rt.formKind(plain), \
               core.name(engine), core.namespace(made), String(core.namespace(plain)), \
               core.prStr(engine), core.str(plain), core.str(core.keyword(null, 'k')), core.str(engine, '/', plain), \
               core.equal(engine, made), core.hash(engine) === core.hash(made), core.compare(engine, core.symbol('ns', 'b')), \
               core.get(rt.arrayMapFromArray([engine, 1]), made), String(core.meta(engine)), \
               String(__rt.formKind(withM)), core.isSymbol(withM), core.equal(withM, made), core.hash(withM) === core.hash(made), \
               core.get(core.meta(withM), core.keyword(null, 'a')), core.prStr(withM), core.str(withM), \
               __rt.formKind(back), String(core.meta(back)), core.isSymbol(fm), __rt.formKind(core.meta(fm)), \
               __rt.formKind(em), __rt.formKind(core.meta(em)), core.equal(em, made), \
               String(__rt.formKind(slash)), core.name(slash), String(core.namespace(slash)), core.prStr(slash), \
               __rt.formKind(core.symbol('a/b')), core.prStr(core.symbol('a/b')), __rt.formKind(core.symbol(null, '/')), core.prStr(core.symbol(null, '/')), \
               __rt.formKind(pos), core.prStr(pos), \
               __rt.formKind(core.nsName(coreNs)), core.prStr(core.nsName(coreNs)), core.str(coreNs), \
               core.isSpecialSymbol(core.symbol(null, 'if')), __rt.formKind(core.gensym()), core.prStr(core.munge(core.symbol(null, 'a-b'))), \
               __rt.formKind(core.readString('x/y')), core.prStr(core.readString('x/y'))].join(' ');"
        ),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        out.string().unwrap().to_string_lossy(),
        "true true false symbol symbol a ns null ns/a b :k ns/a/b true true -1 1 null \
         null true true true 1 ns/a ns/a symbol null true map symbol map true \
         null a/b null a/b symbol a/b symbol / symbol ns/c \
         symbol cljs.core cljs.core true symbol a_b symbol x/y"
    );
    let current_ns = run(
        &rt,
        &format!("const rt = (\n{SHARED_RUNTIME}\n); return rt.core.currentNs;"),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    rt.publish(&current_ns)
        .unwrap_or_else(|e| panic!("core.currentNs does not publish: {e}"));
}

/// A lazy result of the real `map` publishes (its thunk captures the
/// sealed collections; light-cljs-666, acceptance 3) and a task spawned
/// separately walks it; and `map` over a 40-element vector calls the mapped
/// function a chunk at a time: 32 calls for the first element, none more
/// through the 32nd, the remaining 8 for the 33rd.
#[test]
fn shared_profile_lazy_map_result_publishes_and_maps_in_chunks() {
    let rt = Runtime::new();
    let booted = run(
        &rt,
        &format!(
            "const rt = (\n{SHARED_RUNTIME}\n); const core = rt.core; \
             let calls = 0; const f = x => {{ calls++; return x + 1; }}; \
             const over = core.mapSeq(x => x * 2, __rt.formList(1, 2, 3)); \
             let v = core.vector(); for (let i = 0; i < 40; i++) v = core.conj(v, i); \
             const mapped = core.mapSeq(f, v); \
             const before = calls; core.first(mapped); const afterFirst = calls; core.nth(mapped, 31); const afterChunk = calls; \
             core.nth(mapped, 32); const afterNext = calls; core.count(mapped); const all = calls; \
             return __rt.formVector(over, core.sharedTaskCells, [before, afterFirst, afterChunk, afterNext, all, core.nth(mapped, 39)].join(' '));"
        ),
    )
    .unwrap_or_else(|e| panic!("boot: {e}"));
    let (over, make_cells, counts) = rt.scope(|s| {
        let booted = s.local(&booted).unwrap();
        (
            s.persist(s.form_nth(booted, 0).unwrap()),
            s.persist(s.form_nth(booted, 1).unwrap()),
            s.string(s.form_nth(booted, 2).unwrap())
                .unwrap()
                .to_string_lossy(),
        )
    });
    assert_eq!(counts, "0 32 32 40 40 40");
    rt.publish(&over)
        .unwrap_or_else(|e| panic!("the lazy map result does not publish: {e}"));
    let walk = compile_function(
        "function walk(xs, sharedTaskCells) { \
           globalThis.__lightSharedCells = sharedTaskCells(); \
           const out = []; \
           for (let s = __rt.cljSeq(xs); s != null; s = __rt.cljNext(s)) out.push(__rt.cljFirst(s)); \
           return out.join(' '); }",
    )
    .unwrap();
    let task = rt.spawn(&walk, &[&over, &make_cells]).unwrap();
    let out = std::thread::scope(|s| {
        s.spawn(|| match task.run().unwrap() {
            RunOutcome::Complete(v) => v.string().unwrap().to_string_lossy(),
            other => panic!("{other:?}"),
        })
        .join()
        .unwrap()
    });
    assert_eq!(out, "2 4 6");
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

/// The value of the JavaScript in `SHARED_EVAL` (or in the file named by
/// `SHARED_EVAL_FILE`, for an expression the shell would mangle), run after
/// the boot with `rt` and `core` in scope and printed as a string, then its
/// publication path, for finding what a member holds. `SHARED_EXPORTS`
/// (space separated) names bindings the runtime does not export, exported
/// from a copy of the runtime for the expression (`rt.sharedSnapshots`).
#[test]
#[ignore = "diagnostic: SHARED_EVAL=\"rt.Loadable.prototype\""]
fn shared_profile_eval() {
    let rt = Runtime::new();
    let body = std::env::var("SHARED_EVAL")
        .ok()
        .or_else(|| {
            std::env::var("SHARED_EVAL_FILE")
                .ok()
                .map(|path| std::fs::read_to_string(path).expect("SHARED_EVAL_FILE"))
        })
        .unwrap_or_default();
    const ANCHOR: &str = "return sharedProfileBoot(Object.freeze({setDeclarationDemand, ";
    let exports = std::env::var("SHARED_EXPORTS").unwrap_or_default();
    let runtime = SHARED_RUNTIME.replace(
        ANCHOR,
        &format!(
            "return sharedProfileBoot(Object.freeze({{{}setDeclarationDemand, ",
            exports
                .split_whitespace()
                .map(|name| format!("{name}, "))
                .collect::<String>()
        ),
    );
    let value = run(
        &rt,
        &format!("const rt = (\n{runtime}\n); const core = rt.core; return ({body});"),
    )
    .unwrap_or_else(|e| panic!("boot: {e}"));
    let text = run(
        &rt,
        &format!(
            "const rt = (\n{runtime}\n); const core = rt.core; const value = ({body}); \
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
