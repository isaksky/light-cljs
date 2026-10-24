//! Static acyclic linking, followed by a resumable bytecode driver. Compiler
//! maps contain only private keys/slots; all mutable instances are allocated
//! by NewCell in the entry task, never retained in a loaded record.

use super::*;
use crate::frontend::{ModuleBinding, Span, SyntaxErrorKind, lower_module, module_bindings};
use crate::{Assembler, Executable, Opcode};

fn error(node: &ModuleNode, span: Span, message: impl Into<String>) -> SyntaxError {
    SyntaxError::new(
        node.module.source.text(),
        SyntaxErrorKind::Invalid,
        span,
        message,
    )
}

fn requested(node: &ModuleNode, specifier: &JsString) -> ModuleKey {
    let index = node
        .module
        .record()
        .unwrap()
        .requested_modules()
        .iter()
        .position(|s| s == specifier)
        .expect("record request");
    node.requested_modules[index]
}

fn post_order(graph: &ModuleGraph) -> Result<Vec<ModuleKey>, SyntaxError> {
    let mut state = HashMap::new();
    let mut order = Vec::new();
    let mut stack = vec![(graph.entry, 0)];
    state.insert(graph.entry, false);
    while let Some((key, next)) = stack.last_mut() {
        let node = &graph.nodes[key];
        if let Some(dep) = node.requested_modules.get(*next).copied() {
            *next += 1;
            match state.get(&dep) {
                Some(true) => {}
                Some(false) => {
                    return Err(error(
                        node,
                        Span::new(0, 0),
                        "cyclic module graphs cannot be executed yet",
                    ));
                }
                None => {
                    state.insert(dep, false);
                    stack.push((dep, 0));
                }
            }
        } else {
            state.insert(*key, true);
            order.push(*key);
            stack.pop();
        }
    }
    Ok(order)
}

fn resolve_export(
    graph: &ModuleGraph,
    mut key: ModuleKey,
    mut name: String,
) -> Result<(ModuleKey, String), SyntaxError> {
    // Acyclic graphs guarantee termination. Follow indirect exports without
    // recursion, including exports of imported bindings classified by parsing.
    loop {
        let node = &graph.nodes[&key];
        let record = node.module.record().expect("JavaScript checked first");
        if let Some(export) = record
            .local_export_entries()
            .iter()
            .find(|e| e.export_name == name)
        {
            return Ok((key, export.local_name.clone()));
        }
        if let Some(export) = record
            .indirect_export_entries()
            .iter()
            .find(|e| e.export_name == name)
        {
            key = requested(node, &export.specifier);
            name = export.import_name.clone();
            continue;
        }
        return Err(error(
            node,
            Span::new(0, 0),
            format!(
                "Could not find export '{name}' in module '{}'",
                node.module.source.name()
            ),
        ));
    }
}

pub(super) fn compile_graph(graph: &ModuleGraph) -> Result<Executable, SyntaxError> {
    let order = post_order(graph)?;
    let mut bindings: HashMap<ModuleKey, Vec<ModuleBinding>> = HashMap::new();
    let mut slots = HashMap::new();
    let mut count = 0usize;
    let mut asm = Assembler::new();
    // Allocate the entire environment before instantiating any functions.
    for key in &order {
        let node = &graph.nodes[key];
        let Some(record) = node.module.record() else {
            return Err(error(
                node,
                Span::new(0, 0),
                "JSON modules cannot be executed yet",
            ));
        };
        let module_bindings = module_bindings(node.module.source.text(), record)?;
        for b in &module_bindings {
            if b.is_import() {
                continue;
            }
            if count == u16::MAX as usize {
                return Err(error(node, record.span(), "too many graph binding cells"));
            }
            let slot = count as u16;
            slots.insert((*key, b.name.name.clone()), slot);
            count += 1;
            asm.op(Opcode::NewCell);
            if b.is_var() {
                asm.op(Opcode::Dup)
                    .op(Opcode::Undefined)
                    .op(Opcode::CellInit);
            }
            asm.set_local(slot);
        }
        bindings.insert(*key, module_bindings);
    }
    // All links are checked before constructing runnable body code.
    for key in &order {
        let node = &graph.nodes[key];
        for import in node.module.record().unwrap().import_entries() {
            let crate::frontend::ImportName::Name(name) = &import.import_name else {
                unreachable!()
            };
            let origin = resolve_export(graph, requested(node, &import.specifier), name.clone())?;
            let slot = slots[&origin];
            slots.insert((*key, import.local_name.clone()), slot);
        }
    }
    // Every module's function declarations exist before the first body.
    for instantiate in [true, false] {
        for key in &order {
            let node = &graph.nodes[key];
            let bindings = &bindings[key];
            let code = lower_module(
                node.module.source.name(),
                node.module.source.text(),
                node.module.record().unwrap(),
                bindings,
                instantiate,
            )?;
            asm.function(&code);
            for b in bindings {
                asm.get_local(slots[&(*key, b.name.name.clone())]);
            }
            asm.call(bindings.len() as u16).op(Opcode::Pop);
        }
    }
    asm.op(Opcode::Undefined).op(Opcode::Return);
    asm.finish("<module graph>", 0, count as u16)
        .map_err(|e| error(&graph.nodes[&graph.entry], Span::new(0, 0), e.to_string()))
}

pub(super) fn failure(message: String) -> Executable {
    let mut asm = Assembler::new();
    asm.op(Opcode::ThrowError)
        .raw(crate::vm::ThrowKind::ModuleLink as u8)
        .raw(0)
        .op(Opcode::Return);
    let span = Span::new(0, message.len());
    Executable::load_compiled(
        asm.into_spec("<module link>", 0, 0),
        Arc::from("<module link>"),
        Arc::from(message),
        vec![
            crate::SourceLocation {
                span,
                line: 1,
                column: 1
            };
            2
        ],
        HashMap::new(),
    )
    .expect("fixed module failure bytecode")
}
