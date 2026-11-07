//! Bounded source export for the Goose experiment. This module never runs code.
//! The engine frontend supplies parsing, binding resolution and lowering.
use std::collections::BTreeSet;

use crate::frontend::{ScriptOptions, Span, SyntaxError, SyntaxErrorKind, compile_script_with};
use crate::value::Primitive;
use crate::vm::Executable;
use crate::vm::instr::{Opcode, OperandKind};

/// Version 3 (light-cljs-823) adds the direct-eval site bit of instruction
/// descriptors ([`DIRECT_EVAL_SITE`]); version-2 readers refuse it explicitly.
pub const VERSION: u32 = 3;
pub const MAX_SOURCE: usize = 8 * 1024 * 1024;
pub const MAX_RECORDS: usize = 1_048_576;
pub const MAX_FUNCTIONS: usize = 65_536;

/// Source-owned, pointer-free transport. Byte layout is documented in the
/// prototype README; integer fields are little endian, all table IDs are u32.
#[derive(Debug)]
pub struct Export {
    pub bytes: Vec<u8>,
    pub functions: usize,
    pub instructions: usize,
    pub operations: BTreeSet<&'static str>,
    pub string_constants: BTreeSet<String>,
    pub form_operations: BTreeSet<String>,
}

fn problem(source: &str, kind: SyntaxErrorKind, span: Span, message: &str) -> SyntaxError {
    SyntaxError::new(source, kind, span, message)
}

fn bounded(n: usize, limit: usize, source: &str) -> Result<u32, SyntaxError> {
    if n > limit {
        return Err(problem(
            source,
            SyntaxErrorKind::TooLarge,
            Span::new(0, 0),
            "Goose export resource budget exceeded",
        ));
    }
    u32::try_from(n).map_err(|_| {
        problem(
            source,
            SyntaxErrorKind::TooLarge,
            Span::new(0, 0),
            "Goose export u32 overflow",
        )
    })
}

// Every opcode the frontend emits is exported (light-cljs-783,
// docs/javascript-coverage.md decision 1). Export never asserts that the Goose
// VM executes an operation: an unimplemented opcode fails when it runs, with a
// fault that names it, and a dead one never fails.

/// GSP operand category of an opcode (descriptor bits 8..15).
fn operand_tag(kind: OperandKind) -> u32 {
    match kind {
        OperandKind::None => 0,
        OperandKind::Const | OperandKind::Name => 1,
        OperandKind::Local => 2,
        OperandKind::Target => 3,
        OperandKind::Function => 4,
        OperandKind::Capture => 5,
        _ => 6,
    }
}

/// The Goose names of the operand categories (`operand::` in the generated
/// schema), each with a kind of its category: `Immediate` is every other u16
/// operand (counts, selectors, host operations).
const OPERAND_CATEGORIES: [(&str, OperandKind); 7] = [
    ("None", OperandKind::None),
    ("Const", OperandKind::Const),
    ("Local", OperandKind::Local),
    ("Target", OperandKind::Target),
    ("Function", OperandKind::Function),
    ("Capture", OperandKind::Capture),
    ("Immediate", OperandKind::Count),
];

/// Stack-effect word bits 24..27: what the multiplied pop count is
/// (`count::` in the generated schema).
const COUNT_SOURCES: [&str; 4] = ["None", "Operand", "Captures", "FormArity"];

/// Stack-effect word bits 28..31 (`flow::` in the generated schema).
fn flow_code(flow: crate::vm::instr::Flow) -> u32 {
    use crate::vm::instr::Flow;
    match flow {
        Flow::Next => 0,
        Flow::Jump => 1,
        Flow::Branch => 2,
        Flow::Catch => 3,
        Flow::End => 4,
    }
}

/// Descriptor bit 24: the instruction is the call of a direct-eval site
/// (`eval(...)` written as a plain call; `ExecutableSpec::eval_sites`). Image
/// metadata, not an opcode: only the four call opcodes of
/// [`DIRECT_EVAL_CALLS`] carry it, and bits 25..31 stay zero. The site's scope
/// descriptors (bindings, receiver, super home) are not exported; the Goose VM
/// fails such a call by name when its callee is the realm's %eval%.
pub const DIRECT_EVAL_SITE: u32 = 1 << 24;
/// The opcodes the lowerer emits at a direct-eval site's offset.
pub const DIRECT_EVAL_CALLS: [Opcode; 4] = [
    Opcode::Call,
    Opcode::CallMethod,
    Opcode::CallSpread,
    Opcode::CallMethodSpread,
];

/// The tag of a constant record (its first word). 5 is never used: it is the
/// Goose VM's heap-reference value tag. A Symbol constant names a well-known
/// symbol by its index here.
const UNDEFINED_CONSTANT: u32 = 0;
const NULL_CONSTANT: u32 = 1;
const BOOL_CONSTANT: u32 = 2;
const NUMBER_CONSTANT: u32 = 3;
const STRING_CONSTANT: u32 = 4;
const SYMBOL_CONSTANT: u32 = 6;
const BIGINT_CONSTANT: u32 = 7;
/// Every constant tag, named as its `Primitive` variant (`constant::` in
/// `inspection_schema`).
const CONSTANT_TAGS: [(&str, u32); 7] = [
    ("Undefined", UNDEFINED_CONSTANT),
    ("Null", NULL_CONSTANT),
    ("Bool", BOOL_CONSTANT),
    ("Number", NUMBER_CONSTANT),
    ("String", STRING_CONSTANT),
    ("Symbol", SYMBOL_CONSTANT),
    ("BigInt", BIGINT_CONSTANT),
];
pub const WELL_KNOWN_SYMBOLS: [&str; 15] = [
    "iterator",
    "asyncIterator",
    "toPrimitive",
    "hasInstance",
    "toStringTag",
    "dispose",
    "asyncDispose",
    "unscopables",
    "species",
    "isConcatSpreadable",
    "match",
    "matchAll",
    "search",
    "replace",
    "split",
];

