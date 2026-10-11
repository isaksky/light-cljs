//! Owned deferred analysis. No catalog borrow survives analysis or engine entry.
//! Frames retain immutable definition-site resolution/macro snapshots. JS roots
//! have their own state machine; analysis cycles are diagnosed separately.
use crate::{
    MacroHost, Program,
    analyzer::{AnalysisOptions, AnalysisWarning, NamespaceResolution},
    compiler_bootstrap::Bootstrap,
    compiler_data::Form,
    evaluator::MacroEvaluator,
};
use std::{cell::RefCell, collections::HashMap, rc::Rc};

pub(crate) struct DefinitionSnapshot {
    pub namespace: String,
    pub aliases: Vec<(String, String)>,
    pub requires: Vec<String>,
    pub resolution: NamespaceResolution,
    pub resolution_program: Program,
    pub options: AnalysisOptions,
    pub source_index: usize,
    pub definition_name: Option<String>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct VarId(usize);
#[derive(Clone, Copy, Debug)]
struct UnitRequest {
    var: VarId,
    generation: usize,
}
struct Unit {
    request: UnitRequest,
    name: String,
    form: Form,
    snapshot: DefinitionSnapshot,
    host: MacroHost,
    state: AnalysisState,
}
#[derive(Clone)]
enum AnalysisState {
    Unstarted,
    Analyzing,
    Analyzed(Rc<Program>, Vec<AnalysisWarning>),
    Failed(String, Option<crate::AnalysisError>),
}
#[derive(Default)]
struct Catalog {
    units: Vec<Rc<RefCell<Unit>>>,
    active: HashMap<String, (Option<usize>, String)>,
    bindings: HashMap<String, (VarId, usize)>,
    stack: Vec<usize>,
    bootstrap: Option<Bootstrap>,
}
#[derive(Default)]
pub(crate) struct CompilationSession {
    catalog: RefCell<Catalog>,
}
impl CompilationSession {
    pub fn new() -> Rc<Self> {
        Rc::new(Self::default())
    }
    pub fn attach(self: &Rc<Self>, bootstrap: &Bootstrap) -> Result<(), String> {
        if self.catalog.borrow().bootstrap.is_some() {
            return Ok(());
        }
        let evaluator = &bootstrap.evaluator;
        let weak = Rc::downgrade(self);
        let engine = Rc::downgrade(evaluator);
        evaluator
            .install_unit_service("__lightCompileUnit", move |ctx, id| {
                let session = weak.upgrade().ok_or("compilation session expired")?;
                let evaluator = engine.upgrade().ok_or("evaluator expired")?;
                let active = evaluator.active(ctx).map_err(|e| e.to_string())?;
                let (program, _) = session.analyze(id as usize, Some(&active))?;
                Ok(crate::emitter::emit_deferred_unit(&program))
            })
            .map_err(|e| e.to_string())?;
        // Registry and root states stay in JS, so ready reads never cross Rust.
        let value = evaluator
            .evaluate_js(include_str!("compiler_roots.js"))
            .map_err(|e| e.to_string())?;
        evaluator.release(value).map_err(|e| e.to_string())?;
        // Capture the canonical runtime without a second runtime or native adapter.
        let value = evaluator
            .evaluate_unit("rt => { globalThis.__lightRuntimeForCompiler = rt; return null; }")
            .map_err(|e| e.to_string())?;
        evaluator.release(value).map_err(|e| e.to_string())?;
        let active = {
            let mut catalog = self.catalog.borrow_mut();
            catalog.bootstrap = Some(bootstrap.clone());
            catalog.active.clone()
        };
        for (name, (unit, location)) in active {
            Self::install(evaluator.as_ref(), &name, unit, false, &location)?;
        }
        Ok(())
    }
    fn install(
        engine: &dyn MacroEvaluator,
        name: &str,
        unit: Option<usize>,
        once: bool,
        location: &str,
    ) -> Result<(), String> {
        // Qualified CLJS names cannot contain these escaped JS string delimiters.
        let name = name
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r");
        let location = location
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r");
        let value = engine
            .evaluate_unit(&format!(
                "rt => globalThis.__lightRegisterUnit(\"{name}\",{}, {once}, \"{location}\")",
                unit.map_or("null".into(), |n| n.to_string())
            ))
            .map_err(|e| e.to_string())?;
        engine.release(value).map_err(|e| e.to_string())
    }
    pub fn register(
        &self,
        name: String,
        form: Option<Form>,
        snapshot: DefinitionSnapshot,
        host: MacroHost,
        once: bool,
        location: String,
    ) -> Result<Option<usize>, String> {
        let (id, bootstrap) = {
            let mut c = self.catalog.borrow_mut();
            let next_var = VarId(c.bindings.len());
            let binding = c.bindings.entry(name.clone()).or_insert((next_var, 0));
            if form.is_some() {
                binding.1 += 1;
            }
            let request = UnitRequest {
                var: binding.0,
                generation: binding.1,
            };
            let id = form.map(|form| {
                let id = c.units.len();
                c.units.push(Rc::new(RefCell::new(Unit {
                    request,
                    name: name.clone(),
                    form,
                    snapshot,
                    host,
                    state: AnalysisState::Unstarted,
                })));
                id
            });
            if !(id.is_none() && c.active.contains_key(&name)
                || once && c.active.get(&name).is_some_and(|(unit, _)| unit.is_some()))
            {
                c.active.insert(name.clone(), (id, location.clone()));
            }
            (id, c.bootstrap.clone())
        };
        if let Some(bootstrap) = bootstrap {
            Self::install(bootstrap.evaluator.as_ref(), &name, id, once, &location)?;
        }
        Ok(id)
    }
    pub fn diagnostic(&self, id: usize) -> Option<crate::AnalysisError> {
        let unit = self.catalog.borrow().units.get(id)?.clone();
        match &unit.borrow().state {
            AnalysisState::Failed(_, diagnostic) => diagnostic.clone(),
            _ => None,
        }
    }
    pub fn analyze(
        &self,
        id: usize,
        engine: Option<&dyn MacroEvaluator>,
    ) -> Result<(Rc<Program>, Vec<AnalysisWarning>), String> {
        let (unit, bootstrap) = {
            let c = self.catalog.borrow();
            (
                c.units.get(id).cloned().ok_or("unknown compilation unit")?,
                c.bootstrap.clone(),
            )
        };
        let (form, snapshot, mut host) = {
            let mut unit = unit.borrow_mut();
            match &unit.state {
                AnalysisState::Analyzed(program, warnings) => {
                    return Ok((program.clone(), warnings.clone()));
                }
                AnalysisState::Failed(error, _) => return Err(error.clone()),
                AnalysisState::Analyzing => {
                    let c = self.catalog.borrow();
                    let mut path = c
                        .stack
                        .iter()
                        .map(|i| {
                            if *i == id {
                                unit.name.clone()
                            } else {
                                c.units[*i].borrow().name.clone()
                            }
                        })
                        .collect::<Vec<_>>();
                    path.push(unit.name.clone());
                    return Err(format!("compile dependency cycle: {}", path.join(" -> ")));
                }
                AnalysisState::Unstarted => (),
            }
            if self.catalog.borrow().stack.len() >= unit.host.limits.expansion_depth.min(64) {
                let message =
                    "compiler macro expansion depth limit exceeded in deferred units".to_owned();
                unit.state = AnalysisState::Failed(message.clone(), None);
                return Err(message);
            }
            unit.state = AnalysisState::Analyzing;
            // The request has stable identity even if a newer same-kind source
            // generation is activated while this frame is suspended.
            let request = unit.request;
            debug_assert!(request.generation > 0);
            debug_assert_eq!(self.catalog.borrow().bindings[&unit.name].0, request.var);
            let mut host = unit.host.macro_analysis_frame();
            if host.bootstrap.is_none() {
                host.bootstrap = bootstrap;
            }
            (unit.form.clone(), unit.snapshot.clone(), host)
        };
        self.catalog.borrow_mut().stack.push(id);
        let guard = AnalysisGuard {
            session: self,
            unit: unit.clone(),
            id,
        };
        let analysis = crate::analyzer::analyze_deferred_form(&form, &mut host, &snapshot, engine);
        let diagnostic = analysis.as_ref().err().cloned();
        let result = analysis.map(|(p, w)| (Rc::new(p), w)).map_err(|e| {
            let loc = crate::compiler_data::DiagnosticLocation::from_span(&form.span());
            format!(
                "{}:{}:{}: {}: {e}",
                loc.source,
                loc.line,
                loc.column,
                unit.borrow().name
            )
        });
        unit.borrow_mut().state = match &result {
            Ok((p, w)) => AnalysisState::Analyzed(p.clone(), w.clone()),
            Err(e) => AnalysisState::Failed(e.clone(), diagnostic),
        };
        drop(guard);
        result
    }
}
impl Clone for DefinitionSnapshot {
    fn clone(&self) -> Self {
        Self {
            namespace: self.namespace.clone(),
            aliases: self.aliases.clone(),
            requires: self.requires.clone(),
            resolution: self.resolution.clone(),
            resolution_program: self.resolution_program.clone(),
            options: self.options,
            source_index: self.source_index,
            definition_name: self.definition_name.clone(),
        }
    }
}
struct AnalysisGuard<'a> {
    session: &'a CompilationSession,
    unit: Rc<RefCell<Unit>>,
    id: usize,
}
impl Drop for AnalysisGuard<'_> {
    fn drop(&mut self) {
        let mut c = self.session.catalog.borrow_mut();
        debug_assert_eq!(c.stack.pop(), Some(self.id));
        let mut unit = self.unit.borrow_mut();
        if matches!(unit.state, AnalysisState::Analyzing) {
            unit.state = AnalysisState::Failed("analysis interrupted".into(), None);
        }
    }
}
