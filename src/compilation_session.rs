//! Owned deferred analysis. No catalog borrow survives analysis or engine entry.
//! Frames retain immutable definition-site resolution/macro snapshots. JS roots
//! have their own state machine; analysis cycles are diagnosed separately.
use crate::{
    MacroHost, Program,
    analyzer::{AnalysisOptions, AnalysisWarning, NamespaceResolution},
    compiler_bootstrap::Bootstrap,
    compiler_data::{Datum, DatumKind, Form, Name},
    evaluator::MacroEvaluator,
};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
};

pub(crate) struct DefinitionSnapshot {
    pub namespace: String,
    pub aliases: Vec<(String, String)>,
    pub requires: Vec<String>,
    pub resolution: NamespaceResolution,
    pub resolution_program: Program,
    pub global_names: crate::name_index::NameIndex,
    pub namespace_globals: (usize, crate::arena::Arena<crate::analyzer::GlobalId>),
    /// In an interactive session, its namespaces at definition time.
    pub session_namespaces: crate::analyzer::SessionNamespaces,
    pub options: AnalysisOptions,
    pub unchecked_arrays: bool,
    pub source_index: usize,
    pub definition_name: Option<String>,
    pub definition_test: Option<Form>,
    pub definition_test_unit: Option<usize>,
    pub extension_frontier: usize,
    /// The scope of generated names the unit is analyzed in, numbered in
    /// the scope it was registered from ([`crate::generated_names::child`]).
    pub names: Option<std::sync::Arc<str>>,
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
    /// The source index of the namespace that defined it, on which
    /// `light.build` records what the unit reads.
    source: u32,
    // Consumed by the one analysis. The analyzed result keeps any resolution
    // data it needs; these inputs must not pin an obsolete second snapshot.
    // Keep large inputs out of line so completed units retain no empty space
    // for the definition snapshot or the macro host's many shared tables.
    form: Option<Form>,
    snapshot: Option<Box<DefinitionSnapshot>>,
    host: Option<Box<MacroHost>>,
    state: AnalysisState,
    analyses: usize,
    // Count actual engine demand separately from pure source preparation.
    // Ready or failed JavaScript roots never initialize a second time.
    emitted: bool,
}
#[derive(Clone)]
enum AnalysisState {
    Unstarted,
    Analyzing,
    Analyzed(Rc<Program>, Vec<AnalysisWarning>),
    Prepared(
        std::rc::Weak<Program>,
        Vec<AnalysisWarning>,
        Rc<crate::emitter::DeferredUnit>,
    ),
    Failed(String, Option<crate::AnalysisError>),
}
#[derive(Clone)]
enum Registration {
    Root(String, Option<usize>, Option<usize>, bool, String),
    Bundle(Rc<BundleRecord>),
}
/// A declaration bundle's registration, kept as data: its JavaScript names
/// the bundle's unit ID, which an artifact records as a relocatable reference.
pub(crate) struct BundleRecord {
    /// The products, by `ns/name`.
    pub products: Vec<String>,
    pub unit: usize,
    pub extensions: Vec<(String, Option<String>, String)>,
    pub location: String,
    pub once: bool,
    pub group: bool,
}
impl BundleRecord {
    /// The unit that registers the bundle in the compile-time session.
    pub fn source(&self) -> String {
        fn quoted(s: &str) -> String {
            let mut out = String::from("\"");
            for c in s.encode_utf16() {
                out.push_str(&format!("\\u{c:04x}"));
            }
            out.push('"');
            out
        }
        let names = self
            .products
            .iter()
            .map(|name| quoted(name))
            .collect::<Vec<_>>()
            .join(",");
        let indices = self
            .extensions
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
        let unit = self.unit;
        let code = format!(
            "(() => {{ let code; return rt => {{ code ??= (0,eval)(globalThis.__lightCompileUnit({unit})); return code(rt); }}; }})()"
        );
        format!(
            "rt => globalThis.__lightRegisterBundle([{names}], {code}, [{indices}], {}, {}, {}, {unit})",
            quoted(&self.location),
            self.once,
            self.group
        )
    }
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
    /// While namespace artifacts are captured ([`crate::artifact_capture`]),
    /// what the catalog does not otherwise keep about its units and macros.
    capture: RefCell<Option<SessionCapture>>,
    /// `gensym` names reserved for installed namespaces before the session
    /// started ([`crate::generated_names`]), which it reserves when it starts.
    gensyms_pending: RefCell<Vec<Vec<u16>>>,
    /// The `gensym` counter reserved before the session started.
    gensym_floor: Cell<u64>,
    /// Whether capture reads what `gensym` returns.
    gensyms_logged: Cell<bool>,
}
/// What capture needs beyond the catalog, recorded only while it is on.
#[derive(Default)]
struct SessionCapture {
    /// By unit ID from `first_unit` on.
    units: Vec<UnitCapture>,
    first_unit: usize,
    macros: Vec<CapturedMacro>,
    expansions: Vec<CapturedExpansion>,
}
/// A source macro call analysis expanded, for an artifact's provenance.
pub(crate) struct CapturedExpansion {
    /// The macro, `ns/name`, and its generation when it ran.
    pub key: String,
    pub generation: usize,
    pub definition_source: String,
    pub definition: std::ops::Range<usize>,
    pub call: std::ops::Range<usize>,
    pub source_index: usize,
}
#[derive(Default)]
struct UnitCapture {
    /// The globals of the unit's definition snapshot.
    view_globals: usize,
    extension_frontier: usize,
    /// The program's globals the unit's own analysis added.
    added: Vec<usize>,
}
/// A `defmacro`'s compile-time function, as the macro host evaluated it.
pub(crate) struct CapturedMacro {
    pub key: String,
    pub code: String,
    pub view_globals: usize,
    /// Its namespace's reader context as its registration names it.
    pub reader_context: Option<crate::analyzer::RuntimeReaderNamespace>,
}
/// A unit as capture reads it from the catalog.
pub(crate) struct UnitRecord {
    pub name: String,
    pub generation: usize,
    pub analyses: usize,
    pub emitted: bool,
    /// The prepared compile-time JavaScript, or why there is none.
    pub code: Result<String, &'static str>,
    pub view_globals: usize,
    pub extension_frontier: usize,
    pub added: Vec<usize>,
    /// The reader contexts its prepared registration names.
    pub reader_contexts: Option<
        crate::shared::Shared<
            crate::layered::Layered<
                String,
                crate::shared::Shared<crate::analyzer::RuntimeReaderNamespace>,
            >,
        >,
    >,
}
/// A registration of an installed namespace, with this compilation's unit IDs
/// ([`CompilationSession::install`]).
pub(crate) enum InstalledRegistration {
    Root {
        name: String,
        unit: Option<usize>,
        test: Option<usize>,
        once: bool,
        location: String,
    },
    Bundle(Rc<BundleRecord>),
}
/// A registration as capture reads it from the catalog.
pub(crate) enum RegistrationRecord {
    Root {
        name: String,
        unit: Option<usize>,
        test: Option<usize>,
        once: bool,
        location: String,
    },
    Bundle(Rc<BundleRecord>),
}
impl CompilationSession {
    /// Record what namespace artifacts need from now on.
    pub fn begin_capture(&self) {
        let first_unit = self.catalog.borrow().units.len();
        *self.capture.borrow_mut() = Some(SessionCapture {
            first_unit,
            ..Default::default()
        });
    }
    pub fn capturing(&self) -> bool {
        self.capture.borrow().is_some()
    }
    /// How many units, registrations and bundles the catalog holds.
    pub fn capture_marks(&self) -> (usize, usize, usize) {
        let c = self.catalog.borrow();
        (c.units.len(), c.registrations.len(), c.bundle_count)
    }
    pub fn capture_macro(
        &self,
        key: String,
        code: String,
        view_globals: usize,
        reader_context: Option<crate::analyzer::RuntimeReaderNamespace>,
    ) {
        if let Some(capture) = self.capture.borrow_mut().as_mut() {
            capture.macros.push(CapturedMacro {
                key,
                code,
                view_globals,
                reader_context,
            });
        }
    }
    pub fn capture_expansion(&self, expansion: CapturedExpansion) {
        if let Some(capture) = self.capture.borrow_mut().as_mut() {
            capture.expansions.push(expansion);
        }
    }
    /// The macros declared and the source macro calls expanded since the
    /// last call.
    pub fn take_captured(&self) -> (Vec<CapturedMacro>, Vec<CapturedExpansion>) {
        self.capture
            .borrow_mut()
            .as_mut()
            .map(|capture| {
                (
                    std::mem::take(&mut capture.macros),
                    std::mem::take(&mut capture.expansions),
                )
            })
            .unwrap_or_default()
    }
    /// Note the program globals the analysis of `unit` appended.
    pub fn capture_added_globals(&self, unit: usize, added: Vec<usize>) {
        if let Some(capture) = self.capture.borrow_mut().as_mut()
            && let Some(entry) = unit
                .checked_sub(capture.first_unit)
                .and_then(|index| capture.units.get_mut(index))
        {
            entry.added = added;
        }
    }
    /// The units from ID `from` on.
    pub fn captured_units(&self, from: usize) -> Result<Vec<UnitRecord>, String> {
        let catalog = self.catalog.borrow();
        let mut capture = self.capture.borrow_mut();
        let capture = capture.as_mut().ok_or("capture is off")?;
        let mut records = Vec::new();
        for (id, unit) in catalog.units.iter().enumerate().skip(from) {
            let entry = id
                .checked_sub(capture.first_unit)
                .and_then(|index| capture.units.get_mut(index))
                .ok_or("a unit registered before capture began")?;
            let unit = unit.borrow();
            let reader_contexts = match &unit.state {
                AnalysisState::Prepared(_, _, prepared) => Some(prepared.reader_contexts().clone()),
                _ => None,
            };
            let code = match &unit.state {
                AnalysisState::Prepared(_, _, prepared) => Ok(prepared.code().to_owned()),
                AnalysisState::Unstarted => Err("a deferred unit was never analyzed"),
                AnalysisState::Analyzing => Err("a deferred unit is still being analyzed"),
                AnalysisState::Analyzed(..) => Err("a deferred unit was analyzed but not prepared"),
                AnalysisState::Failed(..) => Err("a deferred unit failed"),
            };
            records.push(UnitRecord {
                name: unit.name.clone(),
                generation: unit.request.generation,
                analyses: unit.analyses,
                emitted: unit.emitted,
                code,
                view_globals: entry.view_globals,
                extension_frontier: entry.extension_frontier,
                added: std::mem::take(&mut entry.added),
                reader_contexts,
            });
        }
        Ok(records)
    }
    /// The registrations from position `from` on.
    pub fn captured_registrations(&self, from: usize) -> Vec<RegistrationRecord> {
        self.catalog
            .borrow()
            .registrations
            .iter()
            .skip(from)
            .map(|registration| match registration {
                Registration::Root(name, unit, test, once, location) => RegistrationRecord::Root {
                    name: name.clone(),
                    unit: *unit,
                    test: *test,
                    once: *once,
                    location: location.clone(),
                },
                Registration::Bundle(bundle) => RegistrationRecord::Bundle(bundle.clone()),
            })
            .collect()
    }
    /// The ID the next registered unit gets.
    pub fn next_unit(&self) -> usize {
        self.catalog.borrow().units.len()
    }
    /// Register again, in order, the units and registrations of a namespace
    /// installed from its artifact (`crate::artifact_install`), as
    /// [`Self::register`] and [`Self::register_bundle`] would have: each
    /// root with a unit adds that unit, prepared, at the next ID, which
    /// `registrations` already name. Nothing is analyzed or executed; a
    /// later demand evaluates a unit's prepared JavaScript as it would a
    /// unit this compilation prepared, and the counts are this
    /// compilation's: no analysis and no execution yet.
    pub fn install(
        &self,
        units: Vec<crate::emitter::DeferredUnit>,
        registrations: Vec<InstalledRegistration>,
        source: u32,
    ) -> Result<(), String> {
        let mut units = units.into_iter();
        for registration in registrations {
            let bootstrap = {
                let mut c = self.catalog.borrow_mut();
                let c = &mut *c;
                match &registration {
                    InstalledRegistration::Root {
                        name,
                        unit,
                        test,
                        once,
                        location,
                    } => {
                        let next_var = VarId(c.bindings.len());
                        let binding = c.bindings.entry(name.clone()).or_insert((next_var, 0));
                        if let Some(id) = *unit {
                            if id != c.units.len() {
                                return Err(format!(
                                    "installed unit {id} is not the next unit {}",
                                    c.units.len()
                                ));
                            }
                            binding.1 += 1;
                            let request = UnitRequest {
                                var: binding.0,
                                generation: binding.1,
                            };
                            let prepared = units
                                .next()
                                .ok_or("an installed root names a unit the artifact lacks")?;
                            if let Some(capture) = self.capture.borrow_mut().as_mut() {
                                capture.units.push(UnitCapture::default());
                            }
                            c.units.push(Rc::new(RefCell::new(Unit {
                                request,
                                name: name.clone(),
                                source,
                                form: None,
                                snapshot: None,
                                host: None,
                                state: AnalysisState::Prepared(
                                    std::rc::Weak::new(),
                                    Vec::new(),
                                    Rc::new(prepared),
                                ),
                                analyses: 0,
                                emitted: false,
                            })));
                        }
                        if !(unit.is_none() && c.active.contains_key(name)
                            || *once && c.active.get(name).is_some_and(|defined| *defined))
                        {
                            c.multifn_targets.remove(name);
                            c.active.insert(name.clone(), unit.is_some());
                        }
                        c.registrations.push(Registration::Root(
                            name.clone(),
                            *unit,
                            *test,
                            *once,
                            location.clone(),
                        ));
                    }
                    InstalledRegistration::Bundle(bundle) => {
                        c.registrations.push(Registration::Bundle(bundle.clone()));
                        c.bundle_count += 1;
                        for name in &bundle.products {
                            if bundle.once && c.active.get(name) == Some(&true) {
                                continue;
                            }
                            if bundle.once {
                                c.multifn_targets.insert(name.clone(), name.clone());
                            } else {
                                c.multifn_targets.remove(name);
                            }
                            c.active.insert(name.clone(), true);
                        }
                    }
                }
                c.bootstrap.clone()
            };
            if let Some(bootstrap) = bootstrap {
                let engine = bootstrap.evaluator.as_ref();
                match registration {
                    InstalledRegistration::Root {
                        name,
                        unit,
                        test,
                        once,
                        location,
                    } => Self::install_root(engine, &name, unit, test, once, &location)?,
                    InstalledRegistration::Bundle(bundle) => {
                        let value = engine
                            .evaluate_unit(&bundle.source())
                            .map_err(|e| e.to_string())?;
                        engine.release(value).map_err(|e| e.to_string())?;
                    }
                }
            }
        }
        if units.next().is_some() {
            return Err("an installed unit has no root".into());
        }
        Ok(())
    }
    pub fn bootstrap(&self) -> Option<Bootstrap> {
        self.catalog.borrow().bootstrap.clone()
    }
    /// Keep the compile-time runtime's `gensym` from returning any of
    /// `names` in this compilation: now if the session has started, else
    /// once it starts ([`crate::generated_names`]).
    pub fn reserve_gensyms(&self, names: &[Vec<u16>], counter: u64) -> Result<(), String> {
        match self.bootstrap() {
            Some(bootstrap) => {
                let engine = bootstrap.evaluator.as_ref();
                Self::raise_gensyms(engine, counter)?;
                Self::import_gensyms(engine, names)
            }
            None => {
                self.gensym_floor.set(self.gensym_floor.get().max(counter));
                self.gensyms_pending
                    .borrow_mut()
                    .extend(names.iter().cloned());
                Ok(())
            }
        }
    }
    /// Keep `gensym` from appending a number at or below `counter`.
    fn raise_gensyms(engine: &dyn MacroEvaluator, counter: u64) -> Result<(), String> {
        if counter == 0 {
            return Ok(());
        }
        let value = engine
            .evaluate_unit(&format!(
                "rt => {{ rt.compilerForms.raiseGensyms({counter}); return null; }}"
            ))
            .map_err(|e| e.to_string())?;
        engine.release(value).map_err(|e| e.to_string())
    }
    /// Start logging what the compile-time runtime's `gensym` returns, for
    /// [`Self::take_gensyms`].
    pub fn log_gensyms(&self) -> Result<(), String> {
        self.gensyms_logged.set(true);
        match self.bootstrap() {
            Some(bootstrap) => Self::take_gensyms_from(bootstrap.evaluator.as_ref()).map(drop),
            None => Ok(()),
        }
    }
    /// Where `gensym`'s counter stands, and what it returned since the last
    /// call or since [`Self::log_gensyms`]; before the session starts, the
    /// counter it will start from and nothing.
    pub fn take_gensyms(&self) -> Result<(u64, Vec<Vec<u16>>), String> {
        match self.bootstrap() {
            Some(bootstrap) => Self::take_gensyms_from(bootstrap.evaluator.as_ref()),
            None => Ok((self.gensym_floor.get(), Vec::new())),
        }
    }
    /// The runtime's `gensym` skips every unqualified symbol name the form
    /// bridge decodes, so importing the names as symbols reserves them.
    fn import_gensyms(engine: &dyn MacroEvaluator, names: &[Vec<u16>]) -> Result<(), String> {
        const CHUNK: usize = 10_000;
        for chunk in names.chunks(CHUNK) {
            let symbols = chunk
                .iter()
                .map(|name| Datum::new(DatumKind::Symbol(Name::from_utf16(None, name))))
                .collect();
            let vector = Datum::new(DatumKind::Collection {
                kind: crate::parser::CollectionKind::Vector,
                children: symbols,
            });
            let limits = crate::form_bridge::ConversionLimits {
                nodes: CHUNK + 1,
                depth: 2,
            };
            let value = engine
                .import_data(&vector, limits)
                .map_err(|e| e.to_string())?;
            engine.release(value).map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    fn take_gensyms_from(engine: &dyn MacroEvaluator) -> Result<(u64, Vec<Vec<u16>>), String> {
        let value = engine
            .evaluate_unit("rt => rt.compilerForms.takeGensyms()")
            .map_err(|e| e.to_string())?;
        let source = crate::compiler_data::Source::new("<gensyms>", "");
        let span = source.span(0..0).expect("an empty span");
        let call = crate::compiler_data::Form::from_data(
            Datum::new(DatumKind::Nil),
            span.clone(),
            crate::compiler_data::Origin::SuppliedData,
        );
        let origin = crate::compiler_data::ExpansionOrigin {
            macro_name: Name::new(None, "gensym"),
            call: span,
            definition: None,
            helpers: Vec::new(),
        };
        // A bound engines pass as a 32-bit integer.
        let limits = crate::form_bridge::ConversionLimits {
            nodes: i32::MAX as usize,
            depth: 2,
        };
        let exported = engine.export_form(value, &call, origin, limits);
        engine.release(value).map_err(|e| e.to_string())?;
        let data = exported.map_err(|e| e.to_string())?.data();
        let DatumKind::Collection { children, .. } = data.kind() else {
            return Err("gensym names are not a vector".into());
        };
        let counter = match children.first().map(Datum::kind) {
            Some(DatumKind::Integer(counter)) if *counter >= 0 => *counter as u64,
            Some(DatumKind::Double(counter)) if *counter >= 0.0 => *counter as u64,
            _ => return Err("the gensym counter is not a count".into()),
        };
        let names = children[1..]
            .iter()
            .map(|name| match name.kind() {
                DatumKind::String(text) => Ok(text.to_vec()),
                _ => Err("a gensym name is not a string".to_owned()),
            })
            .collect::<Result<_, String>>()?;
        Ok((counter, names))
    }
    pub fn new() -> Rc<Self> {
        Rc::new(Self::default())
    }
    pub fn attach(self: &Rc<Self>, bootstrap: &Bootstrap, host: &MacroHost) -> Result<(), String> {
        if self.catalog.borrow().bootstrap.is_some() {
            return Ok(());
        }
        let evaluator = &bootstrap.evaluator;
        // `light.build`: a request names the units whose initializers run;
        // the catalog knows the units being analyzed.
        let weak = Rc::downgrade(self);
        let (build, log) = (host.build.clone(), host.dependency_log.clone());
        evaluator
            .install_string_service(
                "__lightBuildService",
                Rc::new(move |request| {
                    let session = weak.upgrade();
                    let catalog = session
                        .as_ref()
                        .and_then(|session| session.catalog.try_borrow().ok());
                    let source = |unit: usize| {
                        let catalog = catalog.as_ref()?;
                        Some(catalog.units.get(unit)?.try_borrow().ok()?.source)
                    };
                    let analysis = catalog.as_ref().map_or_else(Vec::new, |catalog| {
                        catalog
                            .stack
                            .iter()
                            .filter_map(|&unit| source(unit))
                            .collect()
                    });
                    let units = |unit| source(unit);
                    crate::build_services::serve_request(&build, &log, request, units, analysis)
                }),
            )
            .map_err(|e| e.to_string())?;
        let weak = Rc::downgrade(self);
        evaluator
            .clone()
            .install_compiler_service(
                "__lightCompileUnit",
                Rc::new(move |active, id| {
                    let session = weak.upgrade().ok_or("compilation session expired")?;
                    let unit = session.catalog.borrow().units[id as usize].clone();
                    let prepared = match &unit.borrow().state {
                        AnalysisState::Prepared(_, _, prepared) => Some(prepared.clone()),
                        _ => None,
                    };
                    crate::compile_trace::executed(id as usize, prepared.is_some());
                    let (registration, code) = if let Some(prepared) = prepared {
                        prepared.emit()
                    } else {
                        let (program, _) = session.analyze(id as usize, Some(active))?;
                        crate::emitter::emit_deferred_unit(&program)
                    };
                    unit.borrow_mut().emitted = true;
                    active
                        .evaluate_registration(&registration.source, &registration.chunks)
                        .map_err(|e| e.to_string())?;
                    Ok(code)
                }),
            )
            .map_err(|e| e.to_string())?;
        // Registry and root states stay in JS, so ready reads never cross Rust.
        let value = evaluator
            .evaluate_js(include_str!("compiler_roots.js"))
            .map_err(|e| e.to_string())?;
        evaluator.release(value).map_err(|e| e.to_string())?;
        let value = evaluator
            .evaluate_js(&format!(
                "(globalThis.lightCljsBuild = ({})(globalThis.__lightBuildService, globalThis.__lightBuildCaller), null)",
                crate::build_services::BUILD_OBJECT
            ))
            .map_err(|e| e.to_string())?;
        evaluator.release(value).map_err(|e| e.to_string())?;
        // Capture the canonical runtime without a second runtime or native adapter.
        let value = evaluator
            .evaluate_unit("rt => { globalThis.__lightRuntimeForCompiler = rt; return null; }")
            .map_err(|e| e.to_string())?;
        evaluator.release(value).map_err(|e| e.to_string())?;
        // What installed namespaces reserved before the session started, and
        // the log capture reads.
        let pending = std::mem::take(&mut *self.gensyms_pending.borrow_mut());
        Self::raise_gensyms(evaluator.as_ref(), self.gensym_floor.get())?;
        Self::import_gensyms(evaluator.as_ref(), &pending)?;
        if self.gensyms_logged.get() {
            Self::take_gensyms_from(evaluator.as_ref())?;
        }
        let registrations = {
            let mut catalog = self.catalog.borrow_mut();
            catalog.bootstrap = Some(bootstrap.clone());
            catalog.registrations.clone()
        };
        for registration in registrations {
            match registration {
                Registration::Root(name, unit, test, once, location) => {
                    Self::install_root(evaluator.as_ref(), &name, unit, test, once, &location)?
                }
                Registration::Bundle(bundle) => {
                    let value = evaluator
                        .evaluate_unit(&bundle.source())
                        .map_err(|e| e.to_string())?;
                    evaluator.release(value).map_err(|e| e.to_string())?;
                }
            }
        }
        Ok(())
    }
    pub fn compilation_uses(&self) -> Vec<crate::analyzer::CompilationUse> {
        self.catalog
            .borrow()
            .units
            .iter()
            .map(|unit| {
                let unit = unit.borrow();
                crate::analyzer::CompilationUse {
                    name: unit.name.clone(),
                    generation: unit.request.generation,
                    analyses: unit.analyses,
                    emissions: usize::from(unit.emitted),
                }
            })
            .collect()
    }
    /// How many units this compilation registered, analyzed, prepared and
    /// executed (`--stats`).
    pub fn unit_counts(&self) -> crate::compile_stats::DeferredUnitCounts {
        let catalog = self.catalog.borrow();
        let mut counts = crate::compile_stats::DeferredUnitCounts {
            registered: catalog.units.len(),
            ..Default::default()
        };
        for unit in &catalog.units {
            let unit = unit.borrow();
            counts.analyzed += usize::from(unit.analyses > 0);
            counts.prepared += usize::from(matches!(unit.state, AnalysisState::Prepared(..)));
            counts.executed += usize::from(unit.emitted);
        }
        counts
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
            products,
            extensions,
            once,
            form,
            snapshot,
            host,
            location,
            analyzed,
        } = registration;
        let names = products
            .iter()
            .map(|g| {
                let b = &program.globals[g.0];
                format!("{}/{}", b.namespace, b.name)
            })
            .collect::<Vec<_>>();
        let name = format!("declaration#{}", self.catalog.borrow().units.len());
        let unit = self
            .register(name, Some(form), snapshot, host, false, location.clone())?
            .unwrap();
        let group = analyzed.is_some();
        if let Some(mut program) = analyzed {
            program.shrink_to_fit();
            let unit = self.catalog.borrow().units[unit].clone();
            let mut unit = unit.borrow_mut();
            unit.state = AnalysisState::Analyzed(Rc::new(program), Vec::new());
            unit.analyses = 1;
            unit.form = None;
            unit.snapshot = None;
            unit.host = None;
        }
        let bundle = Rc::new(BundleRecord {
            products: names,
            unit,
            extensions: extensions.to_vec(),
            location,
            once,
            group,
        });
        let source = bundle.source();
        let bootstrap = {
            let mut c = self.catalog.borrow_mut();
            c.registrations.push(Registration::Bundle(bundle));
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
            // Initializers that call `gensym` number in the scope that
            // registers them.
            crate::generated_names::sync_gensym_scope(engine).map_err(|e| e.to_string())?;
            let value = engine.evaluate_unit(&source).map_err(|e| e.to_string())?;
            engine.release(value).map_err(|e| e.to_string())?;
        }
        if group {
            self.release_application_ir(unit);
        }
        Ok(unit)
    }
    fn install_root(
        engine: &dyn MacroEvaluator,
        name: &str,
        unit: Option<usize>,
        test: Option<usize>,
        once: bool,
        location: &str,
    ) -> Result<(), String> {
        engine
            .register_compiler_root(name, unit, once, location, test)
            .map_err(|e| e.to_string())
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
                Some(test),
                snapshot.clone(),
                host.macro_analysis_frame(),
                false,
                location.clone(),
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
                if let Some(capture) = self.capture.borrow_mut().as_mut() {
                    capture.units.push(UnitCapture {
                        view_globals: snapshot.resolution_program.globals.len(),
                        extension_frontier: snapshot.extension_frontier,
                        added: Vec::new(),
                    });
                }
                c.units.push(Rc::new(RefCell::new(Unit {
                    request,
                    name: name.clone(),
                    source: snapshot.source_index as u32,
                    form: Some(form),
                    snapshot: Some(Box::new(snapshot)),
                    host: Some(Box::new(host)),
                    state: AnalysisState::Unstarted,
                    analyses: 0,
                    emitted: false,
                })));
                id
            });
            if !(id.is_none() && c.active.contains_key(&name)
                || once && c.active.get(&name).is_some_and(|defined| *defined))
            {
                c.multifn_targets.remove(&name);
                c.active.insert(name.clone(), id.is_some());
            }
            c.registrations.push(Registration::Root(
                name.clone(),
                id,
                test,
                once,
                location.clone(),
            ));
            (id, c.bootstrap.clone())
        };
        if let Some(bootstrap) = bootstrap {
            Self::install_root(
                bootstrap.evaluator.as_ref(),
                &name,
                id,
                test,
                once,
                &location,
            )?;
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
    /// Called after the analyzed body has been copied into its application
    /// owner. Preparing source is pure: execution and registration stay lazy.
    pub fn release_application_ir(&self, id: usize) {
        let unit = self.catalog.borrow().units[id].clone();
        let mut unit = unit.borrow_mut();
        if let AnalysisState::Analyzed(program, warnings) = &unit.state {
            unit.state = AnalysisState::Prepared(
                Rc::downgrade(program),
                warnings.clone(),
                Rc::new(crate::emitter::DeferredUnit::prepare(program)),
            );
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
                AnalysisState::Prepared(program, warnings, _) => {
                    return program
                        .upgrade()
                        .map(|program| (program, warnings.clone()))
                        .ok_or_else(|| "application IR was already transferred".to_owned());
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
            let depth = self.catalog.borrow().stack.len();
            let host = unit.host.as_ref().expect("unstarted unit host");
            crate::macro_host::WorkMarks::note(&host.work.unit_depth, depth + 1);
            if depth >= host.limits.expansion_depth.min(64) {
                let message =
                    "compiler macro expansion depth limit exceeded in deferred units".to_owned();
                unit.state = AnalysisState::Failed(message.clone(), None);
                unit.form = None;
                unit.snapshot = None;
                unit.host = None;
                return Err(message);
            }
            unit.state = AnalysisState::Analyzing;
            unit.analyses += 1;
            // The request has stable identity even if a newer same-kind source
            // generation is activated while this frame is suspended.
            let request = unit.request;
            debug_assert!(request.generation > 0);
            debug_assert_eq!(self.catalog.borrow().bindings[&unit.name].0, request.var);
            // A unit is analyzed once; its registration-time host is not needed
            // after its analysis frame is taken.
            let mut host = unit
                .host
                .take()
                .expect("unstarted unit host")
                .macro_analysis_frame();
            if host.bootstrap.is_none() {
                host.bootstrap = bootstrap;
            }
            (
                unit.form.take().expect("unstarted unit form"),
                unit.snapshot.take().expect("unstarted unit snapshot"),
                host,
            )
        };
        let depth = {
            let mut catalog = self.catalog.borrow_mut();
            catalog.stack.push(id);
            catalog.stack.len() - 1
        };
        // Declared before the frontier guard, so the frontier's transitions
        // count in the unit's frame (`--stats`, [`crate::compile_trace`]).
        let _traced = crate::compile_trace::enter_unit(
            || crate::compile_trace::UnitInfo {
                id,
                source: unit.borrow().source,
                name: unit.borrow().name.clone(),
                span: form.span(),
                depth,
            },
            &host,
        );
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
            engine
                .compiler_frontier(Some(snapshot.extension_frontier))
                .map_err(|e| e.to_string())?;
            Some(FrontierGuard { engine })
        } else {
            None
        };
        // The unit's macros number in its scope; whatever demanded it, such
        // as a running macro calling it, numbers in its own again after.
        let _gensyms = crate::generated_names::RestoreGensymScope(engine);
        let analysis = crate::analyzer::analyze_deferred_form(&form, &mut host, &snapshot, engine);
        let diagnostic = analysis.as_ref().err().cloned();
        let result = analysis
            .map(|(mut p, w)| {
                p.shrink_to_fit();
                (Rc::new(p), w)
            })
            .map_err(|e| {
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
            global_names: self.global_names.clone(),
            namespace_globals: self.namespace_globals.clone(),
            session_namespaces: self.session_namespaces.clone(),
            options: self.options,
            unchecked_arrays: self.unchecked_arrays,
            source_index: self.source_index,
            definition_name: self.definition_name.clone(),
            definition_test: self.definition_test.clone(),
            definition_test_unit: self.definition_test_unit,
            extension_frontier: self.extension_frontier,
            names: self.names.clone(),
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
        let popped = c.stack.pop();
        debug_assert_eq!(popped, Some(self.id));
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
        engine
            .compiler_frontier(Some(0))
            .map_err(|e| e.to_string())?;
        Ok(Self { engine })
    }
}
impl Drop for FrontierGuard<'_> {
    fn drop(&mut self) {
        // Terminal cancellation makes this heap unusable; no later demand runs.
        let _ = self.engine.compiler_frontier(None);
    }
}

#[cfg(test)]
mod retention_tests {
    use super::*;

    #[test]
    fn completed_units_release_their_analysis_inputs() {
        let mut host = MacroHost::new();
        crate::compile_with_macros("(defn f [x] (+ x 1)) (defmacro m [] (f 41)) (m)", &mut host)
            .unwrap();
        let session = host.session_handle().unwrap();
        let catalog = session.catalog.borrow();
        let mut completed = 0;
        for unit in &catalog.units {
            let unit = unit.borrow();
            if matches!(
                unit.state,
                AnalysisState::Analyzed(..) | AnalysisState::Prepared(..)
            ) {
                completed += 1;
                assert!(unit.form.is_none());
                assert!(unit.snapshot.is_none());
                assert!(unit.host.is_none());
            }
        }
        assert!(completed > 0);
    }

    #[test]
    fn application_assembly_releases_programs_but_retains_lazy_source() {
        let mut host = MacroHost::new();
        crate::compile_with_macros("(defn f [x] (+ x 1)) (defmacro m [] (f 41)) (m)", &mut host)
            .unwrap();
        let session = host.session_handle().unwrap();
        let catalog = session.catalog.borrow();
        let mut released = 0;
        for unit in &catalog.units {
            if let AnalysisState::Prepared(program, _, code) = &unit.borrow().state {
                assert!(program.upgrade().is_none());
                assert!(!code.emit().1.is_empty());
                released += 1;
            }
        }
        assert!(released > 0);
    }
}
