//! Bounded source export for the Goose experiment. This module never runs code.
//! The engine frontend supplies parsing, binding resolution and lowering.
use std::collections::BTreeSet;

use crate::frontend::{Span, SyntaxError, SyntaxErrorKind, compile_script};
use crate::value::Primitive;
use crate::vm::Executable;
use crate::vm::instr::{Opcode, OperandKind};

pub const VERSION: u32 = 2;
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

/// Deliberate export surface. Even dead or nested unsupported operations fail.
/// Export support does not assert that the Goose VM executes that operation.
fn supported(op: Opcode) -> bool {
    use Opcode::*;
    matches!(
        op,
        Const
            | Undefined
            | GetLocal
            | SetLocal
            | Pop
            | Dup
            | Add
            | Sub
            | Mul
            | Div
            | Rem
            | Neg
            | ToNumber
            | ToNumberPair
            | Lt
            | Gt
            | Le
            | Ge
            | StrictEq
            | LooseEq
            | Not
            | Jump
            | JumpIfFalse
            | Return
            | PushCatch
            | PopCatch
            | Throw
            | NewObject
            | NewObjectLiteral
            | Function
            | Call
            | CallMethod
            | CurrentFunction
            | GetCapture
            | NewCell
            | NewFixedCell
            | NewSealableCell
            | CellInit
            | CellGet
            | CellSet
            | Plus
            | CompareLt
            | CompareGt
            | CompareLe
            | CompareGe
            | ToNumeric
            | Inc
            | Dec
            | GetField
            | PutField
            | PutFieldStrict
            | DefineField
            | GetElem
            | PutElem
            | PutElemStrict
            | NewArray
            | NewArguments
            | Rest
            | AppendElement
            | GetThis
            | GetRawThis
            | ResolveThis
            | GetGlobalObject
            | GetGlobal
            | GetGlobalOrUndefined
            | PutGlobal
            | PutGlobalStrict
            | DeclareGlobalVar
            | DeclareGlobalFunction
            | CheckGlobalVar
            | CheckGlobalFunction
            | CheckGlobalLexicalConflict
            | DeclareGlobalLet
            | DeclareGlobalConst
            | InitGlobalLexical
            | DeclareGlobalConstValue
            | NewFormList
            | NewFormVector
            | NewFormMap
            | NewFormSet
            | FormOp
            | NewVector
            | NewMap
            | Count
            | Nth
            | Get
            | GetOr
            | Assoc
            | Dissoc
            | Conj
            | PopLast
            | Typeof
            | ToString
    )
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
        if !spec.eval_sites.is_empty() {
            return Err(problem(
                source,
                SyntaxErrorKind::Unsupported,
                Span::new(0, source.len()),
                "Goose export does not support direct eval",
            ));
        }
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
                Primitive::Undefined => [0, 0, 0],
                Primitive::Null => [1, 0, 0],
                Primitive::Bool(b) => [2, u32::from(*b), 0],
                Primitive::Number(n) => {
                    let b = n.to_bits();
                    [3, b as u32, (b >> 32) as u32]
                }
                Primitive::String(s) => {
                    if let Ok(text) = String::from_utf16(s.code_units()) {
                        self.string_constants.insert(text);
                    }
                    let (start, len) = self.string(s.code_units(), source)?;
                    [4, start, len]
                }
                Primitive::Symbol(_) | Primitive::BigInt(_) => {
                    return Err(problem(
                        source,
                        SyntaxErrorKind::Unsupported,
                        Span::new(0, source.len()),
                        "Goose export does not support Symbol/BigInt constants",
                    ));
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
            if !supported(opcode) {
                return Err(problem(
                    source,
                    SyntaxErrorKind::Unsupported,
                    span,
                    &format!(
                        "Goose export unsupported operation {} in function {}",
                        opcode.name(),
                        spec.name
                    ),
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
            let tag = match kind {
                OperandKind::None => 0,
                OperandKind::Const | OperandKind::Name => 1,
                OperandKind::Local => 2,
                OperandKind::Target => 3,
                OperandKind::Function => 4,
                OperandKind::Capture => 5,
                _ => 6,
            };
            bounded(self.instructions.len() + 1, MAX_RECORDS, source)?;
            self.instructions.push([
                u32::from(opcode as u8) | (tag << 8) | ((size as u32) << 16),
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
        let name = spec
            .function_name
            .as_ref()
            .map_or(&[][..], |s| s.code_units());
        let (name_start, name_len) = self.string(name, source)?;
        let constructor = match spec.constructor_kind {
            crate::ConstructorKind::None => 0,
            crate::ConstructorKind::Ordinary => 1,
            _ => {
                return Err(problem(
                    source,
                    SyntaxErrorKind::Unsupported,
                    Span::new(0, source.len()),
                    "Goose export supports only ordinary function constructors",
                ));
            }
        };
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
            u32::from(spec.strict) | (constructor << 8),
        ];
        Ok(id)
    }
}

/// Parse and lower every function, preserving exact source bytes once. This
/// accepts no values or host callbacks and cannot compute an expansion.
pub fn export_source(name: &str, source: &str) -> Result<Export, SyntaxError> {
    bounded(source.len(), MAX_SOURCE, source)?;
    let executable = compile_script(name, source)?;
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
    let mut text = String::from(
        "// Generated by regenerate.bb from the Rust exporter; do not edit.\nfn export_descriptor(op: i64) -> i64 {\n    return match op {\n",
    );
    for byte in 0..=255 {
        if let Some(op) = Opcode::from_byte(byte)
            && supported(op)
        {
            let kind = op.operand();
            let tag = match kind {
                OperandKind::None => 0,
                OperandKind::Const | OperandKind::Name => 1,
                OperandKind::Local => 2,
                OperandKind::Target => 3,
                OperandKind::Function => 4,
                OperandKind::Capture => 5,
                _ => 6,
            };
            let descriptor = u32::from(byte) | (tag << 8) | ((kind.size() as u32) << 16);
            text.push_str(&format!("        {byte} => {descriptor},\n"));
        }
    }
    text.push_str("        _ => -1,\n    };\n}\n\nfn export_form_arity(op: i64) -> i64 {\n    return match op {\n");
    for op in crate::vm::instr::FormOp::ALL {
        text.push_str(&format!("        {} => {},\n", op as u16, op.arity()));
    }
    text.push_str("        _ => -1,\n    };\n}\n\nfn export_name_operand(op: i64) -> bool {\n    return match op {\n");
    for byte in 0..=255 {
        if let Some(op) = Opcode::from_byte(byte)
            && supported(op)
            && op.operand() == OperandKind::Name
        {
            text.push_str(&format!("        {byte} => true,\n"));
        }
    }
    text.push_str("        _ => false,\n    };\n}\n");
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
    #[test]
    fn parsed_but_unsupported_dead_nested_function_is_rejected() {
        let source = "function dormant() { return new Date(); } 1;";
        assert!(crate::frontend::parse_script(source).is_ok());
        let e = export_source("dead.js", source).unwrap_err();
        assert_eq!(e.kind, SyntaxErrorKind::Unsupported);
        assert!(
            e.message.contains("construct") && e.message.contains("dormant"),
            "{e}"
        );
        assert!(source[e.span.start..e.span.end].contains("new"));
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
        let root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../corpus/serial");
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
            ("unsupported-prototype", "string:[object Object]"),
            ("loop", "limit"),
        ] {
            let source = std::fs::read_to_string(root.join(format!("{name}.js"))).unwrap();
            let export = export_function(&source).unwrap_or_else(|error| panic!("{name}: {error}"));
            let path = root.join(format!("{name}.gsp"));
            if regenerate {
                std::fs::write(path, &export.bytes).unwrap();
            } else {
                assert_eq!(std::fs::read(path).unwrap(), export.bytes, "{name}");
            }
            if name == "loop" {
                continue;
            }
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
            let rt = Runtime::new();
            let executable = crate::frontend::compile_function(&source).unwrap();
            let actual = match rt.spawn(&executable, &[]).unwrap().run().unwrap() {
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
            };
            assert_eq!(actual, expected, "{name}");
        }
        // Constructors and direct const writes are outside the frozen export
        // surface: explicit positioned errors rather than Goose semantics.
        for source in [
            "function f() { return new Date(); }",
            "function f() { const x=1; x=2; return x; }",
        ] {
            assert_eq!(
                export_function(source).unwrap_err().kind,
                SyntaxErrorKind::Unsupported
            );
        }
    }
}