/// Function flags: strict in bit 0, constructor kind in bits 8..15 (none,
/// ordinary, base class, derived class, default derived class), generator in
/// bit 16 and async in bit 17. The generator and async bits restate which of
/// `GeneratorStart`, `AsyncStart` or `AsyncGeneratorStart` the function's code
/// contains, so a loader learns the kind without scanning instructions.
fn constructor_code(kind: crate::ConstructorKind) -> u32 {
    match kind {
        crate::ConstructorKind::None => 0,
        crate::ConstructorKind::Ordinary => 1,
        crate::ConstructorKind::BaseClass => 2,
        crate::ConstructorKind::DerivedClass => 3,
        crate::ConstructorKind::DefaultDerivedClass => 4,
    }
}
/// Every constructor kind, in code order (`constructor::` in the generated schema).
const CONSTRUCTOR_KINDS: [crate::ConstructorKind; 5] = [
    crate::ConstructorKind::None,
    crate::ConstructorKind::Ordinary,
    crate::ConstructorKind::BaseClass,
    crate::ConstructorKind::DerivedClass,
    crate::ConstructorKind::DefaultDerivedClass,
];
fn function_flags(spec: &crate::vm::ExecutableSpec) -> u32 {
    let constructor = constructor_code(spec.constructor_kind);
    let (mut generator, mut asynchronous) = (false, false);
    let mut offset = 0;
    while offset < spec.code.len() {
        let opcode = Opcode::from_byte(spec.code[offset]).expect("validated opcode");
        match opcode {
            Opcode::GeneratorStart => generator = true,
            Opcode::AsyncStart => asynchronous = true,
            Opcode::AsyncGeneratorStart => (generator, asynchronous) = (true, true),
            _ => {}
        }
        offset += 1 + opcode.operand().size();
    }
    u32::from(spec.strict)
        | (constructor << 8)
        | (u32::from(generator) << 16)
        | (u32::from(asynchronous) << 17)
}

/// Generated stack-effect word of an opcode, read off `Instr::stack_effect`
/// and `Opcode::flow` (the engine validator's own rules): pops are
/// `base + multiplier * n` where `source` names n, pushes are fixed.
///
/// bits 0..7 base pops, 8..15 pushes, 16..23 multiplier, 24..27 source
/// (0 none, 1 the u16 operand, 2 the child function's captures, 3 the form
/// operation's arity), 28..31 flow (0 next, 1 jump, 2 branch, 3 catch, 4 end).
fn stack_effect_word(op: Opcode) -> u32 {
    use crate::vm::instr::{FormOp, Instr};
    let effect = |operand: u32, captures: u16| {
        Instr::decode(op, operand, 0, captures)
            .expect("operand decodes")
            .stack_effect()
    };
    let (source, base, multiplier, pushes) = match op.operand() {
        OperandKind::FormOp => {
            let pushes = effect(FormOp::ALL[0] as u32, 0).1;
            for form in FormOp::ALL {
                assert_eq!(
                    effect(form as u32, 0),
                    (form.arity(), pushes),
                    "{op:?} {form:?}"
                );
            }
            (3, 0, 1, pushes)
        }
        kind @ (OperandKind::Function | OperandKind::Count | OperandKind::ArgCount) => {
            let at = |n: u16| {
                if kind == OperandKind::Function {
                    effect(0, n)
                } else {
                    effect(u32::from(n), 0)
                }
            };
            let ((base, pushes), (one, _)) = (at(0), at(1));
            let multiplier = one - base;
            for n in [2, 7, 65_535] {
                assert_eq!(
                    at(n),
                    (base + multiplier * usize::from(n), pushes),
                    "{op:?}"
                );
            }
            (
                if kind == OperandKind::Function { 2 } else { 1 },
                base,
                multiplier,
                pushes,
            )
        }
        _ => {
            // Operand-independent: a valid selector for ThrowError is 0.
            let fixed = effect(0, 0);
            if let Some(other) = Instr::decode(op, 1, 1, 1) {
                assert_eq!(other.stack_effect(), fixed, "{op:?}");
            }
            (0, fixed.0, 0, fixed.1)
        }
    };
    let flow = flow_code(op.flow());
    assert!(base < 256 && pushes < 256 && multiplier < 256, "{op:?}");
    base as u32 | (pushes as u32) << 8 | (multiplier as u32) << 16 | source << 24 | flow << 28
}

#[derive(Default)]
struct Tables {
    functions: Vec<[u32; 13]>,
    constants: Vec<[u32; 3]>,
    children: Vec<u32>,
    instructions: Vec<[u32; 5]>,
    units: Vec<u16>,
    operations: BTreeSet<&'static str>,
    string_constants: BTreeSet<String>,
    form_operations: BTreeSet<String>,
}

