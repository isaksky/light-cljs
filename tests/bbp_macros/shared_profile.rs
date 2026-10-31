//! The shared-heap profile of the real cljs.core (`src/js_shared_profile.js`,
//! assembled by build.rs into `SHARED_RUNTIME`; docs/real-core-design.md,
//! section 1; light-cljs-647): it boots in a boot task and publishes.

use big_boy_pants_js::frontend::compile_function;
use big_boy_pants_js::{HeapPolicy, Primitive, PublishError, Rooted, RunOutcome, Runtime};
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
/// member by cause and the top-level bindings a seal cannot take.
/// The complete profile publishes, including its reader literal builders.
/// The Pants evaluator owns its bridge; this profile does not instantiate
/// the V8 bridge's mutable decode/encode state.
#[test]
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

/// An engine map and a runtime-built map of the same entries hash alike
/// when an entry holds a value the engine hashes by identity and the
/// runtime by value (a Date, a symbol, a list, a runtime-owned set, a map
/// nested in a vector), so each finds the other as a hash-map key: the
/// engine map's hash is the runtime's, cached per task like a sequence's
/// (light-cljs-698); a map of engine values hashes as the engine does.
#[test]
fn shared_profile_equal_engine_maps_have_equal_host_element_hashes() {
    let rt = Runtime::new();
    let out = run(
        &rt,
        &format!(
            "const rt = (\n{SHARED_RUNTIME}\n); const core = rt.core; \
             const d = new Date(0); d.setTime(__rt.identityHash(d) + 1); \
             const s = core.symbol('a'), set = core.hashSet(d); \
             const m = __rt.formMap('k', d), own = rt.arrayMapFromArray(['k', d]); \
             const keyed = __rt.formMap(core.list(s), 1), ownKeyed = rt.arrayMapFromArray([core.list(s), 1]); \
             const nested = __rt.formMap('k', core.vector(core.list(d))), ownNested = rt.arrayMapFromArray(['k', core.vector(core.list(d))]); \
             const setKeyed = __rt.formMap(set, d), ownSetKeyed = rt.arrayMapFromArray([set, d]); \
             const inVector = core.vector(m), ownInVector = core.list(own); \
             const pure = __rt.formMap(core.keyword(null, 'k'), core.vector(1, 'a')); \
             return [core.equal(m, own), core.hash(m) === core.hash(own), core.get(core.hashMap(m, 42), own), core.get(core.hashMap(own, 43), m), \
               core.equal(keyed, ownKeyed), core.hash(keyed) === core.hash(ownKeyed), core.get(core.hashMap(keyed, 1), ownKeyed), core.get(core.hashMap(ownKeyed, 2), keyed), \
               core.equal(nested, ownNested), core.hash(nested) === core.hash(ownNested), core.get(core.hashMap(nested, 3), ownNested), core.get(core.hashMap(ownNested, 4), nested), \
               core.equal(setKeyed, ownSetKeyed), core.hash(setKeyed) === core.hash(ownSetKeyed), core.get(core.hashMap(setKeyed, 5), ownSetKeyed), \
               core.equal(inVector, ownInVector), core.hash(inVector) === core.hash(ownInVector), core.get(core.hashMap(inVector, 6), ownInVector), \
               core.get(core.hashSet(m), own) === m, core.get(core.hashSet(own), m) === own, \
               core.hash(m) === core.hash(m), core.hash(pure) === __rt.formHash(pure), core.hash(pure) === core.hash(rt.arrayMapFromArray([core.keyword(null, 'k'), core.vector(1, 'a')]))].join(' ');"
        ),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        out.string().unwrap().to_string_lossy(),
        "true true 42 43 true true 1 2 true true 3 4 true true 5 true true 6 true true true true true"
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
/// them. This includes the builtin `js` parser, whose literal builders
/// now use engine forms and retain no V8 bridge state.
#[test]
fn shared_profile_registry_getters_keep_the_profile_published() {
    let rt = Runtime::new();
    let booted = run(
        &rt,
        &format!(
            "const rt = (\n{SHARED_RUNTIME}\n); const core = rt.core; \
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
        "gensymScopes:Map0 generatedNames:Set0 tapListeners:Set0 tapQueue:Array protocolSlots:Map0 formHashes:WeakMap formEntries:WeakMap varMetadata:WeakMap atomOwners:WeakMap \
         coreDispatchers:Map+ nativeMethods:Map0 protocolValues:Map0 protocolFunctions:Map0 protocolVariables:Map0 \
         protocolGetters:Map0 reifySites:Map0 protocolImplementations:WeakMap typeNames:WeakMap namespaces:Map2 \
         streams:WeakMap readerMaps:WeakMap tags:Map4 builtinTags:Map4 true"
    );
    rt.publish(&defaults)
        .unwrap_or_else(|e| panic!("the task defaults do not publish after the reads: {e}"));
}

/// Symbols are engine symbol forms (light-cljs-666, acceptance 4): one the
/// engine made and one the runtime's `symbol` makes are recognized by
/// `symbol?`, named, printed, stringified, equal, hashed and compared alike
/// through the real units, and the namespace registry's names are engine
/// symbols, so `currentNs` and the namespaces publish. Metadata is the
/// engine's on an engine symbol, and a map the runtime builds is an engine
/// map since the builders swap, so a symbol given one as metadata is an
/// engine symbol too; one given a runtime-owned map (a sorted map) is a
/// runtime-owned symbol that reads alike, and an engine symbol again once
/// its metadata is engine-built or removed. An unqualified name holding
/// `/`, which no engine form spells, is a runtime-owned symbol too.
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
        // `withM`'s metadata, a map of `arrayMapFromArray`, is an engine map
        // since the builders swap (light-cljs-666), so `withM` is an engine
        // symbol.
        "true true false symbol symbol a ns null ns/a b :k ns/a/b true true -1 1 null \
         symbol true true true 1 ns/a ns/a symbol null true map symbol map true \
         null a/b null a/b symbol a/b symbol / symbol ns/c \
         symbol cljs.core cljs.core true symbol a_b symbol x/y"
    );
    // The positional constructor keeps the slots it is given (the V8
    // profile's `public-symbol-slots`, tests/fixtures/compiler-public-constructors.tsv):
    // a spelling other than the pair's or a cached hash other than the
    // engine's makes a runtime-owned symbol, which `with-meta` copies with
    // both; a cached hash that is the engine's own leaves an engine symbol.
    let out = run(
        &rt,
        &format!(
            "const rt = (\n{SHARED_RUNTIME}\n); const core = rt.core; \
             const alias = new core.Symbol('source', 'name', 'printed/alias', 17, null); \
             const copy = core.withMeta(alias, rt.arrayMapFromArray([core.keyword(null, 'm'), 2])); \
             const engineMeta = core.withMeta(alias, __rt.formMap(core.keyword(null, 'e'), 3)), bare = core.withMeta(copy, null); \
             const hashed = new core.Symbol('ns', 'c', 'ns/c', 17, null), same = new core.Symbol('ns', 'c', 'ns/c', core.hash(core.symbol('ns', 'c')), null); \
             const first = [String(__rt.formKind(alias)), core.namespace(alias), core.name(alias), core.str(alias), core.prStr(alias), alias.str, alias.ns, core.hash(alias), alias._hash, \
               core.equal(alias, core.symbol('printed', 'alias')), core.equal(core.symbol('printed', 'alias'), alias), core.equal(alias, core.symbol('source', 'name')), \
               String(__rt.formKind(copy)), core.hash(copy), core.prStr(copy), core.get(core.meta(copy), core.keyword(null, 'm')), \
               String(__rt.formKind(engineMeta)), core.hash(engineMeta), core.str(engineMeta), __rt.formGet(core.meta(engineMeta), core.keyword(null, 'e'), null), \
               String(__rt.formKind(bare)), core.hash(bare), core.prStr(bare), String(core.meta(bare)), \
               String(__rt.formKind(hashed)), core.hash(hashed), core.prStr(hashed), __rt.formKind(same), core.hash(same) === core.hash(hashed)]; \
             copy._hash = 18; \
             return [...first, core.hash(copy), core.hash(alias)].join(' ');"
        ),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        out.string().unwrap().to_string_lossy(),
        "null source name printed/alias printed/alias printed/alias source 17 17 true true false \
         null 17 printed/alias 2 null 17 printed/alias 3 null 17 printed/alias null \
         null 17 ns/c symbol false 18 17"
    );
    let current_ns = run(
        &rt,
        &format!("const rt = (\n{SHARED_RUNTIME}\n); return rt.core.currentNs;"),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    rt.publish(&current_ns)
        .unwrap_or_else(|e| panic!("core.currentNs does not publish: {e}"));
}

/// A Var named by an engine symbol prints and converts to a string as the
/// V8 profile's does: `varPrototype.toString` spells the name through `str`,
/// since a form has no `toString` (light-cljs-692).
#[test]
fn shared_profile_vars_with_engine_symbol_names_print() {
    let rt = Runtime::new();
    let out = run(
        &rt,
        &format!(
            "const rt = (\n{SHARED_RUNTIME}\n); const core = rt.core; \
             const v = rt.makeVar(() => 1, core.symbol('demo', 'x')), plain = rt.makeVar(() => 2, core.symbol(null, 'a/b')); \
             return [core.prStr(v), core.str(v), rt.displayStr(v), String(v), v.toString(), core.str(plain), core.prStr(v.sym)].join(' ');"
        ),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        out.string().unwrap().to_string_lossy(),
        "#'demo/x #'demo/x #'demo/x #'demo/x #'demo/x #'a/b demo/x"
    );
}

#[test]
fn shared_profile_engine_vars_keep_identity_metadata_and_deferred_roots() {
    let rt = Runtime::new();
    let out = run(&rt, &format!(
        "const rt = (\n{SHARED_RUNTIME}\n); const core = rt.core; \
         const type = __rt.structType('cljs.core/Var', __rt.formVector('val', 'sym', '__metadata', '__deferred'), 1, null); \
         const name = core.symbol('demo', 'x'), metadata = core.arrayMap(core.keyword('tag'), 7); \
         let roots = 0, deferred = 0; \
         const v = __rt.structNew(type, () => {{ roots++; return 42; }}, name, metadata, () => {{ deferred++; return 'test'; }}); \
         const own = rt.makeVar(() => 9, name); \
         const identity = [core.isVar(v), core.isFn(v), core.equal(v, own), core.equal(own, v), core.hash(v) === core.hash(own), core.str(v), core.prStr(v), roots, deferred]; \
         const m = core.meta(v), again = core.meta(v); \
         const copy = core.withMeta(v, core.arrayMap(core.keyword('tag'), 8)); \
         return [...identity, m === again, core.get(m, core.keyword('tag')), core.get(m, core.keyword('test')), deferred, \
           core.get(core.meta(copy), core.keyword('tag')), core.isVar(copy), core.deref(copy), roots].join(' ');"
    )).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        out.string().unwrap().to_string_lossy(),
        "true true true true true #'demo/x #'demo/x 0 0 true 7 test 1 8 true 42 1"
    );
}

#[test]
fn shared_profile_engine_regex_and_lazy_matches_publish() {
    let rt = Runtime::new();
    let out = run(
        &rt,
        &format!(
            "const rt = (\n{SHARED_RUNTIME}\n); const core = rt.core; \
         const re = __rt.formRegex('(a)', ''), built = core.rePattern('(a)'); \
         const matches = core.reSeq(re, 'aba'), applied = core.apply(RegExp, core.vector('a', 'i')); \
         return __rt.formVector(matches, core.sharedTaskCells, \
           [core.isRegex(re), core.isRegex(built), __rt.formKind(built), core.prStr(re), \
            core.prStr(core.reFind(re, 'ba')), core.prStr(core.reMatches(re, 'a')), \
            core.rePattern(re) === re, __rt.formKind(core.rePattern('(?g)a')) === null, \
            __rt.formKind(applied), applied.test('A'), core.apply(RegExp, core.vector(re)) === re, \
            __rt.formKind(core.first(core.mapv(RegExp, core.vector('x')))), \
            __rt.formKind(core.first(core.mapSeq(RegExp, core.vector('x'))))].join(' '));"
        ),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    let (matches, cells, text) = rt.scope(|s| {
        let out = s.local(&out).unwrap();
        (
            s.persist(s.form_nth(out, 0).unwrap()),
            s.persist(s.form_nth(out, 1).unwrap()),
            s.string(s.form_nth(out, 2).unwrap())
                .unwrap()
                .to_string_lossy(),
        )
    });
    assert_eq!(
        text,
        "true true compiler-literal #\"(a)\" [\"a\" \"a\"] [\"a\" \"a\"] true true compiler-literal true true compiler-literal compiler-literal"
    );
    rt.publish(&matches).unwrap();
    let walk = compile_function(
        "function walk(xs, makeCells) { globalThis.__lightSharedCells = makeCells(); \
        return String(__rt.cljCount(xs)); }",
    )
    .unwrap();
    match rt.spawn(&walk, &[&matches, &cells]).unwrap().run().unwrap() {
        RunOutcome::Complete(value) => assert_eq!(value.string().unwrap().to_string_lossy(), "2"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn shared_profile_engine_structs_and_records_keep_their_brands() {
    let rt = Runtime::new();
    let out = run(&rt, &format!(
        "const rt = (\n{SHARED_RUNTIME}\n); const core = rt.core; \
         const type = __rt.structType('demo/Point', __rt.formVector('x', 'y'), true, null); \
         const plain = __rt.structType('demo/Box', __rt.formVector('x'), false, null); \
         const r = __rt.structNew(type, 1, 2), s = __rt.structNew(plain, 1); \
         const x = core.keyword('x'), extra = core.keyword('extra'), tag = core.keyword('tag'); \
         const updated = core.assoc(r, x, 3, extra, 4), metadata = core.arrayMap(tag, 9); \
         const withMeta = core.withMeta(updated, metadata), removed = core.dissoc(withMeta, extra), map = core.dissoc(r, x); \
         return [core.isInstance(type, r), core.isInstance(plain, s), core.isInstance(type, s), \
           core.isRecord(r), core.isRecord(s), core.isMap(r), core.count(r), core.get(r, x), \
           core.prStr(r), core.prStr(core.seq(r)), core.get(updated, x), core.get(updated, extra), \
           core.count(updated), __rt.formKind(updated), core.get(core.meta(removed), tag), core.isRecord(removed), \
           core.isRecord(map), core.isMap(map), core.count(map), core.equal(r, __rt.structNew(type, 1, 2)), \
           core.hash(r) === core.hash(__rt.structNew(type, 1, 2))].join(' ');"
    )).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        out.string().unwrap().to_string_lossy(),
        "true true false true false true 2 1 #demo.Point{:x 1, :y 2} ([:x 1] [:y 2]) 3 4 3 record 9 true false true 1 true true"
    );
}

#[test]
fn shared_profile_record_noops_keep_imported_occurrences() {
    let rt = Runtime::new();
    let values = run(&rt, &format!(
        "const rt = (\n{SHARED_RUNTIME}\n); const core = rt.core; \
         const type = __rt.structType('demo/Point', __rt.formVector('x', 'y'), true, null); \
         const child = core.vector('unchanged'); \
         const record = __rt.cljWithMeta(__rt.structNew(type, child, child), core.arrayMap('tag', 1)); \
         return __rt.formVector(core, record);"
    )).unwrap_or_else(|e| panic!("{e}"));
    let (core, record, nan) = rt.scope(|s| {
        let values = s.local(&values).unwrap();
        let core = s.persist(s.form_nth(values, 0).unwrap());
        let record = s.form_nth(values, 1).unwrap();
        let record = s.new_occurrence(record, Default::default()).unwrap();
        (
            core,
            s.persist(s.new_occurrence(record, Default::default()).unwrap()),
            s.persist(
                s.new_occurrence(
                    s.primitive(big_boy_pants_js::Primitive::Number(f64::NAN)),
                    Default::default(),
                )
                .unwrap(),
            ),
        )
    });
    let check = compile_function(
        "function check(core, record, nan) { \
           globalThis.__lightSharedCells = core.sharedTaskCells(); \
           const x = core.keyword('x'), y = core.keyword('y'), child = core.get(record, x); \
           const changed = core.assoc(record, x, core.vector('changed')); \
           const extended = core.assoc(record, nan, child), extendedAgain = core.assoc(extended, nan, child); \
           return [core.assoc(record, x, child) === record, \
             core.assoc(record, x, child, y, child) === record, \
             core.dissoc(record, core.keyword('missing')) === record, \
             core.withMeta(record, core.meta(record)) === record, changed !== record, \
             core.get(changed, y) === child, core.meta(changed) === core.meta(record), \
             core.get(extended, nan, 'absent') === 'absent', core.count(extendedAgain), core.count(core.dissoc(extended, nan)), \
             __rt.formNth(__rt.formNth(__rt.formPrintItems(extendedAgain), 2), 0) === nan, \
             core.meta(extendedAgain) === core.meta(record)].join(' '); }",
    ).unwrap();
    match rt
        .spawn(&check, &[&core, &record, &nan])
        .unwrap()
        .run()
        .unwrap()
    {
        RunOutcome::Complete(value) => assert_eq!(
            value.string().unwrap().to_string_lossy(),
            "true true true true true true true true 4 3 true true"
        ),
        other => panic!("{other:?}"),
    }
}

#[test]
fn shared_profile_engine_atoms_keep_state_options_and_watch_order() {
    let rt = Runtime::new();
    let out = run(&rt, &format!(
        "const rt = (\n{SHARED_RUNTIME}\n); const core = rt.core; \
         const a = __rt.atomNew(1), seen = __rt.atomNew(null), key = core.keyword('watch'); \
         const made = core.atom(2), metadata = core.arrayMap(core.keyword('tag'), 3); \
         core.resetMeta(a, metadata); core.setValidator(a, x => x >= 0); \
         core.addWatch(a, key, (key, ref, before, after) => {{ __rt.atomCas(seen, __rt.atomLoad(seen), __rt.formVector(before, after)); }}); \
         core.swap(a, x => x + 4); const swapped = core.deref(a), watched = core.prStr(core.deref(seen)); \
         let rejected = false; try {{ core.reset(a, -1); }} catch (e) {{ rejected = true; }} \
         const unchanged = core.deref(a); core.removeWatch(a, key); core.reset(a, 6); \
         core.addWatch(a, key, () => {{ throw new Error('watch'); }}); \
         let threw = false; try {{ core.reset(a, 7); }} catch (e) {{ threw = true; }} \
         return [core.satisfies(core.IAtom, a), core.isInstance(core.Atom, a), __rt.cljIsAtom(made), \
           swapped, watched, rejected, unchanged, core.prStr(core.deref(seen)), threw, core.deref(a), \
           core.get(core.meta(a), core.keyword('tag')), core.getValidator(a)(1), core.prStr(made)].join(' ');"
    )).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        out.string().unwrap().to_string_lossy(),
        "true true true 5 [1 5] true 5 [1 5] true 7 3 true #object[cljs.core.Atom {:val 2}]"
    );
}

#[test]
fn shared_profile_engine_multimethod_brand_is_recognized() {
    let rt = Runtime::new();
    let out = run(
        &rt,
        &format!(
            "const rt = (\n{SHARED_RUNTIME}\n); const core = rt.core; \
         const type = __rt.structType('cljs.core/MultiFn', __rt.formVector('name'), 0, null); \
         const multi = __rt.structNew(type, core.symbol('demo', 'dispatch')); \
         return [core.isInstance(core.MultiFn, multi), core.satisfies(core.IMultiFn, multi), \
           core.name(multi), core.namespace(multi), core.isVar(multi)].join(' ');"
        ),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        out.string().unwrap().to_string_lossy(),
        "true true dispatch demo false"
    );
}

/// The operations that copy a collection's metadata to their result
/// (`preserveMeta`: `empty`, `pop`, `conj`, `assoc`) give an engine result
/// the engine metadata of its input as a form (`formWithMeta`), never by
/// writing to the form; a map the runtime builds (`arrayMapFromArray`) is
/// an engine map too since the builders swap (light-cljs-666), so a
/// vector given one as metadata stays an engine vector (light-cljs-693).
#[test]
fn shared_profile_engine_collection_operations_preserve_metadata() {
    let rt = Runtime::new();
    let out = run(
        &rt,
        &format!(
            "const rt = (\n{SHARED_RUNTIME}\n); const core = rt.core; \
             const tag = core.keyword(null, 'tag'), v = __rt.formWithMeta(core.vector(1, 2), __rt.formMap(tag, true)); \
             const results = [core.empty(v), core.pop(v), core.conj(v, 3), core.assoc(v, 0, 9)]; \
             const own = core.withMeta(core.vector(1, 2), rt.arrayMapFromArray([tag, 'own'])), ownResults = [core.empty(own), core.pop(own), core.conj(own, 3), core.assoc(own, 0, 9)]; \
             const s = core.withMeta(core.hashSet(1, 2), __rt.formMap(tag, 'set')), setResults = [core.empty(s), core.conj(s, 3)]; \
             const bare = core.withMeta(v, null), bareResults = [core.conj(bare, 3), core.pop(bare), core.assoc(bare, 0, 9), core.empty(bare)]; \
             return [...results.map(r => __rt.formKind(r) + ':' + core.count(r) + ':' + __rt.formGet(core.meta(r), tag, null)), \
               ...ownResults.map(r => String(__rt.formKind(r)) + ':' + core.count(r) + ':' + core.get(core.meta(r), tag) + ':' + core.prStr(r)), \
               ...setResults.map(r => core.count(r) + ':' + __rt.formGet(core.meta(r), tag, null)), \
               core.prStr(core.conj(v, 3)), __rt.formGet(__rt.formMeta(core.conj(v, 3)), tag, null), \
               core.prStr(core.meta(bare)), ...bareResults.map(r => core.prStr(core.meta(r)) + ':' + core.prStr(r)), core.prStr(__rt.formMeta(core.conj(bare, 3)))].join(' ');"
        ),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        out.string().unwrap().to_string_lossy(),
        "vector:0:true vector:1:true vector:3:true vector:2:true \
         vector:0:own:[] vector:1:own:[1] vector:3:own:[1 2 3] vector:2:own:[9 2] \
         0:set 3:set [1 2 3] true \
         nil nil:[1 2 3] nil:[1] nil:[9 2] nil:[] nil"
    );
}

/// Public constructors keep caller-owned slots on wrappers; ordinary builders
/// still return engine forms. The constant helpers use those constructors too.
#[test]
fn shared_profile_public_map_and_set_constructors_keep_slots() {
    let rt = Runtime::new();
    let out = run(
        &rt,
        &format!(
            "const rt = (\n{SHARED_RUNTIME}\n); {}",
            r#"
            const core = rt.core;
            const check = (ok, label) => { if (!ok) throw new Error(label); };
            const child = __rt.formVector(7), metadata = core.hashMap('m', 1);
            const pairs = ['k', child, 'k', 2];
            const m = new core.PersistentArrayMap(metadata, 9, pairs, 59);
            check(__rt.formKind(m) === null && m instanceof core.PersistentArrayMap, 'array map wrapper');
            check(core.count(m) === 9 && m.cnt === 9 && m.arr === pairs, 'array map slots');
            check(core.get(m, 'k') === child && core.meta(m) === metadata && core.hash(m) === 59, 'array map values');
            pairs[1] = 5;
            check(core.get(m, 'k') === 5 && core.prStr(m) === '{"k" 5, "k" 2}', 'live positional duplicates');
            const root = core.hashMap('k', child, null, 'old');
            const h = new core.PersistentHashMap(metadata, 7, root, true, 9, 61);
            check(__rt.formKind(h) === null && h instanceof core.PersistentHashMap, 'hash map wrapper');
            check(h.root === root && h.cnt === 7 && h.has_nil_QMARK_ === true && h.nil_val === 9, 'hash map slots');
            check(core.count(h) === 7 && core.get(h, 'k') === child && core.get(h, null) === 9, 'hash map values');
            check(core.meta(h) === metadata && core.hash(h) === 61, 'hash map metadata/hash');
            const s = new core.PersistentHashSet(metadata, root, 67);
            check(__rt.formKind(s) === null && s instanceof core.PersistentHashSet && s.hash_map === root, 'hash set slots');
            check(core.count(s) === 2 && core.get(s, 'k') === 'k' && core.contains(s, null), 'hash set values');
            check(core.meta(s) === metadata && core.hash(s) === 67, 'hash set metadata/hash');
            for (const value of [m, h, s]) {
              const copy = core.withMeta(value, null), cloned = core.clone(value);
              check(copy !== value && core.meta(copy) === null && core.hash(copy) === core.hash(value), 'with-meta');
              check(core.meta(cloned) === metadata && core.hash(cloned) === core.hash(value), 'clone');
            }
            check(core.withMeta(m, null).arr === pairs && core.withMeta(h, null).root === root && core.withMeta(s, null).hash_map === root, 'copy slot identity');
            const constant = rt.constantMapFromArray(['k', child, 'k', 2]);
            const constantSet = rt.constantSetFromArray(['k', 'k']);
            check(core.count(constant) === 2 && core.get(constant, 'k') === child && constant.arr[1] === child, 'constant map');
            check(core.count(constantSet) === 2 && core.get(constantSet, 'k') === 'k', 'constant set');
            check(core.get(core.PersistentArrayMap(null, 1, ['k', 1], null), 'k') === 1, 'called array constructor');
            check(core.count(core.PersistentHashMap(null, 0, null, false, null, null)) === 0, 'called hash constructor');
            check(core.count(core.PersistentHashSet(null, core.hashMap(), null)) === 0, 'called set constructor');
            check(__rt.formKind(rt.arrayMapFromArray(['k', child])) === 'map' && __rt.formKind(core.hashMap('k', child)) === 'map', 'ordinary map builders');
            check(__rt.formKind(core.hashSet('k')) === 'set' && __rt.formKind(rt.setFromArray(['k'])) === 'set', 'ordinary set builders');
            return 'ok';
            "#
        ),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(out.string().unwrap().to_string_lossy(), "ok");
}

#[test]
fn shared_profile_public_collection_updates_keep_constructor_slots() {
    let rt = Runtime::new();
    let out = run(
        &rt,
        &format!(
            "const rt = (\n{SHARED_RUNTIME}\n); {}",
            r#"
            const core = rt.core;
            const check = (ok, label) => { if (!ok) throw new Error(label); };
            const child = core.vector(7), metadata = core.hashMap('m', 1);
            const pairs = ['k', child, 'keep', child];
            const a = new core.PersistentArrayMap(metadata, 2, pairs, 59);
            const h = new core.PersistentHashMap(metadata, 2, core.hashMap('k', child, 'keep', child), false, null, 61);
            for (const original of [a, h]) {
              const added = core.assoc(original, 'new', 3), replaced = core.assoc(original, 'k', 4);
              const removed = core.dissoc(original, 'k'), conjoined = core.conj(original, core.vector('new', 3));
              for (const updated of [added, replaced, removed, conjoined]) {
                check(__rt.formKind(updated) === null && updated instanceof original.constructor, 'updated wrapper');
                check(core.meta(updated) === metadata && core.get(updated, 'keep') === child, 'retained metadata/child');
                check(updated.cnt === core.count(updated), 'updated count slot');
                check(core.hash(updated) !== core.hash(original), 'invalidated cached hash');
              }
              check(added.cnt === 3 && replaced.cnt === 2 && removed.cnt === 1, 'count changes');
              check(core.get(original, 'k') === child && core.count(original) === 2, 'persistent original');
            }
            const full = new core.PersistentArrayMap(metadata, 8, [0, child, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7], 71);
            const promoted = core.assoc(full, 8, 8);
            check(core.count(promoted) === 9 && core.get(promoted, 0) === child && core.meta(promoted) === metadata, 'public promotion');
            const nil = core.assoc(h, null, 9);
            check(nil.has_nil_QMARK_ === true && nil.nil_val === 9 && core.get(nil, null) === 9, 'nil slots');
            check(core.dissoc(nil, null).has_nil_QMARK_ === false, 'removed nil slot');
            const set = new core.PersistentHashSet(metadata, h, 67);
            const added = core.conj(set, 'new'), removed = core.disj(added, 'k');
            check(added instanceof core.PersistentHashSet && removed instanceof core.PersistentHashSet, 'set wrappers');
            check(core.count(added) === 3 && core.count(removed) === 2 && !core.contains(removed, 'k'), 'set changes');
            check(core.meta(added) === metadata && core.meta(removed) === metadata && core.contains(set, 'k'), 'set persistence');
            return 'ok';
            "#
        ),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(out.string().unwrap().to_string_lossy(), "ok");
}

#[test]
fn shared_profile_persistent_updates_preserve_value_semantics() {
    let rt = Runtime::new();
    let out = run(
        &rt,
        &format!(
            "const rt = (\n{SHARED_RUNTIME}\n); {}",
            r#"
            const core = rt.core;
            const check = (ok, label) => { if (!ok) throw new Error(label); };
            const child = core.vector('child'), metadata = core.hashMap('m', true);
            let map = core.withMeta(core.hashMap('keep', child, null, 0), metadata);
            const original = map;
            map = core.assoc(map, 'x', 1, 'y', 2, 'odd');
            check(__rt.formKind(map) === 'map' && core.count(map) === 5 && core.get(map, 'odd') === null, 'native variadic assoc');
            map = core.conj(map, core.vector('z', 3));
            map = core.update(map, 'z', x => x + 1);
            map = core.dissoc(map, 'x', 'y', 'absent', null);
            check(__rt.formKind(map) === 'map' && core.get(map, 'z') === 4 && core.get(map, 'keep') === child, 'native map updates');
            check(core.meta(map) === metadata && core.count(original) === 2 && core.get(original, null) === 0, 'map persistence');
            const expected = core.hashMap('keep', child, 'odd', null, 'z', 4);
            check(core.equal(map, expected) && core.hash(map) === core.hash(expected), 'map equality/hash');
            check(core.assoc(map, 'keep', child) === map && core.dissoc(map, 'absent') === map, 'map no-op');
            let positional = rt.constantMapFromArray(['a', child, 'a', 2, 'b', 3]);
            positional = core.assoc(positional, 'a', 9);
            check(core.prStr(positional) === '{"a" 9, "a" 2, "b" 3}', 'positional duplicates/order');
            check(core.prStr(core.dissoc(positional, 'a')) === '{"b" 3, nil nil}', 'positional removal holes');
            const key = core.vector(1, 2), equalKey = core.list(1, 2);
            const keyed = core.hashMap(key, child);
            const replaced = core.assoc(keyed, equalKey, 9);
            check(core.count(replaced) === 1 && core.get(replaced, key) === 9, 'runtime structural equality fallback');
            check(core.nth(core.first(core.seq(replaced)), 0) === key, 'retained equal key identity');
            check(core.count(core.dissoc(replaced, equalKey)) === 0, 'structural dissoc fallback');
            let set = core.withMeta(core.hashSet('a', 'b'), metadata), oldSet = set;
            set = core.conj(set, 'c', 'd');
            set = core.disj(set, 'a', 'absent');
            check(__rt.formKind(set) === 'set' && core.count(set) === 3 && core.meta(set) === metadata, 'native set updates');
            check(core.contains(oldSet, 'a') && !core.contains(set, 'a') && core.get(set, 'c') === 'c', 'set persistence/lookup');
            check(core.conj(set, 'b') === set && core.disj(set, 'absent') === set, 'set no-op');
            check(core.equal(set, core.hashSet('b', 'c', 'd')) && core.hash(set) === core.hash(core.hashSet('b', 'c', 'd')), 'set equality/hash');
            const named = core.keyword('n', 'k'), namedMap = core.assoc(core.hashMap(named, child), core.keyword('n', 'k'), 9);
            check(core.count(namedMap) === 1 && core.get(namedMap, named) === 9, 'named key equality');
            const canonical = core.symbol('a', 'b');
            const alias = new core.Symbol('other', 'parts', 'a/b', core.hash(canonical), null);
            const aliased = core.hashMap(alias, child), renamed = core.assoc(aliased, canonical, 9);
            check(core.get(aliased, canonical) === child && core.count(renamed) === 1 && core.get(renamed, alias) === 9, 'host-backed named equality');
            check(core.count(core.dissoc(aliased, canonical)) === 0, 'host-backed named dissoc');
            return 'ok';
            "#
        ),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(out.string().unwrap().to_string_lossy(), "ok");
}

#[test]
fn shared_profile_nan_occurrence_keys_use_canonical_equality() {
    let rt = Runtime::new();
    let check = compile_function(&format!(
        "function check(key) {{ const rt = (\n{SHARED_RUNTIME}\n); {} }}",
        r#"
        const core = rt.core;
        const check = (ok, label) => { if (!ok) throw new Error(label); };
        check(!core.equal(key, key), 'NaN canonical equality');
        for (const map of [__rt.formMap(key, 1), __rt.formLiteralMap(__rt.formVector(key, 1))]) {
          check(core.get(map, key, 'missing') === 'missing', 'NaN map lookup');
          check(!core.contains(map, key), 'NaN map contains');
          const added = core.assoc(map, key, 2);
          check(core.count(added) === 2, 'NaN assoc inserts');
          check(typeof key !== 'object' || core.nth(core.first(core.seq(added)), 0) === key, 'NaN stored key identity');
          check(core.get(added, key, 'missing') === 'missing', 'NaN assoc lookup');
          check(core.count(core.dissoc(map, key)) === 1, 'NaN dissoc retains');
          check(core.count(core.assoc(added, key, 3)) === 3 && core.count(core.dissoc(added, key)) === 2, 'NaN repeated map updates');
          const ordinary = core.assoc(map, 'ordinary', 9);
          check(core.count(ordinary) === 2 && core.get(ordinary, key, 'missing') === 'missing', 'ordinary update retains NaN semantics');
          check(core.count(map) === 1, 'NaN map persistence');
        }
        for (const set of [__rt.formSet(key), __rt.formLiteralSet(__rt.formVector(key))]) {
          check(core.get(set, key, 'missing') === 'missing' && !core.contains(set, key), 'NaN set lookup');
          const added = core.conj(set, key);
          check(core.count(added) === 2, 'NaN conj inserts');
          check(typeof key !== 'object' || core.first(core.seq(added)) === key, 'NaN stored member identity');
          check(core.count(core.disj(set, key)) === 1, 'NaN disj retains');
          check(core.count(core.conj(added, key)) === 3 && core.disj(added, key) === added, 'NaN repeated set updates');
          check(core.count(set) === 1, 'NaN set persistence');
        }
        check(core.count(core.hashMap(key, 1, key, 2)) === 2 && core.count(core.hashSet(key, key)) === 2, 'NaN builders retain slots');
        return 'ok';
        "#
    ))
    .unwrap();
    for depth in 0..=2 {
        let key = rt.scope(|s| {
            let mut key = s.primitive(Primitive::Number(f64::NAN));
            for _ in 0..depth {
                key = s.new_occurrence(key, Default::default()).unwrap();
            }
            s.persist(key)
        });
        match rt.spawn(&check, &[&key]).unwrap().run() {
            Ok(RunOutcome::Complete(value)) => {
                assert_eq!(value.string().unwrap().to_string_lossy(), "ok");
            }
            other => panic!("NaN occurrence depth {depth}: {other:?}"),
        }
    }
}

#[test]
fn shared_profile_update_noops_preserve_occurrence_identity() {
    let rt = Runtime::new();
    let values = run(
        &rt,
        "const key = 'keep', metadata = __rt.formMap('m', true); \
         return __rt.formVector(__rt.formWithMeta(__rt.formSet(key), metadata), \
           __rt.formWithMeta(__rt.formMap(key, 1), metadata));",
    )
    .unwrap();
    let check = compile_function(&format!(
        "function check(set, map, key) {{ const rt = (\n{SHARED_RUNTIME}\n); {} }}",
        r#"
        const core = rt.core;
        const check = (ok, label) => { if (!ok) throw new Error(label); };
        check(core.conj(set, 'keep') === set, 'conj existing preserves occurrence');
        check(core.disj(set, 'absent') === set, 'disj absent preserves occurrence');
        check(core.conj(set, 'keep', 'keep') === set, 'variadic conj no-op');
        check(core.disj(set, 'absent', 'other') === set, 'variadic disj no-op');
        check(core.assoc(map, 'keep', 1) === map, 'assoc identical preserves occurrence');
        check(core.dissoc(map, 'absent') === map, 'dissoc absent preserves occurrence');
        check(core.assoc(map, 'keep', 1, 'keep', 1) === map, 'variadic assoc no-op');
        check(core.dissoc(map, 'absent', 'other') === map, 'variadic dissoc no-op');
        check(core.count(core.conj(set, 'added')) === 2 && core.count(set) === 1, 'changed conj');
        check(core.count(core.disj(set, 'keep')) === 0 && core.count(set) === 1, 'changed disj');
        check(core.meta(core.conj(set, 'added')) === core.meta(set), 'changed set metadata');
        check(core.meta(core.assoc(map, 'added', 2)) === core.meta(map), 'changed map metadata');
        const keyed = core.hashMap(key, 'stored'), member = core.hashSet(key);
        check(__rt.formKind(keyed) === 'map' && __rt.formKind(member) === 'set', 'ordinary scalar key stays native');
        check(core.get(keyed, 7) === 'stored' && core.get(member, 7) === key, 'ordinary scalar key equality');
        check(core.nth(core.first(core.seq(keyed)), 0) === key && core.first(core.seq(member)) === key, 'ordinary scalar key identity');
        return 'ok';
        "#
    ))
    .unwrap();
    for depth in 1..=2 {
        let wrapped = rt.scope(|s| {
            [0, 1, 2].map(|index| {
                let mut value = if index == 2 {
                    s.primitive(Primitive::Number(7.0))
                } else {
                    s.form_nth(s.local(&values).unwrap(), index).unwrap()
                };
                for _ in 0..depth {
                    value = s.new_occurrence(value, Default::default()).unwrap();
                }
                s.persist(value)
            })
        });
        match rt
            .spawn(&check, &[&wrapped[0], &wrapped[1], &wrapped[2]])
            .unwrap()
            .run()
        {
            Ok(RunOutcome::Complete(value)) => {
                assert_eq!(value.string().unwrap().to_string_lossy(), "ok");
            }
            other => panic!("collection occurrence depth {depth}: {other:?}"),
        }
    }
}

/// Pause the same owning task around each measured operation. Boot, input
/// construction and assertions are outside the paired allocation samples.
fn shared_profile_measured_allocations(body: &str) -> Vec<usize> {
    let rt = Runtime::with_heap_policy(
        Default::default(),
        HeapPolicy {
            hard_limit_bytes: Some(1 << 30),
            ..Default::default()
        },
    );
    let code = compile_function(&format!(
        "function measure() {{ const rt = (\n{SHARED_RUNTIME}\n); const core = rt.core; {body} }}"
    ))
    .unwrap();
    let task = rt.spawn(&code, &[]).unwrap();
    let ready = rt.root(Primitive::Null);
    let mut samples = Vec::new();
    loop {
        match task.run().unwrap() {
            RunOutcome::Waiting(_) => {
                samples.push(rt.heap_stats().admitted_bytes_total);
                let requests = task.take_producer_requests();
                assert_eq!(requests.len(), 1);
                let request = &requests[0];
                let wakeups = rt
                    .complete(
                        request.key(),
                        request.producer(),
                        big_boy_pants_js::Completion::Ready(&ready),
                    )
                    .unwrap();
                assert_eq!(wakeups.task_ids(), [task.id()]);
            }
            RunOutcome::Complete(result) => {
                assert_eq!(result.string().unwrap().to_string_lossy(), "ok");
                break;
            }
            other => panic!("{other:?}"),
        }
    }
    assert_eq!(samples.len() % 2, 0);
    let allocations: Vec<_> = samples
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| pair[1] - pair[0])
        .collect();
    assert!(
        allocations.iter().all(|bytes| *bytes > 0),
        "byte accounting must be enabled: {allocations:?}"
    );
    allocations
}

#[test]
fn shared_profile_hash_map_updates_copy_only_changed_paths() {
    let allocations = shared_profile_measured_allocations(
        r#"
        const check = (ok, label) => { if (!ok) throw new Error(label); };
        let phase = 0;
        for (const n of [32, 512, 4096]) {
          let map = __rt.formMap(), set = __rt.formSet();
          const child = __rt.formVector(7), metadata = __rt.formMap('m', true);
          for (let i = 0; i < n; i++) {
            map = __rt.formAssoc(map, i, child); set = __rt.formConj(set, i);
          }
          map = __rt.formWithMeta(map, metadata); set = __rt.formWithMeta(set, metadata);
          for (let operation = 0; operation < 7; operation++) {
            globalThis.__lightSharedCells = core.sharedTaskCells();
            __rt.need(713, phase++);
            let result;
            switch (operation) {
              case 0: result = core.assoc(map, 0, 9); break;
              case 1: result = core.assoc(map, -1, 9); break;
              case 2: result = core.dissoc(map, 0); break;
              case 3: result = core.conj(map, __rt.formVector(-1, 9)); break;
              case 4: result = core.update(map, 0, () => 9); break;
              case 5: result = core.conj(set, -1); break;
              case 6: result = core.disj(set, 0); break;
            }
            __rt.need(713, phase++);
            check(__rt.formKind(result) === (operation < 5 ? 'map' : 'set'), 'native result');
            check(core.meta(result) === metadata, 'metadata');
            if (operation < 5) check(core.get(result, 1) === child && core.get(map, 0) === child, 'unchanged child/original');
            const expected = operation === 0 || operation === 4 ? n : operation === 2 || operation === 6 ? n - 1 : n + 1;
            check(core.count(result) === expected, 'updated count');
          }
        }
        return 'ok';
    "#,
    );
    eprintln!(
        "single updates at 32/512/4096 entries, admitted bytes: {:?}",
        allocations.chunks(7).collect::<Vec<_>>()
    );
    assert_eq!(allocations.len(), 21);
    for operation in 0..7 {
        assert!(
            allocations[14 + operation] < 4 * allocations[operation],
            "whole-collection allocation in operation {operation}: {allocations:?}"
        );
    }
}

#[test]
fn shared_profile_frequencies_allocation_grows_with_input() {
    let allocations = shared_profile_measured_allocations(
        r#"
        let phase = 0;
        for (const n of [64, 256]) {
          let input = __rt.formVector();
          for (let i = 0; i < n; i++) input = __rt.formConj(input, i);
          __rt.need(713, phase++);
          const result = core.frequencies(input);
          __rt.need(713, phase++);
          if (__rt.formCount(result) !== n || core.get(result, 0) !== 1 || core.get(result, n - 1) !== 1) throw new Error('frequencies values');
        }
        return 'ok';
    "#,
    );
    eprintln!("frequencies 64/256 admitted bytes: {allocations:?}");
    assert_eq!(allocations.len(), 2);
    assert!(
        allocations[1] < 6 * allocations[0],
        "quadratic frequencies allocation: {allocations:?}"
    );
}

/// Sorted-map find returns cached, runtime-owned tree nodes implementing
/// IMapEntry, while ordinary map sequences keep immutable engine entries.
#[test]
fn shared_profile_sorted_map_find_builds_tree_nodes() {
    let rt = Runtime::new();
    let out = run(
        &rt,
        &format!(
            "const rt = (\n{SHARED_RUNTIME}\n); {}",
            r#"
            const core = rt.core;
            const check = (ok, label) => { if (!ok) throw new Error(label); };
            const single = core.sortedMap(1, 'a'), entry = core.find(single, 1);
            check(core.prStr(entry) === '[1 "a"]', 'single find');
            check(core.keyProtocol(entry) === 1 && core.valProtocol(entry) === 'a', 'entry key/val');
            check(core.isMapEntry(entry) && core.isVector(entry) && entry instanceof core.MapEntry, 'node brand');
            check(__rt.formKind(entry) === null && entry === single.tree && entry === core.find(single, 1), 'cached node');
            check(entry.left === null && entry.right === null && core.find(single, 2) === null, 'single edges');
            const empty = core.sortedMap();
            check(empty.tree === null && core.find(empty, 1) === null, 'empty tree');
            const child = core.vector('child'), metadata = core.hashMap('meta', true);
            const m = core.withMeta(core.sortedMap(3, 'c', 1, 'a', 4, 'd', 2, child), metadata);
            const root = m.tree, nodes = [];
            const visit = node => {
              if (node === null) return;
              visit(node.left);
              check(core.isMapEntry(node) && core.keyProtocol(node) === node.key && core.valProtocol(node) === node.val, 'traversal entry');
              check(core.find(m, node.key) === node, 'find node identity');
              nodes.push(node.key);
              visit(node.right);
            };
            visit(root);
            check(nodes.join(',') === '1,2,3,4' && m.tree === root, 'ordered cached traversal');
            check(core.valProtocol(core.find(m, 2)) === child && core.meta(m) === metadata, 'value/metadata identity');
            check(core.prStr(core.keys(m)) === '(1 2 3 4)' && core.count(m) === 4, 'sequence order');
            const cmp = (a, b) => core.nth(b, 0) - core.nth(a, 0);
            const retained = core.vector(2, 'retained'), equal = core.vector(2, 'query');
            const descending = core.sortedMapBy(cmp, core.vector(1), 'one', retained, child, equal, 'new', core.vector(3), 'three');
            const found = core.find(descending, equal);
            check(core.keyProtocol(found) === retained && found.key === retained && found.val === 'new', 'comparator retains key');
            check(core.prStr(core.keys(descending)) === '([3] [2 "retained"] [1])', 'custom order');
            check(core.comparatorProtocol(descending)(retained, core.vector(1)) < 0, 'comparator preserved');
            check(core.find(descending, core.vector(0)) === null, 'custom missing');
            const supplied = new core.PersistentTreeMap(cmp, descending.tree, 3, metadata, 71);
            check(supplied.tree === descending.tree && core.find(supplied, equal) === found, 'public tree identity');
            check(core.meta(supplied) === metadata && core.hash(supplied) === 71, 'public metadata/hash');
            const ordinary = core.first(core.seq(core.hashMap('k', child)));
            check(__rt.cljIsMapEntry(ordinary) && __rt.formKind(ordinary) === 'vector', 'ordinary engine entry');
            check(core.keyProtocol(ordinary) === 'k' && core.valProtocol(ordinary) === child, 'ordinary entry identity');
            return ordinary;
            "#
        ),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    rt.publish(&out)
        .unwrap_or_else(|e| panic!("ordinary map entry does not publish: {e}"));
}

/// Tree construction remains callable through published runtime functions.
#[test]
fn shared_profile_sorted_map_find_members_publish() {
    let rt = Runtime::new();
    let members = run(
        &rt,
        &format!(
            "const rt = (\n{SHARED_RUNTIME}\n); const core = rt.core; \
             return __rt.formVector(core.find, core.sortedMap, core.keyProtocol, core.valProtocol, core.sharedTaskCells);"
        ),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    let values = rt.scope(|s| {
        let members = s.local(&members).unwrap();
        (0..5)
            .map(|i| s.persist(s.form_nth(members, i).unwrap()))
            .collect::<Vec<_>>()
    });
    for value in &values[..4] {
        rt.publish(value)
            .unwrap_or_else(|e| panic!("sorted map member does not publish: {e}"));
    }
    let call = compile_function(
        "function call(find, sortedMap, key, val, taskCells) { \
           globalThis.__lightSharedCells = taskCells(); \
           const m = sortedMap(1, 'a'), node = find(m, 1); \
           return String(key(node) === 1 && val(node) === 'a' && node === m.tree); }",
    )
    .unwrap();
    let args = values.iter().collect::<Vec<_>>();
    let task = rt.spawn(&call, &args).unwrap();
    std::thread::scope(|s| {
        s.spawn(|| match task.run().unwrap() {
            RunOutcome::Complete(v) => assert_eq!(v.string().unwrap().to_string_lossy(), "true"),
            other => panic!("{other:?}"),
        })
        .join()
        .unwrap();
    });
}

/// An engine map is a map to the real units (the maps view, light-cljs-666):
/// `map?`, `count`, `get`, `contains?`, `find`, `keys`, `vals`, `seq`,
/// printing, equality and hashing with a runtime-built map of the same
/// entries, `reduce-kv`, `into`, and the updates (`assoc`, `dissoc`, `conj`,
/// `empty`, `merge`), so engine metadata is readable throughout; and a map
/// entry is a runtime-owned pair with the vectors' prototype, so the `seq`
/// of a runtime-built map works as well.
#[test]
fn shared_profile_engine_maps_are_maps() {
    let rt = Runtime::new();
    let out = run(
        &rt,
        &format!(
            "const rt = (\n{SHARED_RUNTIME}\n); const core = rt.core; \
             const a = core.keyword(null, 'a'), b = core.keyword(null, 'b'), c = core.keyword(null, 'c'); \
             const m = __rt.formMap(a, 1, b, 2), own = rt.arrayMapFromArray([a, 1, b, 2]), entry = core.first(m); \
             const sym = core.withMeta(core.symbol(null, 's'), m), vec = __rt.formWithMeta(core.vector(1), m); \
             return [core.isMap(m), core.count(m), core.get(m, a), core.get(m, c, 'none'), core.contains(m, a), core.contains(m, c), \
               core.prStr(m), core.prStr(core.seq(m)), core.prStr(entry), core.prStr(core.first(entry)), core.nth(entry, 1), core.isMapEntry(entry), core.isVector(entry), core.nth(entry, 1), \
               core.equal(m, own), core.equal(own, m), core.equal(m, __rt.formMap(b, 2, a, 1)), core.equal(m, rt.arrayMapFromArray([a, 1])), core.hash(m) === core.hash(own), \
               core.prStr(core.keys(m)), core.prStr(core.vals(m)), core.prStr(core.find(m, b)), String(core.find(m, c)), \
               core.prStr(core.assoc(m, c, 3)), core.count(core.dissoc(m, a)), core.prStr(core.dissoc(m, a)), core.prStr(core.conj(m, core.vector(c, 4))), \
               core.prStr(core.empty(m)), core.count(core.empty(m)), core.prStr(core.into(core.vector(), m)), core.reduceKv((acc, _k, v) => acc + v, 0, m), \
               core.prStr(core.merge(m, rt.arrayMapFromArray([c, 3]))), core.prStr(core.merge(own, m)), core.prStr(core.zipmap(core.vector(a), core.vector(9))), \
               core.get(core.meta(sym), a), core.get(core.meta(vec), b), core.prStr(core.meta(vec)), core.equal(core.meta(vec), own), \
               core.prStr(core.seq(core.hashMap(1, 2))), core.prStr(core.first(core.hashMap(1, 2))), core.isVector(core.first(core.hashMap(1, 2))), \
               core.prStr(core.vec(m)), core.prStr(core.into(rt.arrayMapFromArray([]), m)), core.get(core.into(rt.arrayMapFromArray([]), m), b), \
               core.get(core.selectKeys(m, core.vector(b)), b), core.count(core.selectKeys(m, core.vector(b)))].join(' ');"
        ),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        out.string().unwrap().to_string_lossy(),
        // `formMap` builds a hash map, whose entries print in the engine's
        // trie order (`:b` before `:a`), and an update of it is a hash map
        // again (`:c` first; the builders swap, light-cljs-666); a
        // runtime-built array map keeps its insertion order.
        "true 2 1 none true false \
         {:b 2, :a 1} ([:b 2] [:a 1]) [:b 2] :b 2 true true 2 \
         true true true false true \
         (:b :a) (2 1) [:b 2] null \
         {:c 3, :b 2, :a 1} 1 {:b 2} {:c 4, :b 2, :a 1} \
         {} 0 [[:b 2] [:a 1]] 3 \
         {:c 3, :b 2, :a 1} {:a 1, :b 2} {:a 9} \
         1 2 {:b 2, :a 1} true \
         ([1 2]) [1 2] true \
         [[:b 2] [:a 1]] {:b 2, :a 1} 2 \
         2 1"
    );
}

/// Converting an already-built positional map preserves every entry, even
/// when lowering made keys equal or both members of an entry are undefined.
/// Ordinary array-map builders still deduplicate with ClojureScript equality.
#[test]
fn shared_profile_positional_map_updates_keep_entries() {
    let rt = Runtime::new();
    let out = run(
        &rt,
        &format!(
            "const rt = (\n{SHARED_RUNTIME}\n); const core = rt.core; \
             const m = __rt.formLiteralMap(__rt.formVector('a', 1, 'a', 2)), updated = core.assoc(m, 'a', 3); \
             const undef = core.arrayMap(undefined, undefined); \
             const k1 = core.vector('key'), k2 = core.vector('key'), child = core.vector('unchanged'); \
             const metadata = core.arrayMap('tag', 'positional'); \
             const original = __rt.formWithMeta(__rt.formLiteralMap(__rt.formVector(k1, 1, k2, child)), metadata); \
             const copy = core.assoc(original, core.vector('key'), 3), items = __rt.formPrintItems(copy); \
             const ordinary = core.arrayMap(k1, 1, k2, 2); \
             return [core.count(updated), core.get(updated, 'a'), core.prStr(updated), core.prStr(m), \
               core.count(undef), core.contains(undef, undefined), core.get(undef, undefined, 'missing') === undefined, \
               core.count(copy), core.get(copy, k2) === 3, core.meta(copy) === metadata, \
               __rt.formNth(__rt.formNth(items, 0), 0) === k1, \
               __rt.formCount(items) === 2 && __rt.formNth(__rt.formNth(items, 1), 0) === k2, \
               __rt.formCount(items) === 2 && __rt.formNth(__rt.formNth(items, 1), 1) === child, core.get(original, k1), \
               core.count(ordinary), core.get(ordinary, core.vector('key'))].join(' ');"
        ),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        out.string().unwrap().to_string_lossy(),
        "2 3 {\"a\" 3, \"a\" 2} {\"a\" 1, \"a\" 2} \
         1 true true 2 true true true true true 1 1 2"
    );
}

/// An engine set is a set (the sets view, light-cljs-666): the predicates,
/// count, membership, printing, equality and hashing with a runtime-built
/// set of the same members, `seq`, `conj`, `disj`, `into` and `empty`
/// read it as a positional set of its members.
#[test]
fn shared_profile_engine_sets_are_sets() {
    let rt = Runtime::new();
    let out = run(
        &rt,
        &format!(
            "const rt = (\n{SHARED_RUNTIME}\n); const core = rt.core; \
             const a = core.keyword(null, 'a'), b = core.keyword(null, 'b'), c = core.keyword(null, 'c'); \
             const s = __rt.formSet(a, b), own = rt.setFromArray([a, b]); \
             return [core.isSet(s), core.isColl(s), core.count(s), core.contains(s, a), core.contains(s, c), core.prStr(core.get(s, b)), String(core.get(s, c)), \
               core.prStr(s), core.prStr(core.seq(s)), core.prStr(core.first(s)), core.equal(s, own), core.equal(own, s), core.equal(s, rt.setFromArray([a])), \
               core.hash(s) === core.hash(own), core.count(core.conj(s, c)), core.contains(core.conj(s, c), c), core.count(core.disj(s, a)), core.contains(core.disj(s, a), a), \
               core.count(core.empty(s)), core.prStr(core.into(core.vector(), s)), core.count(core.into(s, core.vector(c, a))), core.count(core.set(s)), \
               core.count(rt.setFromArray([s, __rt.formSet(b, a)]))].join(' ');"
        ),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        out.string().unwrap().to_string_lossy(),
        "true true 2 true false :b null #{:a :b} (:a :b) :a true true false true 3 true 1 false 0 [:a :b] 3 2 1"
    );
}

/// A `#js` literal the profile's reader reads is an engine compiler
/// literal, as the Pants build's reader builds one (the `compilerForms`
/// swap, light-cljs-666): the reader entry points then hold nothing of the
/// V8 form bridge and publish. The public array predicate recognizes these
/// literals while keeping its ordinary JavaScript array behavior.
#[test]
fn shared_profile_reader_js_literals_are_engine_literals() {
    let rt = Runtime::new();
    let out = run(
        &rt,
        &format!(
            "const rt = (\n{SHARED_RUNTIME}\n); const core = rt.core; \
             const array = core.readString('#js [1 2]'), object = core.readString('#js {{:a 1 \"b\" 2}}'); \
             const payload = __rt.formLiteralPayload(object); \
             return [__rt.formKind(array), __rt.formName(array), __rt.formKind(__rt.formLiteralPayload(array)), __rt.formCount(__rt.formLiteralPayload(array)), \
               __rt.formKind(object), __rt.formName(object), __rt.formKind(payload), __rt.formCount(payload), __rt.formGet(payload, 'a', null), __rt.formGet(payload, 'b', null), \
               core.isArray(array), core.isArray(core.readString('#js []')), core.isArray(object), \
               core.isArray([]), core.isArray([1, 2]), core.isArray({{length: 2}}), \
               core.isArray(core.vector(1, 2)), core.isArray(core.keyword('js-array')), \
               core.isArray(null), core.isArray(undefined), core.isArray('js-array'), core.isArray(2)].join(' ');"
        ),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        out.string().unwrap().to_string_lossy(),
        "compiler-literal js-array vector 2 compiler-literal js-object map 2 1 2 true true false true true false false false false false false false"
    );
    for member in [
        "readString",
        "read",
        "ednReadString",
        "readerInitialTagTable",
        "isArray",
    ] {
        let value = run(
            &rt,
            &format!("const rt = (\n{SHARED_RUNTIME}\n); return rt.core['{member}'];"),
        )
        .unwrap_or_else(|e| panic!("{e}"));
        rt.publish(&value)
            .unwrap_or_else(|e| panic!("core.{member} does not publish: {e}"));
    }
}

/// Imported literal occurrences have the same array classification as the
/// reader's values, including through nested occurrence wrappers.
#[test]
fn shared_profile_array_predicate_reads_occurrences() {
    let rt = Runtime::new();
    let values = run(
        &rt,
        &format!(
            "const rt = (\n{SHARED_RUNTIME}\n); const core = rt.core; \
             return __rt.formVector(core.isArray, core.readString('#js [1 2]'), \
               core.readString('#js {{:a 1}}'), core.vector(1, 2));"
        ),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    let predicate = rt.scope(|s| s.persist(s.form_nth(s.local(&values).unwrap(), 0).unwrap()));
    let check =
        compile_function("function check(predicate, value) { return predicate(value); }").unwrap();
    for (index, expected) in [(1, true), (2, false), (3, false)] {
        let wrapped = rt.scope(|s| {
            let value = s.form_nth(s.local(&values).unwrap(), index).unwrap();
            let wrapped = s.new_occurrence(value, Default::default()).unwrap();
            s.persist(s.new_occurrence(wrapped, Default::default()).unwrap())
        });
        match rt.spawn(&check, &[&predicate, &wrapped]).unwrap().run() {
            Ok(RunOutcome::Complete(value)) => {
                assert_eq!(value.view(), big_boy_pants_js::ValueView::Bool(expected));
            }
            other => panic!("{other:?}"),
        }
    }
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

/// Engine chunked sequences keep their chunk boundary when the real units
/// read them, and mapping a chunk produces a publishable engine chunk too.
#[test]
fn shared_profile_engine_chunks_survive_lazy_mapping_and_publication() {
    let rt = Runtime::new();
    let result = run(
        &rt,
        &format!(
            "const rt = (\n{SHARED_RUNTIME}\n); const core = rt.core; \
             let input = __rt.formVector(); for (let i = 0; i < 40; i++) input = __rt.formConj(input, i); \
             const source = __rt.cljSeq(input); \
             let calls = 0; const mapped = core.mapSeq(x => {{ calls++; return x + 1; }}, source); \
             core.first(mapped); const first = calls; core.nth(mapped, 31); const last = calls; \
             core.nth(mapped, 32); const next = calls; \
             const pure = core.mapSeq(x => x * 2, source); \
             core.first(pure); \
             return __rt.formVector(pure, core.sharedTaskCells, \
               [core.isChunkedSeq(source), core.count(core.chunkFirst(source)), core.first(core.chunkRest(source)), \
                first, last, next, core.isChunkedSeq(core.seq(pure))].join(' '));"
        ),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    let (mapped, cells, counts) = rt.scope(|s| {
        let result = s.local(&result).unwrap();
        (
            s.persist(s.form_nth(result, 0).unwrap()),
            s.persist(s.form_nth(result, 1).unwrap()),
            s.string(s.form_nth(result, 2).unwrap())
                .unwrap()
                .to_string_lossy(),
        )
    });
    assert_eq!(counts, "true 32 32 32 32 40 true");
    rt.publish(&mapped).unwrap();
    let walk = compile_function(
        "function walk(xs, makeCells) { globalThis.__lightSharedCells = makeCells(); \
         return [__rt.cljNth(xs, 32), __rt.cljCount(xs)].join(' '); }",
    )
    .unwrap();
    let out = match rt.spawn(&walk, &[&mapped, &cells]).unwrap().run().unwrap() {
        RunOutcome::Complete(value) => value,
        other => panic!("{other:?}"),
    };
    assert_eq!(out.string().unwrap().to_string_lossy(), "64 40");
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
/// less the boot fails with `HeapLimit`. Re-measured after the third
/// attempt under a 256 MiB limit: 69 MB peak over 10 collections, 5.4 MB
/// live after a collection at the end of the boot, so the peak is the
/// boot's garbage. The budget is open until that garbage is understood
/// (light-cljs-666 hand-back).
#[test]
#[ignore = "the shared-heap profile's boot allocates 69 MB for 5.4 MB live; the 4 MiB budget is open (light-cljs-666)"]
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
