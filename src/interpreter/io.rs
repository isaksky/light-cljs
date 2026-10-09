//! Explicit host capabilities. Handles contain no interpreter-owned edges.
use super::*;
use std::io::{self as stdio, BufRead, Write};

/// An owned, blocking input capability. Each successful call supplies a complete
/// line (including its terminator) or the final unterminated line. None is EOF.
/// Empty chunks are rejected. Callbacks must not retain interpreter handles;
/// store application data instead. Arbitrary host code cannot be preempted.
pub trait HostInput {
    fn read_line(&mut self) -> stdio::Result<Option<Vec<u16>>>;
}
impl<F: FnMut() -> stdio::Result<Option<Vec<u16>>>> HostInput for F {
    fn read_line(&mut self) -> stdio::Result<Option<Vec<u16>>> {
        self()
    }
}
/// Exact UTF-16 output. A write is attempted once; errors may follow partial host
/// effects and are never retried. Callbacks must not retain interpreter handles.
pub trait HostOutput {
    fn write(&mut self, text: &[u16]) -> stdio::Result<()>;
    fn flush(&mut self) -> stdio::Result<()> {
        Ok(())
    }
}
impl<F: FnMut(&[u16]) -> stdio::Result<()>> HostOutput for F {
    fn write(&mut self, text: &[u16]) -> stdio::Result<()> {
        self(text)
    }
}
/// Explicit UTF-8 adapter; construction never discovers ambient stdin.
pub struct Utf8Input<R>(pub R);
impl<R: BufRead> HostInput for Utf8Input<R> {
    fn read_line(&mut self) -> stdio::Result<Option<Vec<u16>>> {
        let mut line = Vec::new();
        loop {
            let bytes = self.0.fill_buf()?;
            if bytes.is_empty() {
                break;
            }
            let end = bytes.iter().position(|b| matches!(b, b'\r' | b'\n'));
            let count = end.map_or(bytes.len(), |i| i + 1);
            line.extend_from_slice(&bytes[..count]);
            self.0.consume(count);
            if end.is_some() {
                break;
            }
        }
        if line.is_empty() {
            return Ok(None);
        }
        let line = String::from_utf8(line)
            .map_err(|e| stdio::Error::new(stdio::ErrorKind::InvalidData, e))?;
        Ok(Some(line.encode_utf16().collect()))
    }
}
/// Explicit UTF-8 adapter. Unpaired surrogates fail instead of being replaced.
pub struct Utf8Output<W>(pub W);
impl<W: Write> HostOutput for Utf8Output<W> {
    fn write(&mut self, text: &[u16]) -> stdio::Result<()> {
        let text = String::from_utf16(text)
            .map_err(|e| stdio::Error::new(stdio::ErrorKind::InvalidData, e))?;
        self.0.write_all(text.as_bytes())
    }
    fn flush(&mut self) -> stdio::Result<()> {
        self.0.flush()
    }
}
/// A shareable capture buffer, containing only UTF-16 data, never runtime roots.
#[derive(Clone, Default)]
pub struct OutputBuffer(Rc<RefCell<Vec<u16>>>);
impl OutputBuffer {
    pub fn units(&self) -> Vec<u16> {
        self.0.borrow().clone()
    }
    pub fn take(&self) -> Vec<u16> {
        std::mem::take(&mut *self.0.borrow_mut())
    }
}
impl HostOutput for OutputBuffer {
    fn write(&mut self, text: &[u16]) -> stdio::Result<()> {
        self.0.borrow_mut().extend_from_slice(text);
        Ok(())
    }
}

