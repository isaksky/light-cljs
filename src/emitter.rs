//! Recursive JavaScript emission from resolved expressions; no name lookup.

use std::{collections::HashSet, fmt::Write};

use crate::analyzer::{
    ExprContext, ExprId, ExprKind, FnMethod, GlobalId, LocalId, MethodDispatch, Program,
    RecurTargetId, StringId,
};

/// Emit one JavaScript expression whose value is the last source form (or nil).
/// The result is self-contained, including its ClojureScript truth predicate.
pub fn emit_js(program: &Program) -> String {
    emit_js_mode(program, None, false)
}

/// Emit a function accepting the session canonical runtime as its sole argument.
/// This avoids embedding or recreating runtime brands for every evaluated unit.
pub fn emit_js_unit(program: &Program) -> String {
    emit_unit_mode(program, false)
}

/// Compile-time globals use the session demand registry; all semantic lowering is shared.
pub(crate) fn emit_deferred_unit(program: &Program) -> String {
    emit_unit_mode(program, true)
}

fn emit_unit_mode(program: &Program, demand_roots: bool) -> String {
    let raw = program.strings.iter().map(|s| String::from_utf16_lossy(s)).collect::<String>();
    let mut index = 0;
    let parameter = loop {
        let candidate = format!("__lightSessionRuntime{index}");
        if !raw.contains(&candidate) { break candidate; }
        index += 1;
    };
    format!("({parameter}) => {}", emit_js_mode(program, Some(&parameter), demand_roots))
}

fn emit_js_mode(program: &Program, shared: Option<&str>, demand_roots: bool) -> String {
    let mut emitter = Emitter {
        program,
        demand_roots,
        output: String::new(),
        indent: 1,
        captures: Vec::new(),
        reify_captures: Vec::new(),
        reify_receivers: Vec::new(),
        capture_resets: Vec::new(),
        field_resets: Vec::new(),
        used: HashSet::new(),
        returned_definition: None,
        raw_segments: Vec::new(),
    };
    emitter
        .output
        .push_str("(() => {\n  const truth = (value) => value != null && value !== false;\n");
    let prelude_end = emitter.output.len();
    if !program.js_globals.is_empty()
        || program
            .globals
            .iter()
            .any(|global| global.name.contains('.'))
    {
        emitter.output.push_str("  const host = (name) => { if (!(name in globalThis)) throw new ReferenceError(name + \" is not defined\"); return globalThis[name]; };\n");
    }
    for global in 0..program.globals.len() {
        writeln!(emitter.output, "  let g{global};").unwrap();
    }
    emitter.emit(program.root, ExprContext::Return);
    emitter.output.push_str("})()\n");
    if program.exprs.iter().enumerate().any(|(id, expr)| {
        emitter.used.contains(&ExprId(id)) && (
        matches!(
            expr.kind,
            ExprKind::Keyword(_)
                | ExprKind::Symbol(_)
                | ExprKind::List(_)
                | ExprKind::Vector(_)
                | ExprKind::Map(_)
                | ExprKind::Set(_)
                | ExprKind::CoreFn(_)
                | ExprKind::ReaderLiteral { .. }
                | ExprKind::WithMeta { .. }
                | ExprKind::Var { .. }
                | ExprKind::WithOutStr(_)
                | ExprKind::DefProtocol { .. }
                | ExprKind::ExtendType { .. }
                | ExprKind::ProtocolPredicate { .. }
                | ExprKind::Reify { .. }
                | ExprKind::DefType { .. }
                | ExprKind::Assert { message: Some(_), .. }
                | ExprKind::Case { strict: false, .. }
                | ExprKind::Case { keyword_tests: true, .. }
                | ExprKind::Case { default: None, .. }

        ) || matches!(&expr.kind, ExprKind::Fn { methods, .. } if methods.iter().any(|m| m.rest.is_some()))
        )
    }) {
        let runtime = if let Some(parameter) = shared { format!("  const rt = {parameter};\n") } else {
            format!("  const rt = {};\n", include_str!("js_runtime.js").trim())
        };
        for range in &mut emitter.raw_segments { if range.start >= prelude_end { range.start += runtime.len(); range.end += runtime.len(); } }
        if shared.is_none() {
            let start = prelude_end + runtime.find("(() =>").unwrap();
            emitter.raw_segments.push(start..prelude_end + runtime.len() - 2);
        }
        emitter.output.insert_str(prelude_end, &runtime);
    }

    // Raw JavaScript is an escape hatch, but its user-authored identifiers must
    // not accidentally capture compiler locals or be captured by our helpers.
    if program
        .exprs
        .iter()
        .any(|expr| matches!(expr.kind, ExprKind::JsTemplate { .. }))
    {
        hygienic_js(emitter.output, emitter.raw_segments)
    } else {
        emitter.output
    }
}

