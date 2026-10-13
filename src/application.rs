//! Conservative, optional application preparation over Rust semantic IR.
//! Compile-time unit emission never enters this boundary.
use crate::analyzer::{ExprId, ExprKind, GlobalId, Program};
use crate::program_fragment::children;
use std::collections::HashMap;

#[cfg(test)]
thread_local! { static PREPARATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }

/// How much whole-program work application preparation does before emission.
/// Levels are ordered: each one includes everything the previous one does.
/// See docs/advanced-optimization.md for the roadmap behind `Advanced`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OptimizationLevel {
    /// Emit every definition and call as analyzed. This output is the
    /// differential oracle that every other level must agree with.
    None,
    /// Conservative preparation: remove discarded definitions with inert
    /// initializers and specialize eager top-level calls. The default.
    #[default]
    Prepare,
    /// Release mode. It accepts the deliberate semantic differences listed
    /// under "Advanced-mode contract" in docs/advanced-optimization.md. Until
    /// the passes of that design land it emits exactly what `Prepare` emits;
    /// they hook in through [`OptimizationLevel::advanced`].
    Advanced,
}
impl OptimizationLevel {
    /// Every level from weakest to strongest, for differential tests.
    pub const ALL: [Self; 3] = [Self::None, Self::Prepare, Self::Advanced];

    /// The CLI spelling accepted by `--optimize` and [`str::parse`].
    pub fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Prepare => "prepare",
            Self::Advanced => "advanced",
        }
    }
    /// Whether the conservative removal and specialization passes run.
    pub fn prepares(self) -> bool {
        self >= Self::Prepare
    }
    /// Whether release-only passes may run. They must preserve results
    /// relative to `None` except under the advanced-mode contract.
    pub fn advanced(self) -> bool {
        self >= Self::Advanced
    }
}
/// `false` is [`OptimizationLevel::None`] and `true` is
/// [`OptimizationLevel::Prepare`], the meaning of the former boolean option.
impl From<bool> for OptimizationLevel {
    fn from(optimize: bool) -> Self {
        if optimize { Self::Prepare } else { Self::None }
    }
}
impl std::fmt::Display for OptimizationLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}
/// An optimization level name other than `none`, `prepare` or `advanced`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnknownOptimizationLevel(pub String);
impl std::fmt::Display for UnknownOptimizationLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "unknown optimization level {:?}: expected none, prepare or advanced",
            self.0
        )
    }
}
impl std::error::Error for UnknownOptimizationLevel {}
impl std::str::FromStr for OptimizationLevel {
    type Err = UnknownOptimizationLevel;
    fn from_str(name: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|level| level.name() == name)
            .ok_or_else(|| UnknownOptimizationLevel(name.to_owned()))
    }
}