pub(super) struct InputState {
    reader: Option<Box<dyn HostInput>>,
    text: Vec<u16>,
    position: usize,
    eof: bool,
    failed: bool,
    closed: bool,
    name: String,
    line_offset: usize,
}
impl InputState {
    fn fill(&mut self, c: &Context) -> Result<bool, NativeError> {
        if self.eof {
            return Ok(false);
        }
        c.checkpoint(PollPoint::Read)?;
        let result = self.reader.as_mut().unwrap().read_line();
        c.accounting.check()?;
        let line = result.map_err(|e| NativeError::Host(e.to_string()))?;
        let Some(line) = line else {
            self.eof = true;
            return Ok(false);
        };
        if line.is_empty() {
            return Err(NativeError::Host(
                "input callback returned an empty line without a terminator".into(),
            ));
        }
        c.charge_allocation(line.len() as u64)?;
        self.text.extend(line);
        Ok(true)
    }
}
struct StringInputGuard(Rc<RefCell<InputState>>);
impl Drop for StringInputGuard {
    fn drop(&mut self) {
        let mut state = self.0.borrow_mut();
        state.closed = true;
        state.text.clear();
        state.position = 0;
    }
}
fn ensure_open(state: &InputState) -> Result<(), NativeError> {
    if state.closed {
        Err(NativeError::Host("input stream is closed".into()))
    } else {
        Ok(())
    }
}
fn io_error(error: stdio::Error) -> NativeError {
    NativeError::Host(error.to_string())
}
fn current(c: &Context, name: &str) -> Result<RootedValue, NativeError> {
    let id = c.core_var(&Name::new(None, name)).expect("I/O dynamic Var");
    c.accounting.check()?;
    Ok(c.var_get(id).unwrap())
}
fn input(value: &RootedValue) -> Result<Rc<RefCell<InputState>>, NativeError> {
    match value.owner.store.borrow().heap_value(value.value()) {
        Some(HeapValue::Input(stream)) => Ok(stream.clone()),
        _ => Err(NativeError::Type {
            argument: 0,
            expected: "explicit input stream",
        }),
    }
}
fn output(value: &RootedValue) -> Result<Rc<RefCell<Box<dyn HostOutput>>>, NativeError> {
    match value.owner.store.borrow().heap_value(value.value()) {
        Some(HeapValue::Output(stream)) => Ok(stream.clone()),
        _ => Err(NativeError::Type {
            argument: 0,
            expected: "explicit output stream",
        }),
    }
}
fn units(value: &RootedValue, argument: usize) -> Result<Rc<[u16]>, NativeError> {
    match value.inspect() {
        ValueView::String(text) => Ok(text),
        _ => Err(NativeError::Type {
            argument,
            expected: "string",
        }),
    }
}
impl Context {
    /// Read one code form from an explicit stream, preserving the cursor shared
    /// with language `read`/`read-line`. Parser diagnostics drive multiline
    /// buffering; contextual lowering and data-reader callbacks run once.
    /// This entry owns one resource scope. EOF is `None`; a read error fuses the
    /// stream until the host explicitly calls `recover_input`.
    pub fn read_input_form(&mut self, stream: &RootedValue) -> Result<Option<Form>, EvalError> {
        let _scope = self.accounting.enter();
        let result = if Rc::ptr_eq(&self.owner, &stream.owner) {
            read_form(self, stream)
        } else {
            Err(NativeError::WrongOwner)
        };
        result.map_err(|error| {
            if let NativeError::Invocation(forwarded) = &error
                && let InvokeError::Execution(diagnostic) = forwarded.error()
            {
                return *diagnostic.clone();
            }
            let source = input(stream)
                .ok()
                .and_then(|stream| {
                    let state = stream.try_borrow().ok()?;
                    Some(self.source_at_line(
                        state.name.clone(),
                        String::from_utf16_lossy(&state.text),
                        state.line_offset,
                    ))
                })
                .unwrap_or_else(|| self.source("<input>", ""));
            let end = source.text().len();
            EvalError {
                phase: EvalPhase::Read,
                kind: match error {
                    NativeError::Invocation(forwarded) => match forwarded.into_error() {
                        InvokeError::Dispatch(kind) => kind,
                        InvokeError::Execution(_) => unreachable!("handled above"),
                    },
                    error => EvalErrorKind::Native {
                        name: Box::new(Name::new(None, "input")),
                        error,
                    },
                },
                span: Box::new(source.span(end..end).unwrap()),
                related: None,
                origin: Origin::Source,
                frames: Vec::new(),
                exception: None,
            }
        })
    }

