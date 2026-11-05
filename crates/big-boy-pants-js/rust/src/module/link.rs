//! Source lowering is pre-run. Graph traversal, ResolveExport, environment
//! allocation and module execution are saved, budgeted steps of the entry task.

use super::dfs::{Dfs, Event};
use super::resolve::{Resolution, Resolver};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::ops::Bound::{Excluded, Unbounded};

use super::*;
use crate::frontend::{ImportName, ModuleBinding, ModuleRecord, lower_module, module_bindings};
use crate::heap::gc::{Trace, TraceEdge};
use crate::heap::{Heap, HeapObject};
use crate::value::Value;
use crate::vm::dispatch::Fault;
use crate::vm::function::BindingCell;
use crate::{Assembler, Executable, Opcode, TaskId};

pub(super) type Origin = (ModuleKey, String);
pub(super) const NAMESPACE: &str = "*namespace*";
// None is an ambiguous star export; absence is a missing export.
pub(super) type Exports = HashMap<ModuleKey, BTreeMap<String, Option<Origin>>>;

#[derive(Debug)]
pub(super) struct PreparedModule {
    name: String,
    pub(super) record: ModuleRecord,
    pub(super) local_exports: BTreeMap<String, String>,
    pub(super) indirect_exports: BTreeMap<String, Origin>,
    pub(super) requests: Vec<ModuleKey>,
    pub(super) request_keys: HashMap<Vec<u16>, ModuleKey>,
    pub(super) imports: HashMap<String, Origin>,
    bindings: Vec<ModuleBinding>,
    instantiate: Executable,
    body: Executable,
    json: Option<JsString>,
}

#[derive(Debug)]
enum Stage {
    Visit,
    Allocate {
        module: usize,
        binding: usize,
    },
    ExportNames {
        module: usize,
        local: usize,
        indirect: usize,
        star: usize,
    },
    SpreadNames,
    Exports {
        module: usize,
        after: Option<String>,
    },
    Imports {
        module: usize,
        binding: usize,
    },
    Instantiate(usize),
    Evaluate,
    Done,
    CreateNamespaces {
        module: usize,
        entry: usize,
    },
    PopulateNamespaces {
        module: usize,
        after: Option<String>,
    },
    ParseJson(usize),
}

pub(crate) enum Action {
    Continue,
    Run(Executable, Vec<Value>),
    Failed(Box<crate::VmError>),
    Complete,
}

#[derive(Debug)]
pub(crate) struct Linker {
    modules: HashMap<ModuleKey, PreparedModule>,
    initial_error: Option<String>,
    stage: Stage,
    entry: ModuleKey,
    walk: Dfs,
    components: HashMap<ModuleKey, ModuleKey>,
    // Successful completion and errors belong to SCC roots, never records.
    evaluated: HashMap<ModuleKey, bool>,
    errors: HashMap<ModuleKey, crate::VmError>,
    names: HashMap<ModuleKey, BTreeSet<String>>,
    star_parents: HashMap<ModuleKey, Vec<ModuleKey>>,
    pending_names: VecDeque<(ModuleKey, String)>,
    spreading: Option<(ModuleKey, String, usize)>,
    resolver: Option<(ModuleKey, String, Resolver)>,
    order: Vec<ModuleKey>,
    export_order: Vec<ModuleKey>,
    exports: Exports,
    slots: HashMap<Origin, usize>,
    cells: Vec<Value>,
    arguments: HashMap<ModuleKey, Vec<Value>>,
    namespaces: HashMap<ModuleKey, crate::heap::ObjectId>,
    json_parser: Option<Box<crate::vm::json::Parser>>,
    pub(crate) running: bool,
}

