//! Experimental namespace execution on the compiler's actual loaded sources.
//! Publication still follows the coordinator's ordinary namespace/drain loop.
use super::*;
use crate::analyzer::namespace_publication::NamespacePublication;

/// Experimental admission only for fresh port-runtime bootstrap. No host
/// services, namespace work or effect-lane waits run while holding a permit.
struct BootGate {
    available: std::sync::Mutex<usize>,
    ready: std::sync::Condvar,
}

impl BootGate {
    fn new(slots: usize) -> Self {
        assert!(slots > 0);
        Self {
            available: std::sync::Mutex::new(slots),
            ready: std::sync::Condvar::new(),
        }
    }

    fn enter(&self) -> BootPermit<'_> {
        let mut available = self.available.lock().unwrap();
        while *available == 0 {
            available = self.ready.wait(available).unwrap();
        }
        *available -= 1;
        BootPermit(self)
    }
}

struct BootPermit<'a>(&'a BootGate);

impl Drop for BootPermit<'_> {
    fn drop(&mut self) {
        *self.0.available.lock().unwrap() += 1;
        self.0.ready.notify_one();
    }
}

pub(super) struct Completed {
    pub publication: NamespacePublication,
    pub metadata: crate::macro_host::NamespaceMetadata,
    pub registrations: Vec<crate::compilation_session::Registration>,
}

/// A contiguous ready batch cannot wait for an unstarted earlier namespace's
/// effect lane. Keep dependencies within the batch for a later batch instead.
pub(super) fn end(files: &[NamespaceSource], first: usize, slots: usize) -> usize {
    let mut names = std::collections::HashSet::new();
    let mut end = first;
    for file in files.iter().skip(first).take(slots) {
        if file.requires.iter().any(|name| names.contains(name)) {
            break;
        }
        names.insert(file.name.clone());
        end += 1;
    }
    end
}

pub(super) fn run(
    analysis: &mut NamespaceAnalysis,
    host: &MacroHost,
    files: &[NamespaceSource],
    first: usize,
    entry: usize,
    options: AnalysisOptions,
) -> std::collections::VecDeque<Result<Completed, NamespaceCompileError>> {
    let session = host.session_handle().expect("compilation session");
    let shared = session.shared().clone();
    let seed = match session.namespace_catalog_seed(host) {
        Ok(seed) => seed,
        Err(error) => return [Err(NamespaceCompileError::new(error))].into(),
    };
    let prefix = session.namespace_prefix();
    let environment = host.environment();
    let runtime = match shared.native_runtime(environment.evaluator_options()) {
        Ok(runtime) => runtime,
        Err(error) => return [Err(NamespaceCompileError::new(error.to_string()))].into(),
    };
    // Starting all contexts together contends on shared runtime allocation
    // counters. Keep analysis at the requested width while two contexts boot;
    // the override remains available for the separate scaling investigation.
    let boot_gate = (environment.evaluator_options().macro_core
        == crate::evaluator::MacroCore::Port)
        .then(|| {
            std::env::var("LIGHT_CLJS_NAMESPACE_BOOT_SLOTS")
                .ok()
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(2)
        })
        .filter(|&slots| slots > 0 && slots < files.len())
        .map(BootGate::new);
    let jobs = files
        .iter()
        .enumerate()
        .map(|(offset, file)| {
            let index = first + offset;
            shared
                .lanes()
                .register_namespace(index, &file.name, &file.requires);
            (
                analysis.namespace_job(index, analysis.unit_slots),
                shared.lanes().reserve_pass(Some(index)),
            )
        })
        .collect::<Vec<_>>();
    std::thread::scope(|scope| {
        let handles = files
            .iter()
            .zip(jobs)
            .enumerate()
            .map(|(offset, (file, (job, pass)))| {
                let (shared, prefix, environment, runtime) = (
                    shared.clone(),
                    prefix.clone(),
                    environment.clone(),
                    runtime.clone(),
                );
                let seed = seed.clone();
                let boot_gate = boot_gate.as_ref();
                scope.spawn(move || {
                    let setup = || -> Result<_, String> {
                        let evaluator = {
                            let _permit = boot_gate.map(BootGate::enter);
                            // In port mode this runs only the private runtime
                            // to its first command boundary, before any catalog
                            // or user dependency is installed on the evaluator.
                            std::rc::Rc::new(runtime.evaluator().map_err(|e| e.to_string())?)
                        };
                        evaluator.enable_parallel_imports();
                        let mut worker = MacroHost::bind(environment, Some(evaluator))?;
                        let session =
                            crate::compilation_session::CompilationSession::namespace_writer_seeded(
                                shared, &prefix, seed,
                            );
                        worker.engine.session = Some(session.clone());
                        session.attach(worker.engine.bootstrap.as_ref().unwrap(), &worker)?;
                        session.reserve_namespace_pass(pass);
                        worker.tables.bootstrap_seed = false;
                        Ok((worker, session))
                    };
                    let (mut worker, session) = setup().map_err(NamespaceCompileError::new)?;
                    let at = |error: AnalysisError| {
                        NamespaceCompileError::new(error.to_string()).at(
                            &file.path,
                            &file.source,
                            error.span,
                        )
                    };
                    let resolution = prepare_source(&mut worker, file).map_err(at)?;
                    let publication = job
                        .analyze(
                            &NamespaceInput {
                                name: &file.name,
                                source: &file.source,
                                syntax: &file.syntax,
                                aliases: &file.aliases,
                                requires: &file.requires,
                            },
                            resolution,
                            first + offset == entry,
                            &mut worker,
                            options,
                        )
                        .map_err(at)?;
                    Ok(Completed {
                        publication,
                        metadata: worker.namespace_metadata(&file.name),
                        registrations: session.namespace_fragment(),
                    })
                })
            })
            .collect::<Vec<_>>();
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::BootGate;
    use std::sync::{
        Barrier,
        atomic::{AtomicUsize, Ordering},
    };

    #[test]
    fn namespace_boot_gate_admits_exactly_its_capacity_and_releases_each_wave() {
        for slots in [1, 2, 4] {
            let gate = BootGate::new(slots);
            let barrier = Barrier::new(slots);
            let active = AtomicUsize::new(0);
            let peak = AtomicUsize::new(0);
            std::thread::scope(|scope| {
                for _ in 0..8 {
                    scope.spawn(|| {
                        let _permit = gate.enter();
                        let current = active.fetch_add(1, Ordering::SeqCst) + 1;
                        peak.fetch_max(current, Ordering::SeqCst);
                        assert!(current <= slots);
                        barrier.wait();
                        active.fetch_sub(1, Ordering::SeqCst);
                    });
                }
            });
            assert_eq!(peak.load(Ordering::SeqCst), slots);
            assert_eq!(*gate.available.lock().unwrap(), slots);
        }
    }

    #[test]
    fn namespace_boot_gate_returns_a_permit_after_unwinding() {
        let gate = BootGate::new(1);
        let panic = std::panic::catch_unwind(|| {
            let _permit = gate.enter();
            panic!("bootstrap failed");
        });
        assert!(panic.is_err());
        assert_eq!(*gate.available.lock().unwrap(), 1);
        drop(gate.enter());
        assert_eq!(*gate.available.lock().unwrap(), 1);
    }
}