struct Emitter<'a> {
    program: &'a Program,
    demand_roots: bool,
    output: String,
    indent: usize,
    captures: Vec<(LocalId, RecurTargetId)>,
    reify_captures: Vec<(LocalId, RecurTargetId)>,
    reify_receivers: Vec<RecurTargetId>,
    capture_resets: Vec<RecurTargetId>,
    field_resets: Vec<RecurTargetId>,
    used: HashSet<ExprId>,
    // Upstream return-context def exits before its following test-property write.
    returned_definition: Option<ExprId>,
    raw_segments: Vec<std::ops::Range<usize>>,
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

    fn local_value(&mut self, id: LocalId) {
        if let Some((_, target)) = self.captures.iter().rev().find(|(local, _)| *local == id) {
            write!(self.output, "captures{}.l{}", target.0, id.0).unwrap();
        } else if let Some((_, target)) = self
            .reify_captures
            .iter()
            .rev()
            .find(|(local, _)| *local == id)
        {
            write!(self.output, "rt.field(self{}, ", target.0).unwrap();
            self.string_units(
                &self.program.locals[id.0]
                    .name
                    .encode_utf16()
                    .collect::<Vec<_>>(),
            );
            self.output.push(')');
        } else if let Some((target, _)) = self.program.field_bindings.get(&id) {
            write!(self.output, "self{}", target.0).unwrap();
            self.member(&crate::analyzer::munge_js_member(
                &self.program.locals[id.0].name,
            ));
        } else {
            self.local(id);
        }
    }

    fn global(&mut self, id: GlobalId) {
        let name = &self.program.globals[id.0].name;
        if name.contains('.') {
            let path = crate::analyzer::munge_js_path(name);
            self.output.push_str("host(");
            self.string_units(&path[0].encode_utf16().collect::<Vec<_>>());
            self.output.push(')');
            for member in &path[1..] {
                self.member(member);
            }
        } else if self.demand_roots {
            self.output.push_str("globalThis[\"__lightCompilerRoots\"][");
            let global = &self.program.globals[id.0];
            self.string_units(&format!("{}/{}", global.namespace, global.name).encode_utf16().collect::<Vec<_>>());
            self.output.push(']');
        } else {
            write!(self.output, "g{}", id.0).unwrap();
        }
    }

    fn compiler_binding(&self, id: ExprId) -> bool {
        self.demand_roots && matches!(self.program.exprs[id.0].kind,
            ExprKind::Global(g) if !self.program.globals[g.0].name.contains('.'))
    }

    fn binding_name(&mut self, id: ExprId) {
        let ExprKind::Global(g) = self.program.exprs[id.0].kind else { unreachable!() };
        let global = &self.program.globals[g.0];
        self.string_units(&format!("{}/{}", global.namespace, global.name).encode_utf16().collect::<Vec<_>>());
    }

    fn assignable(&mut self, id: ExprId) {
        if let ExprKind::JsGlobal(global) = self.program.exprs[id.0].kind {
            self.output.push_str("globalThis");
            for part in &self.program.js_globals[global.0].path {
                self.member(part);
            }
        } else {
            self.value(id);
        }
    }

    fn member(&mut self, name: &str) {
        self.output.push('[');
        self.string_units(&name.encode_utf16().collect::<Vec<_>>());
        self.output.push(']');
    }

    fn arguments(&mut self, args: &[ExprId]) {
        for (index, &arg) in args.iter().enumerate() {
            if index > 0 {
                self.output.push_str(", ");
            }
            self.value(arg);
        }
    }

    fn condition(&mut self, id: ExprId) {
        if let ExprKind::BooleanHint(value) = self.program.exprs[id.0].kind {
            self.value(value);
        } else {
            self.output.push_str("truth(");
            self.value(id);
            self.output.push(')');
        }
    }

    fn string(&mut self, id: StringId) {
        self.string_units(&self.program.strings[id.0]);
    }

    fn string_units(&mut self, units: &[u16]) {
        self.output.push('"');
        for &unit in units {
            match unit {
                0x22 => self.output.push_str("\\\""),
                0x5c => self.output.push_str("\\\\"),
                0x20..=0x7e => self.output.push(char::from_u32(unit.into()).unwrap()),
                _ => write!(self.output, "\\u{unit:04x}").unwrap(),
            }
        }
        self.output.push('"');
    }

    fn emit(&mut self, id: ExprId, context: ExprContext) {
        self.used.insert(id);
        if context == ExprContext::Value {
            self.value(id);
            return;
        }
        match &self.program.exprs[id.0].kind {
            ExprKind::JsTemplate { segments, args } => {
                self.indentation();
                if context == ExprContext::Return {
                    self.output.push_str("return ");
                }
                self.js_template(segments, args);
                self.output.push_str(";\n");
            }
            ExprKind::WithOutStr(body) => {
                self.indentation();
                self.output.push_str("{ let text = \"\"; const savedPrint = rt.core.printFn, savedNewline = rt.core.printNewline;\n");
                self.indent += 1;
                self.indentation();
                self.output.push_str("rt.core.printFn = value => { text += value == null ? \"\" : value; }; rt.core.printNewline = true;\n");
                self.indentation();
                self.output.push_str("try {\n");
                self.indent += 1;
                self.emit(*body, ExprContext::Discard);
                self.indent -= 1;
                self.indentation();
                self.output.push_str("} finally { rt.core.printFn = savedPrint; rt.core.printNewline = savedNewline; }\n");
                if context == ExprContext::Return {
                    self.indentation();
                    self.output.push_str("return text;\n");
                }
                self.indent -= 1;
                self.indentation();
                self.output.push_str("}\n");
            }
            ExprKind::Binding { bindings, body } => {
                self.indentation();
                self.output.push_str("{\n");
                self.indent += 1;
                // Compiler scope saves overlays without demanding the root.
                // Every initializer still sees the outer environment.
                for (i, (target, _)) in bindings.iter().enumerate() {
                    self.indentation();
                    write!(self.output, "const saved{i} = ").unwrap();
                    if self.compiler_binding(*target) {
                        self.output.push_str("globalThis.__lightSaveCompilerBinding(");
                        self.binding_name(*target);
                        self.output.push(')');
                    } else { self.value(*target); }
                    writeln!(self.output, "; let installed{i} = false;").unwrap();
                }
                for (i, (_, init)) in bindings.iter().enumerate() {
                    self.indentation();
                    write!(self.output, "const temporary{i} = ").unwrap();
                    self.value(*init);
                    self.output.push_str(";\n");
                }
                self.indentation();
                self.output.push_str("try {\n");
                self.indent += 1;
                for (i, (target, _)) in bindings.iter().enumerate() {
                    self.indentation();
                    if self.compiler_binding(*target) {
                        self.output.push_str("globalThis.__lightBindCompilerRoot(");
                        self.binding_name(*target);
                        writeln!(self.output, ", temporary{i});").unwrap();
                    } else {
                        self.assignable(*target);
                        writeln!(self.output, " = temporary{i};").unwrap();
                    }
                    self.indentation();
                    writeln!(self.output, "installed{i} = true;").unwrap();
                }
                self.emit(*body, context);
                self.indent -= 1;
                self.indentation();
                self.output.push_str("} finally {\n");
                self.indent += 1;
                for (i, (target, _)) in bindings.iter().enumerate().rev() {
                    self.indentation();
                    write!(self.output, "if (installed{i}) ").unwrap();
                    if self.compiler_binding(*target) {
                        self.output.push_str("globalThis.__lightRestoreCompilerBinding(");
                        self.binding_name(*target);
                        writeln!(self.output, ", saved{i});").unwrap();
                    } else {
                        self.assignable(*target);
                        writeln!(self.output, " = saved{i};").unwrap();
                    }
                }
                self.indent -= 1;
                self.indentation();
                self.output.push_str("}\n");
                self.indent -= 1;
                self.indentation();
                self.output.push_str("}\n");
            }
            ExprKind::Loop {
                target,
                bindings,
                body,
            } => {
                self.indentation();
                self.output.push_str("{\n");
                self.indent += 1;
                // Initializers see preceding lexical bindings, before the fresh
                // per-iteration copies come into scope.
                for &(local, init) in bindings {
                    self.indentation();
                    self.output.push_str("const ");
                    self.local(local);
                    self.output.push_str(" = ");
                    self.value(init);
                    self.output.push_str(";\n");
                }
                self.iteration(*target, *body, context);
                self.indent -= 1;
                self.indentation();
                self.output.push_str("}\n");
            }
            ExprKind::Recur { target, args } => {
                self.indentation();
                self.output.push_str("{\n");
                self.indent += 1;
                for (index, &arg) in args.iter().enumerate() {
                    self.indentation();
                    write!(self.output, "const next{index} = ").unwrap();
                    self.value(arg);
                    self.output.push_str(";\n");
                }
                for index in 0..args.len() {
                    self.indentation();
                    writeln!(self.output, "r{}_{index} = next{index};", target.0).unwrap();
                }
                // Upstream emits non-strict formal parameters: its arguments
                // object aliases supplied parameter slots. Preserve this even
                // though loop iteration locals are copied for closure capture.
                for (index, slot) in self.program.recur_targets[target.0]
                    .argument_slots
                    .iter()
                    .enumerate()
                {
                    if let Some(slot) = slot {
                        self.indentation();
                        writeln!(
                            self.output,
                            "if (arguments.length > {slot}) arguments[{slot}] = next{index};"
                        )
                        .unwrap();
                    }
                }
                if self.field_resets.contains(target) {
                    self.indentation();
                    writeln!(self.output, "self{} = null;", target.0).unwrap();
                }
                if self.capture_resets.contains(target) {
                    self.indentation();
                    writeln!(self.output, "captures{} = null;", target.0).unwrap();
                }
                self.indentation();
                writeln!(self.output, "continue recur{};", target.0).unwrap();
                self.indent -= 1;
                self.indentation();
                self.output.push_str("}\n");
            }
            ExprKind::Throw(value) => {
                self.indentation();
                self.output.push_str("throw ");
                self.value(*value);
                self.output.push_str(";\n");
            }
            ExprKind::Try {
                body,
                catches,
                finally,
            } => {
                if catches.is_empty() && finally.is_none() {
                    self.emit(*body, context);
                    return;
                }
                self.indentation();
                self.output.push_str("try {\n");
                self.indent += 1;
                self.emit(*body, context);
                self.indent -= 1;
                self.indentation();
                self.output.push('}');
                if !catches.is_empty() {
                    self.output.push_str(" catch (caught) {\n");
                    self.indent += 1;
                    for (index, catch) in catches.iter().enumerate() {
                        self.indentation();
                        self.output
                            .push_str(if index == 0 { "if (" } else { "else if (" });
                        if let Some(class) = catch.class {
                            self.output.push_str("caught instanceof ");
                            self.value(class);
                        } else {
                            self.output.push_str("true");
                        }
                        self.output.push_str(") {\n");
                        self.indent += 1;
                        self.indentation();
                        self.output.push_str("const ");
                        self.local(catch.binding);
                        self.output.push_str(" = caught;\n");
                        self.emit(catch.body, context);
                        self.indent -= 1;
                        self.indentation();
                        self.output.push_str("}\n");
                    }
                    self.indentation();
                    self.output.push_str("else { throw caught; }\n");
                    self.indent -= 1;
                    self.indentation();
                    self.output.push('}');
                }
                if let Some(finally) = finally {
                    self.output.push_str(" finally {\n");
                    self.indent += 1;
                    self.emit(*finally, ExprContext::Discard);
                    self.indent -= 1;
                    self.indentation();
                    self.output.push('}');
                }
                self.output.push('\n');
            }
            ExprKind::Case {
                strict,
                keyword_tests,
                test,
                branches,
                default,
            } => {
                self.indentation();
                self.output.push_str("{ const caseValue = ");
                self.value(*test);
                self.output.push_str(";\n");
                self.indent += 1;
                let switch = *strict || *keyword_tests;
                if switch {
                    self.indentation();
                    self.output.push_str(if *keyword_tests {
                        "switch (rt.core.isKeyword(caseValue) ? rt.core.str(caseValue).slice(1) : null) {\n"
                    } else { "switch (caseValue) {\n" });
                    self.indent += 1;
                }
                for (index, &(constant, branch)) in branches.iter().enumerate() {
                    self.indentation();
                    if switch {
                        self.output.push_str("case ");
                        if *keyword_tests {
                            let ExprKind::Keyword(name) = self.program.exprs[constant.0].kind
                            else {
                                unreachable!()
                            };
                            let name = &self.program.named_literals[name.0];
                            let mut spelling = Vec::new();
                            if let Some(namespace) = name.namespace {
                                spelling.extend_from_slice(&self.program.strings[namespace.0]);
                                spelling.push('/' as u16);
                            }
                            spelling.extend_from_slice(&self.program.strings[name.name.0]);
                            self.string_units(&spelling);
                        } else {
                            self.value(constant);
                        }
                        self.output.push_str(": {\n");
                    } else {
                        self.output
                            .push_str(if index == 0 { "if (" } else { "else if (" });
                        self.output.push_str("rt.core.equal(caseValue, ");
                        self.value(constant);
                        self.output.push_str(")) {\n");
                    }
                    self.indent += 1;
                    self.emit(branch, context);
                    if switch {
                        self.indentation();
                        self.output.push_str("break;\n");
                    }
                    self.indent -= 1;
                    self.indentation();
                    self.output.push_str("}\n");
                }
                self.indentation();
                self.output.push_str(if switch {
                    "default: {\n"
                } else if branches.is_empty() {
                    "{\n"
                } else {
                    "else {\n"
                });
                self.indent += 1;
                if let Some(default) = default {
                    self.emit(*default, context);
                } else {
                    self.indentation();
                    self.output.push_str(if *keyword_tests {
                        "throw new Error(\"No matching clause: \" + (rt.core.isKeyword(caseValue) ? rt.core.str(caseValue).slice(1) : \"\"));\n"
                    } else {
                        "throw new Error(\"No matching clause: \" + rt.core.str(caseValue));\n"
                    });
                }
                self.indent -= 1;
                self.indentation();
                self.output.push_str("}\n");
                if switch {
                    self.indent -= 1;
                    self.indentation();
                    self.output.push_str("}\n");
                }
                self.indent -= 1;
                self.indentation();
                self.output.push_str("}\n");
            }
            ExprKind::If { test, yes, no } => {
                self.indentation();
                self.output.push_str("if (");
                self.condition(*test);
                self.output.push_str(") {\n");
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
            ExprKind::ThisAs { binding, body } => {
                self.indentation();
                self.output.push_str("{ const ");
                self.local(*binding);
                self.output
                    .push_str(" = (this == null ? globalThis : Object(this));\n");
                self.indent += 1;
                self.emit(*body, context);
                self.indent -= 1;
                self.indentation();
                self.output.push_str("}\n");
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
            ExprKind::LetFn { bindings, body } => {
                self.indentation();
                self.output.push_str("{\n");
                self.indent += 1;
                // Each entry into the block creates fresh slots, including
                // within recur iterations. Closures capture these slots, so
                // every peer is visible regardless of declaration order.
                for &(local, _) in bindings {
                    self.indentation();
                    self.output.push_str("let ");
                    self.local(local);
                    self.output.push_str(";\n");
                }
                for &(local, init) in bindings {
                    self.indentation();
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
                let previous = self.returned_definition;
                self.returned_definition = (context == ExprContext::Return).then_some(id);
                self.value(id);
                self.returned_definition = previous;
                self.output.push_str(";\n");
            }
        }
    }

    fn iteration(&mut self, target: RecurTargetId, body: ExprId, context: ExprContext) {
        let frame = &self.program.recur_targets[target.0];
        for (index, &param) in frame.params.iter().enumerate() {
            self.indentation();
            write!(self.output, "let r{}_{index} = ", target.0).unwrap();
            self.local(param);
            self.output.push_str(";\n");
        }
        self.indentation();
        writeln!(self.output, "recur{}: while (true) {{", target.0).unwrap();
        self.indent += 1;
        for (index, &param) in frame.params.iter().enumerate() {
            self.indentation();
            self.output.push_str("const ");
            self.local(param);
            if frame.receiver_from_this && index == 0 {
                self.output.push_str(" = this;\n");
            } else {
                writeln!(self.output, " = r{}_{index};", target.0).unwrap();
            }
        }
        self.emit(body, context);
        if context == ExprContext::Discard {
            self.indentation();
            self.output.push_str("break;\n");
        }
        self.indent -= 1;
        self.indentation();
        self.output.push_str("}\n");
    }

    fn method(&mut self, name: Option<LocalId>, method: &FnMethod) {
        self.method_with_ifn_captures(name, method, false);
    }

    fn method_with_ifn_captures(
        &mut self,
        name: Option<LocalId>,
        method: &FnMethod,
        ifn_call: bool,
    ) {
        self.output.push_str("(function");
        if let Some(name) = name {
            self.output.push(' ');
            self.local(name);
        }
        self.output.push('(');
        for (index, param) in method.params.iter().chain(method.rest.iter()).enumerate() {
            if index > 0 {
                self.output.push_str(", ");
            }
            self.local(*param);
        }
        self.output.push_str(") {\n");
        self.indent += 1;
        let field_reset = ifn_call
            && self.program.type_methods.contains(&method.target)
            && self.program.recur_targets[method.target.0].used;
        if field_reset {
            self.field_resets.push(method.target);
        }
        if self.reify_receivers.contains(&method.target)
            || self.program.type_methods.contains(&method.target)
        {
            self.indentation();
            writeln!(
                self.output,
                "{} self{} = this;",
                if field_reset { "let" } else { "const" },
                method.target.0
            )
            .unwrap();
            if let Some(receiver) = method.params.first() {
                self.indentation();
                self.local(*receiver);
                self.output.push_str(" = this;\n");
            }
        }
        if self.program.recur_targets[method.target.0].receiver_from_this
            && !self.reify_receivers.contains(&method.target)
            && !self.program.type_methods.contains(&method.target)
            && let Some(receiver) = method.params.first()
        {
            // Ordinary extensions rebind the explicit receiver without gaining
            // the hidden field receiver reserved for annotated type methods.
            self.indentation();
            self.local(*receiver);
            self.output.push_str(" = this;\n");
        }
        let previous_captures = self.captures.len();
        // In the pinned CLJS IFn .call adapter, the implicit receiver parameter
        // aliases the hidden field-access receiver and recur sets it to nil.
        // Captured outer-local reads then throw; the direct invoke entry keeps
        // its receiver. Preserve this observable distinction, including reads
        // delayed inside a returned closure.
        let capture_reset = ifn_call
            && self.program.recur_targets[method.target.0].used
            && !method.captures.is_empty();
        if capture_reset {
            self.indentation();
            write!(self.output, "let captures{} = {{", method.target.0).unwrap();
            for (index, &capture) in method.captures.iter().enumerate() {
                if index > 0 {
                    self.output.push_str(", ");
                }
                let reify = self.reify_receivers.contains(&method.target);
                if reify {
                    self.output.push_str("get ");
                }
                self.local(capture);
                self.output
                    .push_str(if reify { "() { return " } else { ": " });
                self.local_value(capture);
                if reify {
                    self.output.push_str("; }");
                }
            }
            self.output.push_str("};\n");
            self.captures
                .extend(method.captures.iter().map(|&id| (id, method.target)));
            self.capture_resets.push(method.target);
        }
        if self.program.recur_targets[method.target.0].used {
            self.iteration(method.target, method.body, ExprContext::Return);
        } else {
            self.emit(method.body, ExprContext::Return);
        }
        self.captures.truncate(previous_captures);
        if field_reset {
            self.field_resets.pop();
        }
        if capture_reset {
            self.capture_resets.pop();
        }
        self.indent -= 1;
        self.indentation();
        self.output.push_str("})");
    }

    fn function(&mut self, name: Option<LocalId>, methods: &[FnMethod], macro_function: bool) {
        if methods.len() == 1 && methods[0].rest.is_none() {
            // A single fixed method deliberately retains ordinary JS missing/
            // extra argument behavior, with no dispatcher or runtime prelude.
            self.method(name, &methods[0]);
            return;
        }
        self.output.push_str("(() => {\n");
        self.indent += 1;
        for (index, method) in methods.iter().enumerate() {
            self.indentation();
            write!(self.output, "const m{index} = ").unwrap();
            self.method(None, method);
            self.output.push_str(";\n");
        }
        let function = name.map_or_else(|| "f".into(), |name| format!("l{}", name.0));
        self.indentation();
        write!(self.output, "const {function} = function(").unwrap();
        let count = if macro_function {
            1
        } else {
            methods
                .iter()
                .map(|m| m.params.len() + usize::from(m.rest.is_some()))
                .max()
                .unwrap()
        };
        for index in 0..count {
            if index > 0 {
                self.output.push_str(", ");
            }
            write!(self.output, "a{index}").unwrap();
        }
        self.output.push_str(") {\n");
        self.indent += 1;
        self.indentation();
        self.output.push_str("switch (arguments.length) {\n");
        self.indent += 1;
        for (index, method) in methods.iter().enumerate().filter(|(_, m)| m.rest.is_none()) {
            self.indentation();
            writeln!(
                self.output,
                "case {}: return m{index}.apply(this, arguments);",
                method.params.len()
            )
            .unwrap();
        }
        self.indentation();
        if let Some((index, method)) = methods.iter().enumerate().find(|(_, m)| m.rest.is_some()) {
            // Like CLJS, the variadic default also receives underfilled calls.
            write!(self.output, "default: return m{index}.call(this, ").unwrap();
            for i in 0..method.params.len() {
                write!(self.output, "arguments[{i}], ").unwrap();
            }
            writeln!(
                self.output,
                "rt.fnRest(arguments, {}));",
                method.params.len()
            )
            .unwrap();
        } else {
            self.output.push_str(if macro_function {
                "default: throw new Error(\"Invalid arity: \" + (arguments.length - 2));\n"
            } else {
                "default: throw new Error(\"Invalid arity: \" + arguments.length);\n"
            });
        }
        self.indent -= 1;
        self.indentation();
        self.output.push_str("}\n");
        self.indent -= 1;
        self.indentation();
        self.output.push_str("};\n");
        for (index, method) in methods.iter().enumerate() {
            self.indentation();
            if method.rest.is_some() {
                writeln!(
                    self.output,
                    "{function}.cljs$core$IFn$_invoke$arity$variadic = m{index};"
                )
                .unwrap();
                self.indentation();
                writeln!(
                    self.output,
                    "{function}.cljs$lang$maxFixedArity = {};",
                    method.params.len()
                )
                .unwrap();
                self.indentation();
                writeln!(
                    self.output,
                    "{function}.cljs$lang$applyTo = (args) => rt.fnApplyTo(m{index}, {}, args);",
                    method.params.len()
                )
                .unwrap();
            } else {
                writeln!(
                    self.output,
                    "{function}.cljs$core$IFn$_invoke$arity${} = m{index};",
                    method.params.len()
                )
                .unwrap();
            }
        }
        if macro_function && methods.iter().all(|method| method.rest.is_none()) {
            self.indentation();
            writeln!(
                self.output,
                "{function}.cljs$lang$maxFixedArity = {};",
                methods
                    .iter()
                    .map(|method| method.params.len())
                    .max()
                    .unwrap()
            )
            .unwrap();
        }
        self.indentation();
        writeln!(self.output, "return {function};").unwrap();
        self.indent -= 1;
        self.indentation();
        self.output.push_str("})()");
    }

    fn object_methods(&mut self, methods: &[(MethodDispatch, FnMethod)]) {
        self.output.push('[');
        for (index, (dispatch, method)) in methods.iter().enumerate() {
            if index > 0 {
                self.output.push_str(", ");
            }
            self.output.push('[');
            let ifn = match dispatch {
                MethodDispatch::Protocol(key) => {
                    self.string_units(&key.encode_utf16().collect::<Vec<_>>());
                    false
                }
                MethodDispatch::Builtin(name) => {
                    self.string_units(&name.encode_utf16().collect::<Vec<_>>());
                    name == "IFn/-invoke"
                }
            };
            write!(
                self.output,
                ", {}, ",
                method.params.len() + if method.rest.is_some() { 2 } else { 0 }
            )
            .unwrap();
            if ifn {
                self.method_with_ifn_captures(None, method, true);
            } else {
                self.function(None, std::slice::from_ref(method), false);
            }
            if ifn {
                self.output.push_str(", ");
                self.method(None, method);
            }
            if method.rest.is_some()
                && matches!(dispatch, MethodDispatch::Builtin(name) if name.starts_with("Object/"))
            {
                write!(self.output, ", null, {}", method.params.len()).unwrap();
            }
            self.output.push(']');
        }
        self.output.push(']');
    }

    fn js_template(&mut self, segments: &[String], args: &[ExprId]) {
        for (index, segment) in segments.iter().enumerate() {
            let start = self.output.len();
            self.output.push_str(segment);
            self.raw_segments.push(start..self.output.len());
            if let Some(&arg) = args.get(index) {
                self.value(arg);
            }
        }
    }

    fn value(&mut self, id: ExprId) {
        self.used.insert(id);
        match &self.program.exprs[id.0].kind {
            ExprKind::Nil => self.output.push_str("null"),
            ExprKind::Construct { callee, args } => {
                self.output.push_str("new ");
                // Host resolution is an implementation call, not a source call.
                // Preserve upstream new/call precedence for actual expressions.
                let host = matches!(self.program.exprs[callee.0].kind, ExprKind::JsGlobal(_));
                if host {
                    self.output.push('(');
                }
                self.value(*callee);
                if host {
                    self.output.push(')');
                }
                self.output.push('(');
                for (index, arg) in args.iter().enumerate() {
                    if index > 0 {
                        self.output.push_str(", ");
                    }
                    self.value(*arg);
                }
                self.output.push(')');
            }
            ExprKind::Assert {
                test,
                message,
                form,
            } => {
                self.output.push_str("(() => { if (!truth(");
                self.value(*test);
                self.output
                    .push_str(")) throw new Error(\"Assert failed: \" + ");
                if let Some(message) = message {
                    self.output.push_str("rt.core.str(");
                    self.value(*message);
                    self.output.push_str(") + \"\\n\" + ");
                }
                self.string(*form);
                self.output.push_str("); return null; })()");
            }
            ExprKind::Reify {
                metadata,
                captures,
                methods,
            } => {
                write!(self.output, "rt.reify({}, [", id.0).unwrap();
                for (index, local) in captures.iter().enumerate() {
                    if index > 0 {
                        self.output.push_str(", ");
                    }
                    self.string_units(
                        &self.program.locals[local.0]
                            .name
                            .encode_utf16()
                            .collect::<Vec<_>>(),
                    );
                }
                self.output.push_str("], [");
                for (index, local) in captures.iter().enumerate() {
                    if index > 0 {
                        self.output.push_str(", ");
                    }
                    self.local_value(*local);
                }
                self.output.push_str("], () => ");
                let old_captures = self.reify_captures.len();
                let old_receivers = self.reify_receivers.len();
                // Each method closes over its own receiver. Nested functions
                // keep that receiver even after the method returns.
                self.output.push('[');
                for (index, method) in methods.iter().enumerate() {
                    if index > 0 {
                        self.output.push_str(", ...");
                    } else {
                        self.output.push_str("...");
                    }
                    self.reify_captures
                        .extend(captures.iter().map(|&local| (local, method.1.target)));
                    self.reify_receivers.push(method.1.target);
                    let enclosing_resets = self.captures.clone();
                    self.captures.retain(|(local, _)| !captures.contains(local));
                    self.object_methods(std::slice::from_ref(method));
                    self.captures = enclosing_resets;
                    self.reify_captures.truncate(old_captures);
                    self.reify_receivers.truncate(old_receivers);
                }
                self.output.push_str("], ");
                if let Some(metadata) = metadata {
                    self.value(*metadata);
                } else {
                    self.output.push_str("null");
                }
                self.output.push(')');
            }
            ExprKind::DefType {
                record,
                global,
                constructor,
                body,
                masks,
                map_constructor,
                fields,
                methods,
            } => {
                self.output.push_str("(() => { ");
                self.global(*global);
                self.output.push_str(" = rt.makeType(");
                self.output.push('[');
                for (index, field) in fields.iter().enumerate() {
                    if index > 0 {
                        self.output.push_str(", ");
                    }
                    self.string_units(&field.encode_utf16().collect::<Vec<_>>());
                }
                self.output.push_str("], ");
                self.object_methods(methods);
                self.output.push_str(if *record {
                    if constructor.is_some() {
                        ", true"
                    } else {
                        ", \"raw\""
                    }
                } else {
                    ", false"
                });
                if let Some(masks) = masks {
                    self.output.push_str(", ");
                    self.value(*masks);
                } else {
                    self.output.push_str(", null");
                }
                self.output.push_str(", ");
                let binding = &self.program.globals[global.0];
                self.string_units(
                    &format!("{}/{}", binding.namespace, binding.name)
                        .encode_utf16()
                        .collect::<Vec<_>>(),
                );
                if *record && constructor.is_some() {
                    self.output.push_str(", () => ");
                    self.global(*global);
                }
                self.output.push_str("); ");
                if let Some(constructor) = constructor {
                    self.global(*constructor);
                    self.output.push_str(" = (...args) => new ");
                    self.global(*global);
                    if *record {
                        self.output.push_str("(...Array.from({length: ");
                        self.output.push_str(&fields.len().to_string());
                        self.output
                            .push_str("}, (_, i) => args[i]), null, null, null); ");
                    } else {
                        self.output.push_str("(...args); ");
                    }
                    self.global(*global);
                    self.output.push_str(".getBasis = () => rt.core.vector(");
                    for (index, field) in fields.iter().enumerate() {
                        if index > 0 {
                            self.output.push_str(", ");
                        }
                        self.output.push_str("rt.symbol(null, ");
                        self.string_units(&field.encode_utf16().collect::<Vec<_>>());
                        self.output.push(')');
                    }
                    self.output.push_str("); ");
                    self.global(*global);
                    self.output.push_str(".cljs$lang$type = true; ");
                    if !*record {
                        self.global(*global);
                        self.output.push_str(".cljs$lang$ctorStr = ");
                        let binding = &self.program.globals[global.0];
                        self.string_units(
                            &format!("{}/{}", binding.namespace, binding.name)
                                .encode_utf16()
                                .collect::<Vec<_>>(),
                        );
                        self.output.push_str("; ");
                    }
                }
                if let Some(map_constructor) = map_constructor {
                    self.global(*map_constructor);
                    self.output.push_str(" = value => rt.recordFromMap(");
                    self.global(*global);
                    self.output.push_str(", value); ");
                }
                self.output.push_str("return ");
                if let Some(body) = body {
                    self.value(*body);
                } else {
                    self.global(*global);
                }
                self.output.push_str("; })()");
            }

            ExprKind::JsArguments => self.output.push_str("arguments"),
            ExprKind::JsTemplate { segments, args } => {
                self.output.push('(');
                self.js_template(segments, args);
                self.output.push(')');
            }
            ExprKind::JsIndex { receiver, indices } => {
                self.output.push('(');
                self.value(*receiver);
                self.output.push(')');
                for index in indices {
                    self.output.push('[');
                    self.value(*index);
                    self.output.push(']');
                }
            }
            ExprKind::JsDelete { receiver, key } => {
                self.output.push_str("(delete (");
                self.value(*receiver);
                self.output.push_str(")[");
                self.value(*key);
                self.output.push_str("])");
            }
            ExprKind::QualifiedMethod { class, method } => {
                if let Some(method) = method {
                    self.output
                        .push_str("(function (x, ...args) { return Reflect.apply(");
                    self.value(*class);
                    self.output.push_str(".prototype");
                    self.member(method);
                    self.output.push_str(", x, args); })");
                } else {
                    self.output
                        .push_str("(function (...args) { return Reflect.construct(");
                    self.value(*class);
                    self.output.push_str(", args); })");
                }
            }
            ExprKind::CopyArguments(dest) => {
                self.output.push_str("((dest) => { for (let i = 0; i < arguments.length; i++) dest.push(arguments[i]); return null; })(");
                self.value(*dest);
                self.output.push(')');
            }
            ExprKind::DefProtocol {
                global,
                methods,
                via_metadata,
            } => {
                self.output.push_str("(() => { ");
                self.global(*global);
                self.output.push_str(" = rt.protocol(");
                let binding = &self.program.globals[global.0];
                self.string_units(
                    &format!("{}/{}", binding.namespace, binding.name)
                        .encode_utf16()
                        .collect::<Vec<_>>(),
                );
                self.output.push_str("); ");
                for method in methods {
                    self.global(method.global);
                    self.output.push_str(" = rt.protocolMethod(");
                    self.string_units(&method.key.encode_utf16().collect::<Vec<_>>());
                    self.output.push_str(", ");
                    let b = &self.program.globals[method.global.0];
                    self.string_units(
                        &format!("{}/{}", b.namespace, b.name)
                            .encode_utf16()
                            .collect::<Vec<_>>(),
                    );
                    write!(
                        self.output,
                        ", {:?}, {}, {}, () => ",
                        method.arities,
                        via_metadata,
                        method
                            .variadic
                            .map_or_else(|| "null".into(), |n| n.to_string())
                    )
                    .unwrap();
                    self.global(method.global);
                    self.output.push_str("); ");
                }
                self.output.push_str("return undefined; })()");
            }
            ExprKind::ProtocolPredicate {
                protocol,
                value,
                key,
                direct,
            } => {
                self.output.push_str("rt.protocolPredicate(");
                if *direct {
                    self.output.push_str("null");
                } else {
                    self.value(*protocol);
                }
                self.output.push_str(", ");
                self.string_units(&key.encode_utf16().collect::<Vec<_>>());
                self.output.push_str(", ");
                self.value(*value);
                write!(self.output, ", {})", direct).unwrap();
            }
            ExprKind::ExtendType {
                instance,
                return_target,
                target,
                methods,
            } => {
                self.output.push_str(if *instance {
                    "rt.extendInstance("
                } else {
                    "rt.extendType("
                });
                self.value(*target);
                self.output.push_str(", ");
                self.object_methods(methods);
                if *instance {
                    write!(self.output, ", {return_target}").unwrap();
                }
                self.output.push(')');
            }
            ExprKind::Bool(value) => self.output.push_str(if *value { "true" } else { "false" }),
            ExprKind::Number(value) => {
                if value.is_nan() {
                    self.output.push_str("NaN");
                } else if *value == f64::INFINITY {
                    self.output.push_str("Infinity");
                } else if *value == f64::NEG_INFINITY {
                    self.output.push_str("-Infinity");
                } else {
                    write!(self.output, "{value}").unwrap();
                }
            }
            ExprKind::String(id) => self.string(*id),
            ExprKind::ReaderLiteral { kind, items } => {
                use crate::compiler_data::CompilerLiteralKind as K;
                self.output.push_str(match kind {
                    K::BigInteger => unreachable!("integers lower to Number"),
                    K::Regex => "rt.core.rePattern(",
                    K::Instant => "new Date(",
                    K::Uuid => "rt.core.uuid(",
                    K::JsArray => "[",
                    K::JsObject => "rt.jsObject([",
                    K::Queue => "rt.queueFromArray([",
                });
                self.arguments(items);
                self.output.push_str(match kind {
                    K::JsArray => "]",
                    K::JsObject | K::Queue => "])",
                    _ => ")",
                });
            }
            ExprKind::WithMeta { value, metadata } => {
                self.output.push_str("rt.core.withMeta(");
                self.value(*value);
                self.output.push_str(", ");
                self.value(*metadata);
                self.output.push(')');
            }
            ExprKind::Var {
                definition,
                value,
                symbol,
                metadata,
            } => {
                self.output.push_str(if *definition {
                    "rt.definitionVar(() => "
                } else {
                    "rt.makeVar(() => "
                });
                self.value(*value);
                self.output.push_str(", rt.symbol(");
                let literal = &self.program.named_literals[symbol.0];
                if let Some(namespace) = literal.namespace {
                    self.string(namespace);
                } else {
                    self.output.push_str("null");
                }
                self.output.push_str(", ");
                self.string(literal.name);
                self.output.push(')');
                if let Some(metadata) = metadata {
                    self.output.push_str(", ");
                    self.value(*metadata);
                }
                if self.demand_roots && *definition {
                    if metadata.is_none() { self.output.push_str(", null"); }
                    self.output.push_str(", true");
                }
                self.output.push(')');
            }
            ExprKind::Keyword(literal) | ExprKind::Symbol(literal) => {
                let name = &self.program.named_literals[literal.0];
                self.output.push_str(
                    if matches!(self.program.exprs[id.0].kind, ExprKind::Symbol(_)) {
                        "rt.symbol("
                    } else {
                        "rt.keyword("
                    },
                );
                if let Some(namespace) = name.namespace {
                    self.string(namespace);
                } else {
                    self.output.push_str("null");
                }
                self.output.push_str(", ");
                self.string(name.name);
                self.output.push(')');
            }
            ExprKind::Vector(items) => {
                self.output.push_str("rt.vectorFromArray([");
                self.arguments(items);
                self.output.push_str("])");
            }
            ExprKind::Map(items) => {
                self.output.push_str("rt.mapFromArray([");
                self.arguments(items);
                self.output.push_str("])");
            }
            ExprKind::Set(items) => {
                // Reader-distinct constants retain the runtime's existing
                // Number-coercion policy. Dynamic member results need the
                // pinned checkout's duplicate check after all operands run.
                let constant = items.iter().all(|item| {
                    matches!(
                        self.program.exprs[item.0].kind,
                        ExprKind::Nil
                            | ExprKind::Bool(_)
                            | ExprKind::Number(_)
                            | ExprKind::String(_)
                            | ExprKind::Keyword(_)
                            | ExprKind::Symbol(_)
                    )
                });
                self.output.push_str(if constant {
                    "rt.setFromArray(["
                } else {
                    "rt.literalSetFromArray(["
                });
                self.arguments(items);
                self.output.push_str("])");
            }
            ExprKind::List(items) => {
                self.output.push_str("rt.listFromArray([");
                self.arguments(items);
                self.output.push_str("])");
            }
            ExprKind::CoreFn(core) => {
                write!(self.output, "rt.core.{}", core.runtime_name()).unwrap();
            }
            ExprKind::Local(local) => self.local_value(*local),
            ExprKind::Global(global) => self.global(*global),
            ExprKind::JsGlobal(global) => {
                let path = &self.program.js_globals[global.0].path;
                write!(self.output, "host(\"{}\")", path[0]).unwrap();
                for part in &path[1..] {
                    self.member(part);
                }
            }
            ExprKind::BooleanHint(value) => self.value(*value),
            ExprKind::Exists(value) | ExprKind::BindingExists(value) => {
                let value = if let ExprKind::BooleanHint(value) = self.program.exprs[value.0].kind {
                    value
                } else {
                    *value
                };
                if matches!(self.program.exprs[id.0].kind, ExprKind::BindingExists(_)) && self.compiler_binding(value) {
                    self.output.push_str("globalThis.__lightHasCompilerRoot(");
                    self.binding_name(value);
                    self.output.push(')');
                    return;
                }
                self.output.push('(');
                if let ExprKind::JsGlobal(global) = self.program.exprs[value.0].kind {
                    let path = &self.program.js_globals[global.0].path;
                    for i in 0..path.len() {
                        if i > 0 {
                            self.output.push_str(" && ");
                        }
                        self.output.push_str("typeof globalThis");
                        for part in &path[..=i] {
                            self.member(part);
                        }
                        self.output.push_str(" !== \"undefined\"");
                    }
                } else {
                    self.output.push_str("typeof ");
                    self.value(value);
                    self.output.push_str(" !== \"undefined\"");
                }
                self.output.push(')');
            }
            ExprKind::SetGlobal { target, value } => {
                self.output.push('(');
                self.assignable(*target);
                self.output.push_str(" = ");
                self.value(*value);
                self.output.push(')');
            }
            ExprKind::Def {
                global,
                init,
                test,
                once,
            } => {
                if self.demand_roots && !self.program.globals[global.0].name.contains('.') {
                    self.output.push_str("globalThis.__lightDefineCompilerRoot(");
                    let binding = &self.program.globals[global.0];
                    self.string_units(&format!("{}/{}", binding.namespace, binding.name).encode_utf16().collect::<Vec<_>>());
                    self.output.push_str(", ");
                    if let Some(init) = init {
                        self.output.push_str("() => ");
                        if let Some(test) = test {
                            self.output.push_str("{ const value = ");
                            self.value(*init);
                            self.output.push_str("; value.cljs$lang$test = ");
                            self.value(*test);
                            self.output.push_str("; return value; }");
                        } else {
                            self.output.push('(');
                            self.value(*init);
                            self.output.push(')');
                        }
                    } else { self.output.push_str("null"); }
                    write!(self.output, ", {once})").unwrap();
                    return;
                }
                self.output.push('(');
                if *once {
                    self.output.push_str("typeof ");
                    self.global(*global);
                    self.output.push_str(" === \"undefined\" ? ");
                }
                if let Some(init) = init {
                    self.output.push('(');
                    self.global(*global);
                    self.output.push_str(" = ");
                    self.value(*init);
                    if let Some(test) = test
                        && self.returned_definition != Some(id)
                    {
                        self.output.push_str(", ");
                        self.global(*global);
                        self.output.push_str("[\"cljs$lang$test\"] = ");
                        self.value(*test);
                        self.output.push_str(", ");
                        self.global(*global);
                    }
                    self.output.push(')');
                } else {
                    self.output.push_str("void 0");
                }
                if *once {
                    self.output.push_str(" : null");
                }
                self.output.push(')');
            }
            ExprKind::InstanceOf { class, value } => {
                self.output.push('(');
                self.value(*value);
                self.output.push_str(" instanceof ");
                self.value(*class);
                self.output.push(')');
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
                self.output.push('(');
                self.condition(*test);
                self.output.push_str(" ? ");
                self.value(*yes);
                self.output.push_str(" : ");
                self.value(*no);
                self.output.push(')');
            }
            ExprKind::ThisAs { .. } => {
                // CLJS value-context this-as has an ordinary function scope;
                // its implicit receiver is the global object, not lexical this.
                self.output.push_str("(function () {\n");
                self.indent += 1;
                self.emit(id, ExprContext::Return);
                self.indent -= 1;
                self.indentation();
                self.output.push_str("})()");
            }
            ExprKind::Try {
                body,
                catches,
                finally,
            } if catches.is_empty() && finally.is_none() => {
                // A clause-free try is its do body. In particular a single
                // value must not introduce a new JavaScript arguments scope.
                if let ExprKind::Do(forms) = &self.program.exprs[body.0].kind
                    && forms.len() <= 1
                {
                    if let Some(&form) = forms.first() {
                        self.value(form);
                    } else {
                        self.output.push_str("null");
                    }
                } else {
                    self.value(*body);
                }
            }
            ExprKind::WithOutStr(_)
            | ExprKind::Binding { .. }
            | ExprKind::Do(_)
            | ExprKind::Let { .. }
            | ExprKind::LetFn { .. }
            | ExprKind::Loop { .. }
            | ExprKind::Try { .. }
            | ExprKind::Throw(_)
            | ExprKind::Case { .. } => {
                self.output.push_str("(function () {\n");
                self.indent += 1;
                self.emit(id, ExprContext::Return);
                self.indent -= 1;
                self.indentation();
                self.output.push_str("})()");
            }
            ExprKind::Fn { name, methods } => {
                self.function(*name, methods, self.program.macro_functions.contains(&id))
            }
            ExprKind::Recur { .. } => unreachable!("analysis forbids recur in value position"),
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

// The compiler emits only quoted strings and ordinary identifier tokens outside
// explicitly recorded raw segments; the runtime body is also excluded here.
fn hygienic_js(source: String, mut raw: Vec<std::ops::Range<usize>>) -> String {
    raw.retain(|range| !range.is_empty());
    raw.sort_by_key(|range| range.start);
    let mut index = 0;
    let prefix = loop {
        let candidate = format!("__light{index}_");
        if !raw
            .iter()
            .any(|range| source[range.clone()].contains(&candidate))
        {
            break candidate;
        }
        index += 1;
    };
    let mut result = String::new();
    let bytes = source.as_bytes();
    let mut at = 0;
    let mut raw_at = 0;
    while at < bytes.len() {
        if raw.get(raw_at).is_some_and(|range| range.start == at) {
            let range = &raw[raw_at];
            result.push_str(&source[range.clone()]);
            at = range.end;
            raw_at += 1;
            continue;
        }
        let start = at;
        if bytes[at] == b'"' {
            at += 1;
            while at < bytes.len() {
                let byte = bytes[at];
                at += 1;
                if byte == b'\\' {
                    at += 1;
                } else if byte == b'"' {
                    break;
                }
            }
        } else if bytes[at].is_ascii_alphabetic() || matches!(bytes[at], b'_' | b'$') {
            at += 1;
            while at < bytes.len()
                && raw.get(raw_at).is_none_or(|range| at < range.start)
                && (bytes[at].is_ascii_alphanumeric() || matches!(bytes[at], b'_' | b'$'))
            {
                at += 1;
            }
            let token = &source[start..at];
            let numbered = [
                "l",
                "g",
                "m",
                "a",
                "r",
                "next",
                "saved",
                "temporary",
                "captures",
                "self",
                "recur",
            ]
            .iter()
            .any(|head| {
                token.strip_prefix(head).is_some_and(|tail| {
                    !tail.is_empty() && tail.bytes().all(|b| b.is_ascii_digit() || b == b'_')
                })
            });
            if numbered
                || matches!(
                    token,
                    "rt" | "truth" | "host" | "f" | "text" | "savedPrint" | "savedNewline"
                )
            {
                result.push_str(&prefix);
            }
        } else {
            at += 1;
        }
        // All non-ASCII source outside raw JS occurs inside escaped strings.
        result.push_str(&source[start..at]);
    }
    result
}
