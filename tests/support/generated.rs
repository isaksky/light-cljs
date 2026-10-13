//! Test-only source construction, not a second compiler/evaluator. All semantics
//! come from the shared Node protocol and the explicitly selected CLJS oracle.
//! Both generators are seeded, bounded and platform-independent:
//!
//! - v1 ([`program`]): one closed expression with ordered `observe` effects. The
//!   execution binary runs it under metamorphic variants
//!   (support/generated_execution.rs).
//! - namespaces ([`namespace_program`]): a DAG of namespaces with definitions,
//!   protocols, types and escapes, plus tagged known-dead garbage. The compiler
//!   binary checks optimization levels against it
//!   (tests/compiler/generated_optimization.rs).
//!
//! `LIGHT_CLJS_GENERATED_SEED=<u64>` replays one seed and
//! `LIGHT_CLJS_GENERATED_COUNT=<n>` runs `n` seeds instead of the fixed twelve.
//! This file has no dependency on the test binary that includes it.

pub(crate) const SEEDS: [u64; 12] = [0, 1, 2, 3, 17, 42, 99, 255, 1024, 65535, 0x5eed, u64::MAX];
const DEPTH: usize = 4;
const NODES: usize = 32;
pub(crate) const VARIANTS: [&str; 4] = ["original", "alpha", "identity", "do"];

#[derive(Clone, Copy, PartialEq)]
enum Shape {
    Value,
    Callable(usize),
}

#[derive(Clone, Debug)]
pub(crate) enum Form {
    Literal(String),
    Local(u32),
    If(Box<Form>, Box<Form>, Box<Form>),
    Do(Vec<Form>),
    Let(u32, Box<Form>, Box<Form>),
    Function(Vec<u32>, Box<Form>),
    Call(Box<Form>, Vec<Form>),
    Observe(Box<Form>),
}

impl Form {
    fn literal(source: &str) -> Self {
        Self::Literal(source.into())
    }

    fn observed(self) -> Self {
        Self::Observe(Box::new(self))
    }

    fn children(&self) -> Vec<&Form> {
        match self {
            Self::Literal(_) | Self::Local(_) => vec![],
            Self::If(a, b, c) => vec![a, b, c],
            Self::Do(forms) => forms.iter().collect(),
            Self::Let(_, a, b) => vec![a, b],
            Self::Function(_, body) | Self::Observe(body) => vec![body],
            Self::Call(callee, args) => std::iter::once(callee.as_ref()).chain(args).collect(),
        }
    }

    pub(crate) fn nodes(&self) -> usize {
        1 + self
            .children()
            .iter()
            .map(|form| form.nodes())
            .sum::<usize>()
    }

    pub(crate) fn depth(&self) -> usize {
        1 + self
            .children()
            .iter()
            .map(|form| form.depth())
            .max()
            .unwrap_or(0)
    }

    // Each binder has a distinct identity; changing only its printable spelling
    // renames all and only its references. Neither prefix can capture observe.
    pub(crate) fn source(&self, alpha: bool) -> String {
        self.source_with(alpha, "observe")
    }

    /// The source with `observer` called for each observation, for example a
    /// host function such as `js/lightObserve` instead of the `observe` local.
    fn source_with(&self, alpha: bool, observer: &str) -> String {
        let name = |id| format!("{}{id}", if alpha { "renamed" } else { "g" });
        let emit = |form: &Form| form.source_with(alpha, observer);
        match self {
            Self::Literal(source) => source.clone(),
            Self::Local(id) => name(id),
            Self::If(test, yes, no) => format!("(if {} {} {})", emit(test), emit(yes), emit(no)),
            Self::Do(forms) => format!(
                "(do {})",
                forms.iter().map(emit).collect::<Vec<_>>().join(" ")
            ),
            Self::Let(id, init, body) => {
                format!("(let* [{} {}] {})", name(id), emit(init), emit(body))
            }
            Self::Function(params, body) => format!(
                "(fn* [{}] {})",
                params.iter().map(name).collect::<Vec<_>>().join(" "),
                emit(body)
            ),
            Self::Call(callee, args) => format!(
                "({}{})",
                emit(callee),
                args.iter()
                    .map(|arg| format!(" {}", emit(arg)))
                    .collect::<String>()
            ),
            Self::Observe(value) => format!("({observer} {})", emit(value)),
        }
    }
}

struct Generator {
    state: u64,
    next_id: u32,
}

impl Generator {
    // Explicit wrapping arithmetic and upper 32 bits make v1 stable across
    // platforms and Rust versions. No dependency on rand or HashMap ordering.
    fn choose(&mut self, count: usize) -> usize {
        assert!(count > 0);
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.state >> 32) as usize) % count
    }

    fn fresh(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn minimum(shape: Shape) -> usize {
        if shape == Shape::Value { 1 } else { 2 }
    }

    fn leaf(&mut self, shape: Shape, scope: &[(u32, Shape)]) -> Form {
        let available: Vec<_> = scope.iter().filter(|(_, found)| *found == shape).collect();
        if !available.is_empty() && self.choose(2) == 0 {
            return Form::Local(available[self.choose(available.len())].0);
        }
        match shape {
            Shape::Value => {
                const LITERALS: [&str; 10] = [
                    "nil",
                    "false",
                    "true",
                    "0",
                    "-0.0",
                    "1",
                    "-7",
                    "2.5",
                    "\"\"",
                    "\"text\\nλ\"",
                ];
                Form::literal(LITERALS[self.choose(LITERALS.len())])
            }
            Shape::Callable(arity) => {
                let params: Vec<_> = (0..arity).map(|_| self.fresh()).collect();
                let mut inner = scope.to_vec();
                inner.extend(params.iter().map(|id| (*id, Shape::Value)));
                Form::Function(params, Box::new(self.leaf(Shape::Value, &inner)))
            }
        }
    }

    fn expression(
        &mut self,
        shape: Shape,
        depth: usize,
        budget: usize,
        scope: &[(u32, Shape)],
    ) -> Form {
        let minimum = Self::minimum(shape);
        assert!(budget >= minimum);
        if depth == 0 || budget <= minimum {
            return self.leaf(shape, scope);
        }
        let form = match self.choose(7) {
            0 if budget >= 2 + 2 * minimum => {
                let branch = (budget - 2) / 2;
                Form::If(
                    Box::new(self.expression(Shape::Value, depth - 1, 1, scope)),
                    Box::new(self.expression(shape, depth - 1, branch, scope)),
                    Box::new(self.expression(shape, depth - 1, branch, scope)),
                )
            }
            1 if budget >= 2 + minimum => {
                let first = (budget - 1 - minimum) / 2 + 1;
                Form::Do(vec![
                    self.expression(Shape::Value, depth - 1, first, scope),
                    self.expression(shape, depth - 1, budget - 1 - first, scope),
                ])
            }
            2 if budget >= 3 + minimum => {
                let init_shape = if self.choose(2) == 0 {
                    Shape::Value
                } else {
                    Shape::Callable(self.choose(3))
                };
                let init_budget = Self::minimum(init_shape).max((budget - 1 - minimum) / 2);
                // The new binding is deliberately absent in its initializer:
                // closures can only call older bindings, so there is no cycle.
                let init = self.expression(init_shape, depth - 1, init_budget, scope);
                let id = self.fresh();
                let mut inner = scope.to_vec();
                inner.push((id, init_shape));
                Form::Let(
                    id,
                    Box::new(init),
                    Box::new(self.expression(shape, depth - 1, budget - 1 - init_budget, &inner)),
                )
            }
            3 if shape == Shape::Value && budget >= 3 => {
                let arity = self.choose(3).min(budget - 3);
                let callee =
                    self.expression(Shape::Callable(arity), depth - 1, budget - 1 - arity, scope);
                let args = (0..arity)
                    .map(|_| self.expression(Shape::Value, depth - 1, 1, scope))
                    .collect();
                Form::Call(Box::new(callee), args)
            }
            4 if shape == Shape::Value => self
                .expression(shape, depth - 1, budget - 1, scope)
                .observed(),
            5 if matches!(shape, Shape::Callable(_)) => {
                let Shape::Callable(arity) = shape else {
                    unreachable!()
                };
                let params: Vec<_> = (0..arity).map(|_| self.fresh()).collect();
                let mut inner = scope.to_vec();
                inner.extend(params.iter().map(|id| (*id, Shape::Value)));
                Form::Function(
                    params,
                    Box::new(self.expression(Shape::Value, depth - 1, budget - 1, &inner)),
                )
            }
            _ => self.leaf(shape, scope),
        };
        assert!(form.nodes() <= budget, "generator exceeded its node budget");
        form
    }
}

