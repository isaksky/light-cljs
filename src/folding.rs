//! Constant folding and dead-branch elimination over the prepared IR: the
//! folding half of Phase 2 step 8 of docs/advanced-optimization.md
//! (light-cljs-214), at `Advanced` only. [`crate::inlining`] is the other
//! half; `prepare_application` alternates the two with definition removal
//! (see "Phase 2 folding and inlining extension points" in the design
//! document).
//!
//! [`fold`] rewrites expressions in place, children first, so each rewrite sees
//! its operands folded:
//!
//! - A read of a global that is `^:const` (or defined by `goog-define`,
//!   [`crate::defines`]), defined once by a top-level `def` whose value is a
//!   [`Constant`] and never assigned, is that constant when the read is in a
//!   later top-level form, which runs after the definition. Upstream
//!   substitutes the value of a `^:const` Var at every reference analyzed after
//!   it in the same way.
//! - A `let` binding of a constant is substituted into its reads and dropped,
//!   and so is a binding nothing reads whose initializer has no effect. This is
//!   what folds `and` and `or`, whose expansions bind their operands.
//! - A call of a core operation on constants is its result when the core
//!   operation table ([`crate::core_operations`]) says the call is `pure` for
//!   those arguments and the program can neither replace the operation nor
//!   make it dispatch to program code ([`ProgramFacts::trusted`]):
//!   arithmetic, comparisons, `=`, predicates, `not` and `str` ([`evaluate`]).
//! - An `if` whose test is a constant is the branch the test selects, so the
//!   other branch, and the definitions only it reads, are gone (the
//!   definitions go when `prepare_application` next removes unread ones).
//!   `when`, `if-not`, `cond` and `and`/`or` in tests are `if`s.
//! - A `do` drops the forms before its last that have no effect, and one form
//!   left is that form; a `^boolean` hint on a constant is the constant.
//!
//! # Soundness
//!
//! Each rewrite gives the value the expression has at run time, with the same
//! effects: a constant is the value of a literal, a read of a binding nothing
//! assigns, or a `pure` call of constants, which has no effect, cannot throw
//! and returns the same value every time. [`evaluate`] reproduces the
//! runtime's definitions of the operations in `src/js_runtime.js` with the
//! same IEEE double operations in the same order. A local that a closure,
//! a type or `js*` raw text may see is never substituted: the emitter names
//! captured locals and raw text may assign its holes. The root, namespace
//! entries and macro functions keep their expression identities, which
//! emission and preparation look up.
//!
//! Literals keep what emission makes of them. Map keys and set members never
//! become constants ([`fenced_positions`]): emission builds maps with
//! constant keys from key shapes and sets of scalars without a duplicate
//! check, which rely on the distinct keys of source literals. A literal the
//! program builds at each evaluation stays built when its items become
//! constants ([`built_literals`], [`keep_built`]), so it never becomes the
//! one pooled value every evaluation shares.
//!
//! # Extension points (light-cljs-221)
//!
//! [`Constant`] is the folder's value domain and [`evaluate`] its table of
//! operations. Folding lookups, counts and `nth` against pooled collection
//! constants adds a composite variant to `Constant` (built from the emitter's
//! `constant_nodes` classification) and rows to `evaluate` for `get`,
//! `count`, `nth` and `contains?`, keeping the throws of out-of-range
//! arguments by not folding them; [`Constant::of`] is where a composite is
//! recognized and [`Constant::into_kind`] where a result is written back.
use crate::analyzer::{CoreFnId, ExprId, ExprKind, NamedLiteralId, Program, StringId};
use crate::core_operations::{Purity, Type};
use crate::inference::ProgramFacts;
use crate::program_fragment::{children, kind_children};

/// A value the folder knows at compile time.
#[derive(Clone, Debug, PartialEq)]
pub enum Constant {
    Nil,
    Bool(bool),
    Number(f64),
    /// UTF-16 code units, as `Program::strings` holds them.
    String(Vec<u16>),
    Keyword(NamedLiteralId),
}

impl Constant {
    /// The constant a literal expression evaluates to.
    pub fn of(program: &Program, id: ExprId) -> Option<Self> {
        Self::of_kind(program, &program.exprs[id.0].kind)
    }

    fn of_kind(program: &Program, kind: &ExprKind) -> Option<Self> {
        Some(match kind {
            ExprKind::Nil => Self::Nil,
            ExprKind::Bool(value) => Self::Bool(*value),
            ExprKind::Number(value) => Self::Number(*value),
            ExprKind::String(string) => Self::String(program.strings[string.0].clone()),
            ExprKind::Keyword(literal) => Self::Keyword(*literal),
            _ => return None,
        })
    }

    /// Its class in the core operation table's lattice.
    pub fn ty(&self) -> Type {
        match self {
            Self::Nil => Type::NIL,
            Self::Bool(_) => Type::BOOLEAN,
            Self::Number(_) => Type::NUMBER,
            Self::String(_) => Type::STRING,
            Self::Keyword(_) => Type::KEYWORD,
        }
    }