impl Linker {
    /// Compile each record independently. No graph traversal, export lookup,
    /// binding-cell allocation or module instantiation happens during spawn.
    pub(crate) fn new(graph: &ModuleGraph) -> Self {
        let mut modules = HashMap::new();
        let mut initial_error = None;
        for node in graph.modules() {
            let loaded = node.module();
            let Some(record) = loaded.record() else {
                initial_error = Some("module record missing".into());
                break;
            };
            let prepare = || {
                if matches!(loaded.source(), ModuleSource::Json { .. }) {
                    return Ok((vec![ModuleBinding::synthetic_default()], entry(), entry()));
                }
                let bindings = module_bindings(loaded.source.text(), record)?;
                let instantiate = lower_module(
                    loaded.source.name(),
                    loaded.source.text(),
                    record,
                    &bindings,
                    true,
                )?;
                let body = lower_module(
                    loaded.source.name(),
                    loaded.source.text(),
                    record,
                    &bindings,
                    false,
                )?;
                Ok::<_, SyntaxError>((bindings, instantiate, body))
            };
            let (bindings, instantiate, body) = match prepare() {
                Ok(prepared) => prepared,
                Err(error) => {
                    initial_error = Some(error.message);
                    break;
                }
            };
            let request_keys: HashMap<_, _> = record
                .requested_modules()
                .iter()
                .map(|specifier| specifier.code_units().to_vec())
                .zip(node.requested_modules.iter().copied())
                .collect();
            let imports = record
                .import_entries()
                .iter()
                .map(|import| {
                    let name = match &import.import_name {
                        ImportName::Name(name) => name.clone(),
                        ImportName::Namespace => NAMESPACE.into(),
                    };
                    (
                        import.local_name.clone(),
                        (request_keys[import.specifier.code_units()], name.clone()),
                    )
                })
                .collect();
            modules.insert(
                loaded.key(),
                PreparedModule {
                    name: loaded.source.name().into(),
                    record: record.clone(),
                    local_exports: BTreeMap::new(),
                    indirect_exports: BTreeMap::new(),
                    requests: node.requested_modules.clone(),
                    request_keys,
                    imports,
                    bindings,
                    instantiate,
                    body,
                    json: match loaded.source() {
                        ModuleSource::Json { text, .. } => Some(JsString::from(text.as_str())),
                        ModuleSource::JavaScript { .. } => None,
                    },
                },
            );
        }
        Self {
            modules,
            initial_error,
            stage: Stage::Visit,
            entry: graph.entry,
            walk: Dfs::new(graph.entry),
            components: HashMap::new(),
            evaluated: HashMap::new(),
            errors: HashMap::new(),
            names: HashMap::new(),
            star_parents: HashMap::new(),
            pending_names: VecDeque::new(),
            spreading: None,
            resolver: None,
            order: Vec::new(),
            export_order: Vec::new(),
            exports: Exports::new(),
            slots: HashMap::new(),
            cells: Vec::new(),
            arguments: HashMap::new(),
            namespaces: HashMap::new(),
            json_parser: None,
            running: false,
        }
    }

    pub(crate) fn phase(&self) -> super::ModulePhase {
        match self.stage {
            Stage::Instantiate(_) => super::ModulePhase::Instantiating,
            Stage::Evaluate | Stage::Done => super::ModulePhase::Evaluating,
            _ => super::ModulePhase::Linking,
        }
    }

    fn resolve_export(
        modules: &HashMap<ModuleKey, PreparedModule>,
        exports: &Exports,
        key: ModuleKey,
        name: &str,
    ) -> Result<Origin, Fault> {
        match exports[&key].get(name) {
            Some(Some(origin)) => Ok(origin.clone()),
            resolution => Err(Fault::SyntaxError(match resolution {
                Some(None) => format!(
                    "Export '{name}' is ambiguous in module '{}'",
                    modules[&key].name
                ),
                _ => format!(
                    "Could not find export '{name}' in module '{}'",
                    modules[&key].name
                ),
            })),
        }
    }

    fn begin_evaluation(&mut self, entry: ModuleKey) -> Result<(), Box<crate::VmError>> {
        let root = self.components[&entry];
        if let Some(error) = self.errors.get(&root) {
            return Err(Box::new(error.clone()));
        }
        self.walk = Dfs::new(entry);
        self.stage = if self.evaluated.contains_key(&root) {
            Stage::Done
        } else {
            Stage::Evaluate
        };
        Ok(())
    }

    /// An uncaught body/dependency failure aborts every still-active SCC,
    /// including a member whose body returned before another member threw.
    /// Completed dependency SCCs retain their successful outcome.
    pub(crate) fn record_failure(&mut self, error: &crate::VmError) {
        if matches!(self.stage, Stage::Evaluate) {
            for key in self.walk.active() {
                self.errors
                    .entry(self.components[&key])
                    .or_insert_with(|| error.clone());
            }
        }
    }