pub(crate) fn program(seed: u64) -> Form {
    let mut generator = Generator {
        state: seed,
        next_id: 0,
    };
    let captured = generator.fresh();
    let function = generator.fresh();
    let parameter = generator.fresh();
    let result = generator.fresh();
    let scope = [(captured, Shape::Value), (function, Shape::Callable(1))];
    let body = generator.expression(
        Shape::Value,
        DEPTH,
        NODES,
        &[(captured, Shape::Value), (parameter, Shape::Value)],
    );
    let expression = generator.expression(Shape::Value, DEPTH, NODES, &scope);
    // Every seed runs ordered initializer, callee, argument, function-body and
    // result observations, even when its random branches are all literals.
    Form::Let(
        captured,
        Box::new(Form::literal("\"capture\"").observed()),
        Box::new(Form::Let(
            function,
            Box::new(Form::Function(
                vec![parameter],
                Box::new(Form::Do(vec![
                    Form::literal("\"body\"").observed(),
                    Form::Local(captured).observed(),
                    Form::Local(parameter).observed(),
                    body,
                ])),
            )),
            Box::new(Form::Let(
                result,
                Box::new(Form::Do(vec![
                    expression.observed(),
                    Form::Call(
                        Box::new(Form::Do(vec![
                            Form::literal("\"callee\"").observed(),
                            Form::Local(function),
                        ])),
                        vec![Form::literal("\"argument\"").observed()],
                    ),
                ])),
                Box::new(Form::Do(vec![
                    Form::literal("\"end\"").observed(),
                    Form::Local(result),
                ])),
            )),
        )),
    )
}

/// The fixed [`SEEDS`], or the seeds the environment selects:
/// `LIGHT_CLJS_GENERATED_SEED` replays exactly one seed and
/// `LIGHT_CLJS_GENERATED_COUNT=n` takes the first `n` of the fixed seeds
/// followed by 4, 5, 6, ... (skipping fixed ones) for longer local runs.
pub(crate) fn seeds() -> Vec<u64> {
    let variable = |name| match std::env::var(name) {
        Ok(value) => Some(
            value
                .parse::<u64>()
                .unwrap_or_else(|_| panic!("{name} must be a decimal u64")),
        ),
        Err(std::env::VarError::NotPresent) => None,
        Err(error) => panic!("invalid {name}: {error}"),
    };
    match (
        variable("LIGHT_CLJS_GENERATED_SEED"),
        variable("LIGHT_CLJS_GENERATED_COUNT"),
    ) {
        (Some(_), Some(_)) => panic!("select one seed or a seed count, not both"),
        (Some(seed), None) => vec![seed],
        (None, Some(count)) => SEEDS
            .into_iter()
            .chain((0..).filter(|seed| !SEEDS.contains(seed)))
            .take(usize::try_from(count).expect("seed count fits in memory"))
            .collect(),
        (None, None) => SEEDS.to_vec(),
    }
}

// Namespace programs with tagged known-dead garbage. Everything observable is a
// host value: effects call `js/lightObserve` (the outcome protocol's per-case
// `observe`) with numbers, primitives or `pr-str` strings, and the entry returns
// a string, so no runtime inspection hook is needed and the harness keeps
// working when advanced output slices the runtime.

const PROGRAM_STREAM: u64 = 0x6e61_6d65_7370_6163;
const GARBAGE_STREAM: u64 = 0x6761_7262_6167_6521;
const OBSERVE: &str = "js/lightObserve";
/// Protocol methods of `Proto` and their parameter counts, receiver included.
const METHODS: [(&str, usize); 3] = [("pm0", 1), ("pm1", 2), ("pm2", 1)];
const KEYWORDS: [&str; 3] = [":a", ":b", ":c"];
/// Bound on the source of one program with every garbage item and escape.
const PROGRAM_BYTES: usize = 64 * 1024;

/// Why a garbage item is dead by construction. Each tag is one shape that one
/// kind of optimization removes; tests/compiler/generated_optimization.rs
/// records which tags still survive (`EXPECTED`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum GarbageTag {
    UnreferencedNamespace,
    UnusedLiteralDef,
    UnusedPureDef,
    UnusedDefn,
    UnusedKeyword,
    UninstantiatedType,
    UncalledProtocolMethod,
}
impl GarbageTag {
    pub(crate) const ALL: [Self; 7] = [
        Self::UnreferencedNamespace,
        Self::UnusedLiteralDef,
        Self::UnusedPureDef,
        Self::UnusedDefn,
        Self::UnusedKeyword,
        Self::UninstantiatedType,
        Self::UncalledProtocolMethod,
    ];

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::UnreferencedNamespace => "unreferenced-namespace",
            Self::UnusedLiteralDef => "unused-literal-def",
            Self::UnusedPureDef => "unused-pure-def",
            Self::UnusedDefn => "unused-defn",
            Self::UnusedKeyword => "unused-keyword",
            Self::UninstantiatedType => "uninstantiated-type",
            Self::UncalledProtocolMethod => "uncalled-protocol-method",
        }
    }

    /// Why every item with this tag is dead when its escape edge is absent.
    pub(crate) fn reason(self) -> &'static str {
        match self {
            Self::UnreferencedNamespace => {
                "a required namespace without requires or effects whose definitions nothing names"
            }
            Self::UnusedLiteralDef => "a def nothing reads, with a literal initializer",
            Self::UnusedPureDef => {
                "a def nothing reads, initialized by pure core calls on literals"
            }
            Self::UnusedDefn => "a defn nothing calls, reads or passes",
            Self::UnusedKeyword => "a keyword that occurs only in a def or defn nothing reads",
            Self::UninstantiatedType => {
                "a defrecord or deftype implementing Proto that nothing constructs"
            }
            Self::UncalledProtocolMethod => {
                "a Proto method that every implementation defines and nothing calls"
            }
        }
    }
}