    /// Whether Clojure counts it as true: anything but `nil` and `false`.
    pub fn truthy(&self) -> bool {
        !matches!(self, Self::Nil | Self::Bool(false))
    }

    /// The literal expression of this value, adding a string to `program`.
    pub fn into_kind(self, program: &mut Program) -> ExprKind {
        match self {
            Self::Nil => ExprKind::Nil,
            Self::Bool(value) => ExprKind::Bool(value),
            Self::Number(value) => ExprKind::Number(value),
            Self::String(units) => {
                program.strings.push(units);
                ExprKind::String(StringId(program.strings.len() - 1))
            }
            Self::Keyword(literal) => ExprKind::Keyword(literal),
        }
    }
}

/// The result of calling `core` on `args` when the runtime's definition is
/// reproduced here; the caller has checked that the call is `pure` for these
/// arguments. `None` leaves the call alone.
pub fn evaluate(program: &Program, core: CoreFnId, args: &[Constant]) -> Option<Constant> {
    use Constant::{Bool, Number};
    let numbers = || -> Option<Vec<f64>> {
        args.iter()
            .map(|arg| match arg {
                Number(value) => Some(*value),
                _ => None,
            })
            .collect()
    };
    // `numericFold`: a left fold over two or more operands.
    let fold = |operation: fn(f64, f64) -> f64| -> Option<f64> {
        let values = numbers()?;
        let (first, rest) = values.split_first()?;
        Some(
            rest.iter()
                .fold(*first, |value, item| operation(value, *item)),
        )
    };
    // `comparison`: one operand is true, else every adjacent pair passes.
    let chain = |test: fn(f64, f64) -> bool| -> Option<Constant> {
        let values = numbers()?;
        Some(Bool(values.windows(2).all(|pair| test(pair[0], pair[1]))))
    };
    let one = || -> Option<f64> {
        match args {
            [Number(value)] => Some(*value),
            _ => None,
        }
    };
    let two = || -> Option<(f64, f64)> {
        match args {
            [Number(n), Number(d)] => Some((*n, *d)),
            _ => None,
        }
    };
    let fix = |x: f64| if x >= 0.0 { x.floor() } else { x.ceil() };
    let quot = |n: f64, d: f64| fix((n - n % d) / d);
    let extrema = |test: fn(f64, f64) -> bool| -> Option<Constant> {
        let values = numbers()?;
        let (first, rest) = values.split_first()?;
        Some(Number(rest.iter().fold(*first, |a, b| {
            if a.is_nan() {
                a
            } else if b.is_nan() || !test(a, *b) {
                *b
            } else {
                a
            }
        })))
    };
    Some(match core {
        CoreFnId::Add => Number(fold(|a, b| a + b).unwrap_or(0.0)),
        CoreFnId::Multiply if args.is_empty() => Number(1.0),
        CoreFnId::Multiply => Number(fold(|a, b| a * b)?),
        CoreFnId::Subtract if args.len() == 1 => Number(-one()?),
        CoreFnId::Subtract => Number(fold(|a, b| a - b)?),
        CoreFnId::Divide if args.len() == 1 => Number(1.0 / one()?),
        CoreFnId::Divide => Number(fold(|a, b| a / b)?),
        CoreFnId::Inc => Number(one()? + 1.0),
        CoreFnId::Dec => Number(one()? - 1.0),
        CoreFnId::Abs => Number(one()?.abs()),
        CoreFnId::Max => extrema(|a, b| a > b)?,
        CoreFnId::Min => extrema(|a, b| a < b)?,
        CoreFnId::Mod => {
            let (n, d) = two()?;
            Number(((n % d) + d) % d)
        }
        CoreFnId::Quot => {
            let (n, d) = two()?;
            Number(quot(n, d))
        }
        CoreFnId::Rem => {
            let (n, d) = two()?;
            Number(n - d * quot(n, d))
        }
        CoreFnId::Less => chain(|a, b| a < b)?,
        CoreFnId::LessEqual => chain(|a, b| a <= b)?,
        CoreFnId::Greater => chain(|a, b| a > b)?,
        CoreFnId::GreaterEqual => chain(|a, b| a >= b)?,
        // `numericEqual` is `equal` on numbers, which is `===`.
        CoreFnId::NumericEqual => chain(|a, b| a == b)?,
        CoreFnId::Equal => Bool(equal_all(program, args)?),
        CoreFnId::NotEqual => Bool(!equal_all(program, args)?),
        CoreFnId::Identical => match args {
            [a, b] if !matches!(a, Constant::Keyword(_)) && !matches!(b, Constant::Keyword(_)) => {
                Bool(equal(program, a, b))
            }
            _ => return None,
        },
        CoreFnId::Not => Bool(!args.first()?.truthy()),
        CoreFnId::Boolean => Bool(args.first()?.truthy()),
        CoreFnId::IsNil => Bool(matches!(args.first()?, Constant::Nil)),
        CoreFnId::IsSome => Bool(!matches!(args.first()?, Constant::Nil)),
        CoreFnId::IsTrue => Bool(matches!(args.first()?, Constant::Bool(true))),
        CoreFnId::IsFalse => Bool(matches!(args.first()?, Constant::Bool(false))),
        CoreFnId::IsBoolean => Bool(matches!(args.first()?, Constant::Bool(_))),
        CoreFnId::IsNumber => Bool(matches!(args.first()?, Constant::Number(_))),
        CoreFnId::IsString => Bool(matches!(args.first()?, Constant::String(_))),
        CoreFnId::IsKeyword => Bool(matches!(args.first()?, Constant::Keyword(_))),
        CoreFnId::IsZero => Bool(matches!(args.first()?, Constant::Number(value) if *value == 0.0)),
        CoreFnId::IsPositive => Bool(one()? > 0.0),
        CoreFnId::IsNegative => Bool(one()? < 0.0),
        CoreFnId::Str => {
            let mut units = Vec::new();
            for arg in args {
                match arg {
                    Constant::Nil => (),
                    Constant::String(string) => units.extend_from_slice(string),
                    Constant::Bool(value) => units.extend(value.to_string().encode_utf16()),
                    Constant::Number(value) => units.extend(js_number(*value).encode_utf16()),
                    // A keyword's text is its printed form, which this does
                    // not reproduce.
                    Constant::Keyword(_) => return None,
                }
            }
            Constant::String(units)
        }
        _ => return None,
    })
}

