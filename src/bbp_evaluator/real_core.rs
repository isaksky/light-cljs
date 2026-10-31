//! The real `cljs.core` for the Pants build's macros (`--macro-core real`;
//! docs/real-core-design.md, section 1 points 7 and 9, section 5).
//!
//! Once per compilation a boot task runs the shared-heap profile
//! (`SHARED_RUNTIME`, assembled by build.rs from `src/js_runtime.js` and
//! `src/js_shared_profile.js`). Each member of its `core` hub that
//! `Runtime::publish` accepts goes on the published hub, a sealed object
//! over the profile's per-task defaults; the members that do not publish
//! are listed, and a task that reads one boots a private copy of the
//! profile once and takes the member from it (the per-task fallback a
//! helper whose value does not publish takes). Every macro task gets the
//! handle `[hub, boot, unpublished names, private boots]`
//! (`nativeMacroTask`'s `realCore` parameter in `src/bbp_runtime.js`),
//! makes its cells over the hub and resolves its `nativeCore$X` bindings
//! and `globalThis.__lightNativeRuntime.core` from them.
use super::{RejectHost, error};
use crate::evaluator::{EvalError, ExecutionControl};
use big_boy_pants_js::{Budget, Executable, Primitive, Rooted, RunOutcome, Runtime, ValueView};
use std::sync::OnceLock;

/// The booted and published real core of one compilation.
pub(super) struct RealCore {
    /// `[hub, boot, unpublished names, private boots]`: the published hub,
    /// the profile's boot function (which captures nothing), the names of
    /// the members that did not publish, and an engine atom counting the
    /// private boots tasks made for them.
    pub(super) handle: Rooted,
    /// Members of the profile's hub, and how many of them published.
    pub(super) members: usize,
    pub(super) published: usize,
    /// The boot task's time, from spawn to the published hub.
    pub(super) boot_nanos: u64,
    /// The members that did not publish, which a task takes from a private
    /// copy of the profile.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(super) unpublished: Vec<String>,
}

impl RealCore {
    /// How many tasks booted a private copy of the profile for a member
    /// that does not publish.
    pub(super) fn private_boots(&self, runtime: &Runtime) -> usize {
        let counter = runtime.scope(|s| {
            let handle = s.local(&self.handle).ok()?;
            let atom = s.form_nth(handle, 3).ok()?;
            Some(s.persist(atom))
        });
        counter
            .and_then(|atom| runtime.atom_load(&atom).ok())
            .and_then(|count| match count.view() {
                ValueView::Number(n) => Some(n as usize),
                _ => None,
            })
            .unwrap_or(0)
    }
}

/// The boot and hub scripts, compiled once per process: executable code is
/// runtime-independent.
fn scripts() -> Result<&'static (Executable, Executable), EvalError> {
    static SCRIPTS: OnceLock<Result<(Executable, Executable), EvalError>> = OnceLock::new();
    SCRIPTS
        .get_or_init(|| {
            let source = crate::runtime_manifest::SHARED_RUNTIME;
            // The boot function captures nothing, so it publishes with the
            // hub: a task that needs a member the hub lacks calls it.
            let boot = format!(
                "function realCoreBoot() {{
                  const boot = () => (\n{source}\n);
                  const rt = boot(), core = rt.core, defaults = Object.getPrototypeOf(core);
                  let names = __rt.formVector(), values = __rt.formVector();
                  for (const name of Object.getOwnPropertyNames(core)) {{
                    const d = Object.getOwnPropertyDescriptor(core, name);
                    if (name === 'sharedTaskCells' || !('value' in d)) continue;
                    names = __rt.formConj(names, name); values = __rt.formConj(values, d.value);
                  }}
                  for (const name of Object.getOwnPropertyNames(defaults)) {{
                    const d = Object.getOwnPropertyDescriptor(defaults, name);
                    names = __rt.formConj(names, 'defaults.' + name);
                    values = __rt.formConj(values, 'value' in d ? d.value : d.get);
                  }}
                  return __rt.formVector(boot, rt, names, values);
                }}"
            );
            let hub = "function realCoreHub(booted, passing, failing, counter) {
                  const core = __rt.formNth(booted, 1).core, defaults = Object.getPrototypeOf(core);
                  const shared = Object.create(null);
                  const hub = Object.create(shared);
                  for (let i = 0; i < __rt.formCount(passing); i++) {
                    const name = __rt.formNth(passing, i);
                    if (name.startsWith('defaults.')) {
                      const d = Object.getOwnPropertyDescriptor(defaults, name.slice(9));
                      Object.defineProperty(shared, name.slice(9), 'value' in d ? {value: d.value} : {get: d.get});
                    } else Object.defineProperty(hub, name, {value: core[name], enumerable: true});
                  }
                  __rt.seal(shared); __rt.seal(hub);
                  return __rt.formVector(hub, __rt.formNth(booted, 0), failing, counter);
                }";
            let compile = |text: &str| {
                big_boy_pants_js::frontend::compile_function(text).map_err(error)
            };
            Ok((compile(&boot)?, compile(hub)?))
        })
        .as_ref()
        .map_err(Clone::clone)
}

