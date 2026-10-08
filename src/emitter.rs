//! Recursive JavaScript emission from resolved expressions; no name lookup.

use std::fmt::Write;

use crate::analyzer::{ExprContext, ExprId, ExprKind, GlobalId, LocalId, Program};

/// Emit one JavaScript expression whose value is the last source form (or nil).
/// The result is self-contained, including its ClojureScript truth predicate.
pub fn emit_js(program: &Program) -> String {
    let mut emitter = Emitter {
        program,
        output: String::new(),
        indent: 1,
    };
    emitter.output.push_str("(() => {\n  \"use strict\";\n  const truth = (value) => value != null && value !== false;\n");
    if !program.js_globals.is_empty() {
        emitter.output.push_str("  const host = (name) => { if (!(name in globalThis)) throw new ReferenceError(name + \" is not defined\"); return globalThis[name]; };\n");
    }
    for global in 0..program.globals.len() {
        writeln!(emitter.output, "  let g{global};").unwrap();
    }
    emitter.emit(program.root, ExprContext::Return);
    emitter.output.push_str("})()\n");
    emitter.output
}

struct Emitter<'a> {
    program: &'a Program,
    output: String,
    indent: usize,
}

impl Emitter<'_> {
    fn indentation(&mut self) {
        for _ in 0..self.indent {
            self.output.push_str("  ");
        }
    }

    fn local(&mut self, id: LocalId) {
        write!(self.output, "l{}", id.0).unwrap();
    }

    fn global(&mut self, id: GlobalId) {
        write!(self.output, "g{}", id.0).unwrap();
    }

    fn member(&mut self, name: &str) {
        // Analyzer validates an ASCII identifier; quotes cannot reach this path.
        write!(self.output, "[\"{name}\"]").unwrap();
    }

    fn arguments(&mut self, args: &[ExprId]) {
        for (index, &arg) in args.iter().enumerate() {
            if index > 0 {
                self.output.push_str(", ");
            }
            self.value(arg);
        }
    }

    fn emit(&mut self, id: ExprId, context: ExprContext) {
        if context == ExprContext::Value {
            self.value(id);
            return;
        }
        match &self.program.exprs[id.0].kind {
            ExprKind::If { test, yes, no } => {
                self.indentation();
                self.output.push_str("if (truth(");
                self.value(*test);
                self.output.push_str(")) {\n");
                self.indent += 1;
                self.emit(*yes, context);
                self.indent -= 1;
                self.indentation();
                self.output.push_str("} else {\n");
                self.indent += 1;
                self.emit(*no, context);
                self.indent -= 1;
                self.indentation();
                self.output.push_str("}\n");
            }
            ExprKind::Do(exprs) => {
                if exprs.is_empty() && context == ExprContext::Return {
                    self.indentation();
                    self.output.push_str("return null;\n");
                }
                for (index, &expr) in exprs.iter().enumerate() {
                    self.emit(
                        expr,
                        if index + 1 == exprs.len() {
                            context
                        } else {
                            ExprContext::Discard
                        },
                    );
                }
            }
            ExprKind::Let { bindings, body } => {
                self.indentation();
                self.output.push_str("{\n");
                self.indent += 1;
                for &(local, init) in bindings {
                    self.indentation();
                    self.output.push_str("const ");
                    self.local(local);
                    self.output.push_str(" = ");
                    self.value(init);
                    self.output.push_str(";\n");
                }
                self.emit(*body, context);
                self.indent -= 1;
                self.indentation();
                self.output.push_str("}\n");
            }
            _ => {
                self.indentation();
                if context == ExprContext::Return {
                    self.output.push_str("return ");
                }
                self.value(id);
                self.output.push_str(";\n");
            }
        }
    }

    fn value(&mut self, id: ExprId) {
        match &self.program.exprs[id.0].kind {
            ExprKind::Nil => self.output.push_str("null"),
            ExprKind::Bool(value) => self.output.push_str(if *value { "true" } else { "false" }),
            ExprKind::Number(value) => write!(self.output, "{value}").unwrap(),
            ExprKind::String(id) => {
                self.output.push('"');
                for &unit in &self.program.strings[id.0] {
                    match unit {
                        0x22 => self.output.push_str("\\\""),
                        0x5c => self.output.push_str("\\\\"),
                        0x20..=0x7e => self.output.push(char::from_u32(unit.into()).unwrap()),
                        _ => write!(self.output, "\\u{unit:04x}").unwrap(),
                    }
                }
                self.output.push('"');
            }
            ExprKind::Local(local) => self.local(*local),
            ExprKind::Global(global) => self.global(*global),
            ExprKind::JsGlobal(global) => {
                let path = &self.program.js_globals[global.0].path;
                write!(self.output, "host(\"{}\")", path[0]).unwrap();
                for part in &path[1..] {
                    self.member(part);
                }
            }
            ExprKind::Def { global, init } => {
                if let Some(init) = init {
                    self.output.push('(');
                    self.global(*global);
                    self.output.push_str(" = ");
                    self.value(*init);
                    self.output.push(')');
                } else {
                    self.output.push_str("void 0");
                }
            }
            ExprKind::Property { receiver, property } => {
                self.output.push('(');
                self.value(*receiver);
                self.output.push(')');
                self.member(property);
            }
            ExprKind::MethodCall {
                receiver,
                method,
                args,
            } => {
                self.output.push('(');
                self.value(*receiver);
                self.output.push(')');
                self.member(method);
                self.output.push('(');
                self.arguments(args);
                self.output.push(')');
            }
            ExprKind::If { test, yes, no } => {
                self.output.push_str("(truth(");
                self.value(*test);
                self.output.push_str(") ? ");
                self.value(*yes);
                self.output.push_str(" : ");
                self.value(*no);
                self.output.push(')');
            }
            ExprKind::Do(_) | ExprKind::Let { .. } => {
                self.output.push_str("(() => {\n");
                self.indent += 1;
                self.emit(id, ExprContext::Return);
                self.indent -= 1;
                self.indentation();
                self.output.push_str("})()");
            }
            ExprKind::Fn { name, params, body } => {
                self.output.push_str("(function");
                if let Some(name) = name {
                    self.output.push(' ');
                    self.local(*name);
                }
                self.output.push('(');
                for (index, &param) in params.iter().enumerate() {
                    if index > 0 {
                        self.output.push_str(", ");
                    }
                    self.local(param);
                }
                self.output.push_str(") {\n");
                self.indent += 1;
                self.emit(*body, ExprContext::Return);
                self.indent -= 1;
                self.indentation();
                self.output.push_str("})");
            }
            ExprKind::Call { callee, args } => {
                self.output.push('(');
                self.value(*callee);
                if matches!(self.program.exprs[callee.0].kind, ExprKind::JsGlobal(_)) {
                    // A JS path is a host invocation and retains its final receiver.
                    self.output.push_str(")(");
                } else {
                    self.output.push_str(").call(null");
                    if !args.is_empty() {
                        self.output.push_str(", ");
                    }
                }
                self.arguments(args);
                self.output.push(')');
            }
        }
    }
}