/// The reachable edge the escape variant adds to a garbage item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Escape {
    /// `(var name)`, dereferenced or called.
    Var,
    /// `with-redefs` of the Var, then its original value.
    WithRedefs,
    /// `resolve` of the quoted qualified symbol, dereferenced or called.
    Resolve,
    /// The function passed to `mapv` as a value.
    FnAsData,
    /// A protocol method called on a constructed instance.
    ProtocolCall,
}
impl Escape {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Var => "var",
            Self::WithRedefs => "with-redefs",
            Self::Resolve => "resolve",
            Self::FnAsData => "fn-as-data",
            Self::ProtocolCall => "protocol-call",
        }
    }
}

/// One known-dead item of a namespace program.
#[derive(Clone, Debug)]
pub(crate) struct Garbage {
    pub(crate) tag: GarbageTag,
    /// `GARBAGE-nnn`, a string or keyword name in the item's code and nowhere
    /// else in the program, so emitted JavaScript contains it exactly when some
    /// of that code survived.
    pub(crate) sentinel: String,
    pub(crate) namespace: String,
    /// The edge that makes the item live in the escape variant, if any.
    pub(crate) escape: Option<Escape>,
}

/// Which rendering includes a piece of source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Gate {
    Program,
    Garbage(usize),
    Escape(usize),
}

#[derive(Clone, Debug)]
struct Item {
    gate: Gate,
    open: String,
    /// Gated text before `close`: garbage protocol methods, their
    /// implementations and `:require` entries.
    extra: Vec<(Gate, String)>,
    close: String,
    /// Gated items rendered right after this one.
    after: Vec<Item>,
    /// How many program globals are defined once this item has run.
    defined: usize,
}
impl Item {
    fn new(gate: Gate, text: String) -> Self {
        Self {
            gate,
            open: text,
            extra: Vec::new(),
            close: String::new(),
            after: Vec::new(),
            defined: 0,
        }
    }

    /// Gated text joins the line before it and breaks no lines of its own, so
    /// adding garbage or escapes moves no program form to another line or
    /// column, and source metadata such as a var's `:line` stays the same.
    fn render(&self, admits: &dyn Fn(Gate) -> bool, out: &mut String) {
        fn push(out: &mut String, gate: Gate, text: &str) {
            if gate == Gate::Program {
                out.push_str(text);
            } else {
                out.extend(text.chars().map(|c| if c == '\n' { ' ' } else { c }));
            }
        }
        if !admits(self.gate) {
            return;
        }
        if self.gate != Gate::Program && out.ends_with('\n') {
            out.pop();
            out.push(' ');
        }
        push(out, self.gate, &self.open);
        for (gate, text) in &self.extra {
            if admits(*gate) {
                push(out, *gate, text);
            }
        }
        push(out, self.gate, &self.close);
        out.push('\n');
        for item in &self.after {
            item.render(admits, out);
        }
    }
}

#[derive(Clone, Debug)]
struct Namespace {
    name: String,
    gate: Gate,
    items: Vec<Item>,
}

/// A generated application: namespaces `gen.n0` ... `gen.nK` whose `:require`
/// edges form a DAG in which every namespace is reachable from the entry
/// `gen.nK`, with tagged garbage woven in. The program text without garbage
/// never depends on the garbage: they come from separate random streams.
#[derive(Clone, Debug)]
pub(crate) struct NamespaceProgram {
    pub(crate) seed: u64,
    pub(crate) entry: String,
    pub(crate) garbage: Vec<Garbage>,
    /// The entry ends with a call at an arity the callee lacks, so the program
    /// throws an `Error` after its other effects (a deliberate rare outcome).
    pub(crate) throws: bool,
    namespaces: Vec<Namespace>,
}
impl NamespaceProgram {
    /// `(relative path, source)` of every namespace when the garbage items that
    /// `garbage` selects are present, with the escape edges of the selected
    /// escaped items when `escapes` is set.
    pub(crate) fn sources(
        &self,
        garbage: &dyn Fn(usize) -> bool,
        escapes: bool,
    ) -> Vec<(String, String)> {
        let admits = |gate| match gate {
            Gate::Program => true,
            Gate::Garbage(index) => garbage(index),
            Gate::Escape(index) => escapes && garbage(index),
        };
        self.namespaces
            .iter()
            .filter(|namespace| admits(namespace.gate))
            .map(|namespace| {
                let mut source = String::new();
                for item in &namespace.items {
                    item.render(&admits, &mut source);
                }
                (format!("{}.cljs", namespace.name.replace('.', "/")), source)
            })
            .collect()
    }
}

#[derive(Clone, Debug)]
enum Kind {
    Number,
    /// A vector of numbers.
    Vector,
    /// A map from `:a` and `:b` to numbers.
    Map,
    /// One of `:a`, `:b` and `:c`.
    Keyword,
    /// Numbers to a number; `rest` is the fixed parameter count before `&`.
    Function {
        fixed: Vec<usize>,
        rest: Option<usize>,
    },
    /// A defrecord or deftype with fields `a` and `b` implementing these
    /// `METHODS` of `Proto`.
    Type {
        methods: Vec<usize>,
    },
}

#[derive(Clone, Debug)]
struct Global {
    namespace: usize,
    name: String,
    kind: Kind,
}

/// Where generated code runs: its namespace, the globals defined before it
/// (`globals[..limit]`) and whether it may call protocol methods.
#[derive(Clone, Copy)]
struct Scope {
    namespace: usize,
    limit: usize,
    protocol: bool,
}

struct Implementation {
    namespace: usize,
    item: usize,
    /// The implementing type's global, or `None` for `extend-type number`.
    instance: Option<usize>,
}

struct Builder {
    rng: Generator,
    namespaces: Vec<Namespace>,
    requires: Vec<Vec<usize>>,
    /// `ancestors[i][j]`: namespace `j` has loaded before `i`'s body runs.
    ancestors: Vec<Vec<bool>>,
    globals: Vec<Global>,
    protocol: usize,
    methods: usize,
    protocol_item: Option<usize>,
    implementations: Vec<Implementation>,
    /// The namespace of `extend-type number` and the methods it implements.
    number: Option<(usize, Vec<usize>)>,
    names: usize,
    throws: bool,
    garbage: Vec<Garbage>,
}