    /// Resume after a failed stream read, discarding the rest of the buffered
    /// input (normally the offending line). Does not read ahead or rewind host
    /// input. Successful streams keep their cursor unchanged.
    pub fn recover_input(&self, stream: &RootedValue) -> Result<(), NativeError> {
        if !Rc::ptr_eq(&self.owner, &stream.owner) {
            return Err(NativeError::WrongOwner);
        }
        let stream = input(stream)?;
        let mut state = stream
            .try_borrow_mut()
            .map_err(|_| NativeError::Host("input stream is already in use".into()))?;
        ensure_open(&state)?;
        if state.failed {
            state.position = state.text.len();
            state.failed = false;
        }
        Ok(())
    }

    /// Release fully consumed lines, reusing buffer capacity. Retained forms and
    /// diagnostics keep their own source; subsequent sources preserve original
    /// line/byte-column locations with byte spans relative to the retained text.
    /// The partially consumed current line stays intact, including a trailing
    /// CR whose possible LF/form-feed partner has not arrived yet.
    pub fn compact_input(&self, stream: &RootedValue) -> Result<(), NativeError> {
        if !Rc::ptr_eq(&self.owner, &stream.owner) {
            return Err(NativeError::WrongOwner);
        }
        let stream = input(stream)?;
        let mut state = stream
            .try_borrow_mut()
            .map_err(|_| NativeError::Host("input stream is already in use".into()))?;
        ensure_open(&state)?;
        let (mut index, mut end, mut lines) = (0, 0, 0);
        while index < state.position {
            let unit = state.text[index];
            index += 1;
            if unit == 13 {
                if index == state.text.len() && !state.eof {
                    break;
                }
                if matches!(state.text.get(index), Some(10 | 12)) {
                    if index == state.position {
                        break;
                    }
                    index += 1;
                }
            }
            if matches!(unit, 10 | 13) {
                end = index;
                lines += 1;
            }
        }
        let line_offset = state
            .line_offset
            .checked_add(lines)
            .ok_or_else(|| NativeError::Host("input line number overflow".into()))?;
        state.text.drain(..end);
        state.position -= end;
        state.line_offset = line_offset;
        Ok(())
    }

