//! Engine-independent services used by compiler macro sessions.
use crate::evaluator::{EvalError, Evaluator, MacroEvaluator, ValueId};
use std::rc::Rc;

pub(crate) type UnitService = Rc<dyn Fn(&dyn MacroEvaluator, u64) -> Result<String, String>>;
pub(crate) trait CompilerEvaluator: MacroEvaluator {
    fn evaluate_js(&self, expression: &str) -> Result<ValueId, EvalError>;
    fn install_compiler_service(
        self: Rc<Self>,
        name: &str,
        callback: UnitService,
    ) -> Result<(), EvalError>;
    #[cfg(all(test, light_v8))]
    fn number(&self, id: ValueId) -> Result<f64, EvalError>;
}
impl CompilerEvaluator for Evaluator {
    fn evaluate_js(&self, expression: &str) -> Result<ValueId, EvalError> {
        Evaluator::evaluate_js(self, expression)
    }
    #[cfg(all(test, light_v8))]
    fn number(&self, id: ValueId) -> Result<f64, EvalError> {
        Evaluator::number(self, id)
    }
    fn install_compiler_service(
        self: Rc<Self>,
        name: &str,
        callback: UnitService,
    ) -> Result<(), EvalError> {
        let weak = Rc::downgrade(&self);
        self.install_unit_service(name, move |ctx, id| {
            let evaluator = weak.upgrade().ok_or("evaluator expired")?;
            let active = evaluator.active(ctx).map_err(|e| e.to_string())?;
            callback(&active, id)
        })
    }
}