    // Keep JSON parsing off the large graph-linking stack frame, since module
    // tasks also run on 64 KiB workers in debug builds.
    #[inline(never)]
    fn parse_json(&mut self, index: usize, heap: &Heap, owner: TaskId) -> Result<Action, Fault> {
        let Some(key) = self.order.get(index) else {
            self.stage = Stage::Instantiate(0);
            return Ok(Action::Continue);
        };
        let node = &self.modules[key];
        let Some(text) = &node.json else {
            self.stage = Stage::ParseJson(index + 1);
            return Ok(Action::Continue);
        };
        if self.json_parser.is_none() {
            self.json_parser = Some(Box::new(crate::vm::json::Parser::new(heap, text.clone())?));
        }
        // Validate every JSON module before entering any JavaScript frame,
        // including hoisted function instantiation. Partial containers remain
        // in the traced continuation between parser steps.
        let value = self
            .json_parser
            .as_mut()
            .unwrap()
            .advance(heap, owner)
            .map_err(|fault| match fault {
                Fault::JsonSyntaxError {
                    message,
                    line,
                    column,
                } => Fault::SyntaxError(format!("{}:{line}:{column}: {message}", node.name)),
                other => other,
            })?;
        if let Some(value) = value {
            let Value::Object(cell) = self.cells[self.slots[&(*key, "*default*".into())]] else {
                return Err(Fault::Internal("JSON default cell missing"));
            };
            heap.with_object_mut(cell, |object| {
                if let HeapObject::Cell(cell) = object {
                    cell.value = Some(value);
                }
            });
            self.json_parser = None;
            self.stage = Stage::ParseJson(index + 1);
        }
        Ok(Action::Continue)
    }

    /// One graph edge, binding, export entry or stage transition per step.
    /// Star tables retain a name cursor, never copy/traverse a whole barrel
    /// in one step. Calls use the VM's existing bounded argument binding.
    pub(crate) fn step(&mut self, heap: &Heap, owner: TaskId) -> Result<Action, Fault> {
        if let Stage::ParseJson(index) = self.stage {
            return self.parse_json(index, heap, owner);
        }
        self.step_link(heap, owner)
    }

