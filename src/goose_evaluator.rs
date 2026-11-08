//! Explicitly gated source-macro execution in the native Goose prototype.
//!
//! The compiler still owns reading, declaration resolution and helper emission.
//! This adapter never opens ScriptHost or installs the canonical JS runtime.
use crate::compiler_data::{Datum, ExpansionOrigin, Form};
use crate::evaluator::{EvalError, EvaluatorOptions, ExecutionControl, MacroEvaluator, ValueId};
use crate::form_bridge::ConversionLimits;
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    rc::Rc,
    sync::{
        Arc, Mutex, TryLockError,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};
#[path = "goose/compiler_bridge.rs"]
mod bridge;
static NEXT: AtomicU64 = AtomicU64::new(1);
/// How long a closing runner may go without a reply before close() kills it.
const CLOSE_QUIET: Duration = Duration::from_secs(5);
/// How long close() waits in all for a closing runner to exit.
const CLOSE_LIMIT: Duration = Duration::from_secs(60);
fn error(e: impl std::fmt::Display) -> EvalError {
    EvalError(format!("Goose: {e}"))
}
/// A runner's failure line. An uncaught exception's line ends with
/// ` uncaught=` and the thrown value's description (light-cljs-874), which
/// leads the error, where the V8 build puts the thrown value
/// (`macro app.macros/m: 42; definition …`); the Goose fault details
/// (function, pc, emitted span) follow it.
/// A reply line without its line ending only: a failure's description may
/// end in spaces that belong to the thrown value.
fn protocol_line(line: &str) -> String {
    line.trim_start().trim_end_matches(['\n', '\r']).to_owned()
}
fn failure(line: &str) -> EvalError {
    match line.split_once(" uncaught=") {
        Some((fault, description)) => EvalError(format!("{description}; Goose: {fault}")),
        None => error(line),
    }
}
#[derive(Clone, Debug)]
enum Event {
    Ready,
    Start,
    Produced,
    Resumed,
    /// A finished job: the next number of its gensym scope's count if
    /// the compiler gave it one, and its result graph.
    Done(Option<u64>, Vec<u8>),
    Need(u64, String),
    Failure(String),
}
/// One initialized subprocess per compilation; clones share only transport.
/// Each outstanding call has its own correlated mailbox. The input lock is
/// held for one command and its payload, never while waiting or invoking the
/// compiler, and the reader thread never takes it, so a writer blocked on a
/// full pipe never stops the replies that drain it. Calls, answers and
/// results travel in the pipes (light-cljs-816); the directory holds only
/// the base image and the definition exports. Every file either side
/// creates there is counted (light-cljs-822): see [`GooseSession::files`].
pub struct GooseSession {
    directory: PathBuf,
    /// The names of the files this adapter created in `directory`, in order.
    created: Mutex<Vec<String>>,
    /// The runner's count of the files it created there, from its `FILES 0 n`
    /// line, printed just before `CLOSED 1`.
    runner_files: Arc<Mutex<Option<u64>>>,
    process: Mutex<Option<Child>>,
    input: Mutex<Option<ChildStdin>>,
    pending: Arc<Mutex<HashMap<u64, mpsc::Sender<Event>>>>,
    next: AtomicU64,
    readers: Mutex<Vec<thread::JoinHandle<()>>>,
    starts: AtomicU64,
    exports: AtomicU64,
    preparations: Arc<AtomicU64>,
    export_ns: AtomicU64,
    bytes: AtomicU64,
    transport: Transport,
    started: Instant,
    spawn_ns: u64,
    bridge_ns: AtomicU64,
    control: Mutex<Option<ExecutionControl>>,
    trace: Arc<Mutex<Vec<String>>>,
    /// Lines the runner has written, for close()'s progress check.
    lines: Arc<AtomicU64>,
    costs: Arc<Mutex<[u64; 5]>>,
    definitions: Mutex<HashMap<DefinitionKey, (u64, String)>>,
    stderr: Arc<Mutex<Vec<u8>>>,
    terminal: Arc<Mutex<Option<EvalError>>>,
    claimed: AtomicBool,
    compiling: AtomicBool,
}
/// Every file created in a session's directory: by this adapter, by name,
/// and by the runner, as the count it reported at close (None before then).
/// A file created and removed again is still counted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionFiles {
    pub compiler: Vec<String>,
    pub runner: Option<u64>,
}
/// What the session sent its jobs and received from them, as counts only:
/// one call for each job submitted ([`GooseSession::execute`]).
#[derive(Default)]
struct Transport {
    calls: AtomicU64,
    argument_nodes: AtomicU64,
    argument_bytes: AtomicU64,
    argument_source_bytes: AtomicU64,
    result_bytes: AtomicU64,
}
#[derive(Clone, Copy, Debug)]
pub struct SessionStats {
    pub subprocesses: u64,
    pub initializations: u64,
    pub jobs: u64,
    pub exports: u64,
    pub definitions: u64,
    pub preparations: u64,
    pub export_ns: u64,
    pub bridge_bytes: u64,
    pub elapsed_ns: u64,
    pub spawn_ns: u64,
    pub rust_bridge_ns: u64,
    pub goose_base_load_ns: u64,
    pub goose_boot_ns: u64,
    pub goose_snapshot_ns: u64,
    pub goose_job_load_ns: u64,
    pub goose_bridge_ns: u64,
    /// The source-macro jobs submitted; in all their argument graphs, the
    /// nodes, the encoded bytes and the source-text bytes those include;
    /// and the bytes of the results received.
    pub calls: u64,
    pub argument_nodes: u64,
    pub argument_bytes: u64,
    pub argument_source_bytes: u64,
    pub result_bytes: u64,
}
impl GooseSession {
    pub fn start(executable: &Path, workers: usize) -> Result<Arc<Self>, EvalError> {
        Self::start_mode(executable, workers, None)
    }
    /// A session with no subprocess, for tests that select the Goose
    /// compiler path without a runner: every Goose call on it fails.
    #[cfg(test)]
    pub(crate) fn detached() -> Arc<Self> {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        Arc::new(Self {
            directory: std::env::temp_dir().join(format!(
                "light-cljs-goose-detached-{}-{id}",
                std::process::id()
            )),
            created: Mutex::new(Vec::new()),
            runner_files: Default::default(),
            process: Mutex::new(None),
            input: Mutex::new(None),
            pending: Default::default(),
            next: AtomicU64::new(1),
            readers: Mutex::new(Vec::new()),
            starts: AtomicU64::new(0),
            exports: AtomicU64::new(0),
            preparations: Default::default(),
            export_ns: AtomicU64::new(0),
            bytes: AtomicU64::new(0),
            transport: Transport::default(),
            started: Instant::now(),
            spawn_ns: 0,
            bridge_ns: AtomicU64::new(0),
            control: Mutex::new(None),
            trace: Default::default(),
            lines: Default::default(),
            costs: Default::default(),
            definitions: Mutex::new(HashMap::new()),
            stderr: Default::default(),
            terminal: Default::default(),
            claimed: AtomicBool::new(false),
            compiling: AtomicBool::new(false),
        })
    }
    fn start_mode(
        executable: &Path,
        workers: usize,
        mode: Option<&str>,
    ) -> Result<Arc<Self>, EvalError> {
        Self::start_retaining(executable, workers, mode, None)
    }
    /// `retained` overrides the runner's retention budget (how many job
    /// workers may block in place on a dependency; 0 saves every wait as a
    /// continuation) through BBP_RETAINED_WORKERS. None inherits the
    /// environment.
    fn start_retaining(
        executable: &Path,
        workers: usize,
        mode: Option<&str>,
        retained: Option<usize>,
    ) -> Result<Arc<Self>, EvalError> {
        let env = retained
            .map(|n| ("BBP_RETAINED_WORKERS", n.to_string()))
            .into_iter()
            .collect::<Vec<_>>();
        Self::start_env(executable, workers, mode, &env)
    }
    /// A session whose runner gets these environment variables (tests:
    /// BBP_RETAINED_WORKERS, BBP_WORKER_TABLE_BYTES); the rest it inherits.
    fn start_env(
        executable: &Path,
        workers: usize,
        mode: Option<&str>,
        env: &[(&str, String)],
    ) -> Result<Arc<Self>, EvalError> {
        let started = Instant::now();
        if !(1..=8).contains(&workers) {
            return Err(error("worker count must be 1..8"));
        }
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let directory =
            std::env::temp_dir().join(format!("light-cljs-goose-{}-{id}", std::process::id()));
        std::fs::create_dir(&directory).map_err(error)?;
        let base = big_boy_pants_js::frontend::goose::export_source("goose-base.js", "rt => null")
            .map_err(error)?;
        std::fs::write(directory.join("base.gsp"), base.bytes).map_err(error)?;
        let mut command = Command::new(executable);
        command.arg(&directory).arg(workers.to_string());
        for (name, value) in env {
            command.env(name, value);
        }
        if let Some(mode) = mode {
            command.arg(mode);
        }
        #[cfg(test)]
        if mode.is_none() {
            let serial = std::env::var("GOOSE_BENCH_CONTROL").is_ok_and(|v| v == "serial");
            if cfg!(feature = "count-allocations") {
                command.arg(if serial {
                    "serial-instrumented"
                } else {
                    "instrumented"
                });
            } else if serial {
                command.arg("serial");
            }
        }
        let mut child = match command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                let _ = std::fs::remove_dir_all(&directory);
                return Err(error(e));
            }
        };
        let input = child.stdin.take().unwrap();
        let spawn_ns = started.elapsed().as_nanos() as u64;
        let output = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        let (tx, rx) = mpsc::channel();
        let pending = Arc::new(Mutex::new(HashMap::from([(0, tx)])));
        let mailboxes = pending.clone();
        let trace = Arc::new(Mutex::new(Vec::new()));
        let traces = trace.clone();
        let lines = Arc::new(AtomicU64::new(0));
        let read = lines.clone();
        let costs = Arc::new(Mutex::new([0u64; 5]));
        let measured = costs.clone();
        let preparations = Arc::new(AtomicU64::new(0));
        let prepared = preparations.clone();
        let terminal = Arc::new(Mutex::new(None));
        let failures = terminal.clone();
        let runner_files = Arc::new(Mutex::new(None));
        let counted = runner_files.clone();
        let reader = thread::spawn(move || {
            let mut output = BufReader::new(output);
            let mut line = String::new();
            let mut terminal_reason = "subprocess exited";
            loop {
                line.clear();
                match (&mut output).take(65537).read_line(&mut line) {
                    Ok(0) => break,
                    Ok(_) if line.len() <= 65536 => {}
                    _ => {
                        terminal_reason = "malformed subprocess reply";
                        break;
                    }
                }
                read.fetch_add(1, Ordering::Relaxed);
                let parts = line.split_whitespace().collect::<Vec<_>>();
                let parsed = (|| {
                    let kind = *parts.first()?;
                    let id = parts.get(1)?.parse::<u64>().ok()?;
                    let event = match kind {
                        "READY"
                            if (parts.len() == 3 || parts.len() == 6)
                                && parts[1..3] == ["1", "1"]
                                && parts[3..].iter().all(|s| s.parse::<u64>().is_ok()) =>
                        {
                            return Some((0, Event::Ready));
                        }
                        "START" if parts.len() == 2 => Event::Start,
                        "PREPARED" if parts.len() == 3 && parts[2].parse::<u64>().is_ok() => {
                            Event::Resumed
                        }
                        "SCHED" | "THREADS" | "PARK" | "JOBS" | "TRIM" | "CENSUS" | "FILES"
                            if id == 0
                                && parts.len()
                                    == (match kind {
                                        "SCHED" => 13,
                                        "THREADS" => 5,
                                        "JOBS" => 32,
                                        "TRIM" => 6,
                                        "CENSUS" => 16,
                                        _ => 3,
                                    })
                                && parts[2..].iter().all(|s| s.parse::<u64>().is_ok()) =>
                        {
                            Event::Resumed
                        }
                        "CLOSED" if parts == ["CLOSED", "1"] => return Some((0, Event::Resumed)),
                        "RESUME" if parts.len() == 2 => Event::Resumed,
                        // A job waiting for another job's delay claim
                        // (light-cljs-808): `WAIT id cell`, traced only.
                        "WAIT" if parts.len() == 3 && parts[2].parse::<u64>().is_ok() => {
                            Event::Resumed
                        }
                        "PRODUCE" if parts.len() == 3 && parts[2].parse::<u64>().is_ok() => {
                            Event::Resumed
                        }
                        "COST" | "FINISH"
                            if parts.len() == 4
                                && parts[2..].iter().all(|s| s.parse::<u64>().is_ok()) =>
                        {
                            Event::Resumed
                        }
                        "METRIC"
                            if matches!(parts.len(), 13 | 15)
                                && parts[2..].iter().all(|s| s.parse::<u64>().is_ok()) =>
                        {
                            Event::Resumed
                        }
                        // A finished job's one reply (light-cljs-816):
                        // `DONE id next steps writes load bridge bytes`,
                        // its METRIC fields if any, then `bytes` bytes of
                        // its result graph.
                        "DONE"
                            if matches!(parts.len(), 8 | 19 | 21)
                                && parts[2..].iter().all(|s| s.parse::<u64>().is_ok()) =>
                        {
                            let next = parts[2].parse::<u64>().ok()?;
                            Event::Done((next > 0).then_some(next), Vec::new())
                        }
                        "NEED"
                            if parts.len() == 6
                                && parts[4..].iter().all(|s| s.parse::<u64>().is_ok()) =>
                        {
                            Event::Need(parts[2].parse().ok()?, parts[3].to_owned())
                        }
                        "PRODUCED" if parts.len() == 3 && parts[2].parse::<u64>().is_ok() => {
                            Event::Produced
                        }
                        "ERROR" | "FAULT" => Event::Failure(protocol_line(&line)),
                        _ => return None,
                    };
                    Some((id, event))
                })();
                match parsed {
                    Some((id, mut event)) => {
                        if let Event::Done(_, bytes) = &mut event {
                            let length = parts[7].parse::<usize>().unwrap();
                            if length > bridge::MAX_BYTES {
                                terminal_reason = "oversized subprocess reply";
                                break;
                            }
                            bytes.resize(length, 0);
                            if output.read_exact(bytes).is_err() {
                                terminal_reason = "truncated subprocess reply";
                                break;
                            }
                        }
                        if parts[0] == "PREPARED" {
                            prepared.fetch_add(1, Ordering::Relaxed);
                        }
                        let mut costs = measured.lock().unwrap();
                        if parts[0] == "READY" && parts.len() == 6 {
                            for i in 0..3 {
                                costs[i] = parts[i + 3].parse().unwrap();
                            }
                        }
                        if parts[0] == "COST" || parts[0] == "DONE" {
                            let at = if parts[0] == "COST" { 2 } else { 5 };
                            for i in 0..2 {
                                costs[i + 3] = costs[i + 3]
                                    .saturating_add(parts[at + i].parse::<u64>().unwrap());
                            }
                        }
                        drop(costs);
                        if parts[0] == "FILES" {
                            *counted.lock().unwrap() = parts[2].parse().ok();
                        }
                        let mut trace = traces.lock().unwrap();
                        if trace.len() == 4096
                            && matches!(
                                parts[0],
                                "JOBS" | "TRIM" | "THREADS" | "FILES" | "CLOSED" | "FAULT"
                            )
                        {
                            trace.remove(0);
                        }
                        let traced = if parts[0] == "DONE" {
                            reply_lines(&parts)
                        } else {
                            vec![line.trim().to_owned()]
                        };
                        for line in traced {
                            if trace.len() < 4096 {
                                trace.push(line);
                            }
                        }
                        let boxes = mailboxes.lock().unwrap();
                        if parts[0] == "FAULT" && id == 0 {
                            *failures.lock().unwrap() = Some(error(line.trim()));
                            for tx in boxes.values() {
                                let _ = tx.send(Event::Failure(line.trim().to_owned()));
                            }
                        } else if let Some(tx) = boxes.get(&id) {
                            let _ = tx.send(event);
                        } else if !matches!(
                            parts[0],
                            "CLOSED"
                                | "SCHED"
                                | "THREADS"
                                | "PARK"
                                | "JOBS"
                                | "TRIM"
                                | "CENSUS"
                                | "FILES"
                        ) {
                            for tx in boxes.values() {
                                let _ =
                                    tx.send(Event::Failure("uncorrelated subprocess reply".into()));
                            }
                            break;
                        }
                    }
                    None => {
                        for tx in mailboxes.lock().unwrap().values() {
                            let _ = tx.send(Event::Failure("malformed subprocess reply".into()));
                        }
                        break;
                    }
                }
            }
            for tx in mailboxes.lock().unwrap().values() {
                let _ = tx.send(Event::Failure(terminal_reason.into()));
            }
        });
        // Drain stderr so a failed native worker cannot block on a full pipe.
        // Error context uses bounded protocol messages, never unbounded logs.
        let errors = Arc::new(Mutex::new(Vec::new()));
        let captured = errors.clone();
        let drain = thread::spawn(move || {
            let mut stderr = stderr;
            let mut bytes = [0u8; 4096];
            while let Ok(n) = stderr.read(&mut bytes) {
                if n == 0 {
                    break;
                }
                let mut captured = captured.lock().unwrap();
                let count = n.min(65536 - captured.len());
                captured.extend_from_slice(&bytes[..count]);
            }
        });
        let session = Arc::new(Self {
            directory,
            created: Mutex::new(vec!["base.gsp".to_owned()]),
            runner_files,
            process: Mutex::new(Some(child)),
            input: Mutex::new(Some(input)),
            pending,
            next: AtomicU64::new(1),
            readers: Mutex::new(vec![reader, drain]),
            starts: AtomicU64::new(0),
            exports: AtomicU64::new(0),
            preparations,
            export_ns: AtomicU64::new(0),
            bytes: AtomicU64::new(0),
            transport: Transport::default(),
            started,
            spawn_ns,
            bridge_ns: AtomicU64::new(0),
            control: Mutex::new(None),
            trace,
            lines,
            costs,
            definitions: Mutex::new(HashMap::new()),
            stderr: errors,
            terminal,
            claimed: AtomicBool::new(false),
            compiling: AtomicBool::new(false),
        });
        match rx.recv_timeout(Duration::from_secs(10)) {
            Ok(Event::Ready) => {
                session.pending.lock().unwrap().remove(&0);
                Ok(session)
            }
            other => {
                session.close();
                Err(error(format!("startup failed: {other:?}")))
            }
        }
    }
    pub fn stats(&self) -> SessionStats {
        let costs = *self.costs.lock().unwrap();
        SessionStats {
            subprocesses: 1,
            initializations: 1,
            jobs: self.starts.load(Ordering::Relaxed),
            exports: self.exports.load(Ordering::Relaxed),
            definitions: self.definitions.lock().unwrap().len() as u64,
            preparations: self.preparations.load(Ordering::Relaxed),
            export_ns: self.export_ns.load(Ordering::Relaxed),
            bridge_bytes: self.bytes.load(Ordering::Relaxed),
            elapsed_ns: self.started.elapsed().as_nanos() as u64,
            spawn_ns: self.spawn_ns,
            rust_bridge_ns: self.bridge_ns.load(Ordering::Relaxed),
            goose_base_load_ns: costs[0],
            goose_boot_ns: costs[1],
            goose_snapshot_ns: costs[2],
            goose_job_load_ns: costs[3],
            goose_bridge_ns: costs[4],
            calls: self.transport.calls.load(Ordering::Relaxed),
            argument_nodes: self.transport.argument_nodes.load(Ordering::Relaxed),
            argument_bytes: self.transport.argument_bytes.load(Ordering::Relaxed),
            argument_source_bytes: self.transport.argument_source_bytes.load(Ordering::Relaxed),
            result_bytes: self.transport.result_bytes.load(Ordering::Relaxed),
        }
    }
    /// Every file created in the session directory so far.
    pub fn files(&self) -> SessionFiles {
        SessionFiles {
            compiler: self.created.lock().unwrap().clone(),
            runner: *self.runner_files.lock().unwrap(),
        }
    }
    pub fn trace(&self) -> Vec<String> {
        self.trace.lock().unwrap().clone()
    }
    pub(crate) fn claim_compilation(&self) -> Result<(), EvalError> {
        self.claimed
            .compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed)
            .map(|_| ())
            .map_err(|_| error("session already belongs to a compilation"))
    }
    pub(crate) fn begin_compilation(&self) {
        if self.compiling.swap(true, Ordering::Relaxed) {
            *self.terminal.lock().unwrap() =
                Some(error("a fresh compilation requires a fresh Goose session"));
            self.close();
        }
    }
    /// Terminal failures retire the entire compilation, including every worker.
    ///
    /// The close request (the end of the runner's input) cancels every job; the runner then runs each saved
    /// waiter's pending finally blocks as a cleanup job (light-cljs-803)
    /// before it prints `CLOSED 1` and exits. close() waits for that exit as
    /// long as the runner keeps replying (each cleanup reports a line),
    /// bounded by [`CLOSE_QUIET`] without a reply and [`CLOSE_LIMIT`] in all,
    /// and only then kills it, reporting the kill as a `KILLED` trace line
    /// and on stderr. Drop closes the same way, so it cannot hang either.
    pub fn close(&self) {
        self.close_within(CLOSE_QUIET, CLOSE_LIMIT);
    }
    /// close() with these bounds on the wait for the runner to exit.
    ///
    /// A writer blocked mid-frame on a full pipe holds the input: close()
    /// waits for it within the quiet bound, then kills the runner, which
    /// fails that write (light-cljs-816).
    fn close_within(&self, quiet_limit: Duration, limit: Duration) {
        if let Some(mut child) = self.process.lock().unwrap().take() {
            let started = Instant::now();
            // Held: a writer kept the input past the quiet bound.
            let (input, held) = loop {
                match self.input.try_lock() {
                    Ok(mut input) => break (input.take(), false),
                    Err(TryLockError::Poisoned(input)) => break (input.into_inner().take(), false),
                    Err(TryLockError::WouldBlock) if started.elapsed() < quiet_limit => {
                        thread::sleep(Duration::from_millis(1))
                    }
                    Err(TryLockError::WouldBlock) => break (None, true),
                }
            };
            // The close request is the end of input: dropping it cannot
            // block on a full pipe, as writing a line could (an input
            // already gone ended at its drop).
            drop(input);
            let mut seen = self.lines.load(Ordering::Relaxed);
            let mut quiet = Instant::now();
            let exited = loop {
                if child.try_wait().ok().flatten().is_some() {
                    break true;
                }
                let now = Instant::now();
                let lines = self.lines.load(Ordering::Relaxed);
                if lines != seen {
                    seen = lines;
                    quiet = now;
                }
                if held || now - quiet >= quiet_limit || now - started >= limit {
                    break false;
                }
                thread::sleep(Duration::from_millis(1));
            };
            if !exited {
                let waited = started.elapsed().as_millis();
                eprintln!(
                    "light-cljs: the Goose runner did not finish closing within {waited} ms; killing it, so some cancellation cleanup may not have run"
                );
                let mut trace = self.trace.lock().unwrap();
                if trace.len() == 4096 {
                    trace.remove(0);
                }
                trace.push(format!("KILLED {waited}"));
            }
            let _ = child.kill();
            let _ = child.wait();
            // A writer that held the input has failed now; no later one writes.
            self.input
                .lock()
                .unwrap_or_else(|input| input.into_inner())
                .take();
        }
        for reader in self.readers.lock().unwrap().drain(..) {
            let _ = reader.join();
        }
        self.pending.lock().unwrap().clear();
        let _ = std::fs::remove_dir_all(&self.directory);
    }
    /// Export `source` to `path`: the adapter's only file write after
    /// start. It records every write into the session directory by name.
    fn export(&self, path: &Path, source: &str) -> Result<(), EvalError> {
        let started = Instant::now();
        let export = big_boy_pants_js::frontend::goose::export_source("compiler-macro.js", source)
            .map_err(error)?;
        if let Ok(name) = path.strip_prefix(&self.directory) {
            let name = name.to_string_lossy().into_owned();
            self.created.lock().unwrap().push(name);
        }
        std::fs::write(path, &export.bytes).map_err(error)?;
        self.exports.fetch_add(1, Ordering::Relaxed);
        self.export_ns
            .fetch_add(started.elapsed().as_nanos() as u64, Ordering::Relaxed);
        Ok(())
    }
    /// Write one command line and its payload to the runner, under the
    /// input lock.
    fn send(&self, header: &str, payload: &[u8]) -> Result<(), EvalError> {
        let mut input = self.input.lock().unwrap();
        let input = input.as_mut().ok_or_else(|| error("session is closed"))?;
        input
            .write_all(header.as_bytes())
            .and_then(|_| input.write_all(payload))
            .and_then(|_| input.flush())
            .map_err(error)
    }
    /// Answer job `id`'s wait `sequence`: `-1` for a missing helper, or
    /// `unit definition` with the initializer's gensym prefix if it has one.
    fn answer_key(&self, id: u64, sequence: u64, key: &str) -> Result<(), EvalError> {
        self.send(&format!("A {id} {sequence} {key}\n"), &[])
    }
    // Compiler CodeIdentity includes the resolved declaration context. Never
    // merge equal source text from distinct definitions or compilation sessions.
    // The lock covers export/publication only, never a VM wait or callback.
    fn definition(&self, key: DefinitionKey, source: &str) -> Result<u64, EvalError> {
        let mut definitions = self.definitions.lock().unwrap();
        if let Some((id, previous)) = definitions.get(&key) {
            // A catalog unit has an immutable resolved request/context. Its
            // equivalent emitted text can change when application IR is drained
            // into DeferredUnit (canonical vs native emission). Keep the first
            // supported export by unit identity, as the once-only producer does.
            // Still service every compiler callback before reaching this cache.
            return if matches!(key, DefinitionKey::Helper(_)) || previous == source {
                Ok(*id)
            } else {
                Err(error(format!(
                    "definition identity/source conflict for {key:?}: {} vs {} bytes",
                    previous.len(),
                    source.len()
                )))
            };
        }
        if definitions.len() >= 1024 {
            return Err(error("definition budget"));
        }
        let id = definitions.len() as u64 + 1;
        self.export(&self.directory.join(format!("definition-{id}.gsp")), source)?;
        definitions.insert(key, (id, source.to_owned()));
        Ok(id)
    }
    fn execute(
        &self,
        key: DefinitionKey,
        source: &str,
        graph: &bridge::Graph,
        c: &ExecutionControl,
        gensym: Option<(&str, u64)>,
        mut helper: impl FnMut(&str) -> Result<(usize, String, Option<String>), EvalError>,
    ) -> Result<(Vec<u8>, Option<u64>), EvalError> {
        let result = (|| {
            if let Some(error) = self.terminal.lock().unwrap().clone() {
                return Err(error);
            }
            c.check()?;
            let id = self.next.fetch_add(1, Ordering::Relaxed);
            let definition = self.definition(key, source)?;
            let bridged = Instant::now();
            let input = graph.encode()?;
            self.bytes.fetch_add(input.len() as u64, Ordering::Relaxed);
            let t = &self.transport;
            t.calls.fetch_add(1, Ordering::Relaxed);
            t.argument_nodes
                .fetch_add(graph.node_count() as u64, Ordering::Relaxed);
            t.argument_bytes
                .fetch_add(input.len() as u64, Ordering::Relaxed);
            t.argument_source_bytes
                .fetch_add(graph.source_bytes() as u64, Ordering::Relaxed);
            self.bridge_ns
                .fetch_add(bridged.elapsed().as_nanos() as u64, Ordering::Relaxed);
            // The call's frame: its definition key, the macro's generated-name
            // scope if any (its prefix and the next number of its count,
            // light-cljs-798), then the argument graph.
            let header = match gensym {
                Some((prefix, start)) => {
                    format!("C {id} {definition} {} {prefix} {start}\n", input.len())
                }
                None => format!("C {id} {definition} {}\n", input.len()),
            };
            let (tx, rx) = mpsc::channel();
            self.pending.lock().unwrap().insert(id, tx);
            self.send(&header, &input)?;
            // A START line comes only in traced runs; the reply counts too.
            let mut started = false;
            loop {
                c.poll()?;
                match rx.recv_timeout(Duration::from_millis(2)) {
                    Ok(Event::Start) => {
                        if !started {
                            started = true;
                            self.starts.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                    Ok(Event::Done(next, bytes)) => {
                        if !started {
                            self.starts.fetch_add(1, Ordering::Relaxed);
                        }
                        self.pending.lock().unwrap().remove(&id);
                        if let Some(error) = self.terminal.lock().unwrap().clone() {
                            return Err(error);
                        }
                        self.bytes.fetch_add(bytes.len() as u64, Ordering::Relaxed);
                        self.transport
                            .result_bytes
                            .fetch_add(bytes.len() as u64, Ordering::Relaxed);
                        return Ok((bytes, next));
                    }
                    Ok(Event::Need(sequence, name)) => {
                        let failed = self.terminal.lock().unwrap().clone();
                        let reply = match failed {
                            Some(error) => Err(error),
                            None => helper(&name),
                        };
                        let (unit, source, prefix) = match reply {
                            Ok(reply) => reply,
                            Err(failure) => {
                                *self.terminal.lock().unwrap() = Some(failure);
                                self.answer_key(id, sequence, "-1")?;
                                continue;
                            }
                        };
                        c.poll()?;
                        // The helper's export, which a worker loads once.
                        let definition = self.definition(DefinitionKey::Helper(unit), &source)?;
                        // `unit definition` or `unit definition prefix`:
                        // Goose produces the helper in its frozen initializer
                        // scope (light-cljs-802).
                        let key = match prefix {
                            Some(prefix) => format!("{unit} {definition} {prefix}"),
                            None => format!("{unit} {definition}"),
                        };
                        self.answer_key(id, sequence, &key)?;
                    }
                    Ok(Event::Produced) => {}
                    Ok(Event::Resumed) => {}
                    Ok(Event::Failure(message)) => {
                        return Err(self
                            .terminal
                            .lock()
                            .unwrap()
                            .clone()
                            .unwrap_or_else(|| failure(&message)));
                    }
                    Ok(_) => return Err(error("unexpected protocol message")),
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(e) => return Err(error(e)),
                }
            }
        })();
        // The outer evaluator owns terminal cleanup, after a caught helper
        // failure has returned through its saved VM continuation.
        result.map_err(|mut e: EvalError| {
            let stderr = self.stderr.lock().unwrap();
            if !stderr.is_empty() {
                e.0.push_str(": ");
                e.0.push_str(&String::from_utf8_lossy(&stderr));
            }
            e
        })
    }
}
/// The trace lines a finished job's reply stands for, as the runner printed
/// them before it folded them into the reply (light-cljs-816): METRIC when it
/// has fields, COST, FINISH and DONE.
fn reply_lines(parts: &[&str]) -> Vec<String> {
    let id = parts[1];
    let mut lines = Vec::with_capacity(4);
    if parts.len() > 8 {
        lines.push(format!("METRIC {id} {}", parts[8..].join(" ")));
    }
    lines.push(format!("COST {id} {} {}", parts[5], parts[6]));
    lines.push(format!("FINISH {id} {} {}", parts[3], parts[4]));
    lines.push(match parts[2] {
        "0" => format!("DONE {id}"),
        next => format!("DONE {id} {next}"),
    });
    lines
}
impl Drop for GooseSession {
    fn drop(&mut self) {
        self.close();
    }
}
#[derive(Clone)]
enum Stored {
    Source(DefinitionKey, String),
    Form(Form, ConversionLimits),
    Data(Datum, ConversionLimits),
    /// A narrowed environment and the one it was narrowed from, whose
    /// conversion still checks and charges what it leaves out.
    Environment(Datum, Datum, ConversionLimits),
    Result(Vec<u8>, Arc<bridge::Originals>, Rc<bridge::Budget>),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum DefinitionKey {
    Compiler(usize),
    Anonymous(u64),
    Helper(usize),
}
/// The compiler's answer to a `Need` for a compile-time root by name: the
/// root's unit, its initializer and what the initializer's `gensym`
/// appends before its own count, if the unit was registered in a scope
/// ([`crate::generated_names::initializer_gensym_prefix`]), resolved by the
/// catalog view that installed it ([`crate::compilation_session::RootIndex`]).
/// Goose never holds a copy of the registrations.
type HelperService =
    Rc<dyn Fn(&dyn MacroEvaluator, &str) -> Result<(usize, String, Option<String>), String>>;
/// A token naming a catalog view that installs a helper service, unique
/// in the process, so that a view installs its service once per evaluator.
pub(crate) fn helper_owner() -> u64 {
    NEXT.fetch_add(1, Ordering::Relaxed)
}
/// A thread-local compiler view with session/version-scoped, non-reused handles.
pub struct GooseEvaluator {
    session: Arc<GooseSession>,
    identity: u64,
    slots: RefCell<HashMap<u64, Stored>>,
    next: Cell<u64>,
    control: ExecutionControl,
    /// The helper service and the token of the view that installed it.
    helper: RefCell<Option<(u64, HelperService)>>,
    /// How many times a helper service was installed.
    helper_installs: Cell<usize>,
    budget: RefCell<Option<Rc<bridge::Budget>>>,
    functions: RefCell<HashMap<usize, ValueId>>,
    /// The generated-name scope the compiler last put the evaluator's
    /// `gensym` in ([`crate::generated_names::sync_gensym_scope`]): its
    /// serial and prefix, or `None` outside every scope, where `gensym`
    /// stays an explicit fault (light-cljs-798).
    gensym: RefCell<Option<(u64, String)>>,
    /// Whether compile-time code installed an extension environment data
    /// could reach ([`crate::evaluator::EnvironmentExtensions`]): unknown
    /// until a compilation prepares the evaluator
    /// ([`Self::track_environment_extensions`]), then `None` for good, as
    /// Goose installs no extension (light-cljs-815).
    environment_extensions: crate::evaluator::EnvironmentTracking,
    /// What the collections narrowed environments left out cost to omit.
    omitted: bridge::OmitCache,
    #[cfg(test)]
    pub(crate) helper_delay: Cell<Duration>,
}
impl GooseEvaluator {
    pub fn new(session: Arc<GooseSession>, options: &EvaluatorOptions) -> Result<Self, EvalError> {
        let control = {
            let mut control = session.control.lock().unwrap();
            if control.is_none() {
                *control = Some(ExecutionControl::new(options)?);
            }
            control.as_ref().unwrap().clone()
        };
        Ok(Self {
            session,
            identity: NEXT.fetch_add(1, Ordering::Relaxed),
            slots: RefCell::new(HashMap::new()),
            next: Cell::new(0),
            control,
            helper: RefCell::new(None),
            helper_installs: Cell::new(0),
            budget: RefCell::new(None),
            functions: RefCell::new(HashMap::new()),
            gensym: RefCell::new(None),
            environment_extensions: Rc::default(),
            omitted: Default::default(),
            #[cfg(test)]
            helper_delay: Cell::new(Duration::ZERO),
        })
    }
    pub(crate) fn session(&self) -> Arc<GooseSession> {
        self.session.clone()
    }
    fn store(&self, value: Stored) -> ValueId {
        let index = self.next.get();
        self.next.set(index + 1);
        self.slots.borrow_mut().insert(index, value);
        ValueId {
            session: self.identity,
            index,
        }
    }
    fn available(&self) -> Result<(), EvalError> {
        if self.session.process.lock().unwrap().is_none() {
            return Err(error("session is closed"));
        }
        if let Some(error) = self.session.terminal.lock().unwrap().clone() {
            return Err(error);
        }
        let result = self.control.poll();
        if result.is_err() && self.budget.borrow().is_none() {
            self.session.close();
        }
        result
    }
    fn get(&self, id: ValueId) -> Result<Stored, EvalError> {
        if id.session != self.identity {
            return Err(error("foreign handle"));
        }
        self.slots
            .borrow()
            .get(&id.index)
            .cloned()
            .ok_or_else(|| error("released handle"))
    }
    /// Install the helper service of the view `owner` names, unless it is
    /// already the installed one: nothing is replayed per macro call.
    pub(crate) fn install_helper(&self, owner: u64, helper: impl FnOnce() -> HelperService) {
        if self
            .helper
            .borrow()
            .as_ref()
            .is_some_and(|(installed, _)| *installed == owner)
        {
            return;
        }
        *self.helper.borrow_mut() = Some((owner, helper()));
        self.helper_installs.set(self.helper_installs.get() + 1);
    }
    /// Start reporting, for the compilation that prepares this evaluator,
    /// that its compile-time code has installed no extension environment
    /// data could reach, which it never will on Goose
    /// ([`crate::evaluator::EnvironmentExtensions`], light-cljs-815): the
    /// Goose runtime has no `extendType` or `extendInstance` leaf, so
    /// `extend-type`, `extend-protocol`, `specify!`, `reify` and a `deftype`
    /// or `defrecord` with methods fail their job with an uncatchable fault
    /// before they write anything, as a write to a realm row or to a form
    /// does; each job starts again from the session image; and the core
    /// operations on form nodes, keyword invocation included, are Goose
    /// `match` code that dispatches to no JavaScript.
    pub(crate) fn track_environment_extensions(&self) {
        let tracking = &self.environment_extensions;
        if tracking.get() == crate::evaluator::EnvironmentExtensions::Untracked {
            tracking.set(crate::evaluator::EnvironmentExtensions::None);
        }
    }
    /// How many times a helper service was installed.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn helper_installs(&self) -> usize {
        self.helper_installs.get()
    }
    /// Retire the session after an outermost call or export failed: every
    /// worker goes with it. The control stays failed as it was, cancelled or
    /// past its deadline, or else is invalidated, as an engine failure
    /// invalidates V8's or QuickJS's. It is the compile-time control of the
    /// whole Goose compiler path (light-cljs-778), whose drains then report
    /// the failing unit's own error rather than charging the polls of the
    /// units before it as a cancellation.
    fn retire(&self) {
        let _ = self.control.poll_many(0);
        self.control.invalidate();
        self.session.close();
    }
}
impl MacroEvaluator for GooseEvaluator {
    fn source_only(&self) -> bool {
        true
    }
    fn control(&self) -> ExecutionControl {
        self.control.clone()
    }
    fn heap(&self) -> usize {
        self.identity as usize
    }
    fn needs_reader_registration(&self) -> bool {
        false
    }
    fn has_native_bridge(&self) -> bool {
        true
    }
    fn environment_extensions(&self) -> Option<crate::evaluator::EnvironmentTracking> {
        Some(self.environment_extensions.clone())
    }
    fn compiler_frontier(&self, _: Option<usize>) -> Result<(), EvalError> {
        Ok(())
    }
    /// Each job of a macro expanding in a scope numbers in the scope's count,
    /// as V8's native runtime does through `__lightGensym`: the job takes the
    /// scope's prefix and next number with it and gives the count back with
    /// its result ([`crate::generated_names::GensymWindow`]). A scope whose
    /// numbers the runtime would count itself (`native` false, a data
    /// reader's) has none on Goose.
    fn gensym_scope(
        &self,
        serial: u64,
        prefix: &str,
        _: &str,
        native: bool,
    ) -> Result<(), EvalError> {
        *self.gensym.borrow_mut() = (native && serial != 0).then(|| (serial, prefix.to_owned()));
        Ok(())
    }
    fn evaluate_unit(&self, source: &str) -> Result<ValueId, EvalError> {
        self.available()?;
        Ok(self.store(Stored::Source(
            DefinitionKey::Anonymous(NEXT.fetch_add(1, Ordering::Relaxed)),
            source.to_owned(),
        )))
    }
    fn cached_source_macro_function(&self, identity: usize) -> Option<ValueId> {
        self.functions.borrow().get(&identity).copied()
    }
    fn source_macro_function(&self, identity: usize, source: &str) -> Result<ValueId, EvalError> {
        self.available()?;
        if let Some(id) = self.cached_source_macro_function(identity) {
            if let Stored::Source(_, previous) = self.get(id)?
                && previous == source
            {
                return Ok(id);
            }
            return Err(error("definition identity/source conflict"));
        }
        let id = self.store(Stored::Source(
            DefinitionKey::Compiler(identity),
            source.to_owned(),
        ));
        self.functions.borrow_mut().insert(identity, id);
        Ok(id)
    }
    fn call(&self, function: ValueId, arguments: &[ValueId]) -> Result<ValueId, EvalError> {
        self.available()?;
        let Stored::Source(key, source) = self.get(function)? else {
            return Err(error("handle is not a function"));
        };
        let values = arguments
            .iter()
            .map(|&id| self.get(id))
            .collect::<Result<Vec<_>, _>>()?;
        let mut limits = ConversionLimits::default();
        for value in &values {
            if let Stored::Form(_, l) | Stored::Data(_, l) | Stored::Environment(_, _, l) = value {
                limits.nodes = limits.nodes.min(l.nodes);
                limits.depth = limits.depth.min(l.depth);
            }
        }
        let previous = self.budget.borrow().clone();
        let budget = previous
            .clone()
            .unwrap_or_else(|| Rc::new(bridge::Budget::new(limits.nodes, self.control.clone())));
        *self.budget.borrow_mut() = Some(budget.clone());
        let scope = self.gensym.borrow().clone();
        let result = (|| {
            let window = scope.as_ref().and_then(|(serial, prefix)| {
                crate::generated_names::GensymWindow::open(*serial).map(|w| (prefix.as_str(), w))
            });
            let bridged = Instant::now();
            let graph = bridge::Graph::arguments_omitting(&values, limits, &budget, &self.omitted)?;
            budget.finish()?;
            self.session
                .bridge_ns
                .fetch_add(bridged.elapsed().as_nanos() as u64, Ordering::Relaxed);
            let gensym = window.as_ref().map(|(prefix, w)| (*prefix, w.start()));
            let (bytes, next) =
                self.session
                    .execute(key, &source, &graph, &self.control, gensym, |name| {
                        #[cfg(test)]
                        thread::sleep(self.helper_delay.get());
                        let helper = self
                            .helper
                            .borrow()
                            .as_ref()
                            .map(|(_, helper)| helper.clone())
                            .ok_or_else(|| error("compiler service is not installed"))?;
                        helper(self, name).map_err(error)
                    })?;
            if let Some((_, window)) = window {
                window
                    .close(next.ok_or_else(|| error("job lost its gensym count"))?)
                    .map_err(error)?;
            }
            Ok(self.store(Stored::Result(
                bytes,
                Arc::new(graph.originals),
                budget.clone(),
            )))
        })();
        let result = budget.settle(result);
        *self.budget.borrow_mut() = previous.clone();
        if result.is_err() && previous.is_none() {
            self.retire();
        }
        result
    }
    fn import_form(&self, f: &Form, limits: ConversionLimits) -> Result<ValueId, EvalError> {
        self.available()?;
        Ok(self.store(Stored::Form(f.clone(), limits)))
    }
    fn import_data(&self, d: &Datum, limits: ConversionLimits) -> Result<ValueId, EvalError> {
        self.available()?;
        Ok(self.store(Stored::Data(d.clone(), limits)))
    }
    /// The narrowed environment `view` with the whole one, `environment`,
    /// whose left-out entries its conversion checks and charges as if it
    /// converted them, since one budget covers every conversion of the
    /// call and Goose refuses data V8 converts (light-cljs-815).
    fn import_environment_view(
        &self,
        view: &Datum,
        environment: &Datum,
        limits: ConversionLimits,
    ) -> Result<ValueId, EvalError> {
        self.available()?;
        Ok(self.store(Stored::Environment(
            view.clone(),
            environment.clone(),
            limits,
        )))
    }
    fn export_form(
        &self,
        id: ValueId,
        call: &Form,
        origin: ExpansionOrigin,
        limits: ConversionLimits,
    ) -> Result<Form, EvalError> {
        let Stored::Result(bytes, originals, budget) = self.get(id)? else {
            return Err(error("handle is not a result"));
        };
        let bridged = Instant::now();
        let result = bridge::Graph::decode(&bytes, &originals, limits, &budget)
            .and_then(|graph| graph.expansion(call, origin, &originals, limits, &budget));
        let result = budget.settle(result);
        self.session
            .bridge_ns
            .fetch_add(bridged.elapsed().as_nanos() as u64, Ordering::Relaxed);
        if result.is_err() && self.budget.borrow().is_none() {
            self.retire();
        }
        result
    }
    fn release(&self, id: ValueId) -> Result<(), EvalError> {
        if let Stored::Source(DefinitionKey::Compiler(identity), _) = self.get(id)? {
            self.functions.borrow_mut().remove(&identity);
        }
        self.slots.borrow_mut().remove(&id.index);
        Ok(())
    }
}

#[cfg(test)]
mod bridge_control_tests {
    use super::*;
    fn runner() -> PathBuf {
        std::env::var_os("GOOSE_COMPILER_RUNNER")
            .expect("run compiler.bb")
            .into()
    }
    #[test]
    #[ignore = "build native runner with compiler.bb"]
    fn goose_compiler_bridge_tails_are_paid_before_execution_and_publication() {
        for workers in [1, 2, 4, 8] {
            for output in [false, true] {
                let session = GooseSession::start(&runner(), workers).unwrap();
                let e = GooseEvaluator::new(session.clone(), &Default::default()).unwrap();
                let function = e.evaluate_unit("rt => (form, env) => form").unwrap();
                let data = e
                    .import_data(
                        &Datum::new(crate::compiler_data::DatumKind::Nil),
                        Default::default(),
                    )
                    .unwrap();
                let result = if output {
                    Some(e.call(function, &[data, data]).unwrap())
                } else {
                    None
                };
                let c = e.control();
                c.poll_many(c.remaining_polls() - if output { 1 } else { 2 })
                    .unwrap();
                let error = if let Some(result) = result {
                    let source = crate::compiler_data::Source::new("tail.cljs", "nil");
                    let call = Form::from_data(
                        Datum::new(crate::compiler_data::DatumKind::Nil),
                        source.span(0..3).unwrap(),
                        crate::compiler_data::Origin::Source,
                    );
                    e.export_form(
                        result,
                        &call,
                        ExpansionOrigin {
                            macro_name: crate::compiler_data::Name::new(None, "tail"),
                            call: call.span(),
                            definition: None,
                            helpers: vec![],
                        },
                        Default::default(),
                    )
                    .unwrap_err()
                } else {
                    e.call(function, &[data, data]).unwrap_err()
                };
                assert!(error.0.contains("budget exhausted"), "{error}");
                if !output {
                    assert!(
                        !session
                            .trace()
                            .iter()
                            .any(|s| s.starts_with("START ") || s.starts_with("DONE "))
                    );
                }
                assert!(c.check().is_err(), "catch cannot clear a failed tail debit");
                assert!(session.process.lock().unwrap().is_none());
                assert!(!session.directory.exists());
            }
            let session = GooseSession::start(&runner(), workers).unwrap();
            let mut host = crate::MacroHost::new();
            host.use_goose_source_macros(session.clone()).unwrap();
            crate::compile_with_macros("(defmacro fresh [] 42) (fresh)", &mut host).unwrap();
            session.close();
            assert!(!session.directory.exists());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn runner() -> PathBuf {
        std::env::var_os("GOOSE_COMPILER_RUNNER")
            .expect("run compiler.bb")
            .into()
    }
    #[test]
    #[ignore = "build native runner with compiler.bb"]
    fn goose_compiler_concurrent_helper_once() {
        for callable in [false, true] {
            for workers in [1, 2, 4, 8] {
                let session =
                    GooseSession::start_mode(&runner(), workers, Some("readiness-test")).unwrap();
                let root = session.directory.join("sources");
                std::fs::create_dir(&root).unwrap();
                let macros = if callable {
                    "(ns macros) (defn shared [form] (list 'quote form)) (defmacro work [n] (loop [i 0] (if (= i n) (shared [11 22]) (recur (+ i 1)))))"
                } else {
                    "(ns macros) (def shared [11 22]) (defmacro work [n] (loop [i 0] (if (= i n) (list 'quote shared) (recur (+ i 1)))))"
                };
                std::fs::write(root.join("macros.cljs"), macros).unwrap();
                std::fs::write(root.join("main.cljs"),"(ns main (:require [macros :refer [work]])) (def shared [99]) (def a (work 20000)) (def b (work 20000)) (def c (work 20000)) (def d (work 20000)) (js/console.log (pr-str [a b c d]))").unwrap();
                std::fs::write(root.join("batch0.cljs"), "(ns batch0 (:require [macros :refer [work]])) (def a (work 20000)) (def b (work 20001)) (def c (work 20002)) (def d (work 20003))").unwrap();
                for stage in 0..crate::analyzer::DRAIN_LAG {
                    let dependency = if stage == 0 {
                        "batch0".to_owned()
                    } else {
                        format!("gate{}", stage - 1)
                    };
                    std::fs::write(
                        root.join(format!("gate{stage}.cljs")),
                        format!("(ns gate{stage} (:require [{dependency}]))"),
                    )
                    .unwrap();
                }
                std::fs::write(root.join("main.cljs"), "(ns main (:require [gate5] [batch0 :as b0] [macros :refer [work]])) (def a (work 20004)) (def b (work 20005)) (def c (work 20006)) (def d (work 20007)) (js/console.log (pr-str [b0/a b0/b b0/c b0/d a b c d]))").unwrap();
                let mut builds = Vec::new();
                let mut expansions = Vec::new();
                for goose in [false, true] {
                    *crate::compiler_bootstrap::goose_capture::CONCURRENT_FORMS
                        .lock()
                        .unwrap() = Some(Vec::new());
                    let mut host = crate::MacroHost::new();
                    host.evaluator_options.memory_limit = 512 << 20;
                    if goose {
                        host.use_goose_source_macros(session.clone()).unwrap();
                    }
                    let _names = crate::generated_names::enter_namespace(0);
                    let (program, _) = crate::compiler::analyze_namespace_entry_recorded(
                        "main",
                        std::slice::from_ref(&root),
                        &mut host,
                        Default::default(),
                        &mut Vec::new(),
                        crate::compiler::EntryRecorders {
                            slots: crate::scheduler::Slots::new(4).unwrap(),
                            ..Default::default()
                        },
                    )
                    .unwrap_or_else(|e| {
                        panic!(
                            "workers={workers} Goose={goose}: {e} trace={:?}",
                            session.trace()
                        )
                    });
                    builds.push(crate::emit_js(&program));
                    expansions.push(
                        crate::compiler_bootstrap::goose_capture::CONCURRENT_FORMS
                            .lock()
                            .unwrap()
                            .take()
                            .unwrap(),
                    );
                }
                assert_eq!(expansions[0].len(), 8);
                crate::compiler_bootstrap::goose_capture::compare_concurrent_forms(
                    expansions.remove(0),
                    expansions.remove(0),
                );
                assert_eq!(builds[0], builds[1], "deterministic compiler output");
                let trace = session.trace();
                let stats = session.stats();
                assert_eq!(stats.initializations, 1);
                assert_eq!(stats.jobs, 8);
                assert_eq!(stats.preparations, 1, "one initialized macro checkpoint");
                assert_eq!(stats.definitions, 2, "macro and shared helper");
                assert_eq!(stats.exports, 2, "export each definition once");
                eprintln!("callable={callable} workers={workers}: {trace:?}");
                assert_eq!(
                    trace
                        .iter()
                        .filter(|line| line.starts_with("PRODUCED "))
                        .count(),
                    1,
                    "one actual helper producer"
                );
                let (mut active, mut peak) = (0i32, 0i32);
                for line in &trace {
                    let kind = line.split_whitespace().next().unwrap();
                    match kind {
                        "START" | "RESUME" | "PRODUCE" => {
                            active += 1;
                            peak = peak.max(active);
                        }
                        "NEED" | "DONE" | "PRODUCED" => active -= 1,
                        _ => {}
                    }
                    assert!(active >= 0, "{trace:?}");
                }
                assert_eq!(active, 0);
                assert!(peak <= workers as i32);
                if workers > 1 {
                    assert!(
                        peak >= 2,
                        "two real compiler macro jobs must overlap: {trace:?}"
                    );
                }
                session.close();
                assert!(!session.directory.exists());
                super::lifecycle_tests::assert_jobs(&session.trace(), workers, 17);
            }
        }
    }
    #[test]
    #[ignore = "build native runner with compiler.bb"]
    fn goose_compiler_caught_nested_conversion_limit_is_terminal() {
        let values = (0..1000)
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join(" ");
        let declaration = format!("(defmacro wide [] (list (quote quote) [{values}])) ");
        // `caught`'s own argument takes most of the limited budget, which
        // its whole `&env` took before source macros that read none were
        // given only the entries they read (light-cljs-815), so that the
        // nested expansion is the one past the limit.
        let filler = (0..800)
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join(" ");
        let nested = format!(
            "{declaration} (def helper (wide)) (defmacro caught [_] (try helper (catch :default ignored 42))) (caught [{filler}])"
        );
        for (source, limited, expect_error) in [
            (format!("{declaration} (wide)"), true, false),
            (nested.clone(), true, true),
            (nested, false, false),
        ] {
            let session = GooseSession::start(&runner(), 1).unwrap();
            let mut host = crate::MacroHost::new();
            if limited {
                host.limits.generated_nodes = 4200;
            }
            host.use_goose_source_macros(session.clone()).unwrap();
            let result = crate::compile_with_macros(&source, &mut host);
            let trace = session.trace();
            eprintln!("nested limited={limited} error={expect_error}: {trace:?}");
            if expect_error {
                let error = result.unwrap_err().to_string();
                assert!(
                    session.terminal.lock().unwrap().is_some(),
                    "failure must happen inside the nested compiler callback: {error} {trace:?}"
                );
                assert!(error.contains("Goose conversion limit"), "{error}");
                assert!(
                    trace.iter().any(|s| s == "DONE 1"),
                    "JS catch must execute before terminal rejection: {trace:?}"
                );
                session.close();
                assert!(error.contains("caught"));
            } else {
                result.unwrap();
                session.close();
            }
        }
    }
}

#[cfg(test)]
#[path = "goose/compiler_tests.rs"]
mod lifecycle_tests;