/// `equal` on scalars: `nil` equals only `nil`, keywords compare by spelling
/// and everything else with `===`.
fn equal(program: &Program, a: &Constant, b: &Constant) -> bool {
    match (a, b) {
        (Constant::Keyword(a), Constant::Keyword(b)) => {
            let (a, b) = (&program.named_literals[a.0], &program.named_literals[b.0]);
            let spelling = |name: Option<StringId>| name.map(|name| &program.strings[name.0]);
            spelling(a.namespace) == spelling(b.namespace)
                && program.strings[a.name.0] == program.strings[b.name.0]
        }
        (Constant::Number(a), Constant::Number(b)) => a == b,
        (a, b) => a == b,
    }
}

/// `equal` over its arguments, pairwise.
fn equal_all(program: &Program, args: &[Constant]) -> Option<bool> {
    (!args.is_empty()).then(|| {
        args.windows(2)
            .all(|pair| equal(program, &pair[0], &pair[1]))
    })
}

/// ECMAScript `Number::toString`, which `str` applies to numbers.
pub fn js_number(value: f64) -> String {
    if value.is_nan() {
        return "NaN".into();
    }
    if value == 0.0 {
        return "0".into();
    }
    if value.is_infinite() {
        return if value > 0.0 { "Infinity" } else { "-Infinity" }.into();
    }
    if value < 0.0 {
        return format!("-{}", js_number(-value));
    }
    // Rust's shortest round-trip digits are the `s` of the specification.
    let scientific = format!("{value:e}");
    let (mantissa, exponent) = scientific.split_once('e').expect("exponent notation");
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let k = digits.len() as i32;
    let n = exponent.parse::<i32>().expect("an integer exponent") + 1;
    if k <= n && n <= 21 {
        format!("{digits}{}", "0".repeat((n - k) as usize))
    } else if 0 < n && n <= 21 {
        format!("{}.{}", &digits[..n as usize], &digits[n as usize..])
    } else if -6 < n && n <= 0 {
        format!("0.{}{digits}", "0".repeat((-n) as usize))
    } else {
        let sign = if n > 0 { '+' } else { '-' };
        let exponent = (n - 1).abs();
        if k == 1 {
            format!("{digits}e{sign}{exponent}")
        } else {
            format!("{}.{}e{sign}{exponent}", &digits[..1], &digits[1..])
        }
    }
}

/// No top-level position: unreachable from the root.
pub(crate) const NOWHERE: u32 = u32::MAX;