    #[inline(never)]
    fn step_link(&mut self, heap: &Heap, owner: TaskId) -> Result<Action, Fault> {
        if let Some(error) = self.initial_error.take() {
            return Err(Fault::SyntaxError(error));
        }
        match &mut self.stage {
            Stage::Visit => match self.walk.step(&self.modules) {
                Event::Body(key) => self.order.push(key),
                Event::Member { key, root } => {
                    self.components.insert(key, root);
                    // A child's body can precede a dependency of its
                    // ancestor inside an SCC. Resolve whole dependency
                    // SCCs first, rather than caching in body post-order.
                    self.export_order.push(key);
                }
                Event::Continue => {}
                Event::Complete => {
                    self.stage = Stage::Allocate {
                        module: 0,
                        binding: 0,
                    }
                }
            },
            Stage::Allocate { module, binding } => {
                if let Some(key) = self.order.get(*module) {
                    if let Some(b) = self.modules[key].bindings.get(*binding) {
                        *binding += 1;
                        if !b.is_import()
                            || self.modules[key]
                                .imports
                                .get(&b.name.name)
                                .is_some_and(|(_, name)| name == NAMESPACE)
                        {
                            let cell =
                                BindingCell::new(owner, b.is_var().then_some(Value::Undefined));
                            let id = heap
                                .alloc(HeapObject::Cell(cell))
                                .ok_or(Fault::OutOfMemory("heap object table exhausted"))?;
                            self.slots
                                .insert((*key, b.name.name.clone()), self.cells.len());
                            self.cells.push(Value::Object(id));
                        }
                    } else {
                        *module += 1;
                        *binding = 0;
                    }
                } else {
                    self.stage = Stage::ExportNames {
                        module: 0,
                        star: 0,
                        local: 0,
                        indirect: 0,
                    };
                }
            }
            Stage::ExportNames {
                module,
                star,
                local,
                indirect,
            } => {
                let Some(key) = self.order.get(*module).copied() else {
                    self.stage = Stage::SpreadNames;
                    return Ok(Action::Continue);
                };
                let node = self.modules.get_mut(&key).unwrap();
                self.names.entry(key).or_default();
                self.exports.entry(key).or_default();
                // Record reverse star edges, then propagate names to a fixed
                // point. Resolution itself uses the spec's resolve set below.
                if let Some(specifier) = node.record.star_export_entries().get(*star) {
                    let dep = node.request_keys[specifier.code_units()];
                    self.star_parents.entry(dep).or_default().push(key);
                    *star += 1;
                } else if let Some(entry) = node.record.local_export_entries().get(*local) {
                    *local += 1;
                    node.local_exports
                        .insert(entry.export_name.clone(), entry.local_name.clone());
                    if self
                        .names
                        .get_mut(&key)
                        .unwrap()
                        .insert(entry.export_name.clone())
                    {
                        self.pending_names
                            .push_back((key, entry.export_name.clone()));
                    }
                } else if let Some(entry) = node.record.indirect_export_entries().get(*indirect) {
                    *indirect += 1;
                    node.indirect_exports.insert(
                        entry.export_name.clone(),
                        (
                            node.request_keys[entry.specifier.code_units()],
                            entry.import_name.clone(),
                        ),
                    );
                    if self
                        .names
                        .get_mut(&key)
                        .unwrap()
                        .insert(entry.export_name.clone())
                    {
                        self.pending_names
                            .push_back((key, entry.export_name.clone()));
                    }
                } else {
                    *module += 1;
                    *star = 0;
                    *local = 0;
                    *indirect = 0;
                }
            }
            Stage::SpreadNames => {
                if let Some((key, name, index)) = &mut self.spreading {
                    if name != "default"
                        && let Some(parent) = self
                            .star_parents
                            .get(key)
                            .and_then(|parents| parents.get(*index))
                    {
                        *index += 1;
                        if self.names.get_mut(parent).unwrap().insert(name.clone()) {
                            self.pending_names.push_back((*parent, name.clone()));
                        }
                    } else {
                        self.spreading = None;
                    }
                } else if let Some((key, name)) = self.pending_names.pop_front() {
                    self.spreading = Some((key, name, 0));
                } else {
                    self.stage = Stage::Exports {
                        module: 0,
                        after: None,
                    };
                }
            }
            Stage::Exports { module, after } => {
                if let Some((key, name, resolver)) = &mut self.resolver {
                    if let Some(resolution) =
                        resolver.step(&self.modules, &self.exports, &self.components)
                    {
                        let node = &self.modules[key];
                        if node.indirect_exports.contains_key(name)
                            && !matches!(resolution, Resolution::Found(_))
                        {
                            let reason = match resolution {
                                Resolution::Ambiguous => "ambiguous export",
                                _ if resolver.circular => {
                                    "circular reference when looking for export"
                                }
                                _ => "Could not find export",
                            };
                            return Err(Fault::SyntaxError(format!(
                                "{reason} '{name}' in module '{}'",
                                node.name
                            )));
                        }
                        match resolution {
                            Resolution::Found(origin) => {
                                self.exports
                                    .get_mut(key)
                                    .unwrap()
                                    .insert(name.clone(), Some(origin));
                            }
                            Resolution::Ambiguous => {
                                self.exports
                                    .get_mut(key)
                                    .unwrap()
                                    .insert(name.clone(), None);
                            }
                            Resolution::Missing => {}
                        }
                        self.resolver = None;
                    }
                } else if let Some(key) = self.export_order.get(*module).copied() {
                    let range = after.as_ref().map_or(Unbounded, |s| Excluded(s.as_str()));
                    if let Some(name) = self.names[&key]
                        .range::<str, _>((range, Unbounded))
                        .next()
                        .cloned()
                    {
                        *after = Some(name.clone());
                        self.resolver = Some((key, name.clone(), Resolver::new(key, name)));
                    } else {
                        *module += 1;
                        *after = None;
                    }
                } else {
                    self.stage = Stage::CreateNamespaces {
                        module: 0,
                        entry: 0,
                    };
                }
            }
            Stage::Imports { module, binding } => {
                let Some(key) = self.order.get(*module).copied() else {
                    self.stage = Stage::ParseJson(0);
                    return Ok(Action::Continue);
                };
                let node = &self.modules[&key];
                if let Some(b) = node.bindings.get(*binding) {
                    let name = b.name.name.clone();
                    let origin = if b.is_import() {
                        let (dep, imported) = &node.imports[&name];
                        if imported == NAMESPACE {
                            let value = Value::Object(self.namespaces[dep]);
                            let Value::Object(cell) = self.cells[self.slots[&(key, name.clone())]]
                            else {
                                return Err(Fault::Internal("namespace import cell missing"));
                            };
                            heap.with_object_mut(cell, |o| {
                                if let HeapObject::Cell(cell) = o {
                                    cell.value = Some(value);
                                }
                            });
                            (key, name)
                        } else {
                            Self::resolve_export(&self.modules, &self.exports, *dep, imported)?
                        }
                    } else {
                        (key, name)
                    };
                    let slot = self.slots[&origin];
                    self.arguments
                        .entry(key)
                        .or_default()
                        .push(self.cells[slot].clone());
                    *binding += 1;
                } else {
                    *module += 1;
                    *binding = 0;
                }
            }
            Stage::ParseJson(_) => unreachable!("JSON parsing uses a separate stack frame"),
            Stage::Instantiate(index) => {
                // The index is advanced before entering the VM frame, so a
                // suspended module can never be selected for a second run.
                let next = *index;
                *index += 1;
                if let Some(key) = self.order.get(next) {
                    let node = &self.modules[key];
                    if node.json.is_some() {
                        return Ok(Action::Continue);
                    }
                    let args = self.arguments.get(key).cloned().unwrap_or_default();
                    self.running = true;
                    return Ok(Action::Run(node.instantiate.clone(), args));
                }
                if let Err(error) = self.begin_evaluation(self.entry) {
                    return Ok(Action::Failed(error));
                }
            }
            Stage::Evaluate => match self.walk.step(&self.modules) {
                Event::Body(key) => {
                    let root = self.components[&key];
                    if let Some(error) = self.errors.get(&root) {
                        return Ok(Action::Failed(Box::new(error.clone())));
                    }
                    if self.evaluated.contains_key(&root) {
                        return Ok(Action::Continue);
                    }
                    if self.modules[&key].json.is_some() {
                        return Ok(Action::Continue);
                    }
                    let args = self.arguments.get(&key).cloned().unwrap_or_default();
                    self.running = true;
                    return Ok(Action::Run(self.modules[&key].body.clone(), args));
                }
                Event::Member { key, root } => {
                    debug_assert_eq!(self.components[&key], root);
                    if key == root {
                        self.evaluated.insert(root, true);
                    }
                }
                Event::Complete => self.stage = Stage::Done,
                Event::Continue => {}
            },
            Stage::Done => return Ok(Action::Complete),
            Stage::CreateNamespaces { module, entry } => {
                let Some(key) = self.order.get(*module) else {
                    self.stage = Stage::PopulateNamespaces {
                        module: 0,
                        after: None,
                    };
                    return Ok(Action::Continue);
                };
                let node = &self.modules[key];
                // Process each source entry once, allocating only namespaces
                // actually requested by an import or namespace re-export.
                let dep = if let Some(import) = node.record.import_entries().get(*entry) {
                    (import.import_name == ImportName::Namespace)
                        .then(|| node.request_keys[import.specifier.code_units()])
                } else if let Some(export) = node
                    .record
                    .indirect_export_entries()
                    .get(*entry - node.record.import_entries().len())
                {
                    (export.import_name == NAMESPACE)
                        .then(|| node.request_keys[export.specifier.code_units()])
                } else {
                    *module += 1;
                    *entry = 0;
                    return Ok(Action::Continue);
                };
                *entry += 1;
                if let Some(dep) = dep
                    && let std::collections::hash_map::Entry::Vacant(slot) =
                        self.namespaces.entry(dep)
                {
                    let ns = heap
                        .alloc(HeapObject::ModuleNamespace(Box::new(
                            super::namespace::Namespace {
                                owner,
                                exports: BTreeMap::new(),
                            },
                        )))
                        .ok_or(Fault::OutOfMemory("heap object table exhausted"))?;
                    // Root the object before allocating its cell; all roots
                    // remain in this saved, task-owned linker continuation.
                    slot.insert(ns);
                    let cell = heap
                        .alloc(HeapObject::Cell(BindingCell::new(
                            owner,
                            Some(Value::Object(ns)),
                        )))
                        .ok_or(Fault::OutOfMemory("heap object table exhausted"))?;
                    self.slots.insert((dep, NAMESPACE.into()), self.cells.len());
                    self.cells.push(Value::Object(cell));
                }
            }
            Stage::PopulateNamespaces { module, after } => {
                let Some(key) = self.order.get(*module) else {
                    self.stage = Stage::Imports {
                        module: 0,
                        binding: 0,
                    };
                    return Ok(Action::Continue);
                };
                if let Some(ns) = self.namespaces.get(key) {
                    let range = after.as_ref().map_or(Unbounded, |s| Excluded(s.as_str()));
                    if let Some((name, origin)) =
                        self.exports[key].range::<str, _>((range, Unbounded)).next()
                    {
                        *after = Some(name.clone());
                        if let Some(origin) = origin {
                            let Value::Object(cell) = self.cells[self.slots[origin]] else {
                                return Err(Fault::Internal("namespace export cell missing"));
                            };
                            heap.with_object_mut(*ns, |o| {
                                if let HeapObject::ModuleNamespace(ns) = o {
                                    ns.exports.insert(JsString::from(name.as_str()), cell);
                                }
                            });
                        }
                        return Ok(Action::Continue);
                    }
                }
                *module += 1;
                *after = None;
            }
        }
        Ok(Action::Continue)
    }
}