impl Tables {
    fn string(&mut self, units: &[u16], source: &str) -> Result<(u32, u32), SyntaxError> {
        let start = bounded(self.units.len(), MAX_RECORDS, source)?;
        let len = bounded(units.len(), MAX_RECORDS, source)?;
        bounded(
            self.units.len().saturating_add(units.len()),
            MAX_RECORDS,
            source,
        )?;
        self.units.extend_from_slice(units);
        Ok((start, len))
    }
    fn function(
        &mut self,
        executable: &Executable,
        source: &str,
        depth: usize,
    ) -> Result<u32, SyntaxError> {
        if depth > 256 {
            return Err(problem(
                source,
                SyntaxErrorKind::TooDeep,
                Span::new(0, 0),
                "Goose function depth budget exceeded",
            ));
        }
        let id = bounded(self.functions.len(), MAX_FUNCTIONS - 1, source)?;
        self.functions.push([0; 13]);
        let spec = executable.to_spec();
        // Direct-eval sites export as marked calls (light-cljs-823): the
        // program runs, and only a direct eval that executes fails, by name.
        let eval_offsets: BTreeSet<u32> = spec.eval_sites.iter().map(|s| s.offset).collect();
        let mut marked = 0;
        // Children are constructed first, then their IDs form one contiguous
        // range. IDs survive all subsequent table growth.
        let mut children = Vec::new();
        for child in &spec.functions {
            children.push(self.function(child, source, depth + 1)?);
        }
        let child_start = bounded(self.children.len(), MAX_RECORDS, source)?;
        bounded(
            self.children.len().saturating_add(children.len()),
            MAX_RECORDS,
            source,
        )?;
        self.children.extend(children);
        let constant_start = bounded(self.constants.len(), MAX_RECORDS, source)?;
        for value in &spec.constants {
            let row = match value {
                Primitive::Undefined => [UNDEFINED_CONSTANT, 0, 0],
                Primitive::Null => [NULL_CONSTANT, 0, 0],
                Primitive::Bool(b) => [BOOL_CONSTANT, u32::from(*b), 0],
                Primitive::Number(n) => {
                    let b = n.to_bits();
                    [NUMBER_CONSTANT, b as u32, (b >> 32) as u32]
                }
                Primitive::String(s) => {
                    if let Ok(text) = String::from_utf16(s.code_units()) {
                        self.string_constants.insert(text);
                    }
                    let (start, len) = self.string(s.code_units(), source)?;
                    [STRING_CONSTANT, start, len]
                }
                Primitive::Symbol(symbol) => {
                    // The frontend emits only well-known symbols
                    // (`Symbol.unscopables` for `with`); a symbol has no
                    // source-stable identity otherwise.
                    let Some(index) = WELL_KNOWN_SYMBOLS
                        .iter()
                        .position(|name| *symbol == crate::value::symbol::well_known(name))
                    else {
                        return Err(problem(
                            source,
                            SyntaxErrorKind::Unsupported,
                            Span::new(0, source.len()),
                            "Goose export supports only well-known Symbol constants",
                        ));
                    };
                    [SYMBOL_CONSTANT, index as u32, 0]
                }
                Primitive::BigInt(n) => {
                    // UTF-16 pool range: the sign (0 or 1), then the magnitude
                    // in little-endian 16-bit digits without a high zero digit.
                    let (negative, limbs) = n.to_sign_magnitude();
                    let mut digits = vec![u16::from(negative)];
                    for limb in limbs {
                        digits.extend([limb as u16, (limb >> 16) as u16]);
                    }
                    while digits.len() > 1 && digits.last() == Some(&0) {
                        digits.pop();
                    }
                    let (start, len) = self.string(&digits, source)?;
                    [BIGINT_CONSTANT, start, len]
                }
            };
            bounded(self.constants.len() + 1, MAX_RECORDS, source)?;
            self.constants.push(row);
        }
        let instruction_start = bounded(self.instructions.len(), MAX_RECORDS, source)?;
        let mut offset = 0;
        let mut index = 0;
        while offset < spec.code.len() {
            // Input has already passed the engine's validator.
            let opcode = Opcode::from_byte(spec.code[offset]).expect("validated opcode");
            let location = executable
                .source_location(index)
                .expect("source-compiled instruction");
            let span = location.span;
            if source.get(span.start..span.end).is_none() {
                return Err(problem(
                    source,
                    SyntaxErrorKind::Internal,
                    Span::new(0, 0),
                    "invalid source provenance in Goose export",
                ));
            }
            let kind = opcode.operand();
            let size = kind.size();
            let mut arg = 0u32;
            for i in 0..size {
                arg |= u32::from(spec.code[offset + 1 + i]) << (8 * i);
            }
            // The kind tag lets the inspection decoder validate references
            // without binding its reader to the engine's private enum layout.
            let tag = operand_tag(kind);
            let site = if eval_offsets.contains(&(offset as u32)) {
                if !DIRECT_EVAL_CALLS.contains(&opcode) {
                    return Err(problem(
                        source,
                        SyntaxErrorKind::Internal,
                        Span::new(0, 0),
                        "direct eval site is not a call in Goose export",
                    ));
                }
                marked += 1;
                DIRECT_EVAL_SITE
            } else {
                0
            };
            bounded(self.instructions.len() + 1, MAX_RECORDS, source)?;
            self.instructions.push([
                u32::from(opcode as u8) | (tag << 8) | ((size as u32) << 16) | site,
                arg,
                bounded(span.start, MAX_SOURCE, source)?,
                bounded(span.end, MAX_SOURCE, source)?,
                bounded(offset, u32::MAX as usize, source)?,
            ]);
            self.operations.insert(opcode.name());
            if opcode == Opcode::FormOp {
                self.form_operations.insert(format!(
                    "{:?}",
                    crate::vm::instr::FormOp::from_u16(arg as u16)
                        .expect("validated form selector")
                ));
            }
            offset += 1 + size;
            index += 1;
        }
        if marked != eval_offsets.len() {
            return Err(problem(
                source,
                SyntaxErrorKind::Internal,
                Span::new(0, 0),
                "direct eval site is not an instruction in Goose export",
            ));
        }
        let name = spec
            .function_name
            .as_ref()
            .map_or(&[][..], |s| s.code_units());
        let (name_start, name_len) = self.string(name, source)?;
        self.functions[id as usize] = [
            instruction_start,
            index as u32,
            constant_start,
            spec.constants.len() as u32,
            child_start,
            spec.functions.len() as u32,
            u32::from(spec.params),
            u32::from(spec.locals),
            u32::from(spec.captures),
            name_start,
            name_len,
            u32::from(spec.function_length.unwrap_or(spec.params)),
            function_flags(&spec),
        ];
        Ok(id)
    }
}

/// Parse and lower every function, preserving exact source bytes once. This
/// accepts no values or host callbacks and cannot compute an expansion.
pub fn export_source(name: &str, source: &str) -> Result<Export, SyntaxError> {
    export_source_with(name, source, ScriptOptions::STRICT)
}

/// [`export_source`] with the script's top level in the mode `options` gives
/// (the conformance harness exports QuickJS's sloppy-mode test files).
pub fn export_source_with(
    name: &str,
    source: &str,
    options: ScriptOptions,
) -> Result<Export, SyntaxError> {
    bounded(source.len(), MAX_SOURCE, source)?;
    let executable = compile_script_with(name, source, options)?;
    encode(&executable, source)
}

/// Export a function body for source smoke tests, never ordinary-call support.
pub fn export_function(source: &str) -> Result<Export, SyntaxError> {
    bounded(source.len(), MAX_SOURCE, source)?;
    let executable = crate::frontend::compile_function(source)?;
    encode(&executable, source)
}

fn encode(executable: &Executable, source: &str) -> Result<Export, SyntaxError> {
    let mut tables = Tables::default();
    assert_eq!(tables.function(executable, source, 0)?, 0);
    let mut bytes = b"GSP\0".to_vec();
    let word = |out: &mut Vec<u8>, n: u32| out.extend_from_slice(&n.to_le_bytes());
    for n in [
        VERSION,
        source.len() as u32,
        tables.functions.len() as u32,
        tables.constants.len() as u32,
        tables.children.len() as u32,
        tables.instructions.len() as u32,
        tables.units.len() as u32,
    ] {
        word(&mut bytes, n);
    }
    bytes.extend_from_slice(source.as_bytes());
    for row in &tables.functions {
        for &n in row {
            word(&mut bytes, n);
        }
    }
    for row in &tables.constants {
        for &n in row {
            word(&mut bytes, n);
        }
    }
    for &n in &tables.children {
        word(&mut bytes, n);
    }
    for row in &tables.instructions {
        for &n in row {
            word(&mut bytes, n);
        }
    }
    for n in &tables.units {
        bytes.extend_from_slice(&n.to_le_bytes());
    }
    Ok(Export {
        bytes,
        functions: tables.functions.len(),
        instructions: tables.instructions.len(),
        operations: tables.operations,
        string_constants: tables.string_constants,
        form_operations: tables.form_operations,
    })
}