/// Fold `program` in place and return how many expressions were rewritten.
pub fn fold(program: &mut Program, built: &mut Vec<bool>) -> usize {
    let mut top = Vec::new();
    flatten(program, program.root, &mut top);
    let facts = ProgramFacts::new(program, &top);
    let count = program.exprs.len();
    built.resize(count, false);

    // The first top-level form that reaches each expression.
    let mut position = vec![NOWHERE; count];
    for (index, &form) in top.iter().enumerate() {
        let mut pending = vec![form];
        while let Some(id) = pending.pop() {
            if position[id.0] == NOWHERE {
                position[id.0] = index as u32;
                pending.extend(children(program, id));
            }
        }
    }

    // Constant globals: defined by one `def` with a value that the root
    // reaches, a direct top-level form, and assigned or defined by nothing
    // else. A `declare` defines no value and does not count.
    let globals = constant_globals(program, &top, &position);

    // Locals that are never substituted, and how often each is read.
    let mut pinned = vec![false; program.locals.len()];
    let mut reads = vec![0u32; program.locals.len()];
    for local in program.field_bindings.keys() {
        pinned[local.0] = true;
    }
    for expr in &program.exprs {
        let methods: &[crate::analyzer::FnMethod] = match &expr.kind {
            ExprKind::Local(local) => {
                reads[local.0] = reads[local.0].saturating_add(1);
                continue;
            }
            ExprKind::Fn { methods, .. } => methods,
            ExprKind::Reify {
                captures, methods, ..
            } => {
                for local in captures {
                    pinned[local.0] = true;
                }
                for (_, method) in methods {
                    method.captures.iter().for_each(|l| pinned[l.0] = true);
                }
                continue;
            }
            ExprKind::DefType { methods, .. } | ExprKind::ExtendType { methods, .. } => {
                for (_, method) in methods {
                    method.captures.iter().for_each(|l| pinned[l.0] = true);
                }
                continue;
            }
            ExprKind::JsTemplate { args, .. } => {
                for arg in args {
                    if let ExprKind::Local(local) = program.exprs[arg.0].kind {
                        pinned[local.0] = true;
                    }
                }
                continue;
            }
            _ => continue,
        };
        for method in methods {
            method.captures.iter().for_each(|l| pinned[l.0] = true);
        }
    }

    // Map keys and set members stay as they are (a local they read keeps its
    // binding).
    let fenced = fenced_positions(program);
    for (index, expr) in program.exprs.iter().enumerate() {
        if let ExprKind::Local(local) = expr.kind
            && fenced[index]
        {
            pinned[local.0] = true;
        }
    }

    // How many expressions the root reaches each one from.
    let mut parents = vec![0u8; count];
    let mut reached = vec![false; count];
    reached[program.root.0] = true;
    let mut pending = vec![program.root];
    while let Some(id) = pending.pop() {
        for child in children(program, id) {
            parents[child.0] = parents[child.0].saturating_add(1);
            if !std::mem::replace(&mut reached[child.0], true) {
                pending.push(child);
            }
        }
    }

    let mut kept = vec![false; count];
    kept[program.root.0] = true;
    for id in program.namespace_entries.keys() {
        kept[id.0] = true;
    }
    for id in &program.macro_functions {
        if id.0 < count {
            kept[id.0] = true;
        }
    }
    let mut folder = Folder {
        program,
        facts,
        position,
        globals,
        pinned,
        reads,
        kept,
        fenced,
        built,
        parents,
        discarded: vec![false; count],
        locals: Vec::new(),
        truths: Vec::new(),
        visited: vec![false; count],
        folded: 0,
    };
    folder.locals = vec![None; folder.program.locals.len()];
    folder.truths = vec![false; folder.program.locals.len()];
    let root = folder.program.root;
    folder.visit(root);
    folder.folded
}

/// By global: the sole `def` with a value that defines it, when the root
/// reaches it (`position` is not [`NOWHERE`]) and nothing else assigns or
/// defines the global (`set!`, `binding`, `with-redefs`, a `js*` hole, a
/// type or protocol). A `declare` defines no value and does not count.
pub(crate) fn sole_definitions(program: &Program, position: &[u32]) -> Vec<Option<ExprId>> {
    let count = program.globals.len();
    let mut definitions: Vec<(u32, Option<ExprId>)> = vec![(0, None); count];
    let mut assigned = vec![false; count];
    for (index, expr) in program.exprs.iter().enumerate() {
        let targets: Vec<ExprId> = match &expr.kind {
            ExprKind::SetGlobal { target, .. } => vec![*target],
            ExprKind::Binding { bindings, .. } => {
                bindings.iter().map(|(target, _)| *target).collect()
            }
            // Raw text may assign its holes.
            ExprKind::JsTemplate { args, .. } => args.clone(),
            ExprKind::Def {
                global, init, once, ..
            } => {
                if position[index] != NOWHERE && (init.is_some() || *once) {
                    let definition = &mut definitions[global.0];
                    definition.0 += 1;
                    definition.1 = Some(ExprId(index));
                }
                continue;
            }
            ExprKind::Global(_) => continue,
            kind => {
                for global in crate::application::globals(kind) {
                    assigned[global.0] = true;
                }
                continue;
            }
        };
        for target in targets {
            if let ExprKind::Global(global) = program.exprs[target.0].kind {
                assigned[global.0] = true;
            }
        }
    }
    definitions
        .into_iter()
        .zip(assigned)
        .map(|(definition, assigned)| match definition {
            (1, definition) if !assigned => definition,
            _ => None,
        })
        .collect()
}

/// For each global, the top-level position of its definition and its value
/// when it is a `^:const` (or `goog-define`) global that [`fold`]
/// substitutes: its sole definition is a direct top-level `def` of a
/// [`Constant`].
fn constant_globals(
    program: &Program,
    top: &[ExprId],
    position: &[u32],
) -> Vec<Option<(u32, ExprKind)>> {
    let definitions = sole_definitions(program, position);
    let mut globals = vec![None; program.globals.len()];
    for (index, binding) in program.globals.iter().enumerate() {
        let Some(def) = definitions[index] else {
            continue;
        };
        let at = position[def.0];
        if !binding.constant || top.get(at as usize) != Some(&def) {
            continue;
        }
        let ExprKind::Def {
            init: Some(init),
            once: false,
            test: None,
            ..
        } = program.exprs[def.0].kind
        else {
            continue;
        };
        let literal = crate::application::unwrap_do(program, init);
        if Constant::of(program, literal).is_some() {
            globals[index] = Some((at, program.exprs[literal.0].kind.clone()));
        }
    }
    globals
}

