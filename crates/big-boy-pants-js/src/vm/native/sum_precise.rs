//! Exact binary64 summation in units of 2^-1074. Separate unsigned totals
//! avoid intermediate floating-point overflow and cancellation loss.
use super::*;
use crate::vm::exception::Exception;

// A finite double needs at most 2098 bits in minimum-subnormal units.
// At most 2^53-1 inputs need fewer than 2151 bits, so 34 limbs suffice.
const LIMBS: usize = 34;
const MAX_COUNT: u64 = (1_u64 << 53) - 1;

#[derive(Debug)]
struct Sum {
    positive: [u64; LIMBS],
    negative: [u64; LIMBS],
    nan: bool,
    positive_infinity: bool,
    negative_infinity: bool,
    negative_zero: bool,
}

impl Sum {
    fn new() -> Self {
        Self {
            positive: [0; LIMBS],
            negative: [0; LIMBS],
            nan: false,
            positive_infinity: false,
            negative_infinity: false,
            negative_zero: true,
        }
    }

    fn add(&mut self, number: f64) {
        let bits = number.to_bits();
        if bits != (-0.0_f64).to_bits() {
            self.negative_zero = false;
        }
        if !number.is_finite() {
            self.nan |= number.is_nan();
            self.positive_infinity |= number == f64::INFINITY;
            self.negative_infinity |= number == f64::NEG_INFINITY;
            return;
        }
        let exponent = ((bits >> 52) & 0x7ff) as usize;
        let significand =
            (bits & ((1_u64 << 52) - 1)) | if exponent == 0 { 0 } else { 1_u64 << 52 };
        let shift = exponent.saturating_sub(1);
        let total = if number.is_sign_negative() {
            &mut self.negative
        } else {
            &mut self.positive
        };
        let mut carry = (significand as u128) << (shift % 64);
        for limb in &mut total[shift / 64..] {
            carry += *limb as u128;
            *limb = carry as u64;
            carry >>= 64;
            if carry == 0 {
                break;
            }
        }
        debug_assert_eq!(carry, 0);
    }

    fn finish(&self) -> f64 {
        if self.nan || (self.positive_infinity && self.negative_infinity) {
            return f64::NAN;
        }
        if self.positive_infinity {
            return f64::INFINITY;
        }
        if self.negative_infinity {
            return f64::NEG_INFINITY;
        }
        let negative = self
            .negative
            .iter()
            .rev()
            .cmp(self.positive.iter().rev())
            .is_gt();
        let (mut magnitude, smaller) = if negative {
            (self.negative, &self.positive)
        } else {
            (self.positive, &self.negative)
        };
        let mut borrow = false;
        for (limb, small) in magnitude.iter_mut().zip(smaller) {
            let (value, b1) = limb.overflowing_sub(*small);
            let (value, b2) = value.overflowing_sub(u64::from(borrow));
            *limb = value;
            borrow = b1 || b2;
        }
        let Some(high) = magnitude.iter().rposition(|limb| *limb != 0) else {
            return if self.negative_zero { -0.0 } else { 0.0 };
        };
        let length = high * 64 + 64 - magnitude[high].leading_zeros() as usize;
        let sign = u64::from(negative) << 63;
        if length <= 52 {
            return f64::from_bits(sign | magnitude[0]);
        }
        let shift = length - 53;
        let mut significand = magnitude[shift / 64] >> (shift % 64);
        if !shift.is_multiple_of(64) && shift / 64 + 1 < LIMBS {
            significand |= magnitude[shift / 64 + 1] << (64 - shift % 64);
        }
        if shift > 0 {
            let halfway = shift - 1;
            let round = magnitude[halfway / 64] & (1_u64 << (halfway % 64)) != 0;
            let sticky = magnitude[..halfway / 64].iter().any(|x| *x != 0)
                || magnitude[halfway / 64] & ((1_u64 << (halfway % 64)) - 1) != 0;
            if round && (sticky || significand & 1 != 0) {
                significand += 1;
            }
        }
        let mut exponent = length - 52;
        if significand == 1_u64 << 53 {
            significand >>= 1;
            exponent += 1;
        }
        if exponent >= 2047 {
            f64::from_bits(sign | (2047_u64 << 52))
        } else {
            f64::from_bits(sign | ((exponent as u64) << 52) | (significand & ((1_u64 << 52) - 1)))
        }
    }
}

#[derive(Debug)]
enum Stage {
    Iterator,
    Next,
    NextResult,
    Done,
    Value,
    Add,
    Close,
    CloseMethod,
    CloseResult,
    Rethrow,
}

#[derive(Debug)]
pub(super) struct State {
    stage: Stage,
    acquisition: Option<iterator::State>,
    iterator: Value,
    next: Value,
    result: Value,
    sum: Sum,
    count: u64,
    pending: Option<Exception>,
}

