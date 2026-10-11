//! Append independently analyzed arenas while preserving global identities.

use crate::analyzer::*;

/// Append an analyzed fragment without changing `target.root` and return its new
/// root ID. Diagnostic spans/source indices retain the fragment's provenance.
/// Existing global IDs remain stable; a fragment definition updates its flags,
/// and supplied metadata replaces metadata on the matching global.
pub(crate) fn append(target: &mut Program, fragment: &Program) -> ExprId {
    let expr_offset = target.exprs.len();
    let mut remap = Remap {
        map_expr: |id: ExprId| ExprId(id.0 + expr_offset),
        local: target.locals.len(),
        recur: target.recur_targets.len(),
        string: target.strings.len(),
        named: target.named_literals.len(),
        globals: Vec::with_capacity(fragment.globals.len()),
        js: Vec::with_capacity(fragment.js_globals.len()),
    };
    for global in &fragment.globals {
        let id = if let Some(index) = target.globals.iter().position(|existing| {
            existing.namespace == global.namespace && existing.name == global.name
        }) {
            let existing = &mut target.globals[index];
            if global.declared {
                existing.declared = true;
                existing.type_descriptor = global.type_descriptor;
                existing.constant = global.constant;
                existing.boolean_tag = global.boolean_tag;
            }
            if let Some(metadata) = global.metadata {
                existing.metadata = Some(ExprId(metadata.0 + expr_offset));
            }
            GlobalId(index)
        } else {
            let id = GlobalId(target.globals.len());
            let mut global = global.clone();
            global.metadata = global.metadata.map(|id| ExprId(id.0 + expr_offset));
            target.globals.push(global);
            id
        };
        remap.globals.push(id);
    }
    for global in &fragment.js_globals {
        let id = match target
            .js_globals
            .iter()
            .position(|existing| existing == global)
        {
            Some(index) => JsGlobalId(index),
            None => {
                let id = JsGlobalId(target.js_globals.len());
                target.js_globals.push(global.clone());
                id
            }
        };
        remap.js.push(id);
    }
    target.strings.extend(fragment.strings.iter().cloned());
    target
        .named_literals
        .extend(fragment.named_literals.iter().cloned().map(|mut n| {
            n.namespace = n.namespace.map(|id| StringId(id.0 + remap.string));
            n.name.0 += remap.string;
            n
        }));
    target.locals.extend(fragment.locals.iter().cloned());
    target
        .recur_targets
        .extend(fragment.recur_targets.iter().cloned().map(|mut t| {
            remap.locals(&mut t.params);
            t
        }));
    for (global, methods) in &fragment.protocols {
        let mut methods = methods.clone();
        for method in &mut methods {
            remap.global(&mut method.global);
        }
        target.protocols.insert(remap.globals[global.0], methods);
    }
    target.record_constructors.extend(
        fragment
            .record_constructors
            .iter()
            .map(|g| remap.globals[g.0]),
    );
    target
        .field_bindings
        .extend(fragment.field_bindings.iter().map(|(l, (r, b))| {
            (
                LocalId(l.0 + remap.local),
                (RecurTargetId(r.0 + remap.recur), *b),
            )
        }));
    target.type_methods.extend(
        fragment
            .type_methods
            .iter()
            .map(|r| RecurTargetId(r.0 + remap.recur)),
    );
    target.macro_functions.extend(
        fragment
            .macro_functions
            .iter()
            .map(|e| ExprId(e.0 + expr_offset)),
    );
    target
        .exprs
        .extend(fragment.exprs.iter().cloned().map(|mut e| {
            remap.kind(&mut e.kind);
            e
        }));
    ExprId(fragment.root.0 + expr_offset)
}