/// Generate the namespace program for `seed`. Programs terminate: functions and
/// method bodies only call what was defined before them, multi-arity functions
/// only delegate to a larger arity, and nothing loops.
pub(crate) fn namespace_program(seed: u64) -> NamespaceProgram {
    let mut builder = Builder {
        rng: Generator {
            state: seed ^ PROGRAM_STREAM,
            next_id: 0,
        },
        namespaces: Vec::new(),
        requires: Vec::new(),
        ancestors: Vec::new(),
        globals: Vec::new(),
        protocol: 0,
        methods: 2,
        protocol_item: None,
        implementations: Vec::new(),
        number: None,
        names: 0,
        throws: false,
        garbage: Vec::new(),
    };
    builder.program();
    builder.rng = Generator {
        state: seed ^ GARBAGE_STREAM,
        next_id: 0,
    };
    builder.garbage_items();
    let program = NamespaceProgram {
        seed,
        entry: format!("gen.n{}", builder.entry()),
        garbage: builder.garbage,
        throws: builder.throws,
        namespaces: builder.namespaces,
    };
    let bytes: usize = program
        .sources(&|_| true, true)
        .iter()
        .map(|(_, source)| source.len())
        .sum();
    assert!(bytes <= PROGRAM_BYTES, "seed {seed}: program source bound");
    program
}

impl Builder {
    fn shuffle<T>(&mut self, items: &mut [T]) {
        for index in (1..items.len()).rev() {
            let other = self.rng.choose(index + 1);
            items.swap(index, other);
        }
    }

    fn small(&mut self) -> String {
        (self.rng.choose(13) as i64 - 3).to_string()
    }

    fn fresh(&mut self, prefix: &str) -> String {
        self.names += 1;
        format!("{prefix}-{}", self.names - 1)
    }

    fn scope(&self, namespace: usize) -> Scope {
        Scope {
            namespace,
            limit: self.globals.len(),
            protocol: true,
        }
    }

    fn entry(&self) -> usize {
        self.requires.len() - 1
    }

    fn push(&mut self, namespace: usize, item: Item) -> usize {
        let defined = self.globals.len();
        let items = &mut self.namespaces[namespace].items;
        items.push(Item { defined, ..item });
        items.len() - 1
    }

    fn statement(&mut self, namespace: usize, text: String) -> usize {
        self.push(namespace, Item::new(Gate::Program, text))
    }

    fn define(&mut self, namespace: usize, name: String, kind: Kind) -> usize {
        self.globals.push(Global {
            namespace,
            name,
            kind,
        });
        self.globals.len() - 1
    }

    fn reference(&self, namespace: usize, global: usize) -> String {
        let global = &self.globals[global];
        if global.namespace == namespace {
            global.name.clone()
        } else {
            format!("n{}/{}", global.namespace, global.name)
        }
    }

    fn constructor(&self, namespace: usize, global: usize) -> String {
        let global = &self.globals[global];
        if global.namespace == namespace {
            format!("->{}", global.name)
        } else {
            format!("n{}/->{}", global.namespace, global.name)
        }
    }

    fn qualified(&self, global: usize) -> String {
        let global = &self.globals[global];
        format!("gen.n{}/{}", global.namespace, global.name)
    }

    fn protocol_name(&self, namespace: usize, name: &str) -> String {
        if namespace == self.protocol {
            name.into()
        } else {
            format!("n{}/{name}", self.protocol)
        }
    }

    fn sees_protocol(&self, namespace: usize) -> bool {
        (namespace == self.protocol && self.protocol_item.is_some())
            || self.requires[namespace].contains(&self.protocol)
    }

    /// A random global defined before `scope` and visible from its namespace
    /// (its own or a directly required one) whose kind `filter` accepts.
    fn pick(&mut self, scope: Scope, filter: impl Fn(&Kind) -> bool) -> Option<usize> {
        let found: Vec<_> = (0..scope.limit)
            .filter(|&index| {
                let global = &self.globals[index];
                (global.namespace == scope.namespace
                    || self.requires[scope.namespace].contains(&global.namespace))
                    && filter(&global.kind)
            })
            .collect();
        (!found.is_empty()).then(|| found[self.rng.choose(found.len())])
    }

    fn arity(&mut self, function: usize) -> usize {
        let Kind::Function { fixed, rest } = &self.globals[function].kind else {
            unreachable!("not a function")
        };
        let mut options = fixed.clone();
        if let Some(rest) = *rest {
            options.extend([rest, rest + 1, rest + 2]);
        }
        options[self.rng.choose(options.len())]
    }

    fn arguments(&mut self, scope: Scope, locals: &[String], depth: usize, count: usize) -> String {
        (0..count)
            .map(|_| format!(" {}", self.number(scope, locals, depth)))
            .collect()
    }

    /// A closed v1 expression whose value is discarded: ordered observations,
    /// closures, `let*` and `if` over literals.
    fn effect(&mut self) -> String {
        self.rng
            .expression(Shape::Value, 2, 8, &[])
            .source_with(false, OBSERVE)
    }

    /// An expression that evaluates to a number without throwing.
    fn number(&mut self, scope: Scope, locals: &[String], depth: usize) -> String {
        if depth > 0 {
            let d = depth - 1;
            let form = match self.rng.choose(16) {
                0 => Some(format!(
                    "(+ {} {})",
                    self.number(scope, locals, d),
                    self.number(scope, locals, d)
                )),
                1 => Some(format!(
                    "(- {} {})",
                    self.number(scope, locals, d),
                    self.number(scope, locals, d)
                )),
                2 => Some(format!(
                    "(* {} {})",
                    self.number(scope, locals, d),
                    self.small()
                )),
                3 => Some(format!("(inc {})", self.number(scope, locals, d))),
                4 => Some(format!(
                    "(if (< {} {}) {} {})",
                    self.number(scope, locals, d),
                    self.number(scope, locals, d),
                    self.number(scope, locals, d),
                    self.number(scope, locals, d)
                )),
                5 => self.call(scope, locals, d),
                6 => self.vector_read(scope),
                7 => self.map_read(scope),
                8 => Some(self.case(scope, locals, d)),
                9 => self.protocol_call(scope, locals, d),
                10 => self.reflective_read(scope, locals, d),
                11 => self.fn_as_data(scope, locals, d),
                12 => Some(format!(
                    "(do {} {})",
                    self.effect(),
                    self.number(scope, locals, d)
                )),
                13 => {
                    let name = format!("x{}", self.rng.fresh());
                    let init = self.number(scope, locals, d);
                    let mut inner = locals.to_vec();
                    inner.push(name.clone());
                    Some(format!(
                        "(let [{name} {init}] (+ {name} {}))",
                        self.number(scope, &inner, d)
                    ))
                }
                _ => None,
            };
            if let Some(form) = form {
                return form;
            }
        }
        match self.rng.choose(4) {
            0 if !locals.is_empty() => locals[self.rng.choose(locals.len())].clone(),
            1 => match self.pick(scope, |kind| matches!(kind, Kind::Number)) {
                Some(global) => self.reference(scope.namespace, global),
                None => self.small(),
            },
            _ => self.small(),
        }
    }

    fn call(&mut self, scope: Scope, locals: &[String], depth: usize) -> Option<String> {
        let function = self.pick(scope, |kind| matches!(kind, Kind::Function { .. }))?;
        let arity = self.arity(function);
        let arguments = self.arguments(scope, locals, depth, arity);
        Some(format!(
            "({}{arguments})",
            self.reference(scope.namespace, function)
        ))
    }