/// The direct top-level forms of the program.
fn flatten(program: &Program, id: ExprId, top: &mut Vec<ExprId>) {
    if let ExprKind::Do(items) = &program.exprs[id.0].kind {
        for item in items {
            flatten(program, *item, top);
        }
    } else {
        top.push(id);
    }
}

struct Folder<'p> {
    program: &'p mut Program,
    facts: ProgramFacts,
    /// By expression: the first top-level form that reaches it.
    position: Vec<u32>,
    /// By global: the top-level position of its sole definition and its
    /// constant value, for `^:const` globals.
    globals: Vec<Option<(u32, ExprKind)>>,
    /// By local: captured by a closure or type, a type field, or a `js*` hole.
    pinned: Vec<bool>,
    /// By local: the expressions that read it.
    reads: Vec<u32>,
    /// By expression: its identity matters (top-level `do` forms, namespace
    /// entries, macro functions), so it keeps its kind and no other
    /// expression takes it over.
    kept: Vec<bool>,
    /// By expression: a map key or set member, which never becomes a constant
    /// here ([`fenced_positions`]).
    fenced: Vec<bool>,
    /// By expression: a literal built at each evaluation ([`built_literals`]),
    /// a mark that moves with the literal when an expression takes it over.
    built: &'p mut Vec<bool>,
    /// By expression: how many expressions the root reaches it from.
    parents: Vec<u8>,
    /// By expression: a `do` form whose value is discarded, set by the `do`
    /// around it before it is visited.
    discarded: Vec<bool>,
    /// By local: the constant substituted for it.
    locals: Vec<Option<ExprKind>>,
    /// By local: bound by a `let` to a literal collection or `fn`, so true.
    truths: Vec<bool>,
    visited: Vec<bool>,
    folded: usize,
}