/// Preserve the definition-time resolution environment and its metadata, without
/// retaining unrelated executable expression graphs. Non-expression IDs remain
/// stable, including the protocol and record/type resolution tables.
pub(crate) fn resolution_snapshot(program: &Program) -> Program {
    use std::cell::RefCell;

    let pending = RefCell::new(
        program
            .globals
            .iter()
            .filter_map(|global| global.metadata)
            .collect::<Vec<_>>(),
    );
    let mut retained = vec![false; program.exprs.len()];
    let visitor = identity_remap(program, |id| {
        pending.borrow_mut().push(id);
        id
    });
    loop {
        let Some(id) = pending.borrow_mut().pop() else {
            break;
        };
        if std::mem::replace(&mut retained[id.0], true) {
            continue;
        }
        // The same exhaustive traversal used by append discovers every child,
        // including nested metadata and function bodies if metadata contains one.
        visitor.kind(&mut program.exprs[id.0].kind.clone());
    }
    let mut indices = vec![ExprId(usize::MAX); program.exprs.len()];
    let mut next = 0;
    for (index, retained) in retained.iter().enumerate() {
        if *retained {
            indices[index] = ExprId(next);
            next += 1;
        }
    }
    let remap = identity_remap(program, |id| indices[id.0]);
    let mut snapshot = program.clone();
    snapshot.exprs = program
        .exprs
        .iter()
        .enumerate()
        .filter_map(|(index, expr)| {
            if !retained[index] {
                return None;
            }
            let mut expr = expr.clone();
            remap.kind(&mut expr.kind);
            Some(expr)
        })
        .collect();
    for global in &mut snapshot.globals {
        remap.optional(&mut global.metadata);
    }
    snapshot.macro_functions = program
        .macro_functions
        .iter()
        .filter(|id| retained[id.0])
        .map(|id| indices[id.0])
        .collect();
    snapshot.root = ExprId(snapshot.exprs.len());
    snapshot.exprs.push(Expr {
        kind: ExprKind::Nil,
        span: 0..0,
        context: ExprContext::Value,
        source_index: 0,
    });
    snapshot
}

fn identity_remap<F: Fn(ExprId) -> ExprId>(program: &Program, map_expr: F) -> Remap<F> {
    Remap {
        map_expr,
        local: 0,
        recur: 0,
        string: 0,
        named: 0,
        globals: (0..program.globals.len()).map(GlobalId).collect(),
        js: (0..program.js_globals.len()).map(JsGlobalId).collect(),
    }
}

struct Remap<F> {
    map_expr: F,
    local: usize,
    recur: usize,
    string: usize,
    named: usize,
    globals: Vec<GlobalId>,
    js: Vec<JsGlobalId>,
}

