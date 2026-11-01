//! Transfer a namespace's top-level result without advancing its deferred
//! drain point. Execution and publication may happen on different threads.
use super::*;
use crate::program_fragment::{Publication, SharedPrefix};
use std::sync::Arc;

pub(crate) struct NamespaceJob {
    top: NamespaceTop,
    shared: SharedPrefix,
    globals_before: crate::arena::Arena<GlobalBinding>,
}

pub(crate) struct NamespacePublication {
    program: Program,
    shared: SharedPrefix,
    root: ExprId,
    drained: DrainedNamespace,
    requires: Vec<String>,
    charges: Option<crate::compilation_session::UnitCharges>,
    changed_globals: Vec<usize>,
}

impl NamespaceAnalysis {
    /// Snapshot only the state analysis reads. Pending merges and compiler
    /// counters remain with the coordinator. `source_index` is canonical.
    pub(crate) fn namespace_job(
        &mut self,
        source_index: usize,
        slots: crate::scheduler::Slots,
    ) -> NamespaceJob {
        self.program.freeze();
        NamespaceJob {
            shared: SharedPrefix::same(&self.program),
            globals_before: self.program.globals.clone(),
            top: NamespaceTop {
                program: self.program.clone(),
                globals: self.globals.clone(),
                members: Default::default(),
                record_layouts: self.record_layouts.clone(),
                source_index,
                unit_slots: slots,
                main_isolate: false,
                deferred_time: None,
            },
        }
    }

    /// Append only top-level IR now. Deferred fragments retain their original
    /// snapshots and relocate when the ordinary `merge_front` drains them.
    pub(crate) fn publish_namespace(&mut self, mut published: NamespacePublication) {
        assert_eq!(published.drained.source_index, self.roots.len());
        let start = self.program.exprs.len();
        let first_lazy = self.program.lazy_targets.len();
        published.program.root = published.root;
        let (root, ids) = crate::program_fragment::append_published(
            &mut self.program,
            &published.program,
            &published.shared,
            &published.drained.namespace,
            &mut self.globals,
            &published.changed_globals,
        );
        published.drained.relocate(ids);
        published.drained.top = start..self.program.exprs.len();
        self.last_lazy = self.lazy_namespaces_since(first_lazy);
        self.roots.push(root);
        self.requires.push(published.requires);
        self.pending.push_back(Pending::Analyzed(published.drained));
    }
}

impl NamespaceJob {
    /// The host belongs to this thread and must share the build's unit catalog.
    /// Namespace workers run their own units; no nested unit pool is started.
    pub(crate) fn analyze(
        mut self,
        input: &NamespaceInput<'_>,
        resolution: NamespaceResolution,
        result: bool,
        host: &mut MacroHost,
        options: AnalysisOptions,
    ) -> Result<NamespacePublication, AnalysisError> {
        let mut warnings = Vec::new();
        let recording = crate::macro_host::MacroFreeUnit::enter(
            input.name.to_owned(),
            false,
            Default::default(),
            true,
        );
        let outcome = self
            .top
            .analyze(input, resolution, result, host, options, &mut warnings);
        let charges = crate::compilation_session::UnitCharges::namespace(
            recording.finish(),
            host,
            input.name,
        );
        let (root, mut drained) = outcome?;
        if let Some(session) = host.session_handle() {
            for unit in &drained.dispatched {
                if session.unit_failed(unit.unit) {
                    return Err(session
                        .diagnostic(unit.unit)
                        .unwrap_or_else(|| AnalysisError {
                            kind: AnalysisErrorKind::MacroExpansion(
                                "a deferred namespace unit failed".into(),
                            ),
                            span: self.top.program.exprs[unit.placeholder.0].span.clone(),
                        }));
                }
            }
        }
        assert!(
            drained.pooled.is_empty(),
            "namespace workers must not start a nested unit pool"
        );
        drained.warnings = HeldWarnings::Held(warnings);
        let changed_globals = self.top.program.globals.changed_entries(
            &self.globals_before,
            self.globals_before.len(),
        );
        Ok(NamespacePublication {
            program: self.top.program,
            shared: self.shared,
            root,
            drained,
            requires: input.requires.to_vec(),
            charges: Some(charges),
            changed_globals,
        })
    }
}