impl Folder<'_> {
    fn visit(&mut self, id: ExprId) {
        if std::mem::replace(&mut self.visited[id.0], true) {
            return;
        }
        let kind = std::mem::replace(&mut self.program.exprs[id.0].kind, ExprKind::Nil);
        let kind = self.rewrite(id, kind);
        self.program.exprs[id.0].kind = kind;
    }

    /// `child` produces the value of `parent`, so it is discarded when
    /// `parent` is and nothing else reads `parent`.
    fn pass_discard(&mut self, parent: ExprId, child: ExprId) {
        if !self.visited[child.0] {
            self.discarded[child.0] = self.discarded[parent.0] && self.parents[parent.0] <= 1;
        }
    }

    /// The constant `id` evaluates to, once visited.
    fn constant(&self, id: ExprId) -> Option<Constant> {
        Constant::of(self.program, id)
    }

    /// The kind of `child`, for its parent to take over, unless the child's
    /// identity matters.
    fn adopt(&mut self, parent: ExprId, child: ExprId) -> Option<ExprKind> {
        if self.kept[child.0]
            || (self.fenced[parent.0]
                && constant_like(self.program, &self.program.exprs[child.0].kind))
        {
            return None;
        }
        self.built[parent.0] = self.built[child.0];
        self.folded += 1;
        Some(self.program.exprs[child.0].kind.clone())
    }

    fn rewrite(&mut self, id: ExprId, kind: ExprKind) -> ExprKind {
        match kind {
            ExprKind::Global(global) => match &self.globals[global.0] {
                Some((at, value))
                    if !self.fenced[id.0]
                        && self.position[id.0] != NOWHERE
                        && self.position[id.0] > *at =>
                {
                    self.folded += 1;
                    value.clone()
                }
                _ => kind,
            },
            ExprKind::Local(local) => match &self.locals[local.0] {
                Some(value) => {
                    self.folded += 1;
                    value.clone()
                }
                None => kind,
            },
            ExprKind::Let { bindings, body } => {
                // Substitution rewrites the reads while it visits the body, so
                // a body another parent already visited keeps its bindings.
                let shared = self.visited[body.0];
                let mut kept = Vec::with_capacity(bindings.len());
                for (local, init) in bindings {
                    self.visit(init);
                    let value = &self.program.exprs[init.0].kind;
                    if self.pinned[local.0] || shared {
                        kept.push((local, init));
                    } else if Constant::of_kind(self.program, value).is_some() {
                        self.locals[local.0] = Some(value.clone());
                        self.folded += 1;
                    } else if self.reads[local.0] == 0 && effect_free(self.program, value) {
                        self.folded += 1;
                    } else {
                        // A literal collection or `fn` is true wherever the
                        // local is tested.
                        if literal_truth(self.program, value) == Some(true) {
                            self.truths[local.0] = true;
                        }
                        kept.push((local, init));
                    }
                }
                self.pass_discard(id, body);
                self.visit(body);
                if kept.is_empty()
                    && !scoped(self.program, body)
                    && let Some(kind) = self.adopt(id, body)
                {
                    return kind;
                }
                ExprKind::Let {
                    bindings: kept,
                    body,
                }
            }
            ExprKind::If { test, yes, no } => {
                self.visit(test);
                self.pass_discard(id, yes);
                self.pass_discard(id, no);
                let truth = match self.program.exprs[test.0].kind {
                    ExprKind::Local(local) if self.truths[local.0] => Some(true),
                    ref kind => literal_truth(self.program, kind),
                };
                if let Some(truth) = truth {
                    let branch = if truth { yes } else { no };
                    self.visit(branch);
                    if let Some(kind) = self.adopt(id, branch) {
                        return kind;
                    }
                }
                self.visit(yes);
                self.visit(no);
                ExprKind::If { test, yes, no }
            }
            ExprKind::BooleanHint(inner) => {
                self.visit(inner);
                if self.constant(inner).is_some()
                    && let Some(kind) = self.adopt(id, inner)
                {
                    return kind;
                }
                ExprKind::BooleanHint(inner)
            }
            ExprKind::Do(items) => {
                // Forms before the last are discarded, and so is the last one
                // of a discarded `do` that nothing else reads.
                let discarded = self.discarded[id.0] && self.parents[id.0] <= 1;
                let last = items.len().saturating_sub(1);
                for (index, item) in items.iter().enumerate() {
                    if !self.visited[item.0] {
                        self.discarded[item.0] = discarded || index != last;
                    }
                    self.visit(*item);
                }
                if self.kept[id.0] || items.is_empty() {
                    return ExprKind::Do(items);
                }
                let forms: Vec<ExprId> = items
                    .iter()
                    .enumerate()
                    .filter(|(index, item)| {
                        (*index == last && !discarded)
                            || self.kept[item.0]
                            || !effect_free(self.program, &self.program.exprs[item.0].kind)
                    })
                    .map(|(_, item)| *item)
                    .collect();
                self.folded += items.len() - forms.len();
                if forms.is_empty() {
                    return ExprKind::Nil;
                }
                if let [form] = forms[..]
                    && !scoped(self.program, form)
                    && let Some(kind) = self.adopt(id, form)
                {
                    return kind;
                }
                ExprKind::Do(forms)
            }
            ExprKind::Call { callee, args } => {
                self.visit(callee);
                for arg in &args {
                    self.visit(*arg);
                }
                if let ExprKind::CoreFn(core) = self.program.exprs[callee.0].kind {
                    if !self.fenced[id.0]
                        && let Some(value) = self.call(core, &args)
                    {
                        self.folded += 1;
                        return value.into_kind(self.program);
                    }
                    if let Some(args) = self.prefix(core, &args) {
                        self.folded += 1;
                        return ExprKind::Call { callee, args };
                    }
                }
                ExprKind::Call { callee, args }
            }
            // These read a global's binding rather than its value.
            ExprKind::Var { .. } | ExprKind::Exists(_) | ExprKind::BindingExists(_) => kind,
            // Assignment targets are not reads.
            ExprKind::SetGlobal { target, value } => {
                if !matches!(
                    self.program.exprs[target.0].kind,
                    ExprKind::Global(_) | ExprKind::Local(_) | ExprKind::CoreFn(_)
                ) {
                    self.visit(target);
                }
                self.visit(value);
                ExprKind::SetGlobal { target, value }
            }
            ExprKind::Binding { bindings, body } => {
                for (_, init) in &bindings {
                    self.visit(*init);
                }
                self.visit(body);
                ExprKind::Binding { bindings, body }
            }
            // Case tests are the constants it dispatches on, as analyzed.
            ExprKind::Case {
                strict,
                keyword_tests,
                test,
                branches,
                default,
            } => {
                self.visit(test);
                for (_, branch) in &branches {
                    self.visit(*branch);
                }
                if let Some(default) = default {
                    self.visit(default);
                }
                ExprKind::Case {
                    strict,
                    keyword_tests,
                    test,
                    branches,
                    default,
                }
            }
            kind => {
                for child in kind_children(self.program, &kind) {
                    self.visit(child);
                }
                kind
            }
        }
    }

    /// The arguments of a call of `+`, `-`, `*` or `/` whose leading number
    /// literals are folded into one: the runtime folds its operands from the
    /// left (`numericFold`) and JavaScript's operators associate to the left,
    /// so `(+ 1 2 x)` is `(+ 3 x)` whatever `x` is.
    fn prefix(&mut self, core: CoreFnId, args: &[ExprId]) -> Option<Vec<ExprId>> {
        if !matches!(
            core,
            CoreFnId::Add | CoreFnId::Subtract | CoreFnId::Multiply | CoreFnId::Divide
        ) || self.facts.replaced(core)
        {
            return None;
        }
        let numbers: Vec<Constant> = args
            .iter()
            .map_while(|arg| match self.program.exprs[arg.0].kind {
                ExprKind::Number(value) => Some(Constant::Number(value)),
                _ => None,
            })
            .collect();
        if numbers.len() < 2 || numbers.len() == args.len() {
            return None;
        }
        let value = evaluate(self.program, core, &numbers)?;
        let mut expr = self.program.exprs[args[0].0].clone();
        expr.kind = value.into_kind(self.program);
        self.program.exprs.push(expr);
        let folded = ExprId(self.program.exprs.len() - 1);
        Some(
            std::iter::once(folded)
                .chain(args[numbers.len()..].iter().copied())
                .collect(),
        )
    }

    /// The value of a call of `core` on `args` when all are constants and the
    /// call is `pure`.
    fn call(&self, core: CoreFnId, args: &[ExprId]) -> Option<Constant> {
        let constants = args
            .iter()
            .map(|arg| self.constant(*arg))
            .collect::<Option<Vec<_>>>()?;
        let types: Vec<Type> = constants.iter().map(Constant::ty).collect();
        let call = core.operation().call(&types);
        if call.purity != Purity::Pure || !self.facts.trusted(core, call.calls) {
            return None;
        }
        evaluate(self.program, core, &constants)
    }
}