    fn vector_read(&mut self, scope: Scope) -> Option<String> {
        let vector = self.pick(scope, |kind| matches!(kind, Kind::Vector))?;
        let vector = self.reference(scope.namespace, vector);
        Some(match self.rng.choose(3) {
            0 => format!("(count {vector})"),
            1 => format!("(nth {vector} {} 0)", self.rng.choose(4)),
            _ => format!("(reduce + {vector})"),
        })
    }

    fn map_read(&mut self, scope: Scope) -> Option<String> {
        let map = self.pick(scope, |kind| matches!(kind, Kind::Map))?;
        let map = self.reference(scope.namespace, map);
        let key = KEYWORDS[self.rng.choose(KEYWORDS.len())];
        Some(match self.rng.choose(3) {
            0 => format!("(get {map} {key} 0)"),
            1 => format!("({key} {map} 0)"),
            _ => match self.pick(scope, |kind| matches!(kind, Kind::Keyword)) {
                Some(keyword) => format!("({} {map} 0)", self.reference(scope.namespace, keyword)),
                None => format!("({key} {map} 0)"),
            },
        })
    }

    fn case(&mut self, scope: Scope, locals: &[String], depth: usize) -> String {
        let test = match self.rng.choose(3) {
            0 => match self.pick(scope, |kind| matches!(kind, Kind::Keyword)) {
                Some(keyword) => self.reference(scope.namespace, keyword),
                None => ":b".into(),
            },
            1 => format!(
                "(if (< {} {}) :a :c)",
                self.number(scope, locals, depth),
                self.number(scope, locals, depth)
            ),
            _ => KEYWORDS[self.rng.choose(KEYWORDS.len())].into(),
        };
        format!(
            "(case {test} :a {} :b {} {})",
            self.number(scope, locals, depth),
            self.number(scope, locals, depth),
            self.number(scope, locals, depth)
        )
    }

    /// An instance whose type implements `method`: a constructed type visible
    /// from `scope`, or a number once `extend-type number` has loaded.
    fn instance(
        &mut self,
        scope: Scope,
        method: usize,
        locals: &[String],
        depth: usize,
    ) -> Option<String> {
        let number = self.number.as_ref().is_some_and(|(namespace, methods)| {
            methods.contains(&method)
                && (*namespace == scope.namespace || self.ancestors[scope.namespace][*namespace])
        });
        let found = self.pick(
            scope,
            |kind| matches!(kind, Kind::Type { methods } if methods.contains(&method)),
        );
        match found {
            Some(global) if !number || self.rng.choose(2) == 0 => Some(format!(
                "({}{})",
                self.constructor(scope.namespace, global),
                self.arguments(scope, locals, depth, 2)
            )),
            _ if number => Some(self.number(scope, locals, depth)),
            _ => None,
        }
    }

    fn protocol_call(&mut self, scope: Scope, locals: &[String], depth: usize) -> Option<String> {
        if !scope.protocol || !self.sees_protocol(scope.namespace) {
            return None;
        }
        let method = self.rng.choose(self.methods);
        let instance = self.instance(scope, method, locals, depth)?;
        let (name, parameters) = METHODS[method];
        let arguments = self.arguments(scope, locals, depth, parameters - 1);
        Some(format!(
            "({} {instance}{arguments})",
            self.protocol_name(scope.namespace, name)
        ))
    }

    /// An escape in expression position: a Var read or called through `var`
    /// or `resolve`.
    fn reflective_read(&mut self, scope: Scope, locals: &[String], depth: usize) -> Option<String> {
        let global = self.pick(scope, |kind| {
            matches!(kind, Kind::Number | Kind::Function { .. })
        })?;
        let var = if self.rng.choose(2) == 0 {
            format!("(resolve '{})", self.qualified(global))
        } else {
            format!("(var {})", self.reference(scope.namespace, global))
        };
        if matches!(self.globals[global].kind, Kind::Number) {
            return Some(format!("@{var}"));
        }
        let arity = self.arity(global);
        Some(format!(
            "({var}{})",
            self.arguments(scope, locals, depth, arity)
        ))
    }

    fn fn_as_data(&mut self, scope: Scope, locals: &[String], depth: usize) -> Option<String> {
        let function = self.pick(scope, |kind| {
            matches!(kind, Kind::Function { fixed, rest }
                if fixed.contains(&1) || rest.is_some_and(|rest| rest <= 1))
        })?;
        Some(format!(
            "(reduce + 0 (map {} [{} {}]))",
            self.reference(scope.namespace, function),
            self.number(scope, locals, depth),
            self.number(scope, locals, depth)
        ))
    }

    fn body(&mut self, scope: Scope, name: &str, locals: &[String]) -> String {
        let value = self.number(scope, locals, 2);
        match self.rng.choose(3) {
            0 => format!("(do ({OBSERVE} \"{name}\") {value})"),
            1 => format!("(do {} {value})", self.effect()),
            _ => value,
        }
    }

    fn program(&mut self) {
        let count = 2 + self.rng.choose(5);
        self.requires = vec![Vec::new(); count];
        for later in 1..count {
            for earlier in 0..later {
                if self.rng.choose(2) == 0 {
                    self.requires[later].push(earlier);
                }
            }
        }
        // Every namespace but the entry has a later requirer, so all load.
        for earlier in 0..count - 1 {
            if !(earlier + 1..count).any(|later| self.requires[later].contains(&earlier)) {
                let later = earlier + 1 + self.rng.choose(count - 1 - earlier);
                self.requires[later].push(earlier);
            }
        }
        self.protocol = self.rng.choose(count - 1);
        self.methods = 2 + self.rng.choose(2);
        let mut hosts = vec![0; count];
        for _ in 0..1 + self.rng.choose(3) {
            let host = self.protocol + self.rng.choose(count - self.protocol);
            hosts[host] += 1;
            if host != self.protocol && !self.requires[host].contains(&self.protocol) {
                self.requires[host].push(self.protocol);
            }
        }
        let mut requires = std::mem::take(&mut self.requires);
        for list in &mut requires {
            self.shuffle(list);
        }
        self.requires = requires;
        self.ancestors = vec![vec![false; count]; count];
        for namespace in 0..count {
            for required in self.requires[namespace].clone() {
                self.ancestors[namespace][required] = true;
                for earlier in 0..count {
                    if self.ancestors[required][earlier] {
                        self.ancestors[namespace][earlier] = true;
                    }
                }
            }
        }
        // Dependencies load depth-first in `:require` order; the first one to
        // load is a namespace without requires.
        let mut first = self.entry();
        while let Some(&required) = self.requires[first].first() {
            first = required;
        }
        let unused_effect = self.rng.choose(count);
        for (namespace, implementations) in hosts.into_iter().enumerate() {
            self.namespace(
                namespace,
                implementations,
                namespace == unused_effect,
                namespace == first,
            );
        }
    }