/// Generated from the same checked opcode surface as the exporter. It is
/// versioned source, never interpreted code supplied by an input image.
pub fn inspection_schema() -> String {
    use crate::vm::instr::{FormOp, ThrowKind};
    let opcodes = || (0..=255).filter_map(Opcode::from_byte);
    let name = |op: Opcode| format!("opcode::{op:?}");
    let mut text = String::from(
        "// Generated by regenerate.bb from the Rust exporter; do not edit.\n\
         // Goose sources name these numbers only through the constants below\n\
         // (crates/big-boy-pants-js/AGENTS.md).\n\
         \n\
         // The byte of every opcode of the shared registry (rust/src/vm/instr.rs),\n\
         // named as its Rust `Opcode` variant.\n",
    );
    for op in opcodes() {
        text.push_str(&format!("let {} = {};\n", name(op), op as u8));
    }
    text.push_str("\n// The selector of every form operation (`FormOp`, the FormOp operand).\n");
    for op in FormOp::ALL {
        text.push_str(&format!("let form_op::{op:?} = {};\n", op as u16));
    }
    text.push_str("\n// The selector of every static throw (`ThrowKind`, ThrowError's operand).\n");
    for kind in (0..=u16::MAX).map_while(ThrowKind::from_u16) {
        text.push_str(&format!("let throw_kind::{kind:?} = {};\n", kind as u16));
    }
    text.push_str(
        "\n// The index of every well-known symbol (a Symbol constant's operand),\n\
         // capitalized: well_known::ToPrimitive is Symbol.toPrimitive.\n",
    );
    for (index, symbol) in WELL_KNOWN_SYMBOLS.iter().enumerate() {
        let (first, rest) = symbol.split_at(1);
        text.push_str(&format!(
            "let well_known::{}{rest} = {index};\n",
            first.to_uppercase()
        ));
    }
    text.push_str(
        "\n// The tag of every constant record (its first word), named as its\n\
         // `Primitive` variant. 5 is never a constant tag.\n",
    );
    for (variant, tag) in CONSTANT_TAGS {
        text.push_str(&format!("let constant::{variant} = {tag};\n"));
    }
    text.push_str("\n// A function's constructor kind (function flags bits 8..15).\n");
    for (kind, code) in CONSTRUCTOR_KINDS.iter().zip(0..) {
        assert_eq!(constructor_code(*kind), code);
        text.push_str(&format!("let constructor::{kind:?} = {code};\n"));
    }
    text.push_str("\n// An opcode's operand category (descriptor bits 8..15).\n");
    for (category, kind) in OPERAND_CATEGORIES {
        text.push_str(&format!(
            "let operand::{category} = {};\n",
            operand_tag(kind)
        ));
    }
    text.push_str("\n// What multiplies a stack effect's pops (stack-effect bits 24..27).\n");
    for (source, code) in COUNT_SOURCES.iter().zip(0..) {
        text.push_str(&format!("let count::{source} = {code};\n"));
    }
    text.push_str("\n// An opcode's control flow (stack-effect bits 28..31).\n");
    {
        use crate::vm::instr::Flow;
        for flow in [Flow::Next, Flow::Jump, Flow::Branch, Flow::Catch, Flow::End] {
            text.push_str(&format!("let flow::{flow:?} = {};\n", flow_code(flow)));
        }
    }
    text.push_str(
        "\n// Every opcode's tables; -1, false or \"\" for an unassigned byte.\n\
         fn export_descriptor(op: i64) -> i64 {\n    return match op {\n",
    );
    for op in opcodes() {
        let kind = op.operand();
        let descriptor =
            u32::from(op as u8) | (operand_tag(kind) << 8) | ((kind.size() as u32) << 16);
        text.push_str(&format!("        {} => {descriptor},\n", name(op)));
    }
    text.push_str("        _ => -1,\n    };\n}\n\nfn export_form_arity(op: i64) -> i64 {\n    return match op {\n");
    for op in FormOp::ALL {
        text.push_str(&format!("        form_op::{op:?} => {},\n", op.arity()));
    }
    text.push_str("        _ => -1,\n    };\n}\n\nfn export_name_operand(op: i64) -> bool {\n    return match op {\n");
    for op in opcodes().filter(|op| op.operand() == OperandKind::Name) {
        text.push_str(&format!("        {} => true,\n", name(op)));
    }
    text.push_str(
        "        _ => false,\n    };\n}\n\n\
         // The call opcodes that may carry the direct-eval site bit (descriptor\n\
         // bit 24, GSP version 3).\n\
         fn export_direct_eval_call(op: i64) -> bool {\n    return match op {\n",
    );
    for op in DIRECT_EVAL_CALLS {
        text.push_str(&format!("        {} => true,\n", name(op)));
    }
    text.push_str(
        "        _ => false,\n    };\n}\n\n\
         // Stack effect and control flow, from the engine validator's rules:\n\
         // bits 0..7 base pops, 8..15 pushes, 16..23 multiplier, 24..27 source of\n\
         // the multiplied count (0 none, 1 operand, 2 child captures, 3 form arity),\n\
         // 28..31 flow (0 next, 1 jump, 2 branch, 3 catch, 4 end).\n\
         fn export_stack_effect(op: i64) -> i64 {\n    return match op {\n",
    );
    for op in opcodes() {
        text.push_str(&format!(
            "        {} => {},\n",
            name(op),
            stack_effect_word(op)
        ));
    }
    text.push_str("        _ => -1,\n    };\n}\n\nfn export_opcode_name(op: i64) -> const u8[:] {\n    return match op {\n");
    for op in opcodes() {
        text.push_str(&format!("        {} => \"{}\",\n", name(op), op.name()));
    }
    text.push_str("        _ => \"\",\n    };\n}\n\nfn export_well_known_symbol(index: i64) -> const u8[:] {\n    return match index {\n");
    for (index, name) in WELL_KNOWN_SYMBOLS.iter().enumerate() {
        text.push_str(&format!("        {index} => \"{name}\",\n"));
    }
    text.push_str(
        "        _ => \"\",\n    };\n}\n\n\
         // Valid selectors of a ThrowKind or built-in global operand are 0..count;\n\
         // -1 for opcodes without such a selector (FormOp has export_form_arity).\n\
         fn export_selector_count(op: i64) -> i64 {\n    return match op {\n",
    );
    for op in opcodes() {
        let valid = |n: u16| match op.operand() {
            OperandKind::ThrowKind => Some(ThrowKind::from_u16(n).is_some()),
            OperandKind::BuiltinGlobal => Some(crate::realm::Global::from_u16(n).is_some()),
            _ => None,
        };
        if valid(0).is_some() {
            let count = (0..=u16::MAX)
                .take_while(|&n| valid(n) == Some(true))
                .count();
            assert!(
                (count..=usize::from(u16::MAX)).all(|n| valid(n as u16) == Some(false)),
                "{op:?}"
            );
            text.push_str(&format!("        {} => {count},\n", name(op)));
        }
    }
    text.push_str("        _ => -1,\n    };\n}\n");
    text
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn whole_function_tree_and_exact_source_survive() {
        let source = "const μ = '\\ud800😀'; function f(x) { return y => x + y; } f(3);";
        let a = export_source("unicode.js", source).unwrap();
        let b = export_source("unicode.js", source).unwrap();
        assert_eq!(a.bytes, b.bytes);
        assert_eq!(a.functions, 3);
        assert_eq!(&a.bytes[32..32 + source.len()], source.as_bytes());
        assert!(a.operations.contains("get_capture"));
        assert!(a.bytes.windows(2).any(|w| w == [0, 216]));
    }
    /// Rows of one GSP table: (byte offset, record count, words per record).
    fn rows(export: &Export, table: usize) -> Vec<Vec<u32>> {
        let word = |at: usize| u32::from_le_bytes(export.bytes[at..at + 4].try_into().unwrap());
        let widths = [13, 3, 1, 5];
        let mut at = 32 + word(8) as usize;
        for (t, width) in widths.iter().enumerate().take(table) {
            at += word(12 + 4 * t) as usize * width * 4;
        }
        (0..word(12 + 4 * table) as usize)
            .map(|i| {
                (0..widths[table])
                    .map(|w| word(at + (i * widths[table] + w) * 4))
                    .collect()
            })
            .collect()
    }
    fn units(export: &Export, start: u32, len: u32) -> Vec<u16> {
        let word = |at: usize| u32::from_le_bytes(export.bytes[at..at + 4].try_into().unwrap());
        let mut at = 32 + word(8) as usize;
        for (t, width) in [13, 3, 1, 5].into_iter().enumerate() {
            at += word(12 + 4 * t) as usize * width * 4;
        }
        (start..start + len)
            .map(|i| {
                let at = at + 2 * i as usize;
                u16::from_le_bytes([export.bytes[at], export.bytes[at + 1]])
            })
            .collect()
    }
    #[test]
    fn unimplemented_operations_on_dead_paths_export() {
        // The whole program exports; the Goose VM fails only if it runs
        // `construct`, naming it there (light-cljs-783).
        let source = "function dormant() { return new Date(); } 1;";
        let export = export_source("dead.js", source).unwrap();
        assert!(
            export.operations.contains("construct"),
            "{:?}",
            export.operations
        );
        let construct = rows(&export, 3)
            .into_iter()
            .find(|row| row[0] & 255 == Opcode::Construct as u32)
            .unwrap();
        assert!(source[construct[2] as usize..construct[3] as usize].contains("new"));
    }
    #[test]
    fn dead_direct_eval_exports_with_exactly_its_call_marked() {
        // A dead function's direct eval no longer refuses the program
        // (light-cljs-823): exactly the site's call carries the marker bit.
        let source = "function f(s) { g(s); return eval(s); } function h(s) { return eval(...s); } \
                      var o = { eval(s) { return s; } }; o.eval(1); (0, eval); 1;";
        let export = export_source("eval.js", source).unwrap();
        let marked: Vec<Vec<u32>> = rows(&export, 3)
            .into_iter()
            .filter(|row| row[0] & DIRECT_EVAL_SITE != 0)
            .collect();
        let ops: Vec<u32> = marked.iter().map(|row| row[0] & 255).collect();
        assert_eq!(
            ops,
            [Opcode::Call as u32, Opcode::CallSpread as u32],
            "{marked:?}"
        );
        for row in &marked {
            // The call's span is its `(`, right after the callee `eval`.
            let text = &source[row[2] as usize - 4..row[3] as usize];
            assert!(text.starts_with("eval("), "{text:?}");
            assert_eq!(row[0] >> 25, 0);
        }
        // No descriptor sets bits 25..31.
        assert!(rows(&export, 3).iter().all(|row| row[0] >> 24 <= 1));
        assert_eq!(VERSION, 3);
        assert_eq!(
            u32::from_le_bytes(export.bytes[4..8].try_into().unwrap()),
            3
        );
    }
    #[test]
    fn symbol_bigint_constants_and_function_kinds_are_encoded() {
        let export = export_source_with(
            "kinds.js",
            "var x; with ({}) { x; } 0n; 12345678901234567890n; class A {} class B extends A {} \
             class C extends A { constructor() { super(); } } function* g() {} \
             async function h() {} async function* k() {} function o() {}",
            ScriptOptions::SLOPPY,
        )
        .unwrap();
        let constants = rows(&export, 1);
        let unscopables = WELL_KNOWN_SYMBOLS
            .iter()
            .position(|n| *n == "unscopables")
            .unwrap();
        assert!(
            constants.contains(&vec![SYMBOL_CONSTANT, unscopables as u32, 0]),
            "{constants:?}"
        );
        let bigints: Vec<Vec<u16>> = constants
            .iter()
            .filter(|row| row[0] == BIGINT_CONSTANT)
            .map(|row| units(&export, row[1], row[2]))
            .collect();
        let big = 12345678901234567890u64;
        let digits = (0..4).map(|i| (big >> (16 * i)) as u16);
        assert!(bigints.contains(&vec![0]), "{bigints:?}");
        assert!(
            bigints.contains(&std::iter::once(0).chain(digits).collect()),
            "{bigints:?}"
        );
        let mut kinds: Vec<u32> = rows(&export, 0).iter().map(|f| f[12] >> 8).collect();
        kinds.sort();
        // Script (0); A, B, C, the default derived B's (2, 4, 3); g, h, k, o:
        // ordinary (1) only for o; generator 1 << 8, async 2 << 8 above the kind.
        for kind in [0, 1, 2, 3, 4, 0x100, 0x200, 0x300] {
            assert!(kinds.contains(&kind), "{kind:#x} in {kinds:x?}");
        }
    }
    #[test]
    fn exported_code_round_trips_through_the_engine_validator() {
        // The exporter writes `Executable::to_spec` bytes, so every operand
        // must survive it: `using` dispose counts once encoded as 0, which
        // the Goose flow checker rejected (pants/generator_return_throw).
        let source = "function* g() { try { using a = null, b = null; yield 1; } finally { 0; } } \
                      async function h() { try { await using c = null; } finally { 0; } } \
                      function k(...r) { const { x, ...y } = r; return `${x}` + [...y]; }";
        let executable =
            crate::frontend::compile_script_with("round.js", source, ScriptOptions::STRICT)
                .unwrap();
        let mut pending = vec![executable];
        let mut seen = 0;
        while let Some(e) = pending.pop() {
            let spec = e.to_spec();
            let again = Executable::load(e.to_spec())
                .unwrap_or_else(|error| panic!("{}: {error}", spec.name));
            assert_eq!(again.to_spec().code, spec.code, "{}", spec.name);
            pending.extend(spec.functions);
            seen += 1;
        }
        assert!(seen >= 4);
        let export = export_source("round.js", source).unwrap();
        assert!(
            export.operations.contains("using_dispose")
                && export.operations.contains("using_dispose_async")
        );
    }
    #[test]
    fn generated_schema_has_a_stack_effect_for_every_opcode() {
        use crate::vm::instr::{Flow, FormOp, Instr};
        let schema = inspection_schema();
        let table = schema
            .split("fn export_stack_effect(op: i64) -> i64 {")
            .nth(1)
            .unwrap()
            .split("_ => -1")
            .next()
            .unwrap();
        let byte = |name: &str| {
            (0..=255)
                .filter_map(Opcode::from_byte)
                .find(|op| format!("opcode::{op:?}") == name)
                .unwrap() as u8
        };
        let words: std::collections::BTreeMap<u8, u64> = table
            .lines()
            .filter_map(|line| line.trim().trim_end_matches(',').split_once(" => "))
            .map(|(op, word)| (byte(op), word.parse().unwrap()))
            .collect();
        assert_eq!(words.len(), 198);
        for (&byte, &word) in &words {
            let op = Opcode::from_byte(byte).unwrap();
            let (base, pushes) = ((word & 255) as usize, ((word >> 8) & 255) as usize);
            let (multiplier, source) = (((word >> 16) & 255) as usize, (word >> 24) & 15);
            for n in [0u16, 1, 3, 200] {
                let (operand, captures, count) = match source {
                    0 => (u32::from(n), n, 0),
                    1 => (u32::from(n), 0, usize::from(n)),
                    2 => (u32::from(n), n, usize::from(n)),
                    _ => {
                        let form = FormOp::ALL[usize::from(n) % FormOp::ALL.len()];
                        (form as u32, 0, form.arity())
                    }
                };
                let Some(instr) = Instr::decode(op, operand, 0, captures) else {
                    assert_eq!(op, Opcode::ThrowError);
                    continue;
                };
                assert_eq!(
                    instr.stack_effect(),
                    (base + multiplier * count, pushes),
                    "{op:?} {n}"
                );
            }
            let flow = [Flow::Next, Flow::Jump, Flow::Branch, Flow::Catch, Flow::End];
            assert_eq!(flow[(word >> 28) as usize], op.flow(), "{op:?}");
        }
    }
    #[test]
    fn operator_provenance_uses_utf8_bytes_of_the_exact_source() {
        let source = "function source(μ) { return μ + 3; }";
        let export = export_function(source).unwrap();
        let word = |offset: usize| {
            u32::from_le_bytes(export.bytes[offset..offset + 4].try_into().unwrap()) as usize
        };
        let instructions = 32 + source.len() + word(12) * 52 + word(16) * 12 + word(20) * 4;
        let mut found = false;
        for i in 0..word(24) {
            let at = instructions + i * 20;
            let span = &source[word(at + 8)..word(at + 12)];
            if word(at) & 255 == Opcode::Plus as usize {
                assert_eq!(span, "+");
                found = true;
            }
        }
        assert!(found);
        let excessive = format!("{}0{}", "(".repeat(300), ")".repeat(300));
        assert_eq!(
            export_source("deep.js", &excessive).unwrap_err().kind,
            SyntaxErrorKind::TooDeep
        );
    }
    #[test]
    fn budgets_check_wide_inputs_before_narrowing() {
        assert!(bounded(65_536, MAX_FUNCTIONS, "").is_ok());
        assert!(bounded(65_537, MAX_FUNCTIONS, "").is_err());
        assert!(bounded(u32::MAX as usize + 1, usize::MAX, "").is_err());
        let source = " ".repeat(MAX_SOURCE + 1);
        assert_eq!(
            export_source("large.js", &source).unwrap_err().kind,
            SyntaxErrorKind::TooLarge
        );
    }
}

