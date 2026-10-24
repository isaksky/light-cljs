//! Source lowering is pre-run. Graph traversal, ResolveExport, environment
//! allocation and module execution are saved, budgeted steps of the entry task.

use std::collections::BTreeMap;
use std::ops::Bound::{Excluded, Unbounded};

use super::*;
use crate::frontend::{ImportName, ModuleBinding, ModuleRecord, lower_module, module_bindings};
use crate::heap::gc::{Trace, TraceEdge};
use crate::heap::{Heap, HeapObject};
use crate::value::Value;
use crate::vm::dispatch::Fault;
use crate::vm::function::BindingCell;
use crate::{Assembler, Executable, Opcode, TaskId};

type Origin = (ModuleKey, String);
// None is an ambiguous star export; absence is a missing export.
type Exports = HashMap<ModuleKey, BTreeMap<String, Option<Origin>>>;

#[derive(Debug)]
struct PreparedModule {
    name: String,
    record: ModuleRecord,
    requests: Vec<ModuleKey>,
    request_keys: HashMap<Vec<u16>, ModuleKey>,
    imports: HashMap<String, Origin>,
    bindings: Vec<ModuleBinding>,
    instantiate: Executable,
    body: Executable,
}

#[derive(Debug)]
enum Stage {
    Visit,
    Allocate {
        module: usize,
        binding: usize,
    },
    Exports {
        module: usize,
        star: usize,
        after: Option<String>,
        local: usize,
        indirect: usize,
    },
    Imports {
        module: usize,
        binding: usize,
    },
    Instantiate(usize),
    Evaluate(usize),
    Done,
}

pub(crate) enum Action {
    Continue,
    Run(Executable, Vec<Value>),
    Complete,
}

