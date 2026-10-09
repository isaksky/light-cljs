//! Bounded native extension of the source-construction approach in generated.rs.
//! Uses the existing SCI wire/corpus; no second evaluator or random dependency.
pub const SEEDS: [u64; 12] = [0, 1, 2, 3, 17, 42, 99, 255, 1024, 65535, 0x5eed, u64::MAX];
pub const VARIANTS: [&str; 4] = ["original", "alpha", "identity", "do"];

#[derive(Clone, Debug)]
pub enum Form {
    Int(i64),
    Pair(u8, Box<Form>, Box<Form>),
}
impl Form {
    fn source(&self, alpha: bool, index: usize) -> String {
        match self {
            Self::Int(n) => n.to_string(),
            Self::Pair(kind, left, right) => {
                let a = left.source(alpha, index * 2);
                let b = right.source(alpha, index * 2 + 1);
                let name = format!("{}{index}", if alpha { "renamed" } else { "local" });
                match kind {
                    0 => format!("(+ {a} {b})"),
                    1 => format!("(let [{name} {a}] (+ {name} {b}))"),
                    2 => format!("((fn [{name}] (+ {name} {b})) {a})"),
                    _ => format!("(if (zero? {a}) {b} (+ {a} {b}))"),
                }
            }
        }
    }
    fn candidates(&self) -> Vec<Self> {
        match self {
            Self::Int(0) => vec![],
            Self::Int(_) => vec![Self::Int(0)],
            Self::Pair(_, a, b) => vec![Self::Int(0), *a.clone(), *b.clone()],
        }
    }
}
#[derive(Clone, Debug)]
pub struct Program {
    pub seed: u64,
    expression: Form,
    items: Vec<i64>,
}
impl Program {
    pub fn new(seed: u64) -> Self {
        fn next(state: &mut u64) -> u64 {
            *state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            *state >> 32
        }
        fn form(state: &mut u64, depth: usize) -> Form {
            if depth == 0 {
                Form::Int((next(state) % 17) as i64 - 8)
            } else {
                Form::Pair(
                    (next(state) % 4) as u8,
                    Box::new(form(state, depth - 1)),
                    Box::new(form(state, depth - 1)),
                )
            }
        }
        let mut state = seed;
        let expression = form(&mut state, 3); // exactly 15 construction nodes
        let items = (0..(next(&mut state) % 5 + 1))
            .map(|_| (next(&mut state) % 21) as i64 - 10)
            .collect();
        Self {
            seed,
            expression,
            items,
        }
    }
    pub fn sources(&self) -> [String; 4] {
        let render = |alpha| {
            let [capture, fun, arg, data, total, remaining, value] = if alpha {
                [
                    "capturedB",
                    "funB",
                    "argB",
                    "dataB",
                    "sumB",
                    "remainingB",
                    "valueB",
                ]
            } else {
                [
                    "capturedA",
                    "funA",
                    "argA",
                    "dataA",
                    "sumA",
                    "remainingA",
                    "valueA",
                ]
            };
            let items = self
                .items
                .iter()
                .map(i64::to_string)
                .collect::<Vec<_>>()
                .join(" ");
            let expr = self.expression.source(alpha, 1);
            // Each seed covers ordered effects, data, scope/capture, function call,
            // loop/recur, destructuring, user macros and bounded lazy consumption.
            format!(
                "(let [{capture} (observe 7) {fun} (fn [{arg}] (+ {capture} {arg})) {data} (mapv {fun} [{items}]) {total} (loop [{remaining} (seq {data}) {total} 0] (if {remaining} (recur (next {remaining}) (+ {total} (first {remaining}))) {total}))] [(vec (map (fn [{value}] (twice-once [once {value}])) {data})) (when (seq {data}) {total}) {{:data {data} :set #{{:a :b}} :exact [1/2 -0.0 \"λ\\n\\uD800\"]}} (let [[head & tail] {data}] [head (vec tail)]) (do (observe \"end\") {expr})])"
            )
        };
        let original = render(false);
        [original.clone(), render(true), format!("(let [identityFresh {original}] identityFresh)"), format!("(do {original})")].map(|body| format!("(defmacro twice-once [[sym expression]] (list 'let [sym expression] (list '+ sym sym))) {body}"))
    }
    pub fn ids(&self) -> [String; 4] {
        VARIANTS.map(|v| format!("sci-generated-v1-{}-{v}", self.seed))
    }
    /// Focused greedy reduction. Only retain a strictly smaller candidate when
    /// the caller confirms the SAME failure; at most 32 candidate evaluations.
    pub fn reduce(&self, mut preserves_failure: impl FnMut(&Self) -> bool) -> Self {
        let mut best = self.clone();
        for _ in 0..8 {
            let mut candidates: Vec<_> = best
                .expression
                .candidates()
                .into_iter()
                .map(|expression| Self {
                    expression,
                    ..best.clone()
                })
                .collect();
            if !best.items.is_empty() {
                candidates.push(Self {
                    items: best.items[..best.items.len() / 2].to_vec(),
                    ..best.clone()
                });
            }
            let Some(smaller) = candidates.into_iter().find(|p| preserves_failure(p)) else {
                break;
            };
            best = smaller;
        }
        best
    }
}
