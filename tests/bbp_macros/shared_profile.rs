//! The shared-heap profile of the real cljs.core (`src/js_shared_profile.js`,
//! assembled by build.rs into `SHARED_RUNTIME`; docs/real-core-design.md,
//! section 1; light-cljs-647): it boots in a boot task and publishes.

use big_boy_pants_js::frontend::compile_function;
use big_boy_pants_js::{PublishError, Rooted, RunOutcome, Runtime};
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
/// in the profile): the store and the writer, once each.
fn boot_store_writes(rt: &Runtime) -> String {
    let calls = run(
        rt,
        &format!(
            "const rt = (\n{SHARED_RUNTIME}\n); \
             const calls = globalThis.__lightSharedSetCalls ?? []; \
             const seen = new Set(); const out = []; \
             for (const call of calls) {{ const lines = call.split('\\n'); const key = lines[0] + ' ' + (lines[2] ?? lines[1]); \
               if (!seen.has(key)) {{ seen.add(key); out.push(key); }} }} \
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
/// member by cause.
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