    fn namespace(
        &mut self,
        namespace: usize,
        implementations: usize,
        unused_effect: bool,
        first: bool,
    ) {
        let mut form = Item::new(Gate::Program, format!("(ns gen.n{namespace}"));
        if self.requires[namespace].is_empty() {
            form.close = ")".into();
        } else {
            form.open.push_str("\n  (:require");
            for required in &self.requires[namespace] {
                form.open
                    .push_str(&format!(" [gen.n{required} :as n{required}]"));
            }
            form.close = "))".into();
        }
        self.namespaces.push(Namespace {
            name: format!("gen.n{namespace}"),
            gate: Gate::Program,
            items: Vec::new(),
        });
        self.push(namespace, form);
        self.statement(namespace, format!("({OBSERVE} \"load gen.n{namespace}\")"));
        if first {
            // println has no default *print-fn*.
            self.statement(namespace, "(enable-console-print!)".into());
        }
        if namespace == self.protocol {
            let mut text = "(defprotocol Proto".to_string();
            for (name, parameters) in &METHODS[..self.methods] {
                let receiver = if *parameters == 1 { "[x]" } else { "[x y]" };
                text.push_str(&format!("\n  ({name} {receiver})"));
            }
            let mut item = Item::new(Gate::Program, text);
            item.close = ")".into();
            self.protocol_item = Some(self.push(namespace, item));
        }
        // 0 is an ordinary top-level form, 1 an implementation, 2 the forced
        // unused def with an effectful initializer.
        let mut steps = vec![0; 2 + self.rng.choose(4)];
        steps.extend(std::iter::repeat_n(1, implementations));
        if unused_effect {
            steps.push(2);
        }
        self.shuffle(&mut steps);
        for step in steps {
            match step {
                0 => self.top_level(namespace),
                1 => self.implementation(namespace),
                _ => {
                    let name = self.fresh("eff");
                    self.statement(namespace, format!("(def {name} ({OBSERVE} \"{name}\"))"));
                }
            }
        }
        if namespace == self.entry() {
            let result = self.result(namespace);
            self.statement(namespace, result);
        }
    }

    fn result(&mut self, namespace: usize) -> String {
        let scope = self.scope(namespace);
        if self.rng.choose(16) == 0
            && let Some(function) = self.pick(
                scope,
                |kind| matches!(kind, Kind::Function { fixed, rest: None } if fixed.len() > 1),
            )
        {
            let Kind::Function { fixed, .. } = &self.globals[function].kind else {
                unreachable!()
            };
            let arity = fixed.iter().max().unwrap() + 1;
            self.throws = true;
            let arguments = self.arguments(scope, &[], 1, arity);
            return format!("({}{arguments})", self.reference(namespace, function));
        }
        format!(
            "(pr-str [{} {} {}])",
            self.number(scope, &[], 3),
            self.number(scope, &[], 3),
            self.number(scope, &[], 3)
        )
    }

    fn top_level(&mut self, namespace: usize) {
        let scope = self.scope(namespace);
        let text = match self.rng.choose(24) {
            0 | 1 => {
                let value = self.small();
                let name = self.fresh("val");
                self.define(namespace, name.clone(), Kind::Number);
                format!("(def {name} {value})")
            }
            2 | 3 => {
                let value = self.number(scope, &[], 2);
                let name = self.fresh("val");
                self.define(namespace, name.clone(), Kind::Number);
                format!("(def {name} {value})")
            }
            4 => {
                let items: Vec<_> = (0..1 + self.rng.choose(3))
                    .map(|_| self.number(scope, &[], 1))
                    .collect();
                let name = self.fresh("vec");
                self.define(namespace, name.clone(), Kind::Vector);
                format!("(def {name} [{}])", items.join(" "))
            }
            5 => {
                let a = self.number(scope, &[], 1);
                let b = self.number(scope, &[], 1);
                let name = self.fresh("map");
                self.define(namespace, name.clone(), Kind::Map);
                format!("(def {name} {{:a {a} :b {b}}})")
            }
            6 => {
                let keyword = KEYWORDS[self.rng.choose(KEYWORDS.len())];
                let name = self.fresh("kw");
                self.define(namespace, name.clone(), Kind::Keyword);
                format!("(def {name} {keyword})")
            }
            7 | 8 => {
                let name = self.fresh("fun");
                let parameters: Vec<String> = ["a", "b"][..self.rng.choose(3)]
                    .iter()
                    .map(|name| name.to_string())
                    .collect();
                let body = self.body(scope, &name, &parameters);
                self.define(
                    namespace,
                    name.clone(),
                    Kind::Function {
                        fixed: vec![parameters.len()],
                        rest: None,
                    },
                );
                format!("(defn {name} [{}] {body})", parameters.join(" "))
            }
            9 => {
                let name = self.fresh("fun");
                let default = self.small();
                let body = self.body(scope, &name, &["a".into(), "b".into()]);
                self.define(
                    namespace,
                    name.clone(),
                    Kind::Function {
                        fixed: vec![1, 2],
                        rest: None,
                    },
                );
                format!("(defn {name}\n  ([a] ({name} a {default}))\n  ([a b] {body}))")
            }
            10 => {
                let name = self.fresh("fun");
                let body = self.body(scope, &name, &["a".into()]);
                self.define(
                    namespace,
                    name.clone(),
                    Kind::Function {
                        fixed: Vec::new(),
                        rest: Some(1),
                    },
                );
                if self.rng.choose(2) == 0 {
                    format!("(defn {name} [a & more] (+ {body} (count more)))")
                } else {
                    format!("(defn {name} [a & more] (apply + {body} more))")
                }
            }
            13 => match self.call(scope, &[], 2) {
                // A bare top-level call, which fixed-entry specialization sees.
                Some(call) => call,
                None => self.observation(scope),
            },
            19 | 20 => {
                // A core function under another name, then a bare call that
                // specialization redirects to the core function.
                const ALIASES: [(&str, &[usize], Option<usize>); 4] = [
                    ("+", &[], Some(0)),
                    ("max", &[], Some(1)),
                    ("inc", &[1], None),
                    ("-", &[], Some(1)),
                ];
                let (core, fixed, rest) = ALIASES[self.rng.choose(ALIASES.len())];
                let name = self.fresh("alias");
                self.statement(namespace, format!("(def {name} {core})"));
                let alias = self.define(
                    namespace,
                    name,
                    Kind::Function {
                        fixed: fixed.to_vec(),
                        rest,
                    },
                );
                let scope = self.scope(namespace);
                let arity = self.arity(alias);
                let arguments = self.arguments(scope, &[], 1, arity);
                format!("({}{arguments})", self.reference(namespace, alias))
            }
            21 | 22 => {
                // A bare call of a multi-arity function at one of its arities.
                let multi =
                    |kind: &Kind| matches!(kind, Kind::Function { fixed, .. } if fixed.len() > 1);
                match self.pick(scope, multi) {
                    Some(function) => {
                        let arity = self.arity(function);
                        let arguments = self.arguments(scope, &[], 1, arity);
                        format!("({}{arguments})", self.reference(namespace, function))
                    }
                    None => self.observation(scope),
                }
            }
            14 => {
                let name = self.fresh("eff");
                let value = self.number(scope, &[], 1);
                format!("(def {name} (do ({OBSERVE} \"{name}\") {value}))")
            }
            15 => {
                let initial = self.number(scope, &[], 1);
                let step = self.number(scope, &[], 1);
                let name = self.fresh("cnt");
                self.statement(namespace, format!("(def {name} {initial})"));
                self.define(namespace, name.clone(), Kind::Number);
                format!("(set! {name} (+ {name} {step}))")
            }
            16 => {
                let name = self.fresh("once");
                let value = self.number(scope, &[], 1);
                let text = format!("(defonce {name} (do ({OBSERVE} \"{name}\") {value}))");
                self.define(namespace, name.clone(), Kind::Number);
                if self.rng.choose(2) == 0 {
                    // A second defonce must not run its initializer.
                    self.statement(namespace, text);
                    format!("(defonce {name} ({OBSERVE} \"{name} again\"))")
                } else {
                    text
                }
            }
            17 => self.with_redefs(scope),
            18 => format!(
                "(println \"gen.n{namespace}\" {})",
                self.number(scope, &[], 2)
            ),
            _ => self.observation(scope),
        };
        self.statement(namespace, text);
    }