impl Trace for Linker {
    fn trace(&self, visit: &mut dyn FnMut(TraceEdge)) {
        if let Some(parser) = &self.json_parser {
            parser.trace(visit);
        }
        self.cells.iter().for_each(|value| value.trace(visit));
        for namespace in self.namespaces.values() {
            Value::Object(*namespace).trace(visit);
        }
        // Lowered executables have immutable primitive constants only. Trace
        // them through the ordinary code path as well, keeping that invariant
        // explicit if the code representation is extended in the future.
        for node in self.modules.values() {
            node.instantiate.0.trace(visit);
            node.body.0.trace(visit);
        }
    }
}

pub(super) fn entry() -> Executable {
    let mut asm = Assembler::new();
    asm.op(Opcode::Undefined).op(Opcode::Return);
    asm.finish("<module graph>", 0, 0)
        .expect("module entry trampoline")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vm::dispatch::{Exit, execute_budgeted};
    use crate::vm::frames::{DiagnosticContext, VmLimits, VmState};

    #[test]
    fn failed_scc_rethrows_from_another_entry_in_same_instance_map() {
        struct Sources;
        impl ModuleHost for Sources {
            fn resolve(
                &mut self,
                name: &JsString,
                _: Option<ModuleKey>,
            ) -> Result<ModuleKey, String> {
                Ok(ModuleKey::new(match name.to_string().as_str() {
                    "a" => 0,
                    "b" => 1,
                    "leaf" => 2,
                    _ => unreachable!(),
                }))
            }
            fn fetch(&mut self, key: ModuleKey) -> Result<ModuleSource, String> {
                let (name, text) = match key {
                    ModuleKey(0) => ("a", "import 'b'; import 'leaf'; throw 42;"),
                    ModuleKey(1) => ("b", "import 'a'; import 'leaf';"),
                    ModuleKey(2) => ("leaf", "export let x = 7;"),
                    _ => unreachable!(),
                };
                Ok(ModuleSource::from_name(name.into(), text.into()))
            }
        }
        let heap = Heap::default();
        let graph = ModuleLoader::new()
            .load(crate::WorldId::INITIAL, &"a".into(), &mut Sources)
            .unwrap();
        let mut link = Linker::new(&graph);
        let owner = TaskId::from_raw(1);
        let error = loop {
            match link
                .step(&heap, owner)
                .unwrap_or_else(|_| panic!("link failed"))
            {
                Action::Continue => {}
                Action::Run(code, args) => {
                    let mut state = VmState::new(
                        &code.0,
                        &args,
                        DiagnosticContext {
                            task: owner,
                            world: graph.world,
                        },
                        VmLimits::DEFAULT,
                    );
                    match execute_budgeted(&mut state, &heap, &mut crate::host::NoHost, &mut None) {
                        Exit::Return(_) => link.running = false,
                        Exit::Fail(error) => {
                            link.record_failure(&error);
                            break error;
                        }
                        other => panic!("{other:?}"),
                    }
                }
                _ => panic!("expected body failure"),
            }
        };
        assert_eq!(
            link.components[&ModuleKey(0)],
            link.components[&ModuleKey(1)]
        );
        assert_ne!(
            link.components[&ModuleKey(0)],
            link.components[&ModuleKey(2)]
        );
        for entry in [ModuleKey(0), ModuleKey(1)] {
            assert_eq!(*link.begin_evaluation(entry).unwrap_err(), error);
        }
        // The already completed leaf stays successful and never runs again.
        link.begin_evaluation(ModuleKey(2)).unwrap();
        assert!(matches!(link.step(&heap, owner), Ok(Action::Complete)));
    }
}
