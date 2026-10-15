//! The compiler's side of the connected REPL (docs/dev-workflow.md, "The
//! connected REPL"): `POST /repl` of the dev server ([`crate::dev_server`])
//! hands each form to the thread that owns the watch's [`DevSession`] as a
//! [`ReplRequest`], which [`ReplRequest::compile`] answers there with what
//! the server sends a runtime. The compiler never waits on a runtime: the
//! server's connection thread sends the unit and waits for the reply.
//!
//! Also here: the REPL commands the server answers itself,
//! `(light.dev/runtimes)` and `(light.dev/select! ID)`, and the mapping of a
//! runtime error's stack back to the sources.

use crate::CollectionKind;
use crate::compiler_data::{DatumKind, Form, Name};
use crate::dev_server::{Diagnostic, Json};
use crate::dev_session::{Action, DevSession, script};
use crate::source_map::SourceMapping;
use crate::source_provider::SourceInput;
use crate::{EvalDiagnostic, LineIndex};
use std::path::{Component, Path, PathBuf};
use std::sync::mpsc::Sender;

/// The functions of the `light.dev` namespace, which the dev client defines
/// in every runtime: `ref` returns the value of a reference number,
/// `inspect` describes it, and `runtimes` and `select!` are the server's
/// commands, which throw when called anywhere but as a form of their own.
pub const LIGHT_DEV: [&str; 4] = ["ref", "inspect", "runtimes", "select!"];

/// Declare the functions of `light.dev` in `session`, so that REPL forms
/// that call them compile without a warning.
pub fn declare(session: &DevSession) -> Result<(), EvalDiagnostic> {
    for name in LIGHT_DEV {
        session.declare("light.dev", name)?;
    }
    Ok(())
}

/// A request of the dev server to the compiler: read the top-level form of
/// `text`, named `file`, at `cursor` in `namespace`, and compile it.
pub struct ReplRequest {
    pub(crate) namespace: String,
    pub(crate) file: String,
    pub(crate) text: String,
    pub(crate) cursor: usize,
    pub(crate) respond: Sender<ReplForm>,
}

impl ReplRequest {
    /// Compile the form with `session`, on the thread that owns it, and
    /// hand the result to the server.
    pub fn compile(self, session: &DevSession) {
        let form = compile(session, &self);
        let _ = self.respond.send(form);
    }
}

/// What the compiler made of a request.
pub(crate) enum ReplForm {
    /// The text has no form at the cursor.
    End,
    Form(Box<CompiledForm>),
}

pub(crate) struct CompiledForm {
    /// The namespace current after the form, the session's from now on.
    pub(crate) namespace: String,
    /// Where the next form begins.
    pub(crate) cursor: usize,
    /// Where the form begins: its byte in the text, and its line and
    /// column, one-based.
    pub(crate) start: usize,
    pub(crate) line: usize,
    pub(crate) column: usize,
    pub(crate) warnings: Vec<Diagnostic>,
    pub(crate) outcome: Outcome,
}

pub(crate) enum Outcome {
    /// The form did not read or compile, and its unit does not run;
    /// `loads` is the script of what it loaded before the error, which the
    /// session counts as loaded and every runtime runs.
    Error { error: Diagnostic, loads: String },
    /// A command the server answers.
    Command(Command),
    /// Run `setup` (a script of what the form loaded and of the units before
    /// its last), then `unit` for the form's value, nil when there is none;
    /// `loads` is the script of what it loaded alone, for the other
    /// runtimes, empty when it loaded nothing.
    Run {
        setup: String,
        loads: String,
        unit: Option<ReplUnit>,
    },
}

/// A REPL form's unit as the server sends it.
pub(crate) struct ReplUnit {
    /// The namespace it runs in.
    pub(crate) namespace: String,
    /// Its JavaScript, without the `sourceURL` comment the server adds.
    pub(crate) js: String,
    pub(crate) mapping: Option<SourceMapping>,
}

/// A REPL command of the server.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Command {
    /// `(light.dev/runtimes)`: the connected runtimes.
    Runtimes,
    /// `(light.dev/select! ID)`: send the session's forms to runtime `ID`,
    /// or with nil to the most recently connected one.
    Select(Option<u64>),
}

