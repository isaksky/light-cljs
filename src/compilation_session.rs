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
    pub unchecked_arrays: bool,
    pub source_index: usize,
    pub definition_name: Option<String>,
    pub definition_test: Option<Form>,
    pub definition_test_unit: Option<usize>,
    pub extension_frontier: usize,
}
pub(crate) struct BundleRegistration<'a> {
    pub products: &'a [crate::analyzer::GlobalId],
    pub extensions: &'a [(String, Option<String>, String)],
    pub once: bool,
    pub form: Form,
    pub snapshot: DefinitionSnapshot,
    pub host: MacroHost,
    pub location: String,
    pub analyzed: Option<Program>,
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
    analyses: usize,
    emission: Option<String>,
}
#[derive(Clone)]
enum AnalysisState {
    Unstarted,
    Analyzing,
    Analyzed(Rc<Program>, Vec<AnalysisWarning>),
    Failed(String, Option<crate::AnalysisError>),
}
#[derive(Clone)]
enum Registration {
    Root(String, Option<usize>, Option<usize>, bool, String),
    Bundle(String),
}
#[derive(Default)]
struct Catalog {
    units: Vec<Rc<RefCell<Unit>>>,
    active: HashMap<String, bool>,
    // Only active defmulti products have stable named demand keys.
    multifn_targets: HashMap<String, String>,
    bindings: HashMap<String, (VarId, usize)>,
    stack: Vec<usize>,
    bootstrap: Option<Bootstrap>,
    registrations: Vec<Registration>,
    bundle_count: usize,
}
#[derive(Default)]
pub(crate) struct CompilationSession {
    catalog: RefCell<Catalog>,
}
impl CompilationSession {
    pub fn bootstrap(&self) -> Option<Bootstrap> {
        self.catalog.borrow().bootstrap.clone()
    }
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
                let unit = session.catalog.borrow().units[id as usize].clone();
                let mut unit = unit.borrow_mut();
                Ok(unit.emission
                    .get_or_insert_with(|| crate::emitter::emit_deferred_unit(&program))
                    .clone())
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
        let registrations = {
            let mut catalog = self.catalog.borrow_mut();
            catalog.bootstrap = Some(bootstrap.clone());
            catalog.registrations.clone()
        };
        for registration in registrations {
            match registration {
                Registration::Root(name, unit, test, once, location) => {
                    Self::install(evaluator.as_ref(), &name, unit, test, once, &location)?
                }
                Registration::Bundle(source) => {
                    let value = evaluator
                        .evaluate_unit(&source)
                        .map_err(|e| e.to_string())?;
                    evaluator.release(value).map_err(|e| e.to_string())?;
                }
            }
        }
        Ok(())
    }
    pub fn compilation_uses(&self) -> Vec<crate::analyzer::CompilationUse> {
        self.catalog.borrow().units.iter().map(|unit| {
            let unit = unit.borrow();
            crate::analyzer::CompilationUse {
                name: unit.name.clone(),
                generation: unit.request.generation,
                analyses: unit.analyses,
                emissions: usize::from(unit.emission.is_some()),
            }
        }).collect()
    }
    pub fn extension_frontier(&self) -> usize {
        self.catalog.borrow().bundle_count
    }
    pub fn multifn_target(&self, name: &str) -> Option<String> {
        self.catalog.borrow().multifn_targets.get(name).cloned()
    }
    pub fn register_bundle(
        &self,
        program: &Program,
        registration: BundleRegistration<'_>,
    ) -> Result<usize, String> {
        let BundleRegistration {
            products, extensions, once, form, snapshot, host, location, analyzed,
        } = registration;
        fn quoted(s: &str) -> String {
            let mut out = String::from("\"");
            for c in s.encode_utf16() {
                out.push_str(&format!("\\u{c:04x}"));
            }
            out.push('"');
            out
        }
        let names = products
            .iter()
            .map(|g| {
                let b = &program.globals[g.0];
                quoted(&format!("{}/{}", b.namespace, b.name))
            })
            .collect::<Vec<_>>()
            .join(",");
        let indices = extensions
            .iter()
            .map(|(p, t, identity)| {
                format!(
                    "[{},{},{}]",
                    quoted(p),
                    t.as_deref().map_or("null".into(), quoted),
                    quoted(identity)
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let name = format!("declaration#{}", self.catalog.borrow().units.len());
        let unit = self
            .register(name, Some(form), snapshot, host, false, location.clone())?
            .unwrap();
        let group = analyzed.is_some();
        if let Some(program) = analyzed {
            let unit = self.catalog.borrow().units[unit].clone();
            let mut unit = unit.borrow_mut();
            unit.state = AnalysisState::Analyzed(Rc::new(program), Vec::new());
            unit.analyses = 1;
        }
        let code = format!(
            "(() => {{ let code; return rt => {{ code ??= (0,eval)(globalThis.__lightCompileUnit({unit})); return code(rt); }}; }})()"
        );
        let source = format!(
            "rt => globalThis.__lightRegisterBundle([{names}], {code}, [{indices}], {}, {once}, {group})",
            quoted(&location)
        );
        let bootstrap = {
            let mut c = self.catalog.borrow_mut();
            c.registrations.push(Registration::Bundle(source.clone()));
            c.bundle_count += 1;
            for g in products {
                let b = &program.globals[g.0];
                let name = format!("{}/{}", b.namespace, b.name);
                if once && c.active.get(&name) == Some(&true) {
                    continue;
                }
                if once {
                    c.multifn_targets.insert(name.clone(), name.clone());
                } else {
                    c.multifn_targets.remove(&name);
                }
                c.active.insert(name, true);
            }
            c.bootstrap.clone()
        };
        if let Some(bootstrap) = bootstrap {
            let engine = bootstrap.evaluator.as_ref();
            let value = engine.evaluate_unit(&source).map_err(|e| e.to_string())?;
            engine.release(value).map_err(|e| e.to_string())?;
        }
        Ok(unit)
    }
    fn install(
        engine: &dyn MacroEvaluator,
        name: &str,
        unit: Option<usize>,
        test: Option<usize>,
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
                "rt => globalThis.__lightRegisterUnit(\"{name}\",{}, {once}, \"{location}\", {})",
                unit.map_or("null".into(), |n| n.to_string()),
                test.map_or("null".into(), |n| n.to_string())
            ))
            .map_err(|e| e.to_string())?;
        engine.release(value).map_err(|e| e.to_string())
    }
    pub fn register(
        &self,
        name: String,
        form: Option<Form>,
        mut snapshot: DefinitionSnapshot,
        host: MacroHost,
        once: bool,
        location: String,
    ) -> Result<Option<usize>, String> {
        // Executable metadata has its own source-site unit and root state.
        // Reflecting it cannot analyze or execute the ordinary initializer.
        if let Some(test) = snapshot.definition_test.take().filter(|_| form.is_some()) {
            let test_name = format!("{name} :test#{}", self.catalog.borrow().units.len());
            snapshot.definition_test_unit = self.register(
                test_name,
                Some(test), snapshot.clone(), host.macro_analysis_frame(), false, location.clone(),
            )?;
        }
        let test = snapshot.definition_test_unit;
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
                    analyses: 0,
                    emission: None,
                })));
                id
            });
            if !(id.is_none() && c.active.contains_key(&name)
                || once && c.active.get(&name).is_some_and(|defined| *defined))
            {
                c.multifn_targets.remove(&name);
                c.active.insert(name.clone(), id.is_some());
            }
            c.registrations
                .push(Registration::Root(name.clone(), id, test, once, location.clone()));
            (id, c.bootstrap.clone())
        };
        if let Some(bootstrap) = bootstrap {
            Self::install(bootstrap.evaluator.as_ref(), &name, id, test, once, &location)?;
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
            unit.analyses += 1;
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
        let evaluator = host.bootstrap.as_ref().map(|b| b.evaluator.clone());
        let engine = engine.or_else(|| {
            evaluator
                .as_ref()
                .map(|e| e.as_ref() as &dyn MacroEvaluator)
        });
        let _frontier = if let Some(engine) = engine {
            let value = engine
                .evaluate_unit(&format!(
                    "rt => globalThis.__lightEnterCompilerFrontier({})",
                    snapshot.extension_frontier
                ))
                .map_err(|e| e.to_string())?;
            engine.release(value).map_err(|e| e.to_string())?;
            Some(FrontierGuard { engine })
        } else {
            None
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
            unchecked_arrays: self.unchecked_arrays,
            source_index: self.source_index,
            definition_name: self.definition_name.clone(),
            definition_test: self.definition_test.clone(),
            definition_test_unit: self.definition_test_unit,
            extension_frontier: self.extension_frontier,
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

pub(crate) struct FrontierGuard<'a> {
    engine: &'a dyn MacroEvaluator,
}
impl<'a> FrontierGuard<'a> {
    pub(crate) fn bootstrap(engine: &'a dyn MacroEvaluator) -> Result<Self, String> {
        // Used only after attaching a compilation with user declarations.
        let value = engine
            .evaluate_unit("rt => globalThis.__lightEnterCompilerFrontier(0)")
            .map_err(|e| e.to_string())?;
        engine.release(value).map_err(|e| e.to_string())?;
        Ok(Self { engine })
    }
}
impl Drop for FrontierGuard<'_> {
    fn drop(&mut self) {
        // Terminal cancellation makes this heap unusable; no later demand runs.
        if let Ok(value) = self
            .engine
            .evaluate_unit("rt => globalThis.__lightLeaveCompilerFrontier()")
        {
            let _ = self.engine.release(value);
        }
    }
}
