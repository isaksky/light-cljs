//! Test-only source construction, not a second compiler/evaluator. All semantics
//! come from the shared Node protocol and the explicitly selected CLJS oracle.
use super::*;

const SEEDS: [u64; 12] = [0, 1, 2, 3, 17, 42, 99, 255, 1024, 65535, 0x5eed, u64::MAX];
const DEPTH: usize = 4;
const NODES: usize = 32;
const VARIANTS: [&str; 4] = ["original", "alpha", "identity", "do"];

#[derive(Clone, Copy, PartialEq)]
enum Shape {
    Value,
    Callable(usize),
}

#[derive(Clone, Debug)]
enum Form {
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

    fn nodes(&self) -> usize {
        1 + self
            .children()
            .iter()
            .map(|form| form.nodes())
            .sum::<usize>()
    }

    fn depth(&self) -> usize {
        1 + self
            .children()
            .iter()
            .map(|form| form.depth())
            .max()
            .unwrap_or(0)
    }

    // Each binder has a distinct identity; changing only its printable spelling
    // renames all and only its references. Neither prefix can capture observe.
    fn source(&self, alpha: bool) -> String {
        let name = |id| format!("{}{id}", if alpha { "renamed" } else { "g" });
        let emit = |form: &Form| form.source(alpha);
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
            Self::Observe(value) => format!("(observe {})", emit(value)),
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

fn program(seed: u64) -> Form {
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

fn seeds() -> Vec<u64> {
    match std::env::var("LIGHT_CLJS_GENERATED_SEED") {
        Ok(value) => vec![
            value
                .parse()
                .expect("LIGHT_CLJS_GENERATED_SEED must be a decimal u64"),
        ],
        Err(std::env::VarError::NotPresent) => SEEDS.to_vec(),
        Err(error) => panic!("invalid LIGHT_CLJS_GENERATED_SEED: {error}"),
    }
}

fn generated_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for seed in seeds() {
        eprintln!("GENERATED v1 seed {seed}");
        let form = program(seed);
        assert!(
            form.nodes() <= 100 && form.depth() <= 12,
            "seed {seed}: generator bounds"
        );
        let original = form.source(false);
        // The identity name is outside both alpha-renaming alphabets and is
        // scoped only after its initializer. Each expression executes once.
        let sources = [
            original.clone(),
            form.source(true),
            format!("(let* [identityFresh {original}] identityFresh)"),
            format!("(do {original})"),
        ];
        for (variant, source) in VARIANTS.iter().zip(sources) {
            assert!(source.len() <= 8192, "seed {seed}: source bound");
            cases.push(Case {
                id: format!("generated-v1-{seed}-{variant}"),
                source,
                outcome: String::new(),
                trace: String::new(),
                stdout: String::new(),
            });
        }
    }
    cases
}

fn input_cases() -> Vec<Case> {
    if let Some(path) = std::env::var_os("LIGHT_CLJS_GENERATED_REPLAY_TSV") {
        assert!(
            std::env::var_os("LIGHT_CLJS_GENERATED_SEED").is_none(),
            "select seed or replay TSV, not both"
        );
        let text = fs::read_to_string(&path).expect("required generated replay TSV unavailable");
        return replay_cases(&text);
    }
    generated_cases()
}

fn replay_cases(text: &str) -> Vec<Case> {
    assert!(text.len() <= 512 * 1024, "replay source exceeds bound");
    let cases: Vec<_> = text
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| {
            let (id, source) = line
                .split_once('\t')
                .expect("replay rows require ID and source");
            assert!(
                !source.contains('\t') && source.len() <= 8192,
                "invalid replay source"
            );
            Case {
                id: id.into(),
                source: source.into(),
                outcome: String::new(),
                trace: String::new(),
                stdout: String::new(),
            }
        })
        .collect();
    assert!(
        !cases.is_empty() && cases.len() <= 48 && cases.len() % 4 == 0,
        "replay requires 1–12 complete four-variant groups"
    );
    for group in cases.as_chunks::<4>().0 {
        let base = group[0]
            .id
            .strip_suffix("-original")
            .expect("first replay variant must be original");
        for (case, variant) in group.iter().zip(VARIANTS) {
            assert_eq!(
                case.id,
                format!("{base}-{variant}"),
                "replay variants missing or reordered"
            );
        }
    }
    cases
}

fn equivalent(output: &str, cases: &[Case]) -> Result<(), String> {
    let lines = outcomes(output, cases)?;
    let (groups, remainder) = cases.as_chunks::<4>();
    if groups.is_empty() || !remainder.is_empty() {
        return Err("expected complete four-variant groups".into());
    }
    for (group, actual) in groups.iter().zip(lines.as_chunks::<4>().0) {
        let mut baseline = None;
        for (case, line) in group.iter().zip(actual) {
            let prefix = format!("{{\"id\":{},", json_string(&case.id));
            let observable = line.strip_prefix(&prefix).unwrap();
            if !observable.starts_with("\"outcome\":{\"status\":\"returned\",") {
                return Err(format!(
                    "valid generated program did not return: {}: {}\n{line}",
                    case.id, case.source
                ));
            }
            if let Some(expected) = baseline {
                if observable != expected {
                    return Err(format!(
                        "metamorphic mismatch: {}\noriginal: {}\ncounterpart: {}\n{expected}\n{observable}",
                        case.id, group[0].source, case.source
                    ));
                }
            } else {
                baseline = Some(observable);
            }
        }
    }
    Ok(())
}

fn save_cases(cases: &[Case]) -> TempDir {
    let dir = TempDir::new("generated-v1");
    fs::write(
        dir.0.join("cases.tsv"),
        cases
            .iter()
            .map(|case| format!("{}\t{}\n", case.id, case.source))
            .collect::<String>(),
    )
    .unwrap();
    dir
}

#[test]
fn generated_programs_preserve_results_and_traces_under_transformations() {
    let cases = input_cases();
    let _artifacts = save_cases(&cases);
    let output = compiled_cases(&cases);
    equivalent(&output, &cases).unwrap_or_else(|error| panic!("{error}"));
}

#[test]
#[ignore = "requires Clojure CLI, Java, Node, and the pinned ClojureScript Maven dependencies"]
fn generated_programs_match_standard_clojurescript() {
    compare_reference_cases(&input_cases());
}

fn compare_reference_cases(cases: &[Case]) {
    let artifacts = save_cases(cases);
    let path = artifacts.0.join("cases.tsv");
    let references = reference_results(
        "generated-v1",
        &["cases", path.to_str().unwrap()],
        include_str!("execution.js"),
    );
    let actual = compiled_cases(cases);
    equivalent(&actual, cases).unwrap_or_else(|error| panic!("{error}"));
    let actual = outcomes(&actual, cases).unwrap();
    for reference in &references {
        equivalent(&reference.output, cases)
            .unwrap_or_else(|error| panic!("{}: {error}", reference.identity));
        let expected = outcomes(&reference.output, cases).unwrap();
        for ((case, actual), expected) in cases.iter().zip(&actual).zip(expected) {
            assert_eq!(
                *actual, expected,
                "{}: {}\n{}",
                reference.identity, case.id, case.source
            );
        }
        eprintln!(
            "Matched {} generated programs ({} seeds × 4 variants), outcomes and traces against {}",
            cases.len(),
            cases.len() / 4,
            reference.identity
        );
    }
}

#[test]
fn generated_comparison_rejects_changed_traces_errors_and_missing_results() {
    let cases = generated_cases();
    let cases = &cases[..4];
    let output = compiled_cases(cases);
    equivalent(&output, cases).unwrap();
    assert!(
        equivalent(
            &output.replacen("\"callee\",\"argument\"", "\"argument\",\"callee\"", 1),
            cases
        )
        .is_err()
    );
    assert!(equivalent(&output.replace("\"returned\"", "\"threw\""), cases).is_err());
    assert!(equivalent(output.lines().next().unwrap(), cases).is_err());
}

#[test]
fn generated_seed_manifest_is_stable_and_bounded() {
    let mut actual = String::new();
    for seed in SEEDS {
        let form = program(seed);
        let source = form.source(false);
        // FNV-1a identifies the exact generated source; it is not a security hash.
        let fingerprint = source.bytes().fold(14695981039346656037_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(1099511628211)
        });
        actual.push_str(&format!(
            "v1\t{seed}\t{fingerprint:016x}\t{}\t{}\n",
            form.nodes(),
            form.depth()
        ));
    }
    let expected = include_str!("../reference/generated-origins.tsv")
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| format!("{line}\n"))
        .collect::<String>();
    assert_eq!(
        actual, expected,
        "generated origin changed: bump version or deliberately refresh the seed manifest after conformance"
    );
}