/// Whether `id` depends on the function scope emission gives it: outside
/// nested functions it reads `arguments`, holds `js*` raw text (which may read
/// `this` or `arguments`) or is a definition with `:test` metadata. A `let` or
/// `do` in value position is emitted as a function call, so such contents
/// keep the form around them (the emitter's `sequence_keeps_scope` asks the
/// same).
pub(crate) fn scoped(program: &Program, id: ExprId) -> bool {
    let mut pending = vec![id];
    while let Some(id) = pending.pop() {
        match program.exprs[id.0].kind {
            ExprKind::JsArguments
            | ExprKind::CopyArguments(_)
            | ExprKind::JsTemplate { .. }
            | ExprKind::Def { test: Some(_), .. } => return true,
            ExprKind::Fn { .. } => (),
            _ => pending.extend(children(program, id)),
        }
    }
    false
}

/// By expression: a key of a map literal or a member of a set literal. These
/// never become constants here: the emitter builds a map whose keys are all
/// constants from a key shape and a set whose members are all scalars without
/// the duplicate check of members computed at run time, which assume the
/// distinct keys analysis checks in source literals.
pub(crate) fn fenced_positions(program: &Program) -> Vec<bool> {
    let mut fenced = vec![false; program.exprs.len()];
    for expr in &program.exprs {
        match &expr.kind {
            ExprKind::Map(items) | ExprKind::LiteralMap(items) => {
                for key in items.iter().step_by(2) {
                    fenced[key.0] = true;
                }
            }
            ExprKind::Set(items) | ExprKind::LiteralSet(items) => {
                for member in items {
                    fenced[member.0] = true;
                }
            }
            _ => (),
        }
    }
    fenced
}

/// By expression: the literals that the program builds at each evaluation
/// (collection literals and `with-meta` with an item that is not a
/// constant). Folding and inlining carry the mark where they move or copy a
/// literal, and [`keep_built`] keeps every marked literal built.
pub fn built_literals(program: &Program) -> Vec<bool> {
    program
        .exprs
        .iter()
        .map(|expr| pooled_kind(&expr.kind) && !constant_like(program, &expr.kind))
        .collect()
}

/// Keep each literal [`built_literals`] marked built at its site where
/// folding or inlining made all its items constants, which the emitter would
/// otherwise declare once in the constant pool, so that evaluations that
/// built distinct values shared one: one item (a map's first value, so that
/// its constant keys keep their shape) becomes a one-form `do`, which
/// emission writes as the item itself but does not pool.
pub fn keep_built(program: &mut Program, built: &[bool]) {
    for index in 0..program.exprs.len() {
        if !built.get(index).copied().unwrap_or(false)
            || !constant_like(program, &program.exprs[index].kind)
        {
            continue;
        }
        // A map keeps its constant keys, and so its key shape, and wraps its
        // first value.
        let slot = match &program.exprs[index].kind {
            ExprKind::Map(items) | ExprKind::LiteralMap(items) => (items.len() > 1).then_some(1),
            ExprKind::Vector(items)
            | ExprKind::List(items)
            | ExprKind::Set(items)
            | ExprKind::LiteralSet(items) => (!items.is_empty()).then_some(0),
            ExprKind::WithMeta { .. } => Some(0),
            _ => None,
        };
        let Some(slot) = slot else {
            continue;
        };
        let item = match &program.exprs[index].kind {
            ExprKind::WithMeta { value, .. } => *value,
            ExprKind::Vector(items)
            | ExprKind::List(items)
            | ExprKind::Map(items)
            | ExprKind::Set(items)
            | ExprKind::LiteralMap(items)
            | ExprKind::LiteralSet(items) => items[slot],
            _ => unreachable!("a literal"),
        };
        let mut expr = program.exprs[item.0].clone();
        expr.kind = ExprKind::Do(vec![item]);
        program.exprs.push(expr);
        let wrapped = ExprId(program.exprs.len() - 1);
        match &mut program.exprs[index].kind {
            ExprKind::Vector(items)
            | ExprKind::List(items)
            | ExprKind::Map(items)
            | ExprKind::Set(items)
            | ExprKind::LiteralMap(items)
            | ExprKind::LiteralSet(items) => items[slot] = wrapped,
            ExprKind::WithMeta { value, .. } => *value = wrapped,
            _ => unreachable!("a literal"),
        }
    }
}