impl NamespacePublication {
    pub(crate) fn charge(&mut self, host: &MacroHost) -> Result<(), AnalysisError> {
        let charges = self.charges.take().expect("namespace charges publish once");
        host.session_handle()
            .unwrap()
            .charge_namespace(charges, host)
    }
    pub(crate) fn deferred_units(&self) -> impl Iterator<Item = usize> + '_ {
        self.drained.dispatched.iter().map(|unit| unit.unit)
    }
}

impl DrainedNamespace {
    fn relocate(&mut self, ids: Arc<Publication>) {
        for unit in &mut self.units {
            unit.placeholder = ids.expr(unit.placeholder);
            unit.shared = unit.shared.clone().published(ids.clone());
        }
        for unit in &mut self.dispatched {
            unit.placeholder = ids.expr(unit.placeholder);
            unit.shared = unit.shared.clone().published(ids.clone());
        }
        self.checked_array_exprs = self
            .checked_array_exprs
            .iter()
            .map(|id| ids.expr(*id))
            .collect();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(id: usize) -> String {
        format!(
            r#"(ns branch{id})
          (defprotocol P (value [this x]))
          (defrecord Box [n] P (value [_ x] (+ n x)))
          (defmacro plus [x] (list '+ x {id}))
          (defn f [x]
            (let [a (->Box (plus x))
                  b (reify P (value [_ y] (+ x y)))
                  looped (loop [n 3 total 0] (if (zero? n) total (recur (dec n) (+ total n))))]
              (+ (value a 2) (value b 3) looped
                 (count ["different-{id}" :branch{id}/key 'branch{id}/sym]))))"#
        )
    }

    fn analyze_one(
        analysis: &mut NamespaceAnalysis,
        host: &mut MacroHost,
        name: &str,
        source: &str,
        entry: bool,
        warnings: &mut Vec<AnalysisWarning>,
    ) {
        let syntax = crate::parse(source).unwrap();
        let requires = if entry {
            std::iter::once("base".to_owned())
                .chain((0..8).map(|id| format!("branch{id}")))
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        host.session_handle()
            .unwrap()
            .shared()
            .lanes()
            .register_namespace(analysis.roots.len(), name, &requires);
        analysis
            .analyze(
                &NamespaceInput {
                    name,
                    source,
                    syntax: &syntax,
                    aliases: &[],
                    requires: &requires,
                },
                Default::default(),
                entry,
                host,
                Default::default(),
                warnings,
            )
            .unwrap();
        analysis.hold_warnings(warnings);
        while analysis.pending_merges() > DRAIN_LAG {
            analysis.merge_front(host, warnings).unwrap();
        }
    }

    fn compile(parallel: bool) -> (Program, Vec<AnalysisWarning>) {
        let mut host = MacroHost::new();
        let mut analysis = NamespaceAnalysis::new(&mut host);
        host.session_handle().unwrap().shared().lanes().enable();
        if parallel {
            host.session_handle()
                .unwrap()
                .enable_namespace_publication()
                .unwrap();
        }
        analysis.unit_slots = crate::scheduler::Slots::new(8).unwrap();
        let mut warnings = Vec::new();
        // A nonempty common prefix exercises mappings of both shared and
        // shifted entries. Its body must retain its original drain point.
        analyze_one(
            &mut analysis,
            &mut host,
            "base",
            "(ns base) (defn base [x] (+ x 10))",
            false,
            &mut warnings,
        );
        // Lazy loading can reserve a later namespace's globals before its
        // source is analyzed. They belong to every writer's shared prefix.
        for namespace in ["branch0", "branch1"] {
            for name in ["f"] {
                let id = GlobalId(analysis.program.globals.len());
                analysis.globals.insert((namespace.to_owned(), name.to_owned()), id);
                analysis.program.globals.push(GlobalBinding {
                    type_descriptor: false, declared: false, constant: false,
                    define: false, boolean_tag: false, metadata: None,
                    namespace: namespace.to_owned(), name: name.to_owned(),
                    span: 0..0, source_index: 0,
                });
            }
        }
        if parallel {
            let session = host.session_handle().unwrap();
            let shared = session.shared().clone();
            let seed = session.namespace_catalog_seed(&host).unwrap();
            let prefix = session.namespace_prefix();
            let environment = host.environment();
            let runtime = shared
                .native_runtime(environment.evaluator_options())
                .unwrap();
            let jobs = (0..8)
                .map(|id| {
                    shared
                        .lanes()
                        .register_namespace(id + 1, &format!("branch{id}"), &[]);
                    (
                        analysis.namespace_job(id + 1, analysis.unit_slots),
                        shared.lanes().reserve_pass(Some(id + 1)),
                    )
                })
                .collect::<Vec<_>>();
            let results = std::thread::scope(|scope| {
                let handles = jobs
                    .into_iter()
                    .enumerate()
                    .map(|(id, (job, pass))| {
                        let (shared, prefix, environment, runtime) = (
                            shared.clone(),
                            prefix.clone(),
                            environment.clone(),
                            runtime.clone(),
                        );
                        let seed = seed.clone();
                        scope.spawn(move || {
                            let evaluator = std::rc::Rc::new(runtime.evaluator().unwrap());
                            let mut worker = MacroHost::bind(environment, Some(evaluator)).unwrap();
                            let session =
                                crate::compilation_session::CompilationSession::namespace_writer_seeded(
                                    shared, &prefix, seed,
                                );
                            worker.engine.session = Some(session.clone());
                            session
                                .attach(worker.engine.bootstrap.as_ref().unwrap(), &worker)
                                .unwrap();
                            session.reserve_namespace_pass(pass);
                            worker.tables.bootstrap_seed = false;
                            let name = format!("branch{id}");
                            let source = fixture(id);
                            let syntax = crate::parse(&source).unwrap();
                            let published = job
                                .analyze(
                                    &NamespaceInput {
                                        name: &name,
                                        source: &source,
                                        syntax: &syntax,
                                        aliases: &[],
                                        requires: &[],
                                    },
                                    Default::default(),
                                    false,
                                    &mut worker,
                                    Default::default(),
                                )
                                .unwrap();
                            (
                                published,
                                worker.namespace_metadata(&name),
                                session.namespace_fragment(),
                            )
                        })
                    })
                    .collect::<Vec<_>>();
                handles
                    .into_iter()
                    .map(|h| h.join().unwrap())
                    .collect::<Vec<_>>()
            });
            for (mut published, metadata, registrations) in results {
                session
                    .publish_namespace_registrations(registrations)
                    .unwrap();
                published.charge(&host).unwrap();
                session
                    .adopt_completed_namespace_units(published.deferred_units(), &host)
                    .unwrap();
                host.publish_namespace_metadata(&metadata).unwrap();
                analysis.publish_namespace(published);
                while analysis.pending_merges() > DRAIN_LAG {
                    analysis.merge_front(&host, &mut warnings).unwrap();
                }
            }
        } else {
            for id in 0..8 {
                analyze_one(
                    &mut analysis,
                    &mut host,
                    &format!("branch{id}"),
                    &fixture(id),
                    false,
                    &mut warnings,
                );
            }
        }
        for namespace in ["branch0", "branch1"] {
            let defined = analysis.program.globals.iter()
                .find(|global| global.namespace == namespace && global.name == "f")
                .unwrap();
            assert!(defined.declared, "publication lost the definition of {namespace}/f");
        }
        let calls = (0..8)
            .map(|id| format!("(branch{id}/f 10) (branch{id}/plus 100)"))
            .collect::<Vec<_>>()
            .join(" ");
        analyze_one(
            &mut analysis,
            &mut host,
            "entry",
            &format!("(ns entry) (+ (base/base 1) {calls})"),
            true,
            &mut warnings,
        );
        analysis.merge_all(&host, &mut warnings).unwrap();
        (analysis.finish(9, &host).unwrap(), warnings)
    }

    #[test]
    fn concurrent_top_publication_preserves_the_production_deferred_drain_order() {
        let (serial, serial_warnings) = compile(false);
        let (parallel, parallel_warnings) = compile(true);
        assert_eq!(parallel_warnings, serial_warnings);
        assert!(serial_warnings.is_empty());
        for level in [
            crate::application::OptimizationLevel::Prepare,
            crate::application::OptimizationLevel::Advanced,
        ] {
            let options = crate::application::ApplicationOptions::new(level);
            let serial =
                crate::application::prepare_application(&serial, options.clone()).emit_js_mapped();
            let parallel =
                crate::application::prepare_application(&parallel, options).emit_js_mapped();
            assert_eq!(parallel, serial);
            let result = std::process::Command::new("node")
                .args(["--eval", &format!("console.log({});", parallel.0)])
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            // Each branch contributes (34 + id) + (100 + id); base adds11.
            assert_eq!(String::from_utf8_lossy(&result.stdout).trim(), "1139");
        }
    }
}