/// Run `executable` with `args` in a task of its own to its value, polling
/// `control` between bounded steps. The boot runs no host effects.
fn run(
    runtime: &Runtime,
    control: &ExecutionControl,
    executable: &Executable,
    args: &[&Rooted],
) -> Result<Rooted, EvalError> {
    let task = runtime.spawn(executable, args).map_err(error)?;
    loop {
        control.poll()?;
        match task
            .run_with(Budget::instructions(100_000), &mut RejectHost)
            .map_err(error)?
        {
            RunOutcome::Complete(value) => return Ok(value),
            RunOutcome::Yielded => {}
            RunOutcome::Failed(e) => {
                return Err(EvalError(format!(
                    "the real cljs.core (--macro-core real) failed to boot: {e}"
                )));
            }
            other => {
                return Err(EvalError(format!(
                    "the real cljs.core (--macro-core real) did not boot: {other:?}"
                )));
            }
        }
    }
}

/// Whether hub member `name` is the evaluator's machinery rather than
/// `cljs.core`: a member declared by a unit the manifest marks
/// `compile-time` (the namespace registry, `find-ns`, the macro reader
/// context, the form bridge, declaration demand). The Pants build's
/// evaluator runtime (`src/bbp_runtime.js`) owns those, with its command
/// loop and catalog, in either mode (docs/real-core-design.md, section 2),
/// so a task's view keeps the port's.
///
/// `gensym` is the compiler's too: on the Pants build its names come from
/// the compiler's counters and gensym scopes through host effects, and the
/// compiler logs them for the cache (docs/native-macros.md, gensym), where
/// the profile's `gensym` counts in the task's cells
/// (docs/real-core-design.md, section 1 point 6). It stays the evaluator's
/// until the profile's reads the compiler's scope.
fn evaluator_member(manifest: &crate::runtime_manifest::RuntimeManifest<'_>, name: &str) -> bool {
    use crate::runtime_manifest::CoreReference;
    if name == "gensym" {
        return true;
    }
    let binding = match manifest.core_reference(name) {
        Some(CoreReference::Binding(binding) | CoreReference::Member(binding, _)) => binding,
        Some(CoreReference::Cell(_)) | None => return false,
    };
    manifest
        .unit_of(binding)
        .is_some_and(|unit| manifest.unit(unit).compile_time)
}

/// Boot the profile once in a boot task and publish its hub.
pub(super) fn boot(runtime: &Runtime, control: &ExecutionControl) -> Result<RealCore, EvalError> {
    let started = std::time::Instant::now();
    let (boot, hub) = scripts()?;
    let booted = run(runtime, control, boot, &[])?;
    let members = runtime
        .scope(|s| -> Option<Vec<(String, Rooted)>> {
            let booted = s.local(&booted).ok()?;
            let names = s.form_nth(booted, 2).ok()?;
            let values = s.form_nth(booted, 3).ok()?;
            let n = s.form_count(names).ok()?;
            (0..n)
                .map(|i| {
                    let name = s.string(s.form_nth(names, i).ok()?)?.to_string_lossy();
                    Some((name, s.persist(s.form_nth(values, i).ok()?)))
                })
                .collect()
        })
        .ok_or_else(|| error("the real cljs.core's boot returned no member table"))?;
    let manifest = crate::runtime_manifest::RuntimeManifest::embedded();
    let (mut passing, mut failing, mut evaluator) = (Vec::new(), Vec::new(), 0);
    for (name, value) in &members {
        if evaluator_member(manifest, name) {
            evaluator += 1;
        } else if runtime.publish(value).is_ok() {
            passing.push(name.as_str());
        } else {
            failing.push(name.as_str());
        }
    }
    let names = |names: &[&str]| {
        runtime
            .scope(|s| -> Option<Rooted> {
                let items = names
                    .iter()
                    .map(|name| s.try_string_units(&name.encode_utf16().collect::<Vec<_>>()))
                    .collect::<Option<Vec<_>>>()?;
                Some(s.persist(s.new_form_vector(&items).ok()?))
            })
            .ok_or_else(|| error("heap byte or string limit exceeded"))
    };
    let (passing_names, failing_names) = (names(&passing)?, names(&failing)?);
    let counter = runtime
        .new_atom(&runtime.root(Primitive::Number(0.0)))
        .map_err(error)?;
    let handle = run(
        runtime,
        control,
        hub,
        &[&booted, &passing_names, &failing_names, &counter],
    )?;
    runtime.publish(&handle).map_err(|e| {
        EvalError(format!(
            "the real cljs.core's published hub (--macro-core real) does not publish: {e}"
        ))
    })?;
    Ok(RealCore {
        handle,
        members: members.len() - evaluator,
        published: passing.len(),
        boot_nanos: started.elapsed().as_nanos() as u64,
        unpublished: failing.iter().map(|name| name.to_string()).collect(),
    })
}