    fn observation(&mut self, scope: Scope) -> String {
        let values: Vec<_> = (0..1 + self.rng.choose(2))
            .map(|_| self.number(scope, &[], 3))
            .collect();
        format!("({OBSERVE} (pr-str {}))", values.join(" "))
    }

    fn with_redefs(&mut self, scope: Scope) -> String {
        let Some(global) = self.pick(scope, |kind| {
            matches!(kind, Kind::Number | Kind::Function { .. })
        }) else {
            return self.observation(scope);
        };
        let name = self.reference(scope.namespace, global);
        if matches!(self.globals[global].kind, Kind::Number) {
            return format!("({OBSERVE} (pr-str (with-redefs [{name} 100] (+ {name} 1)) {name}))");
        }
        let arity = self.arity(global);
        let inside = self.arguments(scope, &[], 1, arity);
        let outside = self.arguments(scope, &[], 1, arity);
        format!(
            "({OBSERVE} (pr-str (with-redefs [{name} (fn [& xs] 100)] ({name}{inside})) ({name}{outside})))"
        )
    }

    fn implementation(&mut self, namespace: usize) {
        let scope = self.scope(namespace);
        let mut methods: Vec<_> = (0..self.methods)
            .filter(|_| self.rng.choose(2) == 0)
            .collect();
        if methods.is_empty() {
            methods.push(self.rng.choose(self.methods));
        }
        let protocol = self.protocol_name(namespace, "Proto");
        let choice = self.rng.choose(4);
        if choice == 0 && self.number.is_none() {
            let mut text = format!("(extend-type number {protocol}");
            for &method in &methods {
                let (name, parameters) = METHODS[method];
                let locals: Vec<String> = ["x", "y"][..parameters]
                    .iter()
                    .map(|name| name.to_string())
                    .collect();
                let body = self.number(scope, &locals, 2);
                text.push_str(&format!("\n  ({name} [{}] {body})", locals.join(" ")));
            }
            let mut item = Item::new(Gate::Program, text);
            item.close = ")".into();
            let item = self.push(namespace, item);
            self.number = Some((namespace, methods));
            self.implementations.push(Implementation {
                namespace,
                item,
                instance: None,
            });
            return;
        }
        let record = choice != 1;
        self.names += 1;
        let name = format!("{}{}", if record { "Rec" } else { "Type" }, self.names - 1);
        let mut text = format!(
            "({} {name} [a b] {protocol}",
            if record { "defrecord" } else { "deftype" }
        );
        for &method in &methods {
            let (method_name, parameters) = METHODS[method];
            let mut locals: Vec<String> = vec!["a".into(), "b".into()];
            let receiver = if parameters == 1 {
                "[_]"
            } else {
                locals.push("y".into());
                "[_ y]"
            };
            let body = self.number(scope, &locals, 2);
            text.push_str(&format!("\n  ({method_name} {receiver} {body})"));
        }
        let mut item = Item::new(Gate::Program, text);
        item.close = ")".into();
        let item = self.push(namespace, item);
        let global = self.define(
            namespace,
            name.clone(),
            Kind::Type {
                methods: methods.clone(),
            },
        );
        self.implementations.push(Implementation {
            namespace,
            item,
            instance: Some(global),
        });
        // Only some types are constructed.
        if self.rng.choose(3) != 0 {
            let scope = self.scope(namespace);
            let method = methods[self.rng.choose(methods.len())];
            let (method_name, parameters) = METHODS[method];
            let fields = self.arguments(scope, &[], 1, 2);
            let arguments = self.arguments(scope, &[], 1, parameters - 1);
            let text = format!(
                "({OBSERVE} (pr-str ({} (->{name}{fields}){arguments})))",
                self.protocol_name(namespace, method_name)
            );
            self.statement(namespace, text);
        }
    }

    /// The program item after which a garbage item goes: from `first` up to
    /// the last item, but never after the entry's result form.
    fn position(&mut self, namespace: usize, first: usize) -> usize {
        let items = self.namespaces[namespace].items.len();
        let last = items - if namespace == self.entry() { 2 } else { 1 };
        first + self.rng.choose(last + 1 - first)
    }

    fn attach(&mut self, namespace: usize, position: usize, items: Vec<Item>) {
        self.namespaces[namespace].items[position]
            .after
            .extend(items);
    }

    fn garbage_items(&mut self) {
        let mut tags = Vec::new();
        for tag in GarbageTag::ALL {
            for _ in 0..1 + self.rng.choose(2) {
                tags.push(tag);
            }
        }
        self.shuffle(&mut tags);
        for (index, tag) in tags.into_iter().enumerate() {
            // Every program has at least one escaped item.
            let escaped = index == 0 || self.rng.choose(3) == 0;
            let sentinel = format!("GARBAGE-{index:03}");
            let (namespace, escape) = match tag {
                GarbageTag::UnreferencedNamespace => self.dead_namespace(index, &sentinel, escaped),
                GarbageTag::UninstantiatedType => self.dead_type(index, &sentinel, escaped),
                GarbageTag::UncalledProtocolMethod => self.dead_method(index, &sentinel, escaped),
                _ => self.dead_definition(tag, index, &sentinel, escaped),
            };
            self.garbage.push(Garbage {
                tag,
                sentinel,
                namespace,
                escape,
            });
        }
    }