fn compile(session: &DevSession, request: &ReplRequest) -> ReplForm {
    session.set_namespace(&request.namespace);
    let source = SourceInput::new(&request.file, &request.text);
    let mut cursor = request.cursor;
    let at = |span: &crate::compiler_data::SourceSpan| {
        let (line, column) = span
            .source()
            .line_column(span.bytes().start)
            .unwrap_or((1, 1));
        (span.bytes().start, line, column)
    };
    let compiled = |cursor, (start, line, column), warnings, outcome| {
        ReplForm::Form(Box::new(CompiledForm {
            namespace: session.namespace(),
            cursor,
            start,
            line,
            column,
            warnings,
            outcome,
        }))
    };
    let form = match session.read_form(&source, &mut cursor) {
        Ok(None) => return ReplForm::End,
        Ok(Some(form)) => form,
        // A form that does not read discards the rest of the text.
        Err(error) => {
            let location = error.location.as_ref().map_or((0, 1, 1), at);
            let outcome = Outcome::Error {
                error: Diagnostic::located(&error),
                loads: String::new(),
            };
            return compiled(request.text.len(), location, Vec::new(), outcome);
        }
    };
    let location = at(&form.span());
    match command(&form) {
        Some(Ok(command)) => {
            return compiled(cursor, location, Vec::new(), Outcome::Command(command));
        }
        Some(Err(message)) => {
            let error = EvalDiagnostic {
                message,
                location: Some(form.span()),
                data: None,
            };
            let outcome = Outcome::Error {
                error: Diagnostic::located(&error),
                loads: String::new(),
            };
            return compiled(cursor, location, Vec::new(), outcome);
        }
        None => {}
    }
    let actions = session.form(&form);
    let mut warnings = Vec::new();
    let mut error = None;
    let last = actions
        .iter()
        .rposition(|action| matches!(action, Action::Invoke(_)));
    let (mut setup, mut loads, mut unit) = (Vec::new(), Vec::new(), None);
    for (index, action) in actions.into_iter().enumerate() {
        match action {
            Action::Warnings(list) => warnings.extend(list.iter().map(Diagnostic::located)),
            Action::Error(diagnostic) => error = Some(Diagnostic::located(&diagnostic)),
            Action::Invoke(invoked) if Some(index) == last => unit = Some(invoked),
            Action::Invoke(_) => setup.push(action),
            Action::Load(_) | Action::Namespace(_) => {
                loads.push(action.clone());
                setup.push(action);
            }
        }
    }
    // What the form loaded, which the session now counts as loaded, goes
    // to the runtimes also when a later part of the form failed.
    let loads = if loads.iter().any(|action| matches!(action, Action::Load(_))) {
        script(&loads)
    } else {
        String::new()
    };
    let outcome = match error {
        Some(error) => Outcome::Error { error, loads },
        None => Outcome::Run {
            setup: script(&setup),
            loads,
            unit: unit.map(|unit| {
                // The server names the unit's text for its error stacks.
                let url = format!(
                    "\n//# sourceURL={}\n",
                    crate::dev_build::unit_file(&unit.namespace)
                );
                ReplUnit {
                    js: unit.js.strip_suffix(&url).unwrap_or(&unit.js).to_owned(),
                    namespace: unit.namespace,
                    mapping: unit.mapping,
                }
            }),
        },
    };
    compiled(cursor, location, warnings, outcome)
}

/// The command `form` is, if it is a list whose operator is
/// `light.dev/runtimes` or `light.dev/select!`, or why it is not a valid one.
fn command(form: &Form) -> Option<Result<Command, String>> {
    if !matches!(
        form.data().kind(),
        DatumKind::Collection {
            kind: CollectionKind::List,
            ..
        }
    ) {
        return None;
    }
    let children = form.children();
    let DatumKind::Symbol(name) = children.first()?.data().kind().clone() else {
        return None;
    };
    if name == Name::new(Some("light.dev"), "runtimes") {
        return Some(if children.len() == 1 {
            Ok(Command::Runtimes)
        } else {
            Err("light.dev/runtimes takes no arguments".into())
        });
    }
    if name == Name::new(Some("light.dev"), "select!") {
        let id = match children.get(1).map(|child| child.data().kind().clone()) {
            Some(DatumKind::Nil) if children.len() == 2 => Some(None),
            Some(DatumKind::Integer(id)) if children.len() == 2 && id > 0 => Some(Some(id as u64)),
            Some(DatumKind::Double(id))
                if children.len() == 2 && id >= 1.0 && id.fract() == 0.0 && id < 9e15 =>
            {
                Some(Some(id as u64))
            }
            _ => None,
        };
        return Some(id.map(Command::Select).ok_or_else(|| {
            "light.dev/select! takes a runtime ID, as (light.dev/runtimes) lists them, or nil"
                .to_owned()
        }));
    }
    None
}