impl<F: Fn(ExprId) -> ExprId> Remap<F> {
    fn expr(&self, id: &mut ExprId) {
        *id = (self.map_expr)(*id);
    }
    fn local(&self, id: &mut LocalId) {
        id.0 += self.local;
    }
    fn global(&self, id: &mut GlobalId) {
        *id = self.globals[id.0];
    }
    fn exprs(&self, ids: &mut [ExprId]) {
        for id in ids {
            self.expr(id);
        }
    }
    fn locals(&self, ids: &mut [LocalId]) {
        for id in ids {
            self.local(id);
        }
    }
    fn optional(&self, id: &mut Option<ExprId>) {
        if let Some(id) = id {
            self.expr(id);
        }
    }
    fn bindings(&self, bindings: &mut [(LocalId, ExprId)]) {
        for (local, expr) in bindings {
            self.local(local);
            self.expr(expr);
        }
    }
    fn pairs(&self, pairs: &mut [(ExprId, ExprId)]) {
        for (a, b) in pairs {
            self.expr(a);
            self.expr(b);
        }
    }
    fn method(&self, method: &mut FnMethod) {
        self.locals(&mut method.params);
        if let Some(rest) = &mut method.rest {
            self.local(rest);
        }
        self.expr(&mut method.body);
        method.target.0 += self.recur;
        self.locals(&mut method.captures);
    }
    fn methods(&self, methods: &mut [(MethodDispatch, FnMethod)]) {
        for (_, method) in methods {
            self.method(method);
        }
    }
    fn kind(&self, kind: &mut ExprKind) {
        use ExprKind::*;
        match kind {
            Nil | Bool(_) | Number(_) | CoreFn(_) | JsArguments => {}
            String(id) => id.0 += self.string,
            Keyword(id) | Symbol(id) => id.0 += self.named,
            Local(id) => self.local(id),
            Global(id) => self.global(id),
            JsGlobal(id) => *id = self.js[id.0],
            ReaderLiteral { items, .. }
            | List(items)
            | Vector(items)
            | Map(items)
            | Set(items)
            | Do(items) => self.exprs(items),
            WithMeta { value, metadata } => {
                self.expr(value);
                self.expr(metadata);
            }
            Var {
                value,
                symbol,
                metadata,
                ..
            } => {
                self.expr(value);
                symbol.0 += self.named;
                self.optional(metadata);
            }
            ThisAs { binding, body } => {
                self.local(binding);
                self.expr(body);
            }
            JsTemplate { args, .. } => self.exprs(args),
            JsIndex { receiver, indices } => {
                self.expr(receiver);
                self.exprs(indices);
            }
            JsDelete { receiver, key } => {
                self.expr(receiver);
                self.expr(key);
            }
            QualifiedMethod { class, .. } => self.expr(class),
            CopyArguments(id) | WithOutStr(id) | Exists(id) | BindingExists(id) | BooleanHint(id) | Throw(id) => {
                self.expr(id)
            }
            DefProtocol {
                global, methods, ..
            } => {
                self.global(global);
                for method in methods {
                    self.global(&mut method.global);
                }
            }
            ProtocolPredicate {
                protocol, value, ..
            } => {
                self.expr(protocol);
                self.expr(value);
            }
            ExtendType {
                target, methods, ..
            } => {
                self.expr(target);
                self.methods(methods);
            }
            Reify {
                metadata,
                captures,
                methods,
            } => {
                self.optional(metadata);
                self.locals(captures);
                self.methods(methods);
            }
            DefType {
                global,
                constructor,
                body,
                masks,
                map_constructor,
                methods,
                ..
            } => {
                self.global(global);
                if let Some(id) = constructor {
                    self.global(id);
                }
                if let Some(id) = map_constructor {
                    self.global(id);
                }
                self.optional(body);
                self.optional(masks);
                self.methods(methods);
            }
            Construct { callee, args } | Call { callee, args } => {
                self.expr(callee);
                self.exprs(args);
            }
            Assert {
                test,
                message,
                form,
            } => {
                self.expr(test);
                self.optional(message);
                form.0 += self.string;
            }
            Def {
                global, init, test, ..
            } => {
                self.global(global);
                self.optional(init);
                self.optional(test);
            }
            SetGlobal { target, value } => {
                self.expr(target);
                self.expr(value);
            }
            Binding { bindings, body } => {
                self.pairs(bindings);
                self.expr(body);
            }
            Property { receiver, .. } => self.expr(receiver),
            MethodCall { receiver, args, .. } => {
                self.expr(receiver);
                self.exprs(args);
            }
            If { test, yes, no } => {
                self.expr(test);
                self.expr(yes);
                self.expr(no);
            }
            Let { bindings, body } | LetFn { bindings, body } => {
                self.bindings(bindings);
                self.expr(body);
            }
            Loop {
                target,
                bindings,
                body,
            } => {
                target.0 += self.recur;
                self.bindings(bindings);
                self.expr(body);
            }
            Recur { target, args } => {
                target.0 += self.recur;
                self.exprs(args);
            }
            Try {
                body,
                catches,
                finally,
            } => {
                self.expr(body);
                for catch in catches {
                    self.optional(&mut catch.class);
                    self.local(&mut catch.binding);
                    self.expr(&mut catch.body);
                }
                self.optional(finally);
            }
            Case {
                test,
                branches,
                default,
                ..
            } => {
                self.expr(test);
                self.pairs(branches);
                self.optional(default);
            }
            Fn { name, methods } => {
                if let Some(name) = name {
                    self.local(name);
                }
                for method in methods {
                    self.method(method);
                }
            }
            InstanceOf { class, value } => {
                self.expr(class);
                self.expr(value);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn analyze(source: &str) -> Program {
        crate::analyze(source, &crate::parse(source).unwrap()).unwrap()
    }

    #[test]
    fn appended_fragments_execute_with_shared_globals_and_distinct_arenas() {
        let mut target = analyze("(def shared (js/Math.max 0 10)) (let [x \"original\"] x)");
        let fragment = analyze(
            "(def shared 40) (def answer (loop [n 2 v shared] (if (zero? n) v (recur (dec n) (inc v))))) (pr-str [(js/Math.abs (- answer)) :sample/value \"fragment\"])",
        );
        let original_root = target.root;
        let original_global = target
            .globals
            .iter()
            .position(|g| g.name == "shared")
            .unwrap();
        let root = append(&mut target, &fragment);
        assert_eq!(target.root, original_root);
        assert_eq!(
            target.globals.iter().filter(|g| g.name == "shared").count(),
            1
        );
        assert_eq!(target.globals[original_global].name, "shared");
        target.exprs.push(Expr {
            kind: ExprKind::Do(vec![original_root, root]),
            span: 0..0,
            context: ExprContext::Value,
            source_index: 0,
        });
        target.root = ExprId(target.exprs.len() - 1);
        let output =
            std::process::Command::new(std::env::var_os("NODE").unwrap_or_else(|| "node".into()))
                .args([
                    "--eval",
                    &format!("console.log({});", crate::emit_js(&target)),
                ])
                .output()
                .expect("fragment execution requires Node");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert_eq!(stdout, "[42 :sample/value \"fragment\"]\n");
    }

    #[test]
    fn metadata_is_remapped_and_absent_metadata_preserves_existing_value() {
        let mut target = analyze("(def ^{:doc \"old\"} shared 1)");
        let fragment = analyze("(def ^{:doc \"new\"} shared 2)");
        let offset = target.exprs.len();
        let metadata = fragment.globals[0].metadata.unwrap();
        append(&mut target, &fragment);
        assert_eq!(
            target.globals[0].metadata,
            Some(ExprId(metadata.0 + offset))
        );
        let preserved = target.globals[0].metadata;
        let mut fragment = analyze("(def shared 3)");
        fragment.globals[0].metadata = None;
        append(&mut target, &fragment);
        assert_eq!(target.globals[0].metadata, preserved);
    }
    #[test]
    fn resolution_snapshot_retains_metadata_but_excludes_initializers() {
        let program = analyze(
            "(def ^{:doc \"kept\" :dynamic true} shared (let [x 1] (+ x 2))) (defprotocol P (f [this])) (defrecord R [x])",
        );
        let snapshot = resolution_snapshot(&program);
        assert_eq!(snapshot.protocols, program.protocols);
        assert_eq!(snapshot.record_constructors, program.record_constructors);
        assert_eq!(snapshot.globals.len(), program.globals.len());
        assert!(matches!(
            snapshot.exprs[snapshot.root.0].kind,
            ExprKind::Nil
        ));
        assert!(snapshot.exprs.len() < program.exprs.len());
        assert!(!snapshot.exprs.iter().any(|expr| matches!(
            expr.kind,
            ExprKind::Def { .. }
                | ExprKind::Let { .. }
                | ExprKind::DefProtocol { .. }
                | ExprKind::DefType { .. }
        )));
        let old_metadata = program.globals[0].metadata.unwrap();
        let new_metadata = snapshot.globals[0].metadata.unwrap();
        let mut original = program.clone();
        original.root = old_metadata;
        let mut copied = snapshot.clone();
        copied.root = new_metadata;
        let runtime = rquickjs::Runtime::new().unwrap();
        let context = rquickjs::Context::full(&runtime).unwrap();
        context.with(|ctx| {
            let old: String = ctx
                .eval(format!("JSON.stringify({})", crate::emit_js(&original)))
                .unwrap();
            let new: String = ctx
                .eval(format!("JSON.stringify({})", crate::emit_js(&copied)))
                .unwrap();
            assert_eq!(old, new);
        });
    }
}
