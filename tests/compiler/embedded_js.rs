use light_cljs::evaluator::{EvalError, Evaluator, EvaluatorOptions, ValueId};

/// Runs standalone fixtures as scripts in the same global scope between calls.
pub(crate) struct EmbeddedJs(Evaluator);

impl EmbeddedJs {
    pub(crate) fn new() -> Self {
        Self(
            Evaluator::new(EvaluatorOptions {
                memory_limit: 128 << 20,
                stack_limit: 4 << 20,
                timeout: std::time::Duration::from_secs(120),
                ..EvaluatorOptions::default()
            })
            .unwrap(),
        )
    }

    fn evaluate(&self, source: &str) -> Result<ValueId, EvalError> {
        // Indirect eval preserves script completion values and top-level scope.
        self.0.evaluate_js(&format!("(0, eval)({source:?})"))
    }

    pub(crate) fn string(&self, source: &str) -> Result<String, EvalError> {
        let value = self.evaluate(source)?;
        let result = self.0.string(value);
        self.0.release(value)?;
        result
    }
}