#[derive(Clone, Debug, Default)]
pub struct ApplicationOptions {
    pub optimize: OptimizationLevel,
    /// Keep these runtime binding identities and all their source generations.
    pub exports: Vec<GlobalId>,
}
impl ApplicationOptions {
    /// Options at `optimize` without explicit exports. Accepts a level or the
    /// former boolean (`false` is `None`, `true` is `Prepare`).
    pub fn new(optimize: impl Into<OptimizationLevel>) -> Self {
        Self {
            optimize: optimize.into(),
            exports: Vec::new(),
        }
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ApplicationStats {
    pub removed_definitions: usize,
    pub specialized_calls: usize,
    pub retained_globals: usize,
}
/// An owned semantic program plus emission decisions computed entirely in Rust.
/// Repeated emission does not rerun preparation or execute compiler callbacks.
pub struct PreparedApplication {
    level: OptimizationLevel,
    program: Program,
    globals: Vec<bool>,
    calls: HashMap<ExprId, usize>,
    stats: ApplicationStats,
}
impl PreparedApplication {
    pub fn emit_js(&self) -> String {
        crate::emitter::emit_application(&self.program, &self.globals, &self.calls, false)
    }
    /// One ES module for a bundler; see [`crate::emitter::emit_esm`].
    pub fn emit_esm(&self) -> String {
        crate::emitter::emit_application(&self.program, &self.globals, &self.calls, true)
    }
    pub fn stats(&self) -> &ApplicationStats {
        &self.stats
    }
    /// The level this application was prepared at. Level-specific emission
    /// (for example runtime slicing under `Advanced`) reads it here.
    pub fn level(&self) -> OptimizationLevel {
        self.level
    }
}

/// Preserve the entry result, explicit exports, and ordered observable effects.
/// Only discarded top-level definitions with provably inert initializers are
/// candidates for removal. Unknown calls, throws, metadata tests, declaration
/// bundles and standalone expressions stay in their original execution order.
pub fn prepare_application(program: &Program, options: ApplicationOptions) -> PreparedApplication {
    #[cfg(test)]
    PREPARATIONS.with(|count| count.set(count.get() + 1));
    let level = options.optimize;
    let mut prepared = PreparedApplication {
        level,
        program: program.clone(),
        globals: vec![false; program.globals.len()],
        calls: HashMap::new(),
        stats: ApplicationStats::default(),
    };
    let mut top = Vec::new();
    flatten(program, program.root, true, &mut top);
    let mut candidates = vec![false; program.exprs.len()];
    let mut definitions = vec![Vec::new(); program.globals.len()];
    let mut pending = Vec::new();
    for &(id, result) in &top {
        if let ExprKind::Def {
            global, init, test, ..
        } = program.exprs[id.0].kind
        {
            definitions[global.0].push(id);
            if level.prepares()
                && !result
                && test.is_none()
                && init.is_none_or(|i| inert(program, i))
            {
                candidates[id.0] = true;
                continue;
            }
        }
        pending.push(id);
    }
    for g in &options.exports {
        if let Some(defs) = definitions.get(g.0) {
            pending.extend(defs);
        }
    }
    let mut visited = vec![false; program.exprs.len()];
    let mut raw = false;
    while let Some(id) = pending.pop() {
        if std::mem::replace(&mut visited[id.0], true) {
            continue;
        }
        // Only retained raw JS is an application escape. A discarded function
        // containing JS is inert and must not turn its compile-time uses into roots.
        if !raw && matches!(program.exprs[id.0].kind, ExprKind::JsTemplate { .. }) {
            raw = true;
            pending.extend(top.iter().map(|(id, _)| *id));
        }
        for global in globals(&program.exprs[id.0].kind) {
            if !std::mem::replace(&mut prepared.globals[global.0], true) {
                pending.extend(&definitions[global.0]);
            }
        }
        pending.extend(children(program, id));
    }
    for (index, candidate) in candidates.iter().enumerate() {
        if *candidate && !visited[index] {
            prepared.program.exprs[index].kind = ExprKind::Nil;
            prepared.stats.removed_definitions += 1;
        }
    }
    if level.prepares() && !raw {
        specialize(&mut prepared, &top);
    }
    if !level.prepares() {
        prepared.globals.fill(true);
    }
    // Release-only passes (docs/advanced-optimization.md, Phases 1-3) run here,
    // after conservative preparation, under `level.advanced()`. None exist yet,
    // so `Advanced` output is byte-identical to `Prepare` output.
    prepared.stats.retained_globals = prepared.globals.iter().filter(|b| **b).count();
    prepared.stats.specialized_calls += prepared.calls.len();
    prepared
}

fn flatten(program: &Program, id: ExprId, result: bool, out: &mut Vec<(ExprId, bool)>) {
    if let ExprKind::Do(items) = &program.exprs[id.0].kind {
        for (index, child) in items.iter().enumerate() {
            flatten(program, *child, result && index + 1 == items.len(), out);
        }
    } else {
        out.push((id, result));
    }
}
fn inert(program: &Program, id: ExprId) -> bool {
    use ExprKind::*;
    match &program.exprs[id.0].kind {
        Nil | Bool(_) | Number(_) | String(_) | Keyword(_) | Symbol(_) | CoreFn(_) | Fn { .. } => {
            true
        }
        Do(items) | List(items) | Vector(items) => items.iter().all(|id| inert(program, *id)),
        Map(items) | LiteralMap(items) => items
            .as_chunks::<2>()
            .0
            .iter()
            .all(|pair| scalar(program, pair[0]) && inert(program, pair[1])),
        Set(items) | LiteralSet(items) => items.iter().all(|id| scalar(program, *id)),
        // Non-scalar hashing, reader literals, metadata and unknown operations
        // may throw or dispatch user code and are deliberately not pure here.
        _ => false,
    }
}
fn scalar(program: &Program, id: ExprId) -> bool {
    matches!(
        program.exprs[id.0].kind,
        ExprKind::Nil
            | ExprKind::Bool(_)
            | ExprKind::Number(_)
            | ExprKind::String(_)
            | ExprKind::Keyword(_)
            | ExprKind::Symbol(_)
    )
}
fn globals(kind: &ExprKind) -> Vec<GlobalId> {
    match kind {
        ExprKind::Global(g) | ExprKind::Def { global: g, .. } => vec![*g],
        ExprKind::DefProtocol {
            global, methods, ..
        } => std::iter::once(*global)
            .chain(methods.iter().map(|m| m.global))
            .collect(),
        ExprKind::DefType {
            global,
            constructor,
            map_constructor,
            ..
        } => std::iter::once(*global)
            .chain(*constructor)
            .chain(*map_constructor)
            .collect(),
        _ => Vec::new(),
    }
}
fn reachable(program: &Program, root: ExprId) -> Vec<ExprId> {
    let mut pending = vec![root];
    let mut visited = vec![false; program.exprs.len()];
    let mut result = Vec::new();
    while let Some(id) = pending.pop() {
        if std::mem::replace(&mut visited[id.0], true) {
            continue;
        }
        result.push(id);
        pending.extend(children(program, id));
    }
    result
}
fn unwrap_do(program: &Program, mut id: ExprId) -> ExprId {
    while let ExprKind::Do(items) = &program.exprs[id.0].kind {
        if items.len() != 1 {
            break;
        }
        id = items[0];
    }
    id
}
fn dynamic(program: &Program, g: GlobalId) -> bool {
    let Some(id) = program.globals[g.0].metadata else {
        return false;
    };
    let ExprKind::Map(items) = &program.exprs[id.0].kind else {
        return true;
    };
    items.as_chunks::<2>().0.iter().any(|pair| {
        let ExprKind::Keyword(name) = program.exprs[pair[0].0].kind else {
            return false;
        };
        let name = &program.named_literals[name.0];
        name.namespace.is_none()
            && program.strings[name.name.0] == "dynamic".encode_utf16().collect::<Vec<_>>()
            && !matches!(
                program.exprs[pair[1].0].kind,
                ExprKind::Nil | ExprKind::Bool(false)
            )
    })
}
fn specialize(prepared: &mut PreparedApplication, top: &[(ExprId, bool)]) {
    let p = &prepared.program;
    let nodes = reachable(p, p.root);
    let mut defs = vec![Vec::new(); p.globals.len()];
    let mut escaped = vec![false; p.globals.len()];
    let mut callees = vec![false; p.exprs.len()];
    for &id in &nodes {
        if let ExprKind::Call { callee, .. } = p.exprs[id.0].kind {
            callees[callee.0] = true;
        }
    }
    for &id in &nodes {
        match &p.exprs[id.0].kind {
            ExprKind::Def {
                global,
                init,
                once: false,
                test: None,
            } => defs[global.0].push(*init),
            ExprKind::Def { global, .. } => escaped[global.0] = true,
            ExprKind::Global(g) if !callees[id.0] => escaped[g.0] = true,
            _ => (),
        }
    }
    // Specialize only eager top-level calls after the sole initialization. Calls
    // inside functions/branches retain late lookup: no initialization dominance
    // or interprocedural effect assumptions are smuggled into emission.
    let mut initialized = vec![false; p.globals.len()];
    let mut core_calls = Vec::new();
    for &(id, _) in top {
        if let ExprKind::Def { global, .. } = p.exprs[id.0].kind {
            initialized[global.0] = true;
        }
        let ExprKind::Call { callee, ref args } = p.exprs[id.0].kind else {
            continue;
        };
        let ExprKind::Global(g) = p.exprs[callee.0].kind else {
            continue;
        };
        if escaped[g.0] || !initialized[g.0] || dynamic(p, g) || defs[g.0].len() != 1 {
            continue;
        }
        let Some(init) = defs[g.0][0] else {
            continue;
        };
        let init = unwrap_do(p, init);
        if let ExprKind::Fn { methods, .. } = &p.exprs[init.0].kind {
            // Multi-arity wrappers publish these canonical fixed arity entries.
            // Keep .call(null) so this/arguments/recur and argument effects match.
            if methods.len() > 1
                && methods
                    .iter()
                    .filter(|m| m.rest.is_none() && m.params.len() == args.len())
                    .count()
                    == 1
            {
                prepared.calls.insert(id, args.len());
            }
        } else if let ExprKind::CoreFn(core) = p.exprs[init.0].kind {
            core_calls.push((id, callee, core));
        }
    }
    for (id, callee, core) in core_calls {
        let mut expr = prepared.program.exprs[callee.0].clone();
        expr.kind = ExprKind::CoreFn(core);
        let callee = ExprId(prepared.program.exprs.len());
        prepared.program.exprs.push(expr);
        if let ExprKind::Call { callee: target, .. } = &mut prepared.program.exprs[id.0].kind {
            *target = callee;
        }
        prepared.stats.specialized_calls += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn optimization_levels_parse_print_order_and_convert_from_the_former_boolean() {
        for level in OptimizationLevel::ALL {
            assert_eq!(level.name().parse(), Ok(level));
            assert_eq!(level.to_string(), level.name());
        }
        assert_eq!(
            "Advanced".parse::<OptimizationLevel>(),
            Err(UnknownOptimizationLevel("Advanced".into()))
        );
        assert!(OptimizationLevel::None < OptimizationLevel::Prepare);
        assert!(OptimizationLevel::Prepare < OptimizationLevel::Advanced);
        assert_eq!(
            OptimizationLevel::ALL.map(|l| (l.prepares(), l.advanced())),
            [(false, false), (true, false), (true, true)]
        );
        assert_eq!(OptimizationLevel::from(false), OptimizationLevel::None);
        assert_eq!(OptimizationLevel::from(true), OptimizationLevel::Prepare);
        assert_eq!(
            ApplicationOptions::default().optimize,
            OptimizationLevel::Prepare
        );
        let p = crate::analyze(
            "(def unused 1) 2",
            &crate::parse("(def unused 1) 2").unwrap(),
        )
        .unwrap();
        for level in OptimizationLevel::ALL {
            let prepared = prepare_application(&p, ApplicationOptions::new(level));
            assert_eq!(prepared.level(), level);
            assert_eq!(
                prepared.stats().removed_definitions,
                usize::from(level.prepares())
            );
        }
    }
    #[test]
    fn macro_calls_do_not_prepare_applications_or_recompile_unchanged_units() {
        let source = "(def factor 2) (defn helper [x] (* factor x)) (defmacro twice [x] (helper x)) (twice 21) (twice 21) (twice 21)";
        PREPARATIONS.with(|count| count.set(0));
        let p = crate::analyze(source, &crate::parse(source).unwrap()).unwrap();
        PREPARATIONS.with(|count| assert_eq!(count.get(), 0));
        for name in ["cljs.user/factor", "cljs.user/helper"] {
            let uses: Vec<_> = p
                .compilation_uses
                .iter()
                .filter(|u| u.name == name)
                .collect();
            assert_eq!(uses.len(), 1, "{name}: {:?}", p.compilation_uses);
            assert_eq!(uses[0].analyses, 1);
            assert_eq!(uses[0].emissions, 1);
        }
        let prepared = prepare_application(&p, Default::default());
        assert_eq!(prepared.stats().removed_definitions, 2);
        let js = prepared.emit_js();
        assert_eq!(prepared.emit_js(), js);
        PREPARATIONS.with(|count| assert_eq!(count.get(), 1));
        assert!(!js.contains("__lightCompileUnit"));
        assert!(!js.contains("__lightCompilerRoots"));
    }
    #[test]
    fn generation_counters_distinguish_compile_demand_from_application_analysis() {
        let source = "(def x 1) (defmacro read-x [] x) (read-x) (read-x) (def x 2) (read-x) (read-x) (def unused 3) 42";
        let p = crate::analyze(source, &crate::parse(source).unwrap()).unwrap();
        let uses: Vec<_> = p
            .compilation_uses
            .iter()
            .filter(|u| u.name == "cljs.user/x")
            .collect();
        assert_eq!(uses.len(), 2);
        assert_eq!(uses[0].generation, 1);
        assert_eq!(uses[1].generation, 2);
        for u in uses {
            assert_eq!((u.analyses, u.emissions), (1, 1));
        }
        let unused = p
            .compilation_uses
            .iter()
            .find(|u| u.name == "cljs.user/unused")
            .unwrap();
        assert_eq!((unused.analyses, unused.emissions), (1, 0));
    }
}