    fn dead_definition(
        &mut self,
        tag: GarbageTag,
        index: usize,
        sentinel: &str,
        escaped: bool,
    ) -> (String, Option<Escape>) {
        let namespace = self.rng.choose(self.requires.len());
        let position = self.position(namespace, 1);
        let scope = Scope {
            namespace,
            limit: self.namespaces[namespace].items[position].defined,
            protocol: false,
        };
        let name = format!("dead-{index}");
        let (text, function) = match tag {
            GarbageTag::UnusedLiteralDef => {
                let value = match self.rng.choose(4) {
                    0 => format!("\"{sentinel}\""),
                    1 => format!("[\"{sentinel}\" 1 :a]"),
                    2 => format!("{{:a \"{sentinel}\" :b 2}}"),
                    _ => format!("#{{\"{sentinel}\" 3}}"),
                };
                (format!("(def {name} {value})"), false)
            }
            GarbageTag::UnusedPureDef => {
                let value = match self.rng.choose(4) {
                    0 => format!("(str \"{sentinel}\" (+ 1 2))"),
                    1 => format!("(vector \"{sentinel}\" (inc 1))"),
                    2 => format!("(hash-map :a \"{sentinel}\" :b (* 2 3))"),
                    _ => format!("(count [\"{sentinel}\" (- 5 1)])"),
                };
                (format!("(def {name} {value})"), false)
            }
            GarbageTag::UnusedKeyword => {
                if self.rng.choose(2) == 0 {
                    (format!("(def {name} :{sentinel})"), false)
                } else {
                    (
                        format!("(defn {name} [m] (get m :{sentinel} \"none\"))"),
                        true,
                    )
                }
            }
            _ => {
                // Dead code may call live code; it only runs when escaped.
                let live = self.number(scope, &["x".into()], 1);
                let text = match self.rng.choose(3) {
                    0 => format!("(defn {name} [x] (str \"{sentinel}\" {live}))"),
                    1 => format!(
                        "(defn {name}\n  ([x] ({name} x 1))\n  ([x y] (str \"{sentinel}\" (+ {live} y))))"
                    ),
                    _ => format!("(defn {name} [x & more] (apply str \"{sentinel}\" {live} more))"),
                };
                (text, true)
            }
        };
        let mut items = vec![Item::new(Gate::Garbage(index), text)];
        let escape = escaped.then(|| {
            let choices: &[Escape] = if function {
                &[
                    Escape::Var,
                    Escape::Resolve,
                    Escape::WithRedefs,
                    Escape::FnAsData,
                ]
            } else {
                &[Escape::Var, Escape::Resolve, Escape::WithRedefs]
            };
            let escape = choices[self.rng.choose(choices.len())];
            let argument = if tag == GarbageTag::UnusedKeyword {
                "{}"
            } else {
                "1"
            };
            let qualified = format!("gen.n{namespace}/{name}");
            let value = match (escape, function) {
                (Escape::Var, false) => format!("@(var {name})"),
                (Escape::Var, true) => format!("((var {name}) {argument})"),
                (Escape::Resolve, false) => format!("@(resolve '{qualified})"),
                (Escape::Resolve, true) => format!("((resolve '{qualified}) {argument})"),
                (Escape::WithRedefs, false) => format!("(with-redefs [{name} 0] {name}) {name}"),
                (Escape::WithRedefs, true) => format!(
                    "(with-redefs [{name} (fn [& xs] \"redefined\")] ({name} {argument})) ({name} {argument})"
                ),
                _ => format!("(mapv {name} [{argument} {argument}])"),
            };
            items.push(Item::new(
                Gate::Escape(index),
                format!("({OBSERVE} (pr-str {value}))"),
            ));
            escape
        });
        self.attach(namespace, position, items);
        (format!("gen.n{namespace}"), escape)
    }

    fn dead_namespace(
        &mut self,
        index: usize,
        sentinel: &str,
        escaped: bool,
    ) -> (String, Option<Escape>) {
        let name = format!("gen.unused{index}");
        let requirers: Vec<_> = (0..self.requires.len())
            .filter(|&namespace| !self.requires[namespace].is_empty())
            .collect();
        let requirer = requirers[self.rng.choose(requirers.len())];
        self.namespaces[requirer].items[0]
            .extra
            .push((Gate::Garbage(index), format!(" [{name}]")));
        let mut items = vec![Item::new(Gate::Garbage(index), format!("(ns {name})"))];
        // The first definition is the one escapes read.
        items.push(Item::new(
            Gate::Garbage(index),
            format!("(def dead-{index}-0 \"{sentinel}\")"),
        ));
        for definition in 1..1 + self.rng.choose(3) {
            let text = if self.rng.choose(2) == 0 {
                format!("(defn dead-{index}-{definition} [x] (str \"{sentinel}\" x))")
            } else {
                format!("(def dead-{index}-{definition} [:{sentinel} {definition}])")
            };
            items.push(Item::new(Gate::Garbage(index), text));
        }
        self.namespaces.push(Namespace {
            name: name.clone(),
            gate: Gate::Garbage(index),
            items,
        });
        let escape = escaped.then(|| {
            let qualified = format!("{name}/dead-{index}-0");
            let (escape, value) = if self.rng.choose(2) == 0 {
                (Escape::Var, format!("@(var {qualified})"))
            } else {
                (Escape::Resolve, format!("@(resolve '{qualified})"))
            };
            let text = format!("({OBSERVE} (pr-str {value}))");
            self.attach(requirer, 1, vec![Item::new(Gate::Escape(index), text)]);
            escape
        });
        (name, escape)
    }

    fn dead_type(
        &mut self,
        index: usize,
        sentinel: &str,
        escaped: bool,
    ) -> (String, Option<Escape>) {
        let hosts: Vec<_> = (0..self.requires.len())
            .filter(|&namespace| {
                namespace == self.protocol || self.requires[namespace].contains(&self.protocol)
            })
            .collect();
        let namespace = hosts[self.rng.choose(hosts.len())];
        let first = if namespace == self.protocol {
            self.protocol_item.unwrap()
        } else {
            1
        };
        let position = self.position(namespace, first);
        let name = format!("Dead{index}");
        let protocol = self.protocol_name(namespace, "Proto");
        let form = if self.rng.choose(2) == 0 {
            "defrecord"
        } else {
            "deftype"
        };
        let mut items = vec![Item::new(
            Gate::Garbage(index),
            format!("({form} {name} [a b] {protocol}\n  (pm0 [_] (str \"{sentinel}\" a)))"),
        )];
        let escape = escaped.then(|| {
            items.push(Item::new(
                Gate::Escape(index),
                format!(
                    "({OBSERVE} (pr-str ({} (->{name} 1 2))))",
                    self.protocol_name(namespace, "pm0")
                ),
            ));
            Escape::ProtocolCall
        });
        self.attach(namespace, position, items);
        (format!("gen.n{namespace}"), escape)
    }

    fn dead_method(
        &mut self,
        index: usize,
        sentinel: &str,
        escaped: bool,
    ) -> (String, Option<Escape>) {
        let method = format!("deadm-{index}");
        let protocol_item = self.protocol_item.unwrap();
        self.namespaces[self.protocol].items[protocol_item]
            .extra
            .push((Gate::Garbage(index), format!("\n  ({method} [x])")));
        for implementation in &self.implementations {
            self.namespaces[implementation.namespace].items[implementation.item]
                .extra
                .push((
                    Gate::Garbage(index),
                    format!("\n  ({method} [_] \"{sentinel}\")"),
                ));
        }
        let escape = escaped.then(|| {
            let chosen = self.rng.choose(self.implementations.len());
            let implementation = &self.implementations[chosen];
            let (namespace, item) = (implementation.namespace, implementation.item);
            let instance = match implementation.instance {
                Some(global) => format!("({} 1 2)", self.constructor(namespace, global)),
                None => "7".into(),
            };
            let text = format!(
                "({OBSERVE} (pr-str ({} {instance})))",
                self.protocol_name(namespace, &method)
            );
            self.attach(namespace, item, vec![Item::new(Gate::Escape(index), text)]);
            Escape::ProtocolCall
        });
        (format!("gen.n{}", self.protocol), escape)
    }
}