    /// Own an explicit capability. Bind the result to *in* with bind_io or the
    /// ordinary dynamic binding API. Reset creates fresh, unconfigured Vars.
    pub fn input_stream(
        &self,
        name: impl Into<String>,
        reader: impl HostInput + 'static,
    ) -> RootedValue {
        self.new_input(InputState {
            reader: Some(Box::new(reader)),
            text: Vec::new(),
            position: 0,
            eof: false,
            failed: false,
            closed: false,
            name: name.into(),
            line_offset: 0,
        })
    }
    pub fn string_input(&self, name: impl Into<String>, text: impl Into<Vec<u16>>) -> RootedValue {
        let text = text.into();
        let _ = self.charge_allocation(text.len() as u64);
        self.new_input(InputState {
            reader: None,
            text,
            position: 0,
            eof: true,
            failed: false,
            closed: false,
            name: name.into(),
            line_offset: 0,
        })
    }
    fn new_input(&self, state: InputState) -> RootedValue {
        let _ = self.charge_allocation(state.name.len() as u64 + 1);
        let id = HeapId(
            self.owner
                .store
                .borrow_mut()
                .heap
                .insert(HeapValue::Input(Rc::new(RefCell::new(state)))),
        );
        self.owner.value(Value::Heap(id))
    }
    pub fn output_stream(&self, writer: impl HostOutput + 'static) -> RootedValue {
        let _ = self.charge_allocation(1);
        let id = HeapId(
            self.owner
                .store
                .borrow_mut()
                .heap
                .insert(HeapValue::Output(Rc::new(RefCell::new(Box::new(writer))))),
        );
        self.owner.value(Value::Heap(id))
    }
    /// Bind only the supplied channels. Guard drop restores them on success,
    /// failure, cancellation, or Rust unwinding; the streams remain host-owned.
    pub fn bind_io(
        &self,
        input: Option<&RootedValue>,
        out: Option<&RootedValue>,
        err: Option<&RootedValue>,
    ) -> Result<BindingGuard, VarError> {
        let bindings = [("*in*", input), ("*out*", out), ("*err*", err)]
            .into_iter()
            .filter_map(|(name, stream)| {
                stream.map(|stream| {
                    (
                        self.core_var(&Name::new(None, name)).unwrap(),
                        stream.clone(),
                    )
                })
            })
            .collect::<Vec<_>>();
        self.bind_vars(&bindings)
    }
}
fn write(c: &mut Context, text: &RootedValue) -> NativeResult {
    let stream = output(&current(c, "*out*")?)?;
    c.checkpoint(PollPoint::Host)?;
    let result = stream
        .try_borrow_mut()
        .map_err(|_| NativeError::Host("output stream is already in use".into()))?
        .write(&units(text, 0)?);
    c.accounting.check()?;
    result.map_err(io_error)?;
    Ok(c.nil())
}
fn flush(c: &mut Context, _: &[RootedValue]) -> NativeResult {
    let stream = output(&current(c, "*out*")?)?;
    c.checkpoint(PollPoint::Host)?;
    let result = stream
        .try_borrow_mut()
        .map_err(|_| NativeError::Host("output stream is already in use".into()))?
        .flush();
    c.accounting.check()?;
    result.map_err(io_error)?;
    Ok(c.nil())
}
fn print(c: &mut Context, args: &[RootedValue], readable: bool, newline: bool) -> NativeResult {
    let mut options = PrintOptions::dynamic(c)?;
    if !readable {
        options.readably = false;
    }
    // Render/budget before calling the sink, so a failed format cannot duplicate
    // host writes. Each successful print call forwards exactly one UTF-16 chunk.
    let text = printing::render(c, args, options, false, newline)?;
    if !args.is_empty() || newline {
        write(c, &text)?;
    }
    if newline && collection_core::truthy(&current(c, "*flush-on-newline*")?) {
        flush(c, &[])?;
    }
    Ok(c.nil())
}
fn read_line(c: &mut Context, _: &[RootedValue]) -> NativeResult {
    let stream = input(&current(c, "*in*")?)?;
    let mut state = stream
        .try_borrow_mut()
        .map_err(|_| NativeError::Host("input stream is already in use".into()))?;
    c.checkpoint(PollPoint::Read)?;
    ensure_open(&state)?;
    let start = state.position;
    loop {
        c.accounting
            .work((state.text.len() - state.position) as u64)?;
        if let Some(relative) = state.text[state.position..]
            .iter()
            .position(|u| *u == 10 || *u == 13)
        {
            let end = state.position + relative;
            state.position = end + 1;
            if state.text[end] == 13 {
                if state.position == state.text.len() {
                    state.fill(c)?;
                }
                if state.text.get(state.position) == Some(&10) {
                    state.position += 1;
                }
            }
            c.charge_allocation((end - start) as u64)?;
            return Ok(c.string(state.text[start..end].to_vec()));
        }
        state.position = state.text.len();
        if !state.fill(c)? {
            return Ok(if start == state.position {
                c.nil()
            } else {
                c.string(state.text[start..state.position].to_vec())
            });
        }
    }
}
fn incomplete(error: &crate::parser::ParseError) -> bool {
    matches!(
        error.kind,
        crate::parser::ErrorKind::UnexpectedEof(_)
            | crate::parser::ErrorKind::Lexical(
                crate::LexError::UnterminatedString | crate::LexError::UnterminatedRegex
            )
    )
}
fn read_form(c: &mut Context, value: &RootedValue) -> Result<Option<Form>, NativeError> {
    read_form_mode(c, value, None)
}
fn read_form_mode(
    c: &mut Context,
    value: &RootedValue,
    edn: Option<&Rc<EdnOptions>>,
) -> Result<Option<Form>, NativeError> {
    let stream = input(value)?;
    let mut state = stream
        .try_borrow_mut()
        .map_err(|_| NativeError::Host("input stream is already in use".into()))?;
    ensure_open(&state)?;
    if state.failed {
        return Ok(None);
    }
    let result = (|| {
        loop {
            c.checkpoint(PollPoint::Read)?;
            if state.position == state.text.len() && !state.fill(c)? {
                return Ok(None);
            }
            c.charge_allocation(state.text.len() as u64 * 3)?;
            c.accounting.work(state.text.len() as u64)?;
            let text = String::from_utf16(&state.text).map_err(|_| {
                NativeError::Host(
                    "reader source contains unpaired UTF-16; use an escaped source literal".into(),
                )
            })?;
            let offset = char::decode_utf16(state.text[..state.position].iter().copied())
                .map(|unit| unit.expect("source was validated").len_utf8())
                .sum();
            // Probe only the roots needed by this form; semantics and all
            // callbacks execute once after those bytes are available.
            let end = match c.buffered_form_end(&text, offset, edn.is_some()) {
                Ok((end, true)) if !state.eof => {
                    if state.fill(c)? {
                        continue;
                    }
                    end
                }
                Err(error) if incomplete(&error) && !state.eof => {
                    if state.fill(c)? {
                        continue;
                    }
                    text.len()
                }
                Ok((end, _)) => end,
                Err(_) => text.len(), // shared semantic reader owns diagnostics
            };
            let source = c.source_at_line(state.name.clone(), text, state.line_offset);
            let (result, position) = c.read_at(&source, offset, end, edn);
            state.position = source.text()[..position].encode_utf16().count();
            let form = result.map_err(|error| {
                let mut error = EvalError::from(error);
                // EDN is a runtime data operation. Its syntax failures and
                // handler exceptions are catchable; top-level code diagnostics
                // and direct host readers keep the ordinary read boundary.
                if edn.is_some() {
                    error.phase = EvalPhase::Execution;
                }
                NativeError::from(InvokeError::Execution(Box::new(error)))
            })?;
            if form.is_some() || (state.eof && state.position == state.text.len()) {
                return Ok(form);
            }
        }
    })();
    if result.is_err() {
        state.failed = true;
    }
    result
}
fn eof() -> NativeError {
    // EOF is an execution-time language error, distinct from malformed source.
    NativeError::EndOfInput
}
fn read_options(c: &Context, options: &RootedValue) -> Result<Option<RootedValue>, NativeError> {
    if !matches!(options.inspect(), ValueView::Map(_)) {
        return Err(NativeError::Type {
            argument: 0,
            expected: "reader options map",
        });
    }
    // Feature/tag policies belong to the embedding context. :eof is the sole
    // per-call override; unsupported keys fail instead of weakening that policy.
    for (key, _) in options.entries()? {
        if key.inspect() != ValueView::Keyword(Name::new(None, "eof")) {
            return Err(NativeError::Type {
                argument: 0,
                expected: "reader options containing only :eof",
            });
        }
    }
    Ok(options.get(&c.keyword(Name::new(None, "eof")))?)
}
fn read(c: &mut Context, args: &[RootedValue]) -> NativeResult {
    let (stream, default) = match args {
        [] => (current(c, "*in*")?, None),
        [stream] => (stream.clone(), None),
        [options, stream] => (stream.clone(), read_options(c, options)?),
        [stream, error, value] | [stream, error, value, _] => (
            stream.clone(),
            (!collection_core::truthy(error)).then(|| value.clone()),
        ),
        _ => unreachable!(),
    };
    read_form(c, &stream)?
        .map(|form| form.data())
        .or(default)
        .ok_or_else(eof)
}
fn read_string(c: &mut Context, args: &[RootedValue]) -> NativeResult {
    let (text, default) = if args.len() == 1 {
        (&args[0], None)
    } else {
        (&args[1], read_options(c, &args[0])?)
    };
    let text = units(text, args.len() - 1)?;
    c.charge_allocation(text.len() as u64)?;
    let stream = c.string_input("<read-string>", text.to_vec());
    read_form(c, &stream)?
        .map(|form| form.data())
        .or(default)
        .ok_or_else(eof)
}
fn with_output(c: &mut Context, args: &[RootedValue]) -> NativeResult {
    let capture = OutputBuffer::default();
    let stream = c.output_stream(capture.clone());
    let _guard = c.bind_io(None, Some(&stream), None)?;
    c.invoke(&args[0], &[])?;
    Ok(c.string(capture.take()))
}
fn with_input(c: &mut Context, args: &[RootedValue]) -> NativeResult {
    let text = units(&args[0], 0)?;
    c.charge_allocation(text.len() as u64)?;
    let stream = c.string_input("<with-in-str>", text.to_vec());
    let _closed = StringInputGuard(input(&stream)?);
    let _guard = c.bind_io(Some(&stream), None, None)?;
    c.invoke(&args[1], &[]).map_err(Into::into)
}
pub(super) fn with_out_macro(c: &mut Context, args: &[RootedValue]) -> NativeResult {
    let mut body = vec![c.vector(&[])?];
    body.extend_from_slice(&args[2..]);
    let body = binding_macros::call(c, "fn*", &body)?;
    binding_macros::call(c, "with-out-str*", &[body])
}
pub(super) fn with_in_macro(c: &mut Context, args: &[RootedValue]) -> NativeResult {
    let mut body = vec![c.vector(&[])?];
    body.extend_from_slice(&args[3..]);
    let body = binding_macros::call(c, "fn*", &body)?;
    binding_macros::call(c, "with-in-str*", &[args[2].clone(), body])
}
pub fn io_definitions() -> &'static [NativeDefinition] {
    macro_rules! def {
        ($name:literal, $arity:expr, $body:expr) => {
            NativeDefinition {
                metadata: NativeMetadata {
                    namespace: "clojure.core",
                    name: $name,
                    arity: $arity,
                    effects: NativeEffects::Host,
                    errors: &[
                        NativeErrorClass::Reader,
                        NativeErrorClass::Type,
                        NativeErrorClass::Host,
                        NativeErrorClass::Invocation,
                    ],
                },
                callback: $body,
            }
        };
    }
    static DEFINITIONS: &[NativeDefinition] = &[
        def!("print", Arity::AtLeast(0), |c, a| print(c, a, false, false)),
        def!("println", Arity::AtLeast(0), |c, a| print(
            c, a, false, true
        )),
        def!("pr", Arity::AtLeast(0), |c, a| print(c, a, true, false)),
        def!("prn", Arity::AtLeast(0), |c, a| print(c, a, true, true)),
        def!("newline", Arity::Exact(0), |c, _| {
            let text = printing::render(c, &[], PrintOptions::default(), false, true)?;
            write(c, &text)
        }),
        def!("flush", Arity::Exact(0), flush),
        def!("read-line", Arity::Exact(0), read_line),
        def!("read", Arity::Range { min: 0, max: 4 }, read),
        def!("read-string", Arity::Range { min: 1, max: 2 }, read_string),
        def!("with-out-str*", Arity::Exact(1), with_output),
        def!("with-in-str*", Arity::Exact(2), with_input),
        def!("with-out-str", Arity::AtLeast(2), with_out_macro),
        def!("with-in-str", Arity::AtLeast(3), with_in_macro),
    ];
    DEFINITIONS
}

