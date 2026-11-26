use light_cljs::{
    compiler_data::DatumKind,
    evaluator::{EvalError, Evaluator, EvaluatorOptions, ValueId},
    form_bridge::ConversionLimits,
};

/// Runs standalone fixtures as scripts in the same global scope between calls.
pub(crate) struct EmbeddedJs(Evaluator);

impl EmbeddedJs {
    fn options() -> EvaluatorOptions {
        EvaluatorOptions {
            memory_limit: 128 << 20,
            stack_limit: 4 << 20,
            timeout: std::time::Duration::from_secs(120),
            ..EvaluatorOptions::default()
        }
    }

    pub(crate) fn new() -> Self {
        Self(Evaluator::new(Self::options()).unwrap())
    }

    pub(crate) fn interactive() -> Self {
        Self(Evaluator::interactive(Self::options()).unwrap())
    }

    fn evaluate<T>(
        &self,
        source: &str,
        read: impl FnOnce(&Evaluator, ValueId) -> Result<T, EvalError>,
    ) -> Result<T, EvalError> {
        // Each standalone fixture gets its own operation deadline and poll
        // budget so time spent outside this isolate (Node subprocesses and
        // macro compilation between fixtures) is not charged to it.
        self.0.begin_operation(&Self::options())?;
        // Indirect eval gives scripts their completion value and access to the
        // isolate's global object.
        let value = self.0.evaluate_js(&format!("(0, eval)({source:?})"))?;
        let result = read(&self.0, value);
        let release = self.0.release(value);
        let result = result?;
        release?;
        Ok(result)
    }

    pub(crate) fn string(&self, source: &str) -> Result<String, EvalError> {
        self.evaluate(source, Evaluator::string)
    }

    pub(crate) fn number(&self, source: &str) -> Result<f64, EvalError> {
        self.evaluate(source, Evaluator::number)
    }

    pub(crate) fn boolean(&self, source: &str) -> Result<bool, EvalError> {
        self.evaluate(source, |engine, value| {
            let value = engine.export_data(value, ConversionLimits::default())?;
            match value.kind() {
                DatumKind::Boolean(result) => Ok(*result),
                _ => Err(EvalError("Expected Boolean".into())),
            }
        })
    }

    pub(crate) fn collect_garbage(&self) {
        self.0.collect_garbage();
    }
}