/// A REPL unit the server sent, kept to map the stacks of errors thrown
/// through it, also by a function it defined that a later form calls.
pub(crate) struct SentUnit {
    /// The `sourceURL` it ran under.
    pub(crate) url: String,
    pub(crate) mapping: SourceMapping,
    /// The source the form was read from, and its text.
    pub(crate) file: String,
    pub(crate) text: String,
    /// Where the form begins in the text: the unit's own code, which maps
    /// to no form, maps there.
    pub(crate) start: usize,
}

/// `stack`, a runtime error's stack, with each location of a REPL unit in
/// `units` or of a unit of the dev build in `out_dir` replaced by the source
/// location it maps to: `FILE:LINE:COLUMN`, a REPL form's file as the REPL
/// named it and a build source's path. Other locations stay as they are.
/// The frames after the last of a REPL unit, those of the dev client that
/// ran it, are left out.
pub(crate) fn map_stack(stack: &str, units: &[SentUnit], out_dir: &Path) -> String {
    let mut lines = Vec::new();
    let mut last_repl = None;
    for (index, line) in stack.lines().enumerate() {
        let (mapped, repl) = map_line(line, units, out_dir);
        if repl {
            last_repl = Some(index);
        }
        lines.push(mapped);
    }
    if let Some(last) = last_repl {
        lines.truncate(last + 1);
    }
    lines.join("\n")
}

/// `line` of a stack with its locations mapped, and whether one was a REPL
/// unit's.
fn map_line(line: &str, units: &[SentUnit], out_dir: &Path) -> (String, bool) {
    let mut out = String::with_capacity(line.len());
    let mut copied = 0;
    let mut repl = false;
    for (at, _) in line.match_indices(".js:") {
        if at < copied {
            continue;
        }
        let before = &line[..at];
        let start = before
            .rfind([' ', '(', '@', '\t'])
            .map_or(0, |index| index + 1);
        let name_start = before[start..]
            .rfind('/')
            .map_or(start, |index| start + index + 1);
        let name = &line[name_start..at + 3];
        let rest = &line[at + 4..];
        let line_digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        let Some(after_line) = rest[line_digits..].strip_prefix(':') else {
            continue;
        };
        let column_digits = after_line.bytes().take_while(u8::is_ascii_digit).count();
        let (Ok(row), Ok(column)) = (
            rest[..line_digits].parse::<u32>(),
            after_line[..column_digits].parse::<u32>(),
        ) else {
            continue;
        };
        if row == 0 || column == 0 {
            continue;
        }
        let end = at + 4 + line_digits + 1 + column_digits;
        let mapped = match units.iter().rev().find(|unit| unit.url == name) {
            Some(unit) => {
                repl = true;
                map_unit(unit, row - 1, column - 1)
            }
            None => map_build_file(out_dir, name, row - 1, column - 1),
        };
        if let Some(mapped) = mapped {
            out.push_str(&line[copied..start]);
            out.push_str(&mapped);
            copied = end;
        }
    }
    out.push_str(&line[copied..]);
    (out, repl)
}

/// The source location of zero-based `line` and `column` of a REPL unit.
fn map_unit(unit: &SentUnit, line: u32, column: u32) -> Option<String> {
    let byte = match unit.mapping.original(line, column) {
        Some((0, byte)) => byte.max(unit.start),
        _ => unit.start,
    };
    let (line, column) = LineIndex::new(&unit.text).line_column(byte)?;
    Some(format!("{}:{line}:{column}", unit.file))
}