fn edn_options(
    c: &Context,
    value: &RootedValue,
) -> Result<(EdnOptions, Option<RootedValue>), NativeError> {
    let mut options = EdnOptions::default();
    let mut eof = None;
    if !matches!(value.inspect(), ValueView::Map(_)) {
        return Err(NativeError::Type {
            argument: 0,
            expected: "EDN options map",
        });
    }
    for (key, value) in value.entries()? {
        c.checkpoint(PollPoint::Read)?;
        if key.inspect() == ValueView::Keyword(Name::new(None, "eof")) {
            eof = Some(value);
        } else if key.inspect() == ValueView::Keyword(Name::new(None, "default")) {
            if !matches!(value.inspect(), ValueView::Nil) {
                options.default = Some(value);
            }
        } else if key.inspect() == ValueView::Keyword(Name::new(None, "readers")) {
            if matches!(value.inspect(), ValueView::Nil) {
                continue;
            }
            if !matches!(value.inspect(), ValueView::Map(_)) {
                return Err(NativeError::Type {
                    argument: 0,
                    expected: "EDN :readers map",
                });
            }
            for (tag, reader) in value.entries()? {
                let ValueView::Symbol(tag) = tag.inspect() else {
                    return Err(NativeError::Type {
                        argument: 0,
                        expected: "symbol keys in EDN :readers",
                    });
                };
                c.charge_allocation(1)?;
                options.readers.insert(tag, reader);
            }
        } else {
            return Err(NativeError::Type {
                argument: 0,
                expected: "EDN options :eof, :readers, or :default",
            });
        }
    }
    Ok((options, eof))
}
fn edn_read(c: &mut Context, args: &[RootedValue]) -> NativeResult {
    let (stream, options, eof) = match args {
        [] => (current(c, "*in*")?, EdnOptions::default(), None),
        [stream] => (stream.clone(), EdnOptions::default(), None),
        [options, stream] => {
            let (options, eof) = edn_options(c, options)?;
            (stream.clone(), options, eof)
        }
        _ => unreachable!(),
    };
    let options = Rc::new(options);
    read_form_mode(c, &stream, Some(&options))?
        .map(|form| form.data())
        .or(eof)
        .ok_or_else(self::eof)
}
fn edn_read_string(c: &mut Context, args: &[RootedValue]) -> NativeResult {
    if !collection_core::truthy(&args[args.len() - 1]) {
        return Ok(c.nil());
    }
    let (text, options, eof) = if args.len() == 1 {
        (&args[0], EdnOptions::default(), Some(c.nil()))
    } else {
        let (options, eof) = edn_options(c, &args[0])?;
        (&args[1], options, eof)
    };
    let text = units(text, args.len() - 1)?;
    c.charge_allocation(text.len() as u64)?;
    let stream = c.string_input("<edn-read-string>", text.to_vec());
    let options = Rc::new(options);
    read_form_mode(c, &stream, Some(&options))?
        .map(|form| form.data())
        .or(eof)
        .ok_or_else(self::eof)
}
pub fn edn_definitions() -> &'static [NativeDefinition] {
    macro_rules! def {
        ($name:literal, $arity:expr, $effects:expr, $body:expr) => {
            NativeDefinition {
                metadata: NativeMetadata {
                    namespace: "clojure.edn",
                    name: $name,
                    arity: $arity,
                    effects: $effects,
                    errors: &[
                        NativeErrorClass::Reader,
                        NativeErrorClass::Type,
                        NativeErrorClass::Host,
                        NativeErrorClass::Invocation,
                    ],
                },
                callback: $body,
            }
        };
    }
    static DEFINITIONS: &[NativeDefinition] = &[
        def!(
            "read",
            Arity::Range { min: 0, max: 2 },
            NativeEffects::Host,
            edn_read
        ),
        def!(
            "read-string",
            Arity::Range { min: 1, max: 2 },
            NativeEffects::Context,
            edn_read_string
        ),
    ];
    DEFINITIONS
}