#[cfg(test)]
mod serial_tests {
    use super::*;

    // Independent Node/V8 and Pants expectations for original small source
    // cases. They exercise language execution, not macro expansion coverage.
    #[test]
    #[ignore = "bounded Babashka generator inputs supplied by correctness.bb"]
    fn generated_sources_for_goose() {
        let root = std::path::PathBuf::from(
            std::env::var_os("GOOSE_CORRECTNESS_INPUTS").expect("run correctness.bb"),
        );
        let mut inputs = std::fs::read_dir(&root)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|e| e == "js"))
            .collect::<Vec<_>>();
        inputs.sort();
        assert_eq!(inputs.len(), 45, "the v1 bounded manifest must be complete");
        for path in inputs {
            let source = std::fs::read_to_string(&path).unwrap();
            let export = export_function(&source)
                .unwrap_or_else(|error| panic!("{}: {error}\n{source}", path.display()));
            std::fs::write(path.with_extension("gsp"), export.bytes).unwrap();
        }
    }
    #[test]
    fn source_cases_export_and_match_reference_engines() {
        use crate::{RunOutcome, Runtime, ValueView, VmErrorKind};
        fn outcome(outcome: RunOutcome) -> String {
            match outcome {
                RunOutcome::Complete(value) => match value.view() {
                    ValueView::Undefined => "undefined:undefined".into(),
                    ValueView::Bool(b) => format!("boolean:{b}"),
                    ValueView::Number(n) if n == 0.0 && n.is_sign_negative() => "number:-0".into(),
                    ValueView::Number(n) => format!("number:{n}"),
                    ValueView::String => {
                        format!("string:{}", value.string().unwrap().to_string_lossy())
                    }
                    other => panic!("unexpected {other:?}"),
                },
                RunOutcome::Failed(error) => match error.kind() {
                    VmErrorKind::ReferenceError => "exception:ReferenceError".into(),
                    VmErrorKind::TypeError => "exception:TypeError".into(),
                    other => panic!("unexpected {other:?}"),
                },
                other => panic!("unexpected {other:?}"),
            }
        }
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../corpus/serial");
        let regenerate = std::env::var_os("GOOSE_REGENERATE").is_some();
        let schema = root
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("src/export_schema.goose");
        if regenerate {
            std::fs::write(&schema, inspection_schema()).unwrap();
        } else {
            assert_eq!(
                std::fs::read_to_string(&schema).unwrap(),
                inspection_schema()
            );
        }
        for (name, expected) in [
            ("calls", "boolean:true"),
            ("arguments", "boolean:true"),
            ("captures", "boolean:true"),
            ("mutations", "boolean:true"),
            ("control", "boolean:true"),
            ("numbers", "boolean:true"),
            ("strings", "boolean:true"),
            ("global-undefined", "undefined:undefined"),
            ("global-this", "boolean:true"),
            ("global-missing", "exception:ReferenceError"),
            ("negative-zero", "number:-0"),
            ("nan", "number:NaN"),
            ("tdz", "exception:ReferenceError"),
            ("type-error", "exception:TypeError"),
            ("not-callable", "exception:TypeError"),
            ("unsupported-coercion", "number:3"),
            // StringToNumber in unary and binary operators, `++`, `<` and
            // `==` (light-cljs-827; its work limits in serial_tests.goose).
            ("string-to-number", "boolean:true"),
            ("unsupported-prototype", "string:[object Object]"),
            // Unimplemented operations: `new Date` on a dead path, and a
            // `using` declaration reached under a `catch` that cannot swallow
            // its fault there (serial_tests.goose).
            ("unsupported-dead", "number:7"),
            ("unsupported-reached", "number:1"),
            // [[Construct]] of ordinary functions and the realm's
            // constructors, at every budget cut (light-cljs-831), and a
            // dependency wait inside a constructor (snapshot_tests.goose).
            ("construct", "boolean:true"),
            ("construct-wait", "goose"),
            // Nor BigInt values: constants on dead paths, then one reached.
            ("dormant-constants", "number:7"),
            ("bigint-reached", "string:bigint"),
            // Direct eval (light-cljs-823): Goose fails a reached one by name
            // under a `catch`, spread or not; a marked call whose `eval` is
            // not %eval% is an ordinary call; an indirect eval is a step of
            // the built-in Goose does not implement (serial_tests.goose).
            ("direct-eval-reached", "number:2"),
            ("direct-eval-spread", "number:2"),
            ("direct-eval-replaced", "number:2"),
            // Spread-argument limits (light-cljs-850), the Pants engine's
            // 65,535 (Node collects the whole iterator): serial_tests.goose
            // runs it under the default and a 128-value stack limit.
            ("spread-limits", "goose"),
            ("indirect-eval", "number:2"),
            ("loop", "limit"),
            // Control flow (light-cljs-786): finally blocks, native frames,
            // the call depth limit, and an endless loop that only
            // cancellation ends (serial_tests.goose).
            ("finally", "boolean:true"),
            ("native-frame", "boolean:true"),
            // ToPrimitive with hints and Symbol.toPrimitive in every
            // operator family (light-cljs-828).
            ("to-primitive", "boolean:true"),
            // Array.prototype.map and forEach as native frames (light-cljs-789).
            ("array-callbacks", "boolean:true"),
            // Array.prototype.push, join and toString as native frames (light-cljs-842).
            ("array-push-join", "boolean:true"),
            // Bound functions through Forward frames (light-cljs-852).
            ("bound-functions", "boolean:true"),
            // Receivers and strings only serial_tests.goose supplies: push
            // onto rows the job did not allocate, join's work and unit limits.
            ("array-push-receiver", "goose"),
            ("array-join-strings", "goose"),
            // for-of over user iterables and generators (light-cljs-843): at
            // every budget cut, dependency waits in its protocol calls and
            // body (snapshot_tests.goose), and cancellation inside it.
            ("for-of", "boolean:true"),
            ("for-of-wait", "goose"),
            ("cancel-for-of", "limit"),
            // The built-in array and string iterators (light-cljs-844): at
            // every budget cut, dependency waits in an element getter and
            // loop bodies (snapshot_tests.goose), and sources in rows the
            // job did not allocate (serial_tests.goose).
            ("builtin-iterators", "boolean:true"),
            ("builtin-iterator-wait", "goose"),
            ("builtin-iterator-receiver", "goose"),
            // for-in over proxies (light-cljs-845): traps, handler getters
            // and descriptor fields at every budget cut, and a dependency
            // wait in every trap phase, getPrototypeOf's included, which the
            // Pants engine does not implement (snapshot_tests.goose).
            ("proxy-for-in", "boolean:true"),
            ("proxy-for-in-wait", "goose"),
            ("proxy-get", "goose"),
            ("depth", "boolean:true"),
            ("cancel", "limit"),
            ("cancel-break", "limit"),
            ("cancel-fault", "limit"),
            // The property model (light-cljs-787): accessors, prototype
            // chains, deletion and a strict assignment without a setter.
            ("accessors", "boolean:true"),
            ("prototypes", "boolean:true"),
            ("deletes", "boolean:true"),
            ("getter-only", "exception:TypeError"),
            // Key enumeration (light-cljs-804): for-in over a three-object
            // chain, object rest and spread, and ordinary objects' index keys.
            ("for-in", "boolean:true"),
            ("object-rest", "boolean:true"),
            ("index-keys", "boolean:true"),
            // Arrays' and functions' named properties, a function's lazy
            // `prototype` and ToPrimitive through getters (light-cljs-805);
            // a function's own `name` (light-cljs-806); an array method and a
            // generator's `prototype` stay explicit gaps (serial_tests.goose).
            ("array-names", "boolean:true"),
            ("function-names", "boolean:true"),
            ("getter-value-of", "boolean:true"),
            ("function-name", "string:f"),
            ("function-keys", "boolean:true"),
            ("unsupported-array-method", "number:1"),
            ("unsupported-generator-prototype", "boolean:true"),
            // Generators, async functions and promise jobs (light-cljs-790).
            // `async` checks its order in a promise job: a wrong one is an
            // unhandled rejection, which fails Node's run. The "goose" cases
            // end in ways only the Goose job defines (an async entry's
            // settled value, a promise that never settles, a dependency
            // wait, a generator left suspended; serial_tests.goose and
            // snapshot_tests.goose).
            ("generators", "boolean:true"),
            ("async", "boolean:true"),
            ("generator-loop", "boolean:true"),
            ("async-entry", "goose"),
            ("unsettled", "goose"),
            ("suspended-wait", "goose"),
            ("suspended-generator", "goose"),
            ("suspended-stack", "goose"),
            ("cancel-suspended", "goose"),
            ("generator-limit", "goose"),
            ("promise-then", "goose"),
            // Reads and calls on primitive bases through their prototypes
            // (light-cljs-833); serial_tests.goose compares node rows.
            ("primitive-methods", "number:400"),
            // A Symbol base fails by name on Goose (serial_tests.goose).
            ("symbol-base-read", "goose"),
            ("symbol-base-write", "goose"),
            // Reclamation inside a job (light-cljs-847): loops far past the
            // Goose job's row limits, live values held by generators,
            // promises, for-of and a native frame across collections, and
            // a live set over the node limit, which still ends MemoryLimit
            // (serial_tests.goose).
            ("reclaim-loops", "boolean:true"),
            ("reclaim-roots", "boolean:true"),
            ("reclaim-bursts", "boolean:true"),
            ("reclaim-observed", "number:2"),
            ("reclaim-live", "goose"),
        ] {
            let source = std::fs::read_to_string(root.join(format!("{name}.js"))).unwrap();
            let export = export_function(&source).unwrap_or_else(|error| panic!("{name}: {error}"));
            let path = root.join(format!("{name}.gsp"));
            if regenerate {
                std::fs::write(path, &export.bytes).unwrap();
            } else {
                assert_eq!(std::fs::read(path).unwrap(), export.bytes, "{name}");
            }
            if name == "loop" || name.starts_with("cancel") || expected == "goose" {
                continue;
            }
            // Node's default native stack holds fewer than the 10,000 frames
            // `depth` needs; the Pants engine below is its oracle.
            if name != "depth" {
                let js = format!(
                    "try {{ const value = ({source})(); console.log(typeof value === 'number' ? 'number:' + (Object.is(value, -0) ? '-0' : String(value)) : typeof value + ':' + String(value)); }} catch(e) {{ console.log('exception:' + e.name); }}"
                );
                let output = std::process::Command::new("node")
                    .args(["-e", &js])
                    .output()
                    .unwrap();
                assert!(output.status.success());
                assert_eq!(
                    String::from_utf8(output.stdout).unwrap().trim(),
                    expected,
                    "{name}"
                );
            }
            let executable = crate::frontend::compile_function(&source).unwrap();
            let actual = outcome(
                Runtime::new()
                    .spawn(&executable, &[])
                    .unwrap()
                    .run()
                    .unwrap(),
            );
            assert_eq!(actual, expected, "{name}");
        }
        // Scripts (light-cljs-824): top-level declarations and global names,
        // exported as the conformance harness exports a strict script. Run
        // as they are, the Pants engine gives the expectation; Goose also
        // runs them over global properties it installs first, and runs the
        // "goose" case only that way (serial_tests.goose). A "sloppy-" script
        // is exported as a sloppy one.
        for (name, expected) in [
            ("script-declarations", "boolean:true"),
            ("script-setter", "goose"),
            // A Symbol.toPrimitive getter Goose installs (light-cljs-828).
            ("to-primitive-getter", "goose"),
            ("script-reserved-function", "number:1"),
            ("script-reserved-let", "number:2"),
            ("script-reserved-var", "boolean:false"),
            ("script-throw-messages", "boolean:true"),
            ("script-suspension", "boolean:true"),
            // Primitive receivers (light-cljs-833): accessors Goose installs
            // on realm prototypes, and a sloppy `this`, which the Pants
            // engine fails by name.
            ("primitive-receiver", "goose"),
            ("sloppy-this", "goose"),
            // Getters of Symbol.hasInstance and `prototype` Goose installs,
            // run by instanceof (light-cljs-834).
            ("instanceof-hooks", "goose"),
            // Targets of bind with `length` and `name` getters and odd
            // values Goose installs (light-cljs-852).
            ("bound-metadata", "goose"),
        ] {
            let source = std::fs::read_to_string(root.join(format!("{name}.js"))).unwrap();
            let options = if name.starts_with("sloppy-") {
                ScriptOptions::SLOPPY
            } else {
                ScriptOptions::STRICT
            };
            let export = export_source_with(name, &source, options)
                .unwrap_or_else(|error| panic!("{name}: {error}"));
            let path = root.join(format!("{name}.gsp"));
            if regenerate {
                std::fs::write(path, &export.bytes).unwrap();
            } else {
                assert_eq!(std::fs::read(path).unwrap(), export.bytes, "{name}");
            }
            if expected == "goose" {
                continue;
            }
            let executable = compile_script_with(name, &source, ScriptOptions::STRICT).unwrap();
            let actual = outcome(
                Runtime::new()
                    .spawn(&executable, &[])
                    .unwrap()
                    .run()
                    .unwrap(),
            );
            assert_eq!(actual, expected, "{name}");
        }
        // Constructors and direct const writes now export (light-cljs-783):
        // the Goose VM fails `construct` by name when it runs, and runs
        // `throw_error` (light-cljs-824; serial_tests.goose). So does a
        // direct eval (light-cljs-823): its call is marked, and the VM fails
        // it as `direct_eval`.
        for (source, operation) in [
            ("function f() { return new Date(); }", "construct"),
            ("function f() { const x=1; x=2; return x; }", "throw_error"),
            ("function f(s) { return eval(s); }", "call"),
        ] {
            let export = export_function(source).unwrap();
            assert!(export.operations.contains(operation), "{source}");
        }
    }
}