/// The source location of zero-based `line` and `column` of the build file
/// `name` in `out_dir`, by its source map `NAME.map`.
fn map_build_file(out_dir: &Path, name: &str, line: u32, column: u32) -> Option<String> {
    if name.starts_with('.') || name.contains(['/', '\\', ':']) {
        return None;
    }
    let text = std::fs::read_to_string(out_dir.join(format!("{name}.map"))).ok()?;
    let map = Json::parse(&text).ok()?;
    let mappings = map.get("mappings")?.as_str()?;
    let (source, original_line, original_column) = lookup(mappings, line, column)?;
    let Json::Array(sources) = map.get("sources")? else {
        return None;
    };
    let url = sources.get(source)?.as_str()?;
    let path = normalize(&out_dir.join(url));
    Some(format!(
        "{}:{}:{}",
        path.display(),
        original_line + 1,
        original_column + 1
    ))
}

/// `path` with its `.` and `..` components resolved lexically.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// The source index, line and column (zero-based) that Source Map v3
/// `mappings` give the generated `line` and `column`: those of the last
/// segment of the line at or before the column, None when it maps to no
/// source.
fn lookup(mappings: &str, line: u32, column: u32) -> Option<(usize, u32, u32)> {
    let (mut source, mut original_line, mut original_column) = (0i64, 0i64, 0i64);
    for (index, text) in mappings.split(';').enumerate() {
        let mut generated = 0i64;
        let mut found = None;
        for segment in text.split(',').filter(|segment| !segment.is_empty()) {
            let fields = vlq(segment)?;
            generated += fields[0];
            let mapped = fields.len() >= 4;
            if mapped {
                source += fields[1];
                original_line += fields[2];
                original_column += fields[3];
            }
            if index == line as usize && generated <= i64::from(column) {
                found = mapped.then_some((source, original_line, original_column));
            }
        }
        if index == line as usize {
            let (source, line, column) = found?;
            return Some((
                usize::try_from(source).ok()?,
                u32::try_from(line).ok()?,
                u32::try_from(column).ok()?,
            ));
        }
    }
    None
}

/// The fields of one Base64 VLQ segment.
fn vlq(segment: &str) -> Option<Vec<i64>> {
    let mut fields = Vec::new();
    let (mut value, mut shift) = (0i64, 0u32);
    for byte in segment.bytes() {
        let digit = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        };
        if shift > 60 {
            return None;
        }
        value += i64::from(digit & 31) << shift;
        if digit & 32 == 0 {
            fields.push(if value & 1 == 1 {
                -(value >> 1)
            } else {
                value >> 1
            });
            (value, shift) = (0, 0);
        } else {
            shift += 5;
        }
    }
    (shift == 0 && !fields.is_empty()).then_some(fields)
}

/// Append `text` as an EDN string.
pub(crate) fn edn_string(out: &mut String, text: &str) {
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch => out.push(ch),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vlq_segments_decode_and_look_up() {
        assert_eq!(vlq("AAAA"), Some(vec![0, 0, 0, 0]));
        assert_eq!(vlq("SAAQ"), Some(vec![9, 0, 0, 8]));
        assert_eq!(vlq("D"), Some(vec![-1]));
        assert_eq!(vlq("gB"), Some(vec![16]));
        assert_eq!(vlq("g"), None);
        // Line 0: column 0 maps to 0:0:0, column 4 to 0:1:2; line 1 (the
        // fields carry over) column 2 to 0:1:6.
        let mappings = "AAAA,IACE;EAAI";
        assert_eq!(lookup(mappings, 0, 0), Some((0, 0, 0)));
        assert_eq!(lookup(mappings, 0, 3), Some((0, 0, 0)));
        assert_eq!(lookup(mappings, 0, 9), Some((0, 1, 2)));
        assert_eq!(lookup(mappings, 1, 1), None);
        assert_eq!(lookup(mappings, 1, 5), Some((0, 1, 6)));
        assert_eq!(lookup(mappings, 2, 0), None);
    }

    #[test]
    fn stacks_keep_what_they_cannot_map() {
        let stack = "Error: boom\n    at f (http://127.0.0.1:1/runtime.js:3:4)\n    at x.js:1";
        assert_eq!(map_stack(stack, &[], Path::new("/nonexistent")), stack);
        assert_eq!(
            normalize(Path::new("/a/dev/../src/./b.cljs")),
            PathBuf::from("/a/src/b.cljs")
        );
        let mut edn = String::new();
        edn_string(&mut edn, "a \"b\" \\ c\n");
        assert_eq!(edn, r#""a \"b\" \\ c\n""#);
    }
}
