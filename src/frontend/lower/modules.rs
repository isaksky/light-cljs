//! Module code uses ordinary VM calls with private cell parameters. The
//! instantiation and body entry points receive the same task-owned cells;
//! nested closures capture those cells through the ordinary precise-GC path.

use super::*;
use crate::frontend::ast::ModuleItem;
use crate::frontend::{ImportName, ModuleRecord};

#[derive(Clone, Debug)]
pub(crate) struct ModuleBinding {
    pub name: Ident,
    kind: BindingKind,
}

impl ModuleBinding {
    pub fn is_import(&self) -> bool {
        self.kind == BindingKind::Import
    }
    pub fn is_var(&self) -> bool {
        self.kind == BindingKind::Var
    }
}

pub(crate) fn module_bindings(
    source: &str,
    record: &ModuleRecord,
) -> Result<Vec<ModuleBinding>, SyntaxError> {
    let mut bindings = Vec::new();
    for import in record.import_entries() {
        if import.import_name == ImportName::Namespace {
            return Err(SyntaxError::new(
                source,
                SyntaxErrorKind::Unsupported,
                import.span,
                "namespace imports cannot be executed yet",
            ));
        }
        bindings.push(ModuleBinding {
            name: Ident {
                name: import.local_name.clone(),
                span: import.span,
            },
            kind: BindingKind::Import,
        });
    }
    let mut body = Vec::new();
    for item in record.body() {
        let ModuleItem::Statement(stmt) = item else {
            return Err(SyntaxError::new(
                source,
                SyntaxErrorKind::Unsupported,
                record.span(),
                "default exports cannot be executed yet",
            ));
        };
        match &stmt.kind {
            StmtKind::Lexical(decl) => {
                for name in decl.declarators.iter().flat_map(|d| d.name.names()) {
                    bindings.push(ModuleBinding {
                        name: name.clone(),
                        kind: match decl.kind {
                            LexicalKind::Let => BindingKind::Let,
                            LexicalKind::Const | LexicalKind::Using => BindingKind::Const,
                        },
                    });
                }
            }
            StmtKind::FunctionDecl(f) => bindings.push(ModuleBinding {
                name: f.name.clone().expect("module function declaration"),
                kind: BindingKind::Function,
            }),
            _ => {}
        }
        body.push(stmt.clone());
    }
    let mut vars = Vec::new();
    var_names(&body, &mut vars);
    for name in vars {
        if !bindings.iter().any(|b| b.name.name == name.name) {
            bindings.push(ModuleBinding {
                name: name.clone(),
                kind: BindingKind::Var,
            });
        }
    }
    if bindings.len() > MAX_COUNT {
        return Err(SyntaxError::new(
            source,
            SyntaxErrorKind::TooLarge,
            record.span(),
            "too many module bindings",
        ));
    }
    let lower = Lowerer::new(source, None);
    for binding in &bindings {
        lower.check_not_reserved(&binding.name)?;
    }
    Ok(bindings)
}

pub(crate) fn lower_module(
    name: &str,
    source: &str,
    record: &ModuleRecord,
    bindings: &[ModuleBinding],
    instantiate: bool,
) -> Result<Executable, SyntaxError> {
    let lower = |facts| {
        let mut lower = Lowerer::new(source, facts);
        // No script completion/global declarations, implicit arguments or
        // constructor semantics. A plain strict call supplies undefined this.
        lower.enter_function(None, false, true);
        lower.cur_mut().module = true;
        lower.cur_mut().scopes.push(Scope::new());
        for (slot, b) in bindings.iter().enumerate() {
            lower.cur_mut().scopes[0].insert(
                b.name.name.clone(),
                Binding {
                    global: false,
                    global_lexical: false,
                    kind: b.kind,
                    slot: slot as u16,
                    initialized: true,
                    repr: Repr::Cell,
                    key: b.name.span.start,
                },
            );
        }
        lower.cur_mut().next_slot = bindings.len();
        lower.cur_mut().max_slots = bindings.len();
        let mut body = Vec::new();
        for item in record.body() {
            let ModuleItem::Statement(stmt) = item else {
                unreachable!("checked bindings")
            };
            if instantiate {
                if let StmtKind::FunctionDecl(f) = &stmt.kind {
                    let name = lower.declaration_name(f)?;
                    let b = lower.local(name)?;
                    lower.out().op_u16(Opcode::GetLocal, b.slot, f.span);
                    // Always capture the mutable module cell, even for self
                    // recursion: the exporter may replace the function binding.
                    lower.closure(f, None, None)?;
                    lower.out().op(Opcode::CellInit, f.span);
                }
            } else {
                body.push(stmt.clone());
            }
        }
        if !instantiate {
            lower.disposable_body(&body)?;
        }
        lower.out().op(Opcode::Undefined, record.span());
        lower.out().op(Opcode::Return, record.span());
        Ok::<_, SyntaxError>(lower)
    };
    let facts = lower(None)?.facts;
    lower(Some(facts))?.finish_top(name, bindings.len() as u16, 0, JsString::from(""))
}