/// Whether a literal of this kind is one the emitter may pool.
fn pooled_kind(kind: &ExprKind) -> bool {
    matches!(
        kind,
        ExprKind::Vector(_)
            | ExprKind::List(_)
            | ExprKind::Map(_)
            | ExprKind::Set(_)
            | ExprKind::LiteralMap(_)
            | ExprKind::LiteralSet(_)
            | ExprKind::WithMeta { .. }
    )
}

/// Whether an expression of this kind may be a constant the emitter pools:
/// scalars, quoted symbols, and collection literals of such expressions. A
/// superset of the emitter's own classification.
pub(crate) fn constant_like(program: &Program, kind: &ExprKind) -> bool {
    match kind {
        ExprKind::Nil
        | ExprKind::Bool(_)
        | ExprKind::Number(_)
        | ExprKind::String(_)
        | ExprKind::Keyword(_)
        | ExprKind::Symbol(_) => true,
        ExprKind::Vector(items)
        | ExprKind::List(items)
        | ExprKind::Map(items)
        | ExprKind::Set(items)
        | ExprKind::LiteralMap(items)
        | ExprKind::LiteralSet(items)
        | ExprKind::ReaderLiteral { items, .. } => items
            .iter()
            .all(|item| constant_like(program, &program.exprs[item.0].kind)),
        ExprKind::WithMeta { value, metadata } => [value, metadata]
            .iter()
            .all(|item| constant_like(program, &program.exprs[item.0].kind)),
        _ => false,
    }
}

/// Whether an `if` on an expression of this kind takes its first branch,
/// when evaluating it has no effect and the answer is known: the truth of a
/// constant, and `true` for quoted symbols, `fn` and collection literals
/// without effects, which are never `nil` or `false`.
fn literal_truth(program: &Program, kind: &ExprKind) -> Option<bool> {
    if let Some(constant) = Constant::of_kind(program, kind) {
        return Some(constant.truthy());
    }
    match kind {
        ExprKind::Symbol(_)
        | ExprKind::Fn { .. }
        | ExprKind::Vector(_)
        | ExprKind::List(_)
        | ExprKind::Map(_)
        | ExprKind::Set(_)
        | ExprKind::LiteralMap(_)
        | ExprKind::LiteralSet(_) => effect_free(program, kind).then_some(true),
        _ => None,
    }
}

/// Whether evaluating an expression of this kind has no effect and cannot
/// throw: literals, reads of locals, globals and core values, `fn`, and
/// vector and list literals of such expressions, and map and set literals
/// whose keys and members are scalar literals (analysis rejects equal literal
/// keys, which would throw) and whose values are such expressions.
pub(crate) fn effect_free(program: &Program, kind: &ExprKind) -> bool {
    let scalar = |id: &ExprId| {
        matches!(
            program.exprs[id.0].kind,
            ExprKind::Nil
                | ExprKind::Bool(_)
                | ExprKind::Number(_)
                | ExprKind::String(_)
                | ExprKind::Keyword(_)
                | ExprKind::Symbol(_)
        )
    };
    let free = |id: &ExprId| effect_free(program, &program.exprs[id.0].kind);
    match kind {
        ExprKind::Nil
        | ExprKind::Bool(_)
        | ExprKind::Number(_)
        | ExprKind::String(_)
        | ExprKind::Keyword(_)
        | ExprKind::Symbol(_)
        | ExprKind::Local(_)
        | ExprKind::Global(_)
        | ExprKind::CoreFn(_)
        | ExprKind::Fn { .. } => true,
        ExprKind::Vector(items) | ExprKind::List(items) => items.iter().all(free),
        ExprKind::Map(items) | ExprKind::LiteralMap(items) => items
            .as_chunks::<2>()
            .0
            .iter()
            .all(|[key, value]| scalar(key) && free(value)),
        ExprKind::Set(items) | ExprKind::LiteralSet(items) => items.iter().all(scalar),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::js_number;

    #[test]
    fn numbers_print_as_javascript_prints_them() {
        for (value, text) in [
            (0.0, "0"),
            (-0.0, "0"),
            (1.0, "1"),
            (-1.5, "-1.5"),
            (100.0, "100"),
            (0.1, "0.1"),
            (0.1 + 0.2, "0.30000000000000004"),
            (1e21, "1e+21"),
            (1e20, "100000000000000000000"),
            (123456789012345680000.0, "123456789012345680000"),
            (1.5e300, "1.5e+300"),
            (0.000001, "0.000001"),
            (1e-7, "1e-7"),
            (1.25e-7, "1.25e-7"),
            (9007199254740992.0, "9007199254740992"),
            (f64::MAX, "1.7976931348623157e+308"),
            (5e-324, "5e-324"),
            (f64::NAN, "NaN"),
            (f64::INFINITY, "Infinity"),
            (f64::NEG_INFINITY, "-Infinity"),
        ] {
            assert_eq!(js_number(value), text, "{value:e}");
        }
    }
}