#[derive(Debug)]
pub(crate) struct Linker {
    modules: HashMap<ModuleKey, PreparedModule>,
    initial_error: Option<String>,
    stage: Stage,
    stack: Vec<(ModuleKey, usize)>,
    visited: HashMap<ModuleKey, bool>,
    order: Vec<ModuleKey>,
    exports: Exports,
    slots: HashMap<Origin, usize>,
    cells: Vec<Value>,
    arguments: HashMap<ModuleKey, Vec<Value>>,
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
                initial_error = Some("JSON modules cannot be executed yet".into());
                break;
            };
            let prepare = || {
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
                    let ImportName::Name(name) = &import.import_name else {
                        unreachable!("checked namespace")
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
                    requests: node.requested_modules.clone(),
                    request_keys,
                    imports,
                    bindings,
                    instantiate,
                    body,
                },
            );
        }
        Self {
            modules,
            initial_error,
            stage: Stage::Visit,
            stack: vec![(graph.entry, 0)],
            visited: HashMap::from([(graph.entry, false)]),
            order: Vec::new(),
            exports: Exports::new(),
            slots: HashMap::new(),
            cells: Vec::new(),
            arguments: HashMap::new(),
            running: false,
        }
    }

    pub(crate) fn phase(&self) -> super::ModulePhase {
        match self.stage {
            Stage::Instantiate(_) => super::ModulePhase::Instantiating,
            Stage::Evaluate(_) | Stage::Done => super::ModulePhase::Evaluating,
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

    /// One graph edge, binding, export entry or stage transition per step.
    /// Star tables retain a name cursor, never copy/traverse a whole barrel
    /// in one step. Calls use the VM's existing bounded argument binding.
    pub(crate) fn step(&mut self, heap: &Heap, owner: TaskId) -> Result<Action, Fault> {
        if let Some(error) = self.initial_error.take() {
            return Err(Fault::SyntaxError(error));
        }
        match &mut self.stage {
            Stage::Visit => {
                if let Some((key, next)) = self.stack.last_mut() {
                    if let Some(dep) = self.modules[key].requests.get(*next).copied() {
                        *next += 1;
                        match self.visited.get(&dep) {
                            Some(true) => {}
                            Some(false) => {
                                return Err(Fault::SyntaxError(
                                    "cyclic module graphs cannot be executed yet".into(),
                                ));
                            }
                            None => {
                                self.visited.insert(dep, false);
                                self.stack.push((dep, 0));
                            }
                        }
                    } else {
                        self.visited.insert(*key, true);
                        self.order.push(*key);
                        self.stack.pop();
                    }
                } else {
                    self.stage = Stage::Allocate {
                        module: 0,
                        binding: 0,
                    };
                }
            }
            Stage::Allocate { module, binding } => {
                if let Some(key) = self.order.get(*module) {
                    if let Some(b) = self.modules[key].bindings.get(*binding) {
                        *binding += 1;
                        if !b.is_import() {
                            let cell = BindingCell {
                                owner,
                                value: b.is_var().then_some(Value::Undefined),
                            };
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
                    self.stage = Stage::Exports {
                        module: 0,
                        star: 0,
                        after: None,
                        local: 0,
                        indirect: 0,
                    };
                }
            }
            Stage::Exports {
                module,
                star,
                after,
                local,
                indirect,
            } => {
                let Some(key) = self.order.get(*module).copied() else {
                    self.stage = Stage::Imports {
                        module: 0,
                        binding: 0,
                    };
                    return Ok(Action::Continue);
                };
                let node = &self.modules[&key];
                if let Some(specifier) = node.record.star_export_entries().get(*star) {
                    let dep = node.request_keys[specifier.code_units()];
                    let range = after.as_ref().map_or(Unbounded, |s| Excluded(s.as_str()));
                    let next = self.exports[&dep]
                        .range::<str, _>((range, Unbounded))
                        .next()
                        .map(|(name, origin)| (name.clone(), origin.clone()));
                    if let Some((name, origin)) = next {
                        *after = Some(name.clone());
                        if name != "default" {
                            self.exports
                                .entry(key)
                                .or_default()
                                .entry(name)
                                .and_modify(|prior| {
                                    if *prior != origin {
                                        *prior = None;
                                    }
                                })
                                .or_insert(origin);
                        }
                    } else {
                        *star += 1;
                        *after = None;
                    }
                } else if let Some(entry) = node.record.local_export_entries().get(*local) {
                    *local += 1;
                    self.exports.entry(key).or_default().insert(
                        entry.export_name.clone(),
                        Some((key, entry.local_name.clone())),
                    );
                } else if let Some(entry) = node.record.indirect_export_entries().get(*indirect) {
                    *indirect += 1;
                    let (dep, name, exported) = (
                        node.request_keys[entry.specifier.code_units()],
                        entry.import_name.clone(),
                        entry.export_name.clone(),
                    );
                    let origin = Self::resolve_export(&self.modules, &self.exports, dep, &name)?;
                    self.exports
                        .entry(key)
                        .or_default()
                        .insert(exported, Some(origin));
                } else {
                    self.exports.entry(key).or_default();
                    *module += 1;
                    *star = 0;
                    *after = None;
                    *local = 0;
                    *indirect = 0;
                }
            }
            Stage::Imports { module, binding } => {
                let Some(key) = self.order.get(*module).copied() else {
                    self.stage = Stage::Instantiate(0);
                    return Ok(Action::Continue);
                };
                let node = &self.modules[&key];
                if let Some(b) = node.bindings.get(*binding) {
                    let name = b.name.name.clone();
                    let origin = if b.is_import() {
                        let (dep, imported) = &node.imports[&name];
                        Self::resolve_export(&self.modules, &self.exports, *dep, imported)?
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
            Stage::Instantiate(_) | Stage::Evaluate(_) => {
                let instantiate = matches!(self.stage, Stage::Instantiate(_));
                // The index is advanced before entering the VM frame, so a
                // suspended module can never be selected for a second run.
                let index = match &mut self.stage {
                    Stage::Instantiate(i) | Stage::Evaluate(i) => {
                        let n = *i;
                        *i += 1;
                        n
                    }
                    _ => unreachable!(),
                };
                if let Some(key) = self.order.get(index) {
                    let node = &self.modules[key];
                    let code = if instantiate {
                        &node.instantiate
                    } else {
                        &node.body
                    };
                    let args = self.arguments.get(key).cloned().unwrap_or_default();
                    self.running = true;
                    return Ok(Action::Run(code.clone(), args));
                }
                self.stage = if instantiate {
                    Stage::Evaluate(0)
                } else {
                    Stage::Done
                };
            }
            Stage::Done => return Ok(Action::Complete),
        }
        Ok(Action::Continue)
    }
}

impl Trace for Linker {
    fn trace(&self, visit: &mut dyn FnMut(TraceEdge)) {
        self.cells.iter().for_each(|value| value.trace(visit));
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