impl Trace for State {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        self.iterator.trace(visit);
        self.next.trace(visit);
        self.result.trace(visit);
        if let Some(acquisition) = &self.acquisition {
            acquisition.trace(visit);
        }
        if let Some(exception) = &self.pending {
            exception.value.trace(visit);
        }
    }
}

impl State {
    pub(super) fn new(items: Value) -> Self {
        Self {
            stage: Stage::Iterator,
            acquisition: Some(iterator::State::get_iterator(items)),
            iterator: Value::Undefined,
            next: Value::Undefined,
            result: Value::Undefined,
            sum: Sum::new(),
            count: 0,
            pending: None,
        }
    }

    // IteratorStepValue errors propagate directly. Validation/count errors
    // close the iterator; a pending throw wins over errors from return.
    pub(super) fn catches(&self) -> bool {
        matches!(
            self.stage,
            Stage::Add | Stage::Close | Stage::CloseMethod | Stage::CloseResult
        )
    }
    pub(super) fn catches_step(&self) -> bool {
        matches!(self.stage, Stage::Add | Stage::Close)
    }
    pub(super) fn failed(&mut self, exception: Exception) {
        if self.pending.is_none() {
            self.pending = Some(exception);
            self.stage = Stage::Close;
        } else {
            self.stage = Stage::Rethrow;
        }
    }
    fn value(input: Input) -> Result<Value, Fault> {
        match input {
            Input::CallResult(v) => Ok(v),
            _ => Err(Fault::Internal("sumPrecise expected call result")),
        }
    }
    fn read(
        &mut self,
        heap: &Heap,
        task: TaskId,
        base: Value,
        name: &str,
        stage: Stage,
    ) -> Result<Action, Fault> {
        self.stage = stage;
        match property::read(
            heap,
            task,
            &base,
            &property::Key::Name(JsString::from(name)),
        )? {
            Some(property::Read::Deferred(callee)) => Ok(callee.action(base)),
            Some(property::Read::Value(v)) => self.step(heap, task, Input::CallResult(v)),
            None => self.step(heap, task, Input::CallResult(Value::Undefined)),
        }
    }
    pub(super) fn step(
        &mut self,
        heap: &Heap,
        task: TaskId,
        input: Input,
    ) -> Result<Action, Fault> {
        match self.stage {
            Stage::Iterator => {
                let action = self
                    .acquisition
                    .as_mut()
                    .ok_or(Fault::Internal("missing GetIterator"))?
                    .step(heap, task, input)?;
                if let Action::Return(record) = action {
                    self.iterator =
                        property::get_field(heap, task, &record, &JsString::from("iterator"))?;
                    self.next = property::get_field(heap, task, &record, &JsString::from("next"))?;
                    self.acquisition = None;
                    self.stage = Stage::Next;
                    Ok(Action::Continue)
                } else {
                    Ok(action)
                }
            }
            Stage::Next => {
                self.stage = Stage::NextResult;
                Ok(Action::Call {
                    callee: self.next.clone(),
                    this: self.iterator.clone(),
                    args: vec![],
                })
            }
            Stage::NextResult => {
                self.result = Self::value(input)?;
                if !matches!(self.result, Value::Object(_)) {
                    return Err(Fault::TypeError("iterator result must be an object".into()));
                }
                self.read(heap, task, self.result.clone(), "done", Stage::Done)
            }
            Stage::Done => {
                if Self::value(input)?.truthy() {
                    return Ok(Action::Return(Value::Number(self.sum.finish())));
                }
                self.read(heap, task, self.result.clone(), "value", Stage::Value)
            }
            Stage::Value => {
                self.result = Self::value(input)?;
                self.stage = Stage::Add;
                Ok(Action::Continue)
            }
            Stage::Add => {
                if self.count == MAX_COUNT {
                    return Err(Fault::RangeError("sumPrecise has too many elements".into()));
                }
                self.count += 1;
                let Value::Number(number) = self.result else {
                    return Err(Fault::TypeError(
                        "sumPrecise element must be a Number".into(),
                    ));
                };
                self.sum.add(number);
                self.result = Value::Undefined;
                self.stage = Stage::Next;
                Ok(Action::Continue)
            }
            Stage::Close => self.read(
                heap,
                task,
                self.iterator.clone(),
                "return",
                Stage::CloseMethod,
            ),
            Stage::CloseMethod => {
                let method = Self::value(input)?;
                if matches!(method, Value::Undefined | Value::Null) {
                    self.stage = Stage::Rethrow;
                    Ok(Action::Continue)
                } else {
                    self.stage = Stage::CloseResult;
                    Ok(Action::Call {
                        callee: method,
                        this: self.iterator.clone(),
                        args: vec![],
                    })
                }
            }
            Stage::CloseResult => {
                self.stage = Stage::Rethrow;
                Ok(Action::Continue)
            }
            Stage::Rethrow => Err(Fault::Rethrow(
                self.pending
                    .take()
                    .ok_or(Fault::Internal("missing sumPrecise exception"))?,
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Action, Fault, Heap, Input, MAX_COUNT, Stage, State, Sum, TaskId, Value};

    fn sum(values: &[f64]) -> f64 {
        let mut sum = Sum::new();
        for value in values {
            sum.add(*value);
        }
        sum.finish()
    }

    #[test]
    fn element_limit_raises_range_error_before_type_validation() {
        // Fast-forward only the count: traversing 2^53 inputs is impractical.
        let heap = Heap::default();
        let task = TaskId::from_raw(1);
        let mut state = State::new(Value::Undefined);
        state.stage = Stage::Add;
        state.count = MAX_COUNT - 1;
        state.result = Value::Number(1.0);
        assert!(matches!(
            state.step(&heap, task, Input::Start),
            Ok(Action::Continue)
        ));
        assert_eq!(state.sum.finish(), 1.0);
        assert_eq!(state.count, MAX_COUNT);
        state.stage = Stage::Done;
        assert!(matches!(
            state.step(&heap, task, Input::CallResult(Value::Bool(true))),
            Ok(Action::Return(Value::Number(1.0)))
        ));
        for value in [Value::Number(1.0), Value::Undefined] {
            state.stage = Stage::Add;
            state.result = value;
            assert!(state.catches_step());
            assert!(matches!(
                state.step(&heap, task, Input::Start),
                Err(Fault::RangeError(_))
            ));
        }

        // Drive the same catch/close actions used by the trampoline. The
        // sentinel stands for the engine's RangeError object, whose identity
        // must be retained while return is called exactly once.
        let realm = crate::realm::intrinsics(&heap).unwrap();
        let method = Value::Object(realm.object);
        let mut iterator = crate::object::OrdinaryObject::new(&heap, Some(task));
        let key = heap
            .keys()
            .intern(&crate::value::JsString::from("return"))
            .unwrap();
        iterator.define_with(key, method.clone(), crate::object::PropertyFlags::DATA);
        state.iterator = Value::Object(
            heap.alloc(crate::heap::HeapObject::Plain(iterator))
                .unwrap(),
        );
        state.failed(crate::vm::exception::Exception {
            value: Value::Number(17.0),
            error: Box::new(crate::error::VmError {
                kind: crate::error::VmErrorKind::RangeError,
                message: "sumPrecise has too many elements".into(),
                diagnostic: crate::error::Diagnostic {
                    executable: "sum-precise-limit".into(),
                    task,
                    world: crate::ids::WorldId::INITIAL,
                    instruction: 0,
                    byte_offset: None,
                    opcode: None,
                    source: None,
                    call_chain: vec![],
                },
            }),
        });
        let Ok(Action::Call { callee, this, args }) = state.step(&heap, task, Input::Start) else {
            panic!("expected return call");
        };
        assert_eq!(callee, method);
        assert_eq!(this, state.iterator);
        assert!(args.is_empty());
        let result = crate::vm::builtin::call(
            &heap,
            task,
            crate::vm::builtin::Builtin::Object,
            &this,
            &args,
        )
        .unwrap_or_else(|_| panic!("Object() failed"));
        assert!(matches!(
            state.step(&heap, task, Input::CallResult(result)),
            Ok(Action::Continue)
        ));
        let Err(Fault::Rethrow(exception)) = state.step(&heap, task, Input::Start) else {
            panic!("expected original error");
        };
        assert_eq!(exception.value, Value::Number(17.0));
        assert_eq!(
            exception.error.kind(),
            crate::error::VmErrorKind::RangeError
        );
        assert_eq!(state.sum.finish(), 1.0);
        assert!(!state.catches());
    }

    #[test]
    fn rounding_at_every_normal_exponent() {
        let tiny = f64::from_bits(1);
        for exponent in 2..=2046_u64 {
            let power = f64::from_bits(exponent << 52);
            let half = power * f64::EPSILON / 2.0;
            for sign in [-1.0, 1.0] {
                assert_eq!(sum(&[sign * power, sign * half]), sign * power);
                assert_eq!(
                    sum(&[sign * power, sign * half, sign * tiny]),
                    sign * f64::from_bits((exponent << 52) + 1)
                );
                assert_eq!(sum(&[sign * power, -sign * power, tiny]), tiny);
            }
        }
    }

    #[test]
    fn overflow_threshold_and_cancellation() {
        let half = f64::from_bits((1993_u64) << 52); // 2^970
        let tiny = f64::from_bits(1);
        for sign in [-1.0, 1.0] {
            assert_eq!(sum(&[sign * f64::MAX, sign * half]), sign * f64::INFINITY);
            assert_eq!(
                sum(&[sign * f64::MAX, sign * half, -sign * tiny]),
                sign * f64::MAX
            );
            assert_eq!(
                sum(&[sign * f64::MAX, sign * f64::MAX, -sign * f64::MAX]),
                sign * f64::MAX
            );
        }
        assert_eq!(
            sum(&[f64::MIN_POSITIVE, -tiny]).to_bits(),
            f64::MIN_POSITIVE.to_bits() - 1
        );
    }
}
