//! The Goose VM's run-time compiler (light-cljs-856): a C ABI over this
//! package's frontend and Goose exporter, built as a shared library
//!
//! `cargo build --release -p big-boy-pants-js --example goose_frontend`
//!
//! (`target/release/examples/libgoose_frontend.dylib`, `.so` on Linux).
//! Goose programs load it on their first run-time compilation
//! (`crates/big-boy-pants-js/src/runtime_compiler.h`); a program that never
//! compiles at run time never opens it. It compiles and encodes; it never
//! runs JavaScript (`crates/big-boy-pants-js/AGENTS.md`).
//!
//! `bbp_goose_compile(units, count, mode, strict, parameter_units, site,
//! site_len, data, length, line, column) -> status` compiles the `count`
//! UTF-16 code units at `units` ([`export_runtime`]) and returns, through
//! `data` and `length`, a buffer the caller frees with `bbp_goose_free`:
//!
//! * status 0: a GSP image ([`export_runtime`]'s encoding), function 0 the
//!   compiled script or function.
//! * status 1: a compile error, the buffer its UTF-8 message, `line` and
//!   `column` where it is (1-based, as [`SyntaxError`] counts them). The
//!   eval throws a SyntaxError with that message.
//! * status 2: refused, the buffer a UTF-8 reason: bad arguments, a mode not
//!   implemented, or an internal failure (the helper thread, a panic).
//!
//! Modes: 0 indirect eval (`strict` 1 compiles the script strict, otherwise
//! its prologue decides); 1 the Function constructor's source, the
//! parameter list's closing `)` ending `parameter_units` units in; 2 direct
//! eval with the opaque site record `site`/`site_len`, which light-cljs-858
//! defines (refused until then).
//!
//! [`export_runtime`]: big_boy_pants_js::frontend::goose::export_runtime
//! [`SyntaxError`]: big_boy_pants_js::frontend::SyntaxError

use big_boy_pants_js::frontend::SyntaxErrorKind;
use big_boy_pants_js::frontend::goose::{RuntimeMode, export_runtime};

const IMAGE: i32 = 0;
const SYNTAX_ERROR: i32 = 1;
const REFUSED: i32 = 2;

fn compile(
    units: &[u16],
    mode: i32,
    strict: i32,
    parameter_units: i64,
) -> (i32, Vec<u8>, i64, i64) {
    let mode = match (mode, strict) {
        (0, 0 | 1) => RuntimeMode::IndirectEval {
            strict: strict == 1,
        },
        (1, 0) if parameter_units >= 0 && parameter_units as u64 <= units.len() as u64 => {
            RuntimeMode::DynamicFunction {
                parameter_units: parameter_units as usize,
            }
        }
        (2, _) => {
            return (
                REFUSED,
                b"direct eval: no site record yet (light-cljs-858)".to_vec(),
                0,
                0,
            );
        }
        _ => return (REFUSED, b"bad run-time compiler arguments".to_vec(), 0, 0),
    };
    match export_runtime(units, mode) {
        Ok(export) => (IMAGE, export.bytes, 0, 0),
        Err(e) if e.kind == SyntaxErrorKind::Internal => (REFUSED, e.message.into_bytes(), 0, 0),
        Err(e) => (
            SYNTAX_ERROR,
            e.message.into_bytes(),
            i64::from(e.line),
            i64::from(e.column),
        ),
    }
}

/// # Safety
///
/// `units` points to `count` readable UTF-16 units (any pointer when
/// `count` is 0); `data`, `length`, `line` and `column` are writable. The
/// site record is not read yet.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bbp_goose_compile(
    units: *const u16,
    count: i64,
    mode: i32,
    strict: i32,
    parameter_units: i64,
    _site: *const u8,
    _site_len: i64,
    data: *mut *mut u8,
    length: *mut i64,
    line: *mut i64,
    column: *mut i64,
) -> i32 {
    // Goose arrays are packed: read each unit without assuming alignment.
    let units: Vec<u16> = if count <= 0 || units.is_null() {
        Vec::new()
    } else {
        // SAFETY: the caller passes `count` readable units.
        (0..count as usize)
            .map(|i| unsafe { units.add(i).read_unaligned() })
            .collect()
    };
    let (status, bytes, at_line, at_column) =
        std::panic::catch_unwind(|| compile(&units, mode, strict, parameter_units))
            .unwrap_or_else(|_| (REFUSED, b"the run-time compiler panicked".to_vec(), 0, 0));
    let bytes = bytes.into_boxed_slice();
    // SAFETY: the caller passes writable out-parameters.
    unsafe {
        *length = bytes.len() as i64;
        *data = Box::into_raw(bytes).cast::<u8>();
        *line = at_line;
        *column = at_column;
    }
    status
}

/// # Safety
///
/// `data` and `length` are exactly what one `bbp_goose_compile` call
/// returned, and each buffer is freed once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bbp_goose_free(data: *mut u8, length: i64) {
    if data.is_null() {
        return;
    }
    // SAFETY: the buffer is the boxed slice bbp_goose_compile leaked.
    drop(unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(data, length as usize)) });
}
