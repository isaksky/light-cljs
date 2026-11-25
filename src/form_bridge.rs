//! Structured compiler/engine transport. Descriptors are actual engine arrays;
//! no printing, reader invocation, serde or JSON participates in conversion.
use crate::compiler_data::{Datum, DatumKind as D};
/// In-process descriptor ABI version shared with the canonical runtime.
pub const FORM_BRIDGE_VERSION: u32 = 5;
#[derive(Clone, Copy, Debug)]
pub struct ConversionLimits {
    pub nodes: usize,
    pub depth: usize,
}
impl Default for ConversionLimits {
    fn default() -> Self {
        Self {
            nodes: 100_000,
            depth: 128,
        }
    }
}

pub(crate) fn exact(a: &Datum, b: &Datum) -> bool {
    if a.identity() == b.identity() {
        return true;
    }
    let data = match (a.kind(), b.kind()) {
        (D::Nil, D::Nil) => true,
        (D::Boolean(a), D::Boolean(b)) => a == b,
        (D::Double(a), D::Double(b)) => a.to_bits() == b.to_bits(),
        (D::String(a), D::String(b)) => a == b,
        (D::Symbol(a), D::Symbol(b)) | (D::Keyword(a), D::Keyword(b)) => a == b,
        (
            D::Collection {
                kind: a,
                children: ac,
            },
            D::Collection {
                kind: b,
                children: bc,
            },
        ) => a == b && ac.len() == bc.len() && ac.iter().zip(bc).all(|(a, b)| exact(a, b)),
        (
            D::CompilerLiteral {
                kind: a,
                payload: ac,
            },
            D::CompilerLiteral {
                kind: b,
                payload: bc,
            },
        ) => a == b && exact(ac, bc),
        _ => false,
    };
    data && match (a.metadata(), b.metadata()) {
        (None, None) => true,
        (Some(a), Some(b)) => exact(&a, &b),
        _ => false,
    }
}
