//! The dev server of `--watch --serve` (docs/dev-workflow.md, "The server
//! and the client"): a subset of HTTP/1.1 on `std::net`, one thread per
//! connection, that serves a dev build to the runtimes running it.
//!
//! - `GET` (and `HEAD`) serves the files of the build directory and of the
//!   source roots, which the units' source maps name, with a content type
//!   and an ETag; a source root's files are served at the path its source
//!   map URLs resolve to from the directory. `GET /` serves the directory's
//!   `index.html`, or a built-in page that loads `index.js`, calls the
//!   program's value when it is a function, and shows console output.
//! - `GET /events` is a server-sent-events stream of messages, JSON objects
//!   with an `op` field, to one runtime: `hello` with the runtime ID the
//!   server gave the connection, then the build as it is now (a `load` with
//!   `replay`), so a runtime that connects late catches up, and the
//!   diagnostics of the last step; then each step's `warnings`, `error` and
//!   `load`, and the `invoke`s sent to that runtime.
//! - `POST /reply` takes a JSON object, the reply of a runtime to an
//!   `invoke` with its request ID and its runtime ID, and hands it to the
//!   requester when that runtime was the one asked (`409 Conflict` when
//!   another was, and the request still waits).
//! - `POST /repl` is the connected REPL's (`--repl --connect`): it opens
//!   sessions, each with its namespace and chosen runtime, and evaluates
//!   their forms, compiled by the thread that owns the watch's session
//!   ([`DevServer::serve_repl`], [`crate::dev_repl`]) and run by a runtime.
//! - A connection whose first byte opens a bencoded dictionary is an nREPL
//!   client's ([`crate::dev_nrepl`]), whose sessions are those of `POST
//!   /repl`; [`DevServer::serve_nrepl`] adds a port for nREPL alone.
//!
//! A server started with [`DevServer::start_mounts`] serves several builds
//! (docs/build-configuration.md, "Several builds in one process"): each
//! mount's directory and its `events`, `reply` and `repl` endpoints under
//! `/NAME/`, and the first mount's also at `/`. Each mount has its own
//! runtimes and steps; runtime IDs are the server's. The source roots are
//! served once, at the paths every mount's source maps resolve to. REPL
//! sessions are the server's: each belongs to the endpoint that opened it
//! (nREPL's and `/repl`'s are the first mount's) and is bound to one
//! mount, whose compiler and runtimes run its forms, its home to begin with,
//! another after `(light.dev/repl :NAME)`.
//! [`DevServer::start`] serves one unnamed mount at `/` alone.
//!
//! [`DevServer::start_with`] and [`DevServer::start_mounts_with`] take a
//! [`Listen`]: with `next_free`, a port in use gives way to the next one
//! upward, as shadow-cljs's `start-http` does, and without `nrepl` the
//! server's own port answers no nREPL client.
//!
//! JSON is written by hand and read by [`Json::parse`]. The dev client,
//! `client.js` of a dev build (src/dev_client.js), interprets the messages.
//! Loopback is the default address, since a connected runtime evaluates what
//! the server sends it; while bound to loopback, the server answers only
//! requests whose `Host` is a loopback name, so that a page of another site
//! cannot reach it through a DNS name that resolves to this machine.

use crate::dev_build::DevHook;
use crate::dev_repl::{
    Command, CompiledForm, Outcome, ReplForm, ReplRequest, ReplUnit, SentUnit, edn_string,
    map_stack,
};
use crate::eval_session::PrintOptions;
use crate::source_map::json_string;
use std::collections::{HashMap, VecDeque};
use std::fmt::Write as _;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex, MutexGuard, RwLock};
use std::time::Duration;

/// One namespace's unit in a `load` message: the text of its file in the
/// build directory, which defines the namespace through
/// `globalThis.__lightDev.define`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadUnit {
    pub namespace: String,
    /// The unit's file in the build directory, which the client names as the
    /// evaluated text's `sourceURL`, so its `sourceMappingURL` resolves.
    pub file: String,
    pub js: String,
    /// The namespaces of the build it requires, along which a replay under
    /// [`Build::dependents`] reloads downstream namespaces.
    pub requires: Vec<String>,
}

/// A compile diagnostic of an `error` or `warnings` message, located when
/// the compiler knows where.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Diagnostic {
    pub message: String,
    pub file: Option<String>,
    /// One-based.
    pub line: Option<usize>,
    /// One-based.
    pub column: Option<usize>,
}

impl Diagnostic {
    /// `error` located: its message without the location the session put in
    /// front of it, and the file, line and column.
    pub fn located(error: &crate::EvalDiagnostic) -> Self {
        let Some(span) = &error.location else {
            return Self {
                message: error.message.clone(),
                ..Self::default()
            };
        };
        let file = span.source().name();
        let (line, column) = span
            .source()
            .line_column(span.bytes().start)
            .unwrap_or((1, 1));
        let prefix = format!("{file}:{line}:{column}: ");
        Self {
            message: error
                .message
                .strip_prefix(&prefix)
                .unwrap_or(&error.message)
                .to_owned(),
            file: Some(file.to_owned()),
            line: Some(line),
            column: Some(column),
        }
    }
}

/// The build as a runtime that connects now catches up with it: every
/// namespace's unit in load order, the hook functions, which every `load`
/// message carries, and the reload policy of its namespaces, with which a
/// runtime applies a replay.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Build {
    pub units: Vec<LoadUnit>,
    pub before_load: Vec<DevHook>,
    pub after_load: Vec<DevHook>,
    /// The namespaces marked `^:dev/once`: a replay loads one only into a
    /// runtime that has not run it.
    pub once: Vec<String>,
    /// The namespaces marked `^:dev/always`: a replay that loads anything
    /// loads them too.
    pub always: Vec<String>,
    /// `--reload-dependents`: a replay also loads every namespace that
    /// requires one it loads, as each step's load does.
    pub dependents: bool,
}

/// What a watch step produced, for [`DevServer::publish`].
#[derive(Clone, Debug, Default)]
pub struct Step {
    /// The units to load, in load order; none when the step failed.
    pub load: Vec<LoadUnit>,
    pub warnings: Vec<Diagnostic>,
    pub error: Option<Diagnostic>,
    /// The build after the step, when the step changed it.
    pub build: Option<Build>,
}

/// How a dev server listens ([`DevServer::start_with`]).
#[derive(Clone, Copy, Debug)]
pub struct Listen<'a> {
    pub host: &'a str,
    /// The port, 0 for any free port.
    pub port: u16,
    /// When the port is in use, try the next one upward, and so on, as
    /// shadow-cljs's `start-http` does unless `:strict`; otherwise fail.
    pub next_free: bool,
    /// Whether the server's own port answers nREPL clients.
    pub nrepl: bool,
}

impl<'a> Listen<'a> {
    /// Listen on `host` and `port` alone, answering nREPL there too, as
    /// [`DevServer::start`] does.
    pub fn new(host: &'a str, port: u16) -> Self {
        Self {
            host,
            port,
            next_free: false,
            nrepl: true,
        }
    }

    /// Bind the listener: with `next_free`, a port in use (`taken` is called
    /// with it) gives way to the next one, up to 65535.
    fn bind(&self, taken: &mut dyn FnMut(u16)) -> io::Result<TcpListener> {
        let mut port = self.port;
        loop {
            let addresses = (self.host, port).to_socket_addrs()?.collect::<Vec<_>>();
            match TcpListener::bind(&addresses[..]) {
                Err(error)
                    if self.next_free
                        && port != 0
                        && port < u16::MAX
                        && error.kind() == io::ErrorKind::AddrInUse =>
                {
                    taken(port);
                    port += 1;
                }
                bound => return bound,
            }
        }
    }
}

/// A running dev server. Its threads live as long as the process.
pub struct DevServer {
    site: Arc<Site>,
    address: SocketAddr,
}

/// A mount of a dev server: its index in the server's mount table, which is
/// dense and in the order the mounts were given.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MountId(pub u32);

/// A build for [`DevServer::start_mounts`] to serve under `/NAME/`.
#[derive(Clone, Debug, Default)]
pub struct Mount {
    /// One URL path segment: the build's id.
    pub name: String,
    /// The build directory.
    pub out_dir: PathBuf,
    /// The build as a runtime that connects first catches up with it.
    pub build: Build,
}

/// One mount of a running server, whose steps, invokes and REPL forms reach
/// only the runtimes connected to its own event stream. It may go to another
/// thread and outlive the server, after which it reaches no runtime.
#[derive(Clone)]
pub struct DevMount {
    id: MountId,
    shared: Arc<Shared>,
}

/// What the mounts of a server share: the mount table, the source roots,
/// the listeners and the REPL sessions.
pub(crate) struct Site {
    /// The mounts by [`MountId`].
    mounts: Vec<Arc<Shared>>,
    /// The names of the named mounts with their IDs, sorted by name.
    names: Vec<(String, MountId)>,
    /// Each source root with the URL path segments its files are served
    /// under, from the server's root, whichever mount's source map names
    /// them; [`DevServer::restart`] replaces them.
    roots: RwLock<Vec<(PathBuf, Vec<String>)>>,
    /// Whether to answer only requests for a loopback `Host`.
    loopback: bool,
    /// Whether the server's own port answers nREPL clients.
    nrepl: bool,
    /// Whether the server was dropped: it accepts no more connections.
    stopped: AtomicBool,
    /// The addresses of the server's further listeners, which a drop wakes.
    listeners: Mutex<Vec<SocketAddr>>,
    /// The REPL sessions of every mount. Its lock is taken before a mount's
    /// state, never after.
    sessions: Mutex<Sessions>,
}

/// The REPL sessions of a server, by number. A session's ID is its number
/// after `incarnation`, which differs for each server, so a client or a
/// runtime that outlives a server never takes another session for its own.
struct Sessions {
    table: HashMap<u64, ReplSession>,
    next: u64,
    incarnation: String,
}

/// The mount whose endpoints the server's nREPL connections and `/repl` use:
/// the first build named.
pub(crate) const FIRST_MOUNT: MountId = MountId(0);

/// A mount: its build directory, and its build, runtimes and REPL compiler.
pub(crate) struct Shared {
    /// The name the mount is served under, `/NAME/`; none for the one mount
    /// of [`DevServer::start`], served at `/` alone.
    name: Option<String>,
    /// The build directory, canonical, which [`DevServer::restart`] replaces.
    out_dir: RwLock<PathBuf>,
    /// The next runtime ID, which every mount of the server draws from, so
    /// that an ID names one runtime of the server.
    runtime_ids: Arc<AtomicU64>,
    state: Mutex<State>,
}

struct State {
    build: Build,
    /// The number of steps published, the `step` of the last one's messages.
    step: u64,
    /// The last step's `warnings` and `error` messages, which a runtime
    /// receives when it connects.
    diagnostics: Vec<String>,
    /// The connected runtimes, oldest first, with the senders of their
    /// streams.
    runtimes: Vec<(u64, Sender<String>)>,
    /// The `User-Agent` each runtime connected with.
    agents: HashMap<u64, String>,
    next_request: u64,
    /// The requests awaiting a reply, by request ID: the runtime asked and
    /// the requester's sender.
    pending: HashMap<String, (u64, Sender<Json>)>,
    /// Whether the server was dropped: the mount connects no more runtimes.
    stopped: bool,
    /// Where the forms of the REPL sessions bound to the mount go to be
    /// compiled, and the namespace a session starts in, once
    /// [`DevServer::serve_repl`] was called.
    compiler: Option<(Sender<ReplRequest>, String)>,
    /// The newest REPL units sent, whose locations error stacks map.
    sent: VecDeque<SentUnit>,
    next_unit: u64,
}

/// A connected REPL's session: the build it is bound to, its current
/// namespace, and the runtime it chose with `light.dev/select!`, if any.
struct ReplSession {
    /// The mount whose endpoint opened it, the only one that knows it: the
    /// first mount's for nREPL and `/repl`, the mount's for `/NAME/repl`.
    home: MountId,
    /// The build it is bound to, whose compiler compiles its forms and
    /// whose runtimes run them: its home's until `(light.dev/repl :ID)`
    /// binds it to another (docs/build-configuration.md, "Several builds in
    /// one process").
    build: MountId,
    namespace: String,
    /// A runtime of `build`.
    runtime: Option<u64>,
    /// Whether an evaluation of the session is running.
    evaluating: bool,
    /// Whether [`Shared::interrupt`] ended that evaluation: it runs no
    /// further form.
    interrupted: bool,
    /// The request ID of the `invoke` whose reply it waits for, if any, a
    /// request of `build`.
    pending: Option<String>,
    /// The ID of the nREPL message the running evaluation answers, if any.
    message: Option<String>,
    /// Held by an evaluation from start to end, so that a session's
    /// evaluations run one at a time whichever connection sends them.
    lock: Arc<Mutex<()>>,
    /// The session's queue of nREPL evaluations, which one worker thread,
    /// started on its first use, runs in the order they arrived from any
    /// connection. It ends when the session closes.
    queue: Option<Sender<QueuedJob>>,
}

/// A job of a session's queue: an evaluation that sends its own responses.
pub(crate) type QueuedJob = Box<dyn FnOnce() + Send>;

/// How many REPL units the server keeps for mapping error stacks.
const SENT_UNITS: usize = 256;

/// What an `invoke` message carries besides its request ID.
struct InvokeMessage<'a> {
    /// The REPL session's ID, whose `*1`, `*2`, `*3` and `*e` the runtime
    /// keeps.
    session: Option<&'a str>,
    namespace: &'a str,
    /// A script to run first: what the form loaded.
    setup: &'a str,
    /// The unit to call for the value; nil without one.
    js: Option<&'a str>,
    print: &'a PrintOptions,
}

/// How long a stream waits before writing a comment line, which finds a
/// closed connection and keeps proxies from closing an idle one.
const HEARTBEAT: Duration = Duration::from_secs(15);
/// How long a connection may take to send its request.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_HEADER_BYTES: usize = 64 * 1024;
const MAX_BODY_BYTES: usize = 16 * 1024 * 1024;

impl DevServer {
    /// Listen on `host` and `port` (0 for any free port) and serve the dev
    /// build in `out_dir`, whose units' source maps name sources under
    /// `roots`, starting from `build`: one mount, unnamed, at `/`.
    pub fn start(
        host: &str,
        port: u16,
        out_dir: &Path,
        roots: &[PathBuf],
        build: Build,
    ) -> io::Result<Self> {
        Self::start_with(Listen::new(host, port), out_dir, roots, build, &mut |_| {})
    }

    /// [`Self::start`], listening as `listen` says; `taken` is called with
    /// each port in use that gave way to the next.
    pub fn start_with(
        listen: Listen<'_>,
        out_dir: &Path,
        roots: &[PathBuf],
        build: Build,
        taken: &mut dyn FnMut(u16),
    ) -> io::Result<Self> {
        Self::listen(
            listen,
            vec![(None, out_dir.to_path_buf(), build)],
            roots,
            taken,
        )
    }

    /// Listen on `host` and `port` (0 for any free port) and serve each of
    /// `mounts`, whose units' source maps name sources under `roots`: its
    /// build directory and its event, reply and REPL endpoints under
    /// `/NAME/`, and the first mount's also at `/`. The source roots are
    /// served once, at the paths every mount's source maps resolve to. The
    /// mounts' IDs follow their order, from `MountId(0)`.
    pub fn start_mounts(
        host: &str,
        port: u16,
        mounts: Vec<Mount>,
        roots: &[PathBuf],
    ) -> io::Result<Self> {
        Self::start_mounts_with(Listen::new(host, port), mounts, roots, &mut |_| {})
    }

    /// [`Self::start_mounts`], listening as `listen` says; `taken` is called
    /// with each port in use that gave way to the next.
    pub fn start_mounts_with(
        listen: Listen<'_>,
        mounts: Vec<Mount>,
        roots: &[PathBuf],
        taken: &mut dyn FnMut(u16),
    ) -> io::Result<Self> {
        let invalid = |message: String| io::Error::new(io::ErrorKind::InvalidInput, message);
        if mounts.is_empty() {
            return Err(invalid("a dev server serves at least one mount".into()));
        }
        for (index, mount) in mounts.iter().enumerate() {
            if !plain_segment(&mount.name) {
                return Err(invalid(format!(
                    "a mount's name is one URL path segment, not {:?}",
                    mount.name
                )));
            }
            if mounts[..index].iter().any(|other| other.name == mount.name) {
                return Err(invalid(format!("two mounts are named {:?}", mount.name)));
            }
        }
        let mounts = mounts
            .into_iter()
            .map(|mount| (Some(mount.name), mount.out_dir, mount.build))
            .collect();
        Self::listen(listen, mounts, roots, taken)
    }

    /// Serve `mounts`, each a name (none for the unnamed mount), a build
    /// directory and a build.
    fn listen(
        listen: Listen<'_>,
        mounts: Vec<(Option<String>, PathBuf, Build)>,
        roots: &[PathBuf],
        taken: &mut dyn FnMut(u16),
    ) -> io::Result<Self> {
        let runtime_ids = Arc::new(AtomicU64::new(1));
        let mut table = Vec::with_capacity(mounts.len());
        for (name, out_dir, build) in mounts {
            table.push(Arc::new(Shared {
                name,
                out_dir: RwLock::new(std::fs::canonicalize(out_dir)?),
                runtime_ids: runtime_ids.clone(),
                state: Mutex::new(State {
                    build,
                    step: 0,
                    diagnostics: Vec::new(),
                    runtimes: Vec::new(),
                    agents: HashMap::new(),
                    next_request: 1,
                    pending: HashMap::new(),
                    stopped: false,
                    compiler: None,
                    sent: VecDeque::new(),
                    next_unit: 1,
                }),
            }));
        }
        let served = served_roots(roots, &table);
        let mut names = table
            .iter()
            .enumerate()
            .filter_map(|(index, mount)| Some((mount.name.clone()?, MountId(index as u32))))
            .collect::<Vec<_>>();
        names.sort_unstable();
        let listener = listen.bind(taken)?;
        let address = listener.local_addr()?;
        let site = Arc::new(Site {
            mounts: table,
            names,
            roots: RwLock::new(served),
            loopback: address.ip().is_loopback(),
            nrepl: listen.nrepl,
            stopped: AtomicBool::new(false),
            listeners: Mutex::new(Vec::new()),
            sessions: Mutex::new(Sessions {
                table: HashMap::new(),
                next: 1,
                incarnation: incarnation(),
            }),
        });
        accept(listener, &site, connection)?;
        Ok(Self { site, address })
    }

    /// Also answer nREPL on `host` and `port` (0 for any free port), as the
    /// server's own port does: the address it listens on. Its sessions
    /// start bound to the first mount.
    pub fn serve_nrepl(&self, host: &str, port: u16) -> io::Result<SocketAddr> {
        let addresses = (host, port).to_socket_addrs()?.collect::<Vec<_>>();
        let listener = TcpListener::bind(&addresses[..])?;
        let address = listener.local_addr()?;
        self.site
            .listeners
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(address);
        accept(listener, &self.site, crate::dev_nrepl::connection)?;
        Ok(address)
    }

    /// The address the server listens on.
    pub fn address(&self) -> SocketAddr {
        self.address
    }

    /// The mount table: every mount, in [`MountId`] order.
    pub fn mounts(&self) -> Vec<DevMount> {
        (0..self.site.mounts.len())
            .map(|index| self.mount(MountId(index as u32)))
            .collect()
    }

    /// The mount `id`.
    ///
    /// # Panics
    ///
    /// When the server has no mount `id`.
    pub fn mount(&self, id: MountId) -> DevMount {
        DevMount {
            id,
            shared: self.site.mounts[id.0 as usize].clone(),
        }
    }

    /// The mount served under `/NAME/`.
    pub fn mount_named(&self, name: &str) -> Option<MountId> {
        self.site.mount_named(name)
    }

    /// Mount `id`'s build started again, in a session of its own, after its
    /// settings changed (docs/build-configuration.md, "Watching the
    /// configuration file"): serve it from `out_dir`, with the source roots
    /// `roots` for every mount, make `build` the build a runtime catches up
    /// with and `warnings` the diagnostics it then receives, and end the
    /// stream of every runtime of the mount, which connects again, as after
    /// a server restart, and catches up with the new build. Its pending
    /// invokes end as a disconnect ends them; its REPL sessions stay.
    pub fn restart(
        &self,
        id: MountId,
        out_dir: &Path,
        roots: &[PathBuf],
        build: Build,
        warnings: Vec<Diagnostic>,
    ) -> io::Result<()> {
        let shared = &self.site.mounts[id.0 as usize];
        let out_dir = std::fs::canonicalize(out_dir)?;
        *shared
            .out_dir
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = out_dir;
        let served = served_roots(roots, &self.site.mounts);
        *self
            .site
            .roots
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = served;
        let mut state = shared.state();
        state.step += 1;
        state.build = build;
        state.diagnostics.clear();
        if !warnings.is_empty() {
            let message = warnings_message(state.step, &warnings);
            state.diagnostics.push(message);
        }
        for (_, sender) in state.runtimes.drain(..) {
            let _ = sender.send(String::new());
        }
        state.agents.clear();
        state.pending.clear();
        Ok(())
    }

    /// The IDs of the runtimes connected to the first mount, oldest first.
    pub fn runtimes(&self) -> Vec<u64> {
        self.site.mounts[0].runtimes()
    }

    /// [`DevMount::publish`] on the first mount.
    pub fn publish(&self, step: Step) -> u64 {
        self.site.mounts[0].publish(step)
    }

    /// [`DevMount::invoke`] on the first mount.
    pub fn invoke(
        &self,
        runtime: Option<u64>,
        unit: &crate::dev_session::Unit,
        print: &PrintOptions,
    ) -> Result<Invocation, String> {
        self.site.mounts[0].invoke(runtime, unit, print)
    }

    /// [`DevMount::serve_repl`] on the first mount.
    pub fn serve_repl(&self, namespace: &str) -> Receiver<ReplRequest> {
        self.site.mounts[0].serve_repl(namespace)
    }
}

impl DevMount {
    /// Its index in the server's mount table.
    pub fn id(&self) -> MountId {
        self.id
    }

    /// The name it is served under, `/NAME/`; none for the one mount of
    /// [`DevServer::start`].
    pub fn name(&self) -> Option<&str> {
        self.shared.name.as_deref()
    }

    /// The IDs of the runtimes connected to its event stream, oldest first.
    pub fn runtimes(&self) -> Vec<u64> {
        self.shared.runtimes()
    }

    /// Send the messages of a step to every runtime connected to it, in the
    /// order `warnings`, `error`, `load`, and keep its build and diagnostics
    /// for runtimes that connect later. Returns the step's number, which
    /// counts the mount's steps.
    pub fn publish(&self, step: Step) -> u64 {
        self.shared.publish(step)
    }

    /// Send `unit`, a REPL form's unit, to the runtime `runtime` of the
    /// mount, or to its most recently connected one, to evaluate in its
    /// namespace and reply with the value printed by `pr-str` under `print`,
    /// or with the error it threw.
    pub fn invoke(
        &self,
        runtime: Option<u64>,
        unit: &crate::dev_session::Unit,
        print: &PrintOptions,
    ) -> Result<Invocation, String> {
        self.shared.invoke(runtime, unit, print)
    }

    /// Answer its `POST /repl` (under its prefix) from now on, the connected
    /// REPL's endpoint (docs/evaluation.md, "Connected REPL"): its sessions
    /// start in `namespace`, and the forms of every session bound to the
    /// mount come out of the receiver, for the thread that owns the mount's
    /// compiler to answer with [`ReplRequest::answer`]. Until this is called, the endpoint
    /// answers `503 Service Unavailable`.
    pub fn serve_repl(&self, namespace: &str) -> Receiver<ReplRequest> {
        self.shared.serve_repl(namespace)
    }
}

impl Site {
    /// The mount served under `/NAME/`.
    fn mount_named(&self, name: &str) -> Option<MountId> {
        self.names
            .binary_search_by(|(known, _)| known.as_str().cmp(name))
            .ok()
            .map(|index| self.names[index].1)
    }

    /// The mount a request path names, and the path within it: the named
    /// mount whose name is the path's first segment, with the rest of the
    /// path (empty for `/NAME` itself), else the first mount with the whole
    /// path. The first mount's endpoints at the root, `/events`, `/reply`
    /// and `/repl`, stay its own when a mount has the same name, which is
    /// served under `/NAME/` alone.
    fn route<'p>(&self, path: &'p str) -> (MountId, &'p str) {
        if !self.names.is_empty()
            && !matches!(path, "/events" | "/reply" | "/repl")
            && let Some(rest) = path.strip_prefix('/')
        {
            let (segment, rest) = rest.find('/').map_or((rest, ""), |at| rest.split_at(at));
            if let Some(id) = percent_decode(segment).and_then(|name| self.mount_named(&name)) {
                return (id, rest);
            }
        }
        (FIRST_MOUNT, path)
    }

    /// The mount `id`.
    pub(crate) fn mount_of(&self, id: MountId) -> &Arc<Shared> {
        &self.mounts[id.0 as usize]
    }

    fn sessions(&self) -> MutexGuard<'_, Sessions> {
        self.sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// The namespace REPL sessions that `home`'s endpoint opens start in,
    /// once [`DevServer::serve_repl`] was called on it.
    pub(crate) fn start_namespace(&self, home: MountId) -> Option<String> {
        self.mount_of(home).start_namespace()
    }

    /// Open a REPL session at `home`'s endpoint: with the build, namespace
    /// and runtime choice of session `from` when it is one, else bound to
    /// `home` in the namespace its sessions start in. Its number, ID and
    /// namespace; none until [`DevServer::serve_repl`] was called on `home`.
    pub(crate) fn open_session(
        &self,
        home: MountId,
        from: Option<u64>,
    ) -> Option<(u64, String, String)> {
        let mut sessions = self.sessions();
        let start = self.start_namespace(home)?;
        let (build, namespace, runtime) = match from.and_then(|from| sessions.table.get(&from)) {
            Some(from) => (from.build, from.namespace.clone(), from.runtime),
            None => (home, start, None),
        };
        let number = sessions.next;
        sessions.next += 1;
        sessions.table.insert(
            number,
            ReplSession {
                home,
                build,
                namespace: namespace.clone(),
                runtime,
                evaluating: false,
                interrupted: false,
                pending: None,
                message: None,
                lock: Arc::default(),
                queue: None,
            },
        );
        let id = format!("{}-{number}", sessions.incarnation);
        Some((number, id, namespace))
    }

    /// The number of the open session of `home`'s endpoint whose ID is
    /// `id`.
    pub(crate) fn session(&self, home: MountId, id: &str) -> Option<u64> {
        let sessions = self.sessions();
        let number = id
            .strip_prefix(sessions.incarnation.as_str())?
            .strip_prefix('-')?
            .parse::<u64>()
            .ok()?;
        let entry = sessions.table.get(&number)?;
        (entry.home == home).then_some(number)
    }

    /// The IDs of the open sessions of `home`'s endpoint, oldest first.
    pub(crate) fn session_ids(&self, home: MountId) -> Vec<String> {
        let sessions = self.sessions();
        let mut numbers = sessions
            .table
            .iter()
            .filter(|(_, entry)| entry.home == home)
            .map(|(number, _)| *number)
            .collect::<Vec<_>>();
        numbers.sort_unstable();
        numbers
            .into_iter()
            .map(|number| format!("{}-{number}", sessions.incarnation))
            .collect()
    }

    /// The current namespace of session `session`.
    pub(crate) fn session_namespace(&self, session: u64) -> Option<String> {
        let sessions = self.sessions();
        sessions.table.get(&session).map(|s| s.namespace.clone())
    }

    /// Make `namespace` the current namespace of session `session`.
    fn set_session_namespace(&self, session: u64, namespace: &str) {
        if let Some(entry) = self.sessions().table.get_mut(&session) {
            entry.namespace = namespace.to_owned();
        }
    }

    /// Where the forms of session `session`, or of a session `home`'s
    /// endpoint would open, go to be compiled: its build's compiler, once
    /// [`DevServer::serve_repl`] was called on that build.
    pub(crate) fn compiler(
        &self,
        home: MountId,
        session: Option<u64>,
    ) -> Option<Sender<ReplRequest>> {
        let build = session
            .and_then(|session| self.sessions().table.get(&session).map(|s| s.build))
            .unwrap_or(home);
        self.mount_of(build).compiler()
    }

    /// Close session `session`, which the runtimes of every build forget
    /// too, since it may have run in each: whether it was open.
    pub(crate) fn close_session(&self, session: u64) -> bool {
        let id = {
            let mut sessions = self.sessions();
            if sessions.table.remove(&session).is_none() {
                return false;
            }
            format!("{}-{session}", sessions.incarnation)
        };
        let message = format!("{{\"op\":\"forget\",\"session\":\"{id}\"}}");
        for mount in &self.mounts {
            mount.state().broadcast(&message);
        }
        true
    }

    /// End the running evaluation of session `session`, when `message` is
    /// none or the ID of the nREPL message it answers: it stops waiting for
    /// the reply of the runtime, which keeps running what it was sent, and
    /// runs no further form.
    pub(crate) fn interrupt(&self, session: u64, message: Option<&str>) -> Interrupt {
        let mut sessions = self.sessions();
        let Some(entry) = sessions.table.get_mut(&session) else {
            return Interrupt::Idle;
        };
        if !entry.evaluating {
            return Interrupt::Idle;
        }
        if message.is_some_and(|message| entry.message.as_deref() != Some(message)) {
            return Interrupt::Mismatch;
        }
        entry.interrupted = true;
        if let Some(id) = entry.pending.take()
            && let Some((_, sender)) = self.mount_of(entry.build).state().pending.remove(&id)
        {
            let _ = sender.send(Json::Object(vec![("interrupted".into(), Json::Bool(true))]));
        }
        Interrupt::Interrupted
    }

    /// Queue `job` on session `session`, to run on the session's worker
    /// thread after every job queued there before it, from any connection:
    /// whether the session was open to take it.
    pub(crate) fn queue(&self, session: u64, job: QueuedJob) -> bool {
        let mut sessions = self.sessions();
        let Some(entry) = sessions.table.get_mut(&session) else {
            return false;
        };
        let queue = match &mut entry.queue {
            Some(queue) => queue,
            empty => {
                let (sender, jobs) = channel::<QueuedJob>();
                let worker = std::thread::Builder::new()
                    .name("light-cljs REPL session".into())
                    .spawn(move || {
                        // A job that panics leaves the queue to the next.
                        for job in jobs {
                            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job));
                        }
                    });
                if worker.is_err() {
                    return false;
                }
                empty.insert(sender)
            }
        };
        queue.send(job).is_ok()
    }
}

impl Shared {
    fn runtimes(&self) -> Vec<u64> {
        self.state().runtimes.iter().map(|(id, _)| *id).collect()
    }

    fn publish(&self, step: Step) -> u64 {
        let mut state = self.state();
        state.step += 1;
        let number = state.step;
        let mut diagnostics = Vec::new();
        if !step.warnings.is_empty() {
            diagnostics.push(warnings_message(number, &step.warnings));
        }
        if let Some(error) = &step.error {
            let mut message = format!("{{\"op\":\"error\",\"step\":{number},\"error\":");
            write_diagnostic(&mut message, error);
            message.push('}');
            diagnostics.push(message);
        }
        if let Some(build) = step.build {
            state.build = build;
        }
        let mut messages = diagnostics.clone();
        if step.error.is_none() && !step.load.is_empty() {
            messages.push(load_message(number, false, &step.load, &state.build));
        }
        state.diagnostics = diagnostics;
        for message in &messages {
            state.broadcast(message);
        }
        number
    }

    fn invoke(
        &self,
        runtime: Option<u64>,
        unit: &crate::dev_session::Unit,
        print: &PrintOptions,
    ) -> Result<Invocation, String> {
        let mut state = self.state();
        let target = state.target(runtime)?;
        state.send_invoke(
            target,
            &InvokeMessage {
                session: None,
                namespace: &unit.namespace,
                setup: "",
                js: Some(&unit.js),
                print,
            },
        )
    }

    fn serve_repl(&self, namespace: &str) -> Receiver<ReplRequest> {
        let (sender, receiver) = channel();
        self.state().compiler = Some((sender, namespace.to_owned()));
        receiver
    }
}

/// An `invoke` sent to a runtime.
pub struct Invocation {
    /// The runtime that evaluates it.
    pub runtime: u64,
    /// The request ID its reply names.
    pub id: String,
    /// The reply, the JSON object the runtime posted; its sender is dropped
    /// if the runtime disconnects or the server stops first.
    pub reply: Receiver<Json>,
}

impl Drop for DevServer {
    /// Stop listening and end every runtime's stream, so that a client
    /// connects again (to a server started on the port later, say).
    fn drop(&mut self) {
        self.site.stopped.store(true, Ordering::SeqCst);
        for mount in &self.site.mounts {
            let mut state = mount.state();
            state.stopped = true;
            for (_, sender) in state.runtimes.drain(..) {
                let _ = sender.send(String::new());
            }
            state.pending.clear();
        }
        let mut listeners = std::mem::take(
            &mut *self
                .site
                .listeners
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
        );
        // Wake the accepting threads, which see `stopped` and drop their
        // listeners.
        listeners.push(self.address);
        for mut wake in listeners {
            if wake.ip().is_unspecified() {
                wake.set_ip(match wake {
                    SocketAddr::V4(_) => std::net::Ipv4Addr::LOCALHOST.into(),
                    SocketAddr::V6(_) => std::net::Ipv6Addr::LOCALHOST.into(),
                });
            }
            let _ = TcpStream::connect_timeout(&wake, Duration::from_secs(1));
        }
    }
}

/// Accept the connections of `listener` on a thread, each served by `serve`
/// on a thread of its own, until the server stops.
fn accept(
    listener: TcpListener,
    site: &Arc<Site>,
    serve: fn(TcpStream, &Arc<Site>) -> io::Result<()>,
) -> io::Result<()> {
    let accepting = site.clone();
    std::thread::Builder::new()
        .name("light-cljs dev server".into())
        .spawn(move || {
            for stream in listener.incoming() {
                if accepting.stopped.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(stream) = stream else { continue };
                let site = accepting.clone();
                let _ = std::thread::Builder::new()
                    .name("light-cljs dev connection".into())
                    .spawn(move || {
                        let _ = serve(stream, &site);
                    });
            }
        })?;
    Ok(())
}

impl Shared {
    /// The build directory, canonical.
    fn out_dir(&self) -> PathBuf {
        self.out_dir
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Where REPL forms go to be compiled, once [`DevServer::serve_repl`]
    /// was called.
    pub(crate) fn compiler(&self) -> Option<Sender<ReplRequest>> {
        self.state()
            .compiler
            .as_ref()
            .map(|(sender, _)| sender.clone())
    }

    /// The namespace REPL sessions start in, once [`DevServer::serve_repl`]
    /// was called.
    pub(crate) fn start_namespace(&self) -> Option<String> {
        self.state()
            .compiler
            .as_ref()
            .map(|(_, namespace)| namespace.clone())
    }
}

/// What [`Site::interrupt`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Interrupt {
    Interrupted,
    /// No evaluation of the session is running.
    Idle,
    /// The running evaluation answers another message.
    Mismatch,
}

impl State {
    /// Send `message` to every connected runtime, forgetting those whose
    /// streams have ended.
    fn broadcast(&mut self, message: &str) {
        let event = event(message);
        self.runtimes
            .retain(|(_, sender)| sender.send(event.clone()).is_ok());
    }

    /// The runtime `runtime`, or the most recently connected one.
    fn target(&self, runtime: Option<u64>) -> Result<(u64, Sender<String>), String> {
        let target = match runtime {
            Some(id) => self.runtimes.iter().find(|(known, _)| *known == id),
            None => self.runtimes.last(),
        };
        target.cloned().ok_or_else(|| match runtime {
            Some(id) => format!("no runtime {id} is connected"),
            None => "no runtime is connected".to_owned(),
        })
    }

    /// Send an `invoke` to `target` and register its request.
    fn send_invoke(
        &mut self,
        (runtime, sender): (u64, Sender<String>),
        invoke: &InvokeMessage,
    ) -> Result<Invocation, String> {
        let id = self.next_request.to_string();
        self.next_request += 1;
        let mut message = String::from("{\"op\":\"invoke\",\"id\":");
        json_string(&mut message, &id);
        if let Some(session) = invoke.session {
            write!(message, ",\"session\":\"{session}\"").unwrap();
        }
        message.push_str(",\"namespace\":");
        json_string(&mut message, invoke.namespace);
        if !invoke.setup.is_empty() {
            message.push_str(",\"setup\":");
            json_string(&mut message, invoke.setup);
        }
        message.push_str(",\"js\":");
        match invoke.js {
            Some(js) => json_string(&mut message, js),
            None => message.push_str("null"),
        }
        let print = invoke.print;
        let optional = |value: Option<usize>| value.map_or("null".to_owned(), |n| n.to_string());
        write!(
            message,
            ",\"print\":{{\"readably\":{},\"length\":{},\"level\":{},\"meta\":{},\"namespace-maps\":{},\"dup\":{}}}}}",
            print.readably,
            optional(print.length),
            optional(print.level),
            print.meta,
            print.namespace_maps,
            print.dup
        )
        .unwrap();
        let (reply, receiver) = channel();
        if sender.send(event(&message)).is_err() {
            return Err(format!("runtime {runtime} disconnected"));
        }
        self.pending.insert(id.clone(), (runtime, reply));
        Ok(Invocation {
            runtime,
            id,
            reply: receiver,
        })
    }
}

/// `message` as a server-sent event. JSON text holds no line break.
fn event(message: &str) -> String {
    format!("data: {message}\n\n")
}

fn write_diagnostic(out: &mut String, diagnostic: &Diagnostic) {
    out.push_str("{\"message\":");
    json_string(out, &diagnostic.message);
    out.push_str(",\"file\":");
    match &diagnostic.file {
        Some(file) => json_string(out, file),
        None => out.push_str("null"),
    }
    for (key, value) in [("line", diagnostic.line), ("column", diagnostic.column)] {
        match value {
            Some(value) => write!(out, ",\"{key}\":{value}").unwrap(),
            None => write!(out, ",\"{key}\":null").unwrap(),
        }
    }
    out.push('}');
}

/// A `load` message: define `units` in order between the hooks of `build`,
/// which also gives the reload policy a replay is applied with.
fn load_message(step: u64, replay: bool, units: &[LoadUnit], build: &Build) -> String {
    let mut message = format!("{{\"op\":\"load\",\"step\":{step},\"replay\":{replay},\"units\":[");
    for (index, unit) in units.iter().enumerate() {
        if index > 0 {
            message.push(',');
        }
        message.push_str("{\"namespace\":");
        json_string(&mut message, &unit.namespace);
        message.push_str(",\"file\":");
        json_string(&mut message, &unit.file);
        message.push_str(",\"js\":");
        json_string(&mut message, &unit.js);
        message.push_str(",\"requires\":");
        json_names(&mut message, &unit.requires);
        message.push('}');
    }
    message.push(']');
    for (key, hooks) in [
        ("before-load", &build.before_load),
        ("after-load", &build.after_load),
    ] {
        write!(message, ",\"{key}\":[").unwrap();
        for (index, hook) in hooks.iter().enumerate() {
            if index > 0 {
                message.push(',');
            }
            message.push_str("{\"name\":");
            json_string(&mut message, &hook.name);
            write!(message, ",\"async\":{}}}", hook.asynchronous).unwrap();
        }
        message.push(']');
    }
    for (key, namespaces) in [("once", &build.once), ("always", &build.always)] {
        write!(message, ",\"{key}\":").unwrap();
        json_names(&mut message, namespaces);
    }
    write!(message, ",\"dependents\":{}}}", build.dependents).unwrap();
    message
}

/// Append `names` as a JSON array of strings.
fn json_names(out: &mut String, names: &[String]) {
    out.push('[');
    for (index, name) in names.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        json_string(out, name);
    }
    out.push(']');
}

/// The `warnings` message of step `step`.
fn warnings_message(step: u64, warnings: &[Diagnostic]) -> String {
    let mut message = format!("{{\"op\":\"warnings\",\"step\":{step},\"warnings\":[");
    for (index, warning) in warnings.iter().enumerate() {
        if index > 0 {
            message.push(',');
        }
        write_diagnostic(&mut message, warning);
    }
    message.push_str("]}");
    message
}

/// Each of `roots` with the URL path segments its files are served under: a
/// root's files are served where a source map of any of `mounts` names them,
/// which differs between build directories at different depths.
fn served_roots(roots: &[PathBuf], mounts: &[Arc<Shared>]) -> Vec<(PathBuf, Vec<String>)> {
    let mut served = Vec::new();
    for root in roots {
        let Ok(root) = std::fs::canonicalize(root) else {
            continue;
        };
        for mount in mounts {
            if let Some(prefix) = served_prefix(&root, &mount.out_dir()) {
                let entry = (root.clone(), prefix);
                if !served.contains(&entry) {
                    served.push(entry);
                }
            }
        }
    }
    served
}

/// The URL path segments under which the files of `root` are served: where
/// a source map in `out_dir` names them, by a URL relative to it
/// (`../src/app/view.cljs`), which resolves against the server's root by
/// dropping the leading `..` segments (`/src/app/view.cljs`). None for a
/// root that shares no ancestor with `out_dir` (another drive).
fn served_prefix(root: &Path, out_dir: &Path) -> Option<Vec<String>> {
    let parts = root.components().collect::<Vec<_>>();
    let base = out_dir.components().collect::<Vec<_>>();
    let common = parts.iter().zip(&base).take_while(|(a, b)| a == b).count();
    if common == 0 {
        return None;
    }
    parts[common..]
        .iter()
        .map(|part| part.as_os_str().to_str().map(str::to_owned))
        .collect()
}

/// A request: its method, its path (percent-decoded segments), its headers
/// with lowercase names, and its body.
struct Request {
    method: String,
    target: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }
}

/// Read one request from `reader`; None when the connection closed first.
fn read_request(reader: &mut BufReader<TcpStream>) -> io::Result<Option<Request>> {
    let mut head = Vec::new();
    loop {
        let mut line = Vec::new();
        let read = reader
            .by_ref()
            .take((MAX_HEADER_BYTES + 1 - head.len().min(MAX_HEADER_BYTES)) as u64)
            .read_until(b'\n', &mut line)?;
        if read == 0 {
            if head.is_empty() {
                return Ok(None);
            }
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "request ended",
            ));
        }
        head.extend_from_slice(&line);
        if head.len() > MAX_HEADER_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "request head too long",
            ));
        }
        if line == b"\r\n" || line == b"\n" {
            if head.len() == line.len() {
                // A blank line before the request line is ignored.
                head.clear();
                continue;
            }
            break;
        }
    }
    let head = String::from_utf8(head)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "request head is not UTF-8"))?;
    let mut lines = head.lines();
    let invalid = || io::Error::new(io::ErrorKind::InvalidData, "malformed request");
    let mut first = lines.next().ok_or_else(invalid)?.split(' ');
    let (Some(method), Some(target), Some(version), None) =
        (first.next(), first.next(), first.next(), first.next())
    else {
        return Err(invalid());
    };
    if !version.starts_with("HTTP/1.") {
        return Err(invalid());
    }
    let mut headers = Vec::new();
    for line in lines {
        if line.is_empty() {
            break;
        }
        let (name, value) = line.split_once(':').ok_or_else(invalid)?;
        headers.push((name.trim().to_ascii_lowercase(), value.trim().to_owned()));
    }
    let mut request = Request {
        method: method.to_owned(),
        target: target.to_owned(),
        headers,
        body: Vec::new(),
    };
    if request.header("transfer-encoding").is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "chunked request bodies are not supported",
        ));
    }
    if let Some(length) = request.header("content-length") {
        let length = length.parse::<usize>().map_err(|_| invalid())?;
        if length > MAX_BODY_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "request body too large",
            ));
        }
        request.body = vec![0; length];
        reader.read_exact(&mut request.body)?;
    }
    Ok(Some(request))
}

/// Write a complete response and close the connection.
fn respond(
    stream: &mut TcpStream,
    status: &str,
    headers: &[(&str, &str)],
    body: &[u8],
    head_only: bool,
) -> io::Result<()> {
    let mut head = format!("HTTP/1.1 {status}\r\n");
    for (name, value) in headers {
        write!(head, "{name}: {value}\r\n").unwrap();
    }
    if !status.starts_with("204") && !status.starts_with("304") {
        write!(head, "Content-Length: {}\r\n", body.len()).unwrap();
    }
    head.push_str("Connection: close\r\n\r\n");
    stream.write_all(head.as_bytes())?;
    if !head_only && !status.starts_with("204") && !status.starts_with("304") {
        stream.write_all(body)?;
    }
    stream.flush()
}

fn respond_text(stream: &mut TcpStream, status: &str, text: &str) -> io::Result<()> {
    respond(
        stream,
        status,
        &[("Content-Type", "text/plain; charset=utf-8")],
        format!("{text}\n").as_bytes(),
        false,
    )
}

/// Serve one connection: one request, then close, except that an event
/// stream stays open until the runtime goes away.
fn connection(stream: TcpStream, site: &Arc<Site>) -> io::Result<()> {
    stream.set_read_timeout(Some(REQUEST_TIMEOUT))?;
    // A peer that stops reading (a frozen tab) ends its connection rather
    // than holding the thread that writes to it.
    stream.set_write_timeout(Some(REQUEST_TIMEOUT))?;
    // A response or an event is written in parts; none waits for the
    // acknowledgement of the one before (a REPL round trip is milliseconds).
    stream.set_nodelay(true)?;
    // An nREPL client's first message is a bencoded dictionary, `d...`,
    // where an HTTP request begins with its method. A page cannot open such
    // a connection: a method a page may send without a preflight is a word
    // in capitals, and the server answers no preflight.
    let mut first = [0; 1];
    if stream.peek(&mut first)? == 1 && first[0] == b'd' {
        // Without nREPL (`:nrepl false`), the connection closes unanswered.
        if !site.nrepl {
            return Ok(());
        }
        return crate::dev_nrepl::connection(stream, site);
    }
    let mut writer = stream.try_clone()?;
    let mut reader = BufReader::new(stream);
    let request = match read_request(&mut reader) {
        Ok(Some(request)) => request,
        Ok(None) => return Ok(()),
        Err(error) => return respond_text(&mut writer, "400 Bad Request", &error.to_string()),
    };
    if site.loopback && !request.header("host").is_some_and(loopback_host) {
        return respond_text(
            &mut writer,
            "403 Forbidden",
            "the dev server answers only requests for a loopback host",
        );
    }
    let path = request
        .target
        .split(['?', '#'])
        .next()
        .unwrap_or_default()
        .to_owned();
    let head_only = request.method == "HEAD";
    // A mount's endpoints and files are under its prefix; the first mount's
    // are also at the root.
    let (mount, within) = site.route(&path);
    let shared = site.mount_of(mount);
    match (request.method.as_str(), within) {
        ("GET" | "HEAD", "") if !path.is_empty() => {
            // `/NAME` is the directory `/NAME/`, against which its page's
            // relative URLs resolve.
            let location = format!("{path}/{}", &request.target[path.len()..]);
            respond(
                &mut writer,
                "301 Moved Permanently",
                &[
                    ("Location", location.as_str()),
                    ("Content-Type", "text/plain; charset=utf-8"),
                ],
                format!("{location}\n").as_bytes(),
                head_only,
            )
        }
        ("GET", "/events") => events(writer, shared, &request),
        ("POST", "/reply") => reply(&mut writer, shared, &request),
        ("POST", "/repl") => repl(&mut writer, site, mount, &request),
        ("GET" | "HEAD", _) => file(
            &mut writer,
            site,
            shared,
            &request,
            &path,
            within,
            head_only,
        ),
        (_, "/reply") => respond_text(&mut writer, "405 Method Not Allowed", "POST a reply"),
        (_, "/repl") => respond_text(&mut writer, "405 Method Not Allowed", "POST a REPL request"),
        _ => respond_text(
            &mut writer,
            "405 Method Not Allowed",
            "the dev server answers GET, HEAD, POST /reply and POST /repl",
        ),
    }
}

/// Whether `request` comes from a page of another origin than the server's
/// own, which a browser names in its `Origin` header.
fn foreign_origin(request: &Request) -> bool {
    request.header("origin").is_some_and(|origin| {
        request.header("host").is_none_or(|host| {
            origin != format!("http://{host}") && origin != format!("https://{host}")
        })
    })
}

/// Whether the `Host` header names this machine by a loopback name.
fn loopback_host(host: &str) -> bool {
    let name = if let Some(rest) = host.strip_prefix('[') {
        // [::1]:port
        rest.split(']').next().unwrap_or_default()
    } else {
        host.rsplit_once(':').map_or(host, |(name, _)| name)
    };
    let name = name.to_ascii_lowercase();
    name == "localhost"
        || name.ends_with(".localhost")
        || name
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

/// `GET /events`: the runtime's stream.
fn events(mut stream: TcpStream, shared: &Shared, request: &Request) -> io::Result<()> {
    let (sender, receiver) = channel();
    let (runtime, first) = {
        let mut state = shared.state();
        if state.stopped {
            drop(state);
            return respond_text(&mut stream, "503 Service Unavailable", "the server stopped");
        }
        let runtime = shared.runtime_ids.fetch_add(1, Ordering::Relaxed);
        let mut first = event(&format!("{{\"op\":\"hello\",\"runtime\":\"{runtime}\"}}"));
        if !state.build.units.is_empty() {
            first.push_str(&event(&load_message(
                state.step,
                true,
                &state.build.units,
                &state.build,
            )));
        }
        for message in &state.diagnostics {
            first.push_str(&event(message));
        }
        state.runtimes.push((runtime, sender.clone()));
        let agent = request.header("user-agent").unwrap_or_default().to_owned();
        state.agents.insert(runtime, agent);
        (runtime, first)
    };
    // The runtime sends nothing on the stream: a read that ends finds that
    // it went away, and wakes the writer with an empty message.
    let mut closed = stream.try_clone()?;
    closed.set_read_timeout(None)?;
    std::thread::Builder::new()
        .name("light-cljs dev stream".into())
        .spawn(move || {
            let mut buffer = [0; 256];
            while matches!(closed.read(&mut buffer), Ok(read) if read > 0) {}
            let _ = sender.send(String::new());
        })?;
    let result = (|| -> io::Result<()> {
        stream.write_all(
            b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n",
        )?;
        stream.write_all(first.as_bytes())?;
        stream.flush()?;
        loop {
            match receiver.recv_timeout(HEARTBEAT) {
                Ok(message) if message.is_empty() => return Ok(()),
                Ok(message) => stream.write_all(message.as_bytes())?,
                Err(RecvTimeoutError::Timeout) => stream.write_all(b": heartbeat\n\n")?,
                Err(RecvTimeoutError::Disconnected) => return Ok(()),
            }
            stream.flush()?;
        }
    })();
    let mut state = shared.state();
    state.runtimes.retain(|(id, _)| *id != runtime);
    state.agents.remove(&runtime);
    state.pending.retain(|_, (asked, _)| *asked != runtime);
    drop(state);
    let _ = stream.shutdown(std::net::Shutdown::Both);
    result
}

/// `POST /reply`: hand a runtime's reply to its requester.
fn reply(stream: &mut TcpStream, shared: &Shared, request: &Request) -> io::Result<()> {
    // A page of another site cannot forge a REPL's result.
    if foreign_origin(request) {
        return respond_text(
            stream,
            "403 Forbidden",
            "a reply comes from a page of the dev server",
        );
    }
    let json = std::str::from_utf8(&request.body)
        .map_err(|_| "the reply is not UTF-8".to_owned())
        .and_then(Json::parse);
    let json = match json {
        Ok(json @ Json::Object(_)) => json,
        Ok(_) => return respond_text(stream, "400 Bad Request", "a reply is a JSON object"),
        Err(error) => return respond_text(stream, "400 Bad Request", &error),
    };
    let Some(id) = json.get("id").and_then(Json::as_str).map(str::to_owned) else {
        return respond_text(
            stream,
            "400 Bad Request",
            "a reply needs its request's \"id\"",
        );
    };
    let Some(runtime) = json
        .get("runtime")
        .and_then(Json::as_str)
        .map(str::to_owned)
    else {
        return respond_text(
            stream,
            "400 Bad Request",
            "a reply needs the \"runtime\" that sends it",
        );
    };
    // Only the runtime asked consumes the request.
    let mut state = shared.state();
    let pending = match state.pending.get(&id) {
        Some((asked, _)) if asked.to_string() == runtime => state.pending.remove(&id),
        Some((asked, _)) => {
            let message = format!("request {id} was sent to runtime {asked}");
            drop(state);
            return respond_text(stream, "409 Conflict", &message);
        }
        None => None,
    };
    drop(state);
    match pending {
        Some((_, sender)) => {
            let _ = sender.send(json);
            respond(stream, "204 No Content", &[], &[], false)
        }
        None => respond_text(
            stream,
            "404 Not Found",
            &format!("no request {id} awaits a reply"),
        ),
    }
}

/// What sets this server's REPL session IDs apart from another's: the time
/// it started, the process and the servers it started before, in
/// hexadecimal.
fn incarnation() -> String {
    static SERVERS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    let server = SERVERS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("{nanos:x}{:x}{server:x}", std::process::id())
}

fn respond_json(stream: &mut TcpStream, status: &str, json: &str) -> io::Result<()> {
    respond(
        stream,
        status,
        &[("Content-Type", "application/json; charset=utf-8")],
        json.as_bytes(),
        false,
    )
}

/// `POST /repl`: a connected REPL's request, a JSON object with an `op`:
///
/// - `open`: start a session; `{"session", "namespace"}`.
/// - `eval`: evaluate `code`, the text of forms that start at `line` and
///   `column` of `file`, in the session's namespace on its runtime, printed
///   under `print`; `{"namespace", "results"}`, a result per form; a form that does
///   not read ends them.
/// - `close`: forget the session.
///
/// A page cannot send one: a request with an `Origin` or another content
/// type than JSON (which a page cannot send to another origin without the
/// server's consent) is refused.
fn repl(stream: &mut TcpStream, site: &Site, home: MountId, request: &Request) -> io::Result<()> {
    if request.header("origin").is_some() {
        return respond_text(
            stream,
            "403 Forbidden",
            "the REPL endpoint answers REPL clients, not pages",
        );
    }
    if !request
        .header("content-type")
        .is_some_and(|kind| kind.starts_with("application/json"))
    {
        return respond_text(
            stream,
            "415 Unsupported Media Type",
            "a REPL request is a JSON object sent as application/json",
        );
    }
    let json = match std::str::from_utf8(&request.body)
        .map_err(|_| "the request is not UTF-8".to_owned())
        .and_then(Json::parse)
    {
        Ok(json @ Json::Object(_)) => json,
        Ok(_) => return respond_text(stream, "400 Bad Request", "a REPL request is a JSON object"),
        Err(error) => return respond_text(stream, "400 Bad Request", &error),
    };
    let session = json
        .get("session")
        .and_then(Json::as_str)
        .and_then(|id| site.session(home, id));
    match json.get("op").and_then(Json::as_str) {
        Some("open") => {
            let Some((_, id, namespace)) = site.open_session(home, None) else {
                return respond_text(
                    stream,
                    "503 Service Unavailable",
                    "this dev server compiles no REPL forms",
                );
            };
            let mut body = String::from("{\"session\":");
            json_string(&mut body, &id);
            body.push_str(",\"namespace\":");
            json_string(&mut body, &namespace);
            body.push('}');
            respond_json(stream, "200 OK", &body)
        }
        Some("close") => {
            // The runtimes forget the session's history too.
            if let Some(id) = session {
                site.close_session(id);
            }
            respond(stream, "204 No Content", &[], &[], false)
        }
        Some("eval") => {
            let Some(session) = session else {
                return respond_text(
                    stream,
                    "404 Not Found",
                    "no such REPL session; open one first",
                );
            };
            let Some(code) = json.get("code").and_then(Json::as_str) else {
                return respond_text(stream, "400 Bad Request", "eval needs its \"code\"");
            };
            let position = |key: &str| match json.get(key) {
                Some(Json::Number(n)) if *n >= 1.0 && *n <= 1e9 => Some(*n as usize),
                _ => None,
            };
            let text = placed(code, position("line"), position("column"));
            let file = json
                .get("file")
                .and_then(Json::as_str)
                .unwrap_or("<repl>")
                .to_owned();
            let print = print_options(json.get("print"));
            let mut results = Vec::new();
            let each = &mut |result: &FormResult, _: &str| results.push(result_json(result));
            let evaluation = Evaluation {
                file: &file,
                text: &text,
                print: &print,
                namespace: None,
                restore: false,
                message: None,
            };
            match evaluate(site, session, &evaluation, each) {
                Ok(evaluated) => {
                    let mut body = String::from("{\"namespace\":");
                    json_string(&mut body, &evaluated.namespace);
                    body.push_str(",\"results\":[");
                    body.push_str(&results.join(","));
                    body.push_str("]}");
                    respond_json(stream, "200 OK", &body)
                }
                Err(message) => respond_text(stream, "503 Service Unavailable", &message),
            }
        }
        _ => respond_text(
            stream,
            "400 Bad Request",
            "a REPL request's \"op\" is open, eval or close",
        ),
    }
}

/// `code` placed at one-based `line` and `column`, so that the compiler
/// locates its forms there: at the first line and column when one is not
/// given or is beyond a million lines or ten thousand columns.
pub(crate) fn placed(code: &str, line: Option<usize>, column: Option<usize>) -> String {
    let at = |value: Option<usize>, limit: usize| value.filter(|&n| n >= 1 && n <= limit);
    let mut text = "\n".repeat(at(line, 1_000_000).unwrap_or(1) - 1);
    text.push_str(&" ".repeat(at(column, 10_000).unwrap_or(1) - 1));
    text.push_str(code);
    text
}

/// The print options of a request's `print` object, `PrintOptions` defaults
/// for what it does not give.
fn print_options(json: Option<&Json>) -> PrintOptions {
    let mut print = PrintOptions::default();
    let Some(json) = json else { return print };
    let flag = |key: &str, default: bool| match json.get(key) {
        Some(Json::Bool(value)) => *value,
        _ => default,
    };
    let bound = |key: &str, default: Option<usize>| match json.get(key) {
        Some(Json::Number(n)) if *n >= 0.0 && n.fract() == 0.0 => Some(*n as usize),
        Some(Json::Null) => None,
        _ => default,
    };
    print.readably = flag("readably", print.readably);
    print.meta = flag("meta", print.meta);
    print.namespace_maps = flag("namespace-maps", print.namespace_maps);
    print.dup = flag("dup", print.dup);
    print.length = bound("length", print.length);
    print.level = bound("level", print.level);
    print
}

/// What a REPL form came to.
pub(crate) struct FormResult {
    pub(crate) warnings: Vec<Diagnostic>,
    /// The value as the runtime printed it, with its reference number when
    /// it is not finite data, or the error.
    pub(crate) outcome: Result<(String, Option<u64>), FormError>,
}

/// Why a REPL form has no value.
pub(crate) struct FormError {
    /// `compile`, `command`, `runtime` or `interrupted`.
    pub(crate) phase: &'static str,
    pub(crate) diagnostic: Diagnostic,
    /// A runtime error's stack, mapped to the sources, and its data, printed.
    pub(crate) stack: Option<String>,
    pub(crate) data: Option<String>,
}

/// How an evaluation ended.
pub(crate) struct Evaluated {
    /// The session's namespace after it.
    pub(crate) namespace: String,
    /// Whether [`Shared::interrupt`] ended it before its last form.
    pub(crate) interrupted: bool,
}

/// An evaluation for [`evaluate`].
pub(crate) struct Evaluation<'a> {
    /// The forms' text, read as the source `file`.
    pub(crate) file: &'a str,
    pub(crate) text: &'a str,
    pub(crate) print: &'a PrintOptions,
    /// The namespace to make the session's first, if any.
    pub(crate) namespace: Option<&'a str>,
    /// Whether to leave the session's namespace as it was before.
    pub(crate) restore: bool,
    /// The ID of the nREPL message it answers, which an interrupt may name.
    pub(crate) message: Option<&'a str>,
}

/// Evaluate the forms of `evaluation` one at a time for REPL session
/// `session`, after the session's earlier evaluations: each is compiled on
/// the thread of the build the session is bound to, in the session's
/// namespace, then runs on the session's runtime of that build, before the
/// next is read, and `each` receives its result and the namespace after it.
/// A form `(light.dev/repl :ID)` binds the session to another build for the
/// forms after it. How it ended, or why the server could not answer.
pub(crate) fn evaluate(
    site: &Site,
    session: u64,
    evaluation: &Evaluation,
    each: &mut dyn FnMut(&FormResult, &str),
) -> Result<Evaluated, String> {
    let Some(lock) = site
        .sessions()
        .table
        .get(&session)
        .map(|entry| entry.lock.clone())
    else {
        return Err(format!("REPL session {session} was closed"));
    };
    let _turn = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let Evaluation {
        file, text, print, ..
    } = *evaluation;
    let mut restore = None;
    let mut started = false;
    let result = (|| {
        let mut cursor = 0;
        loop {
            let (build, namespace) = {
                let mut sessions = site.sessions();
                let entry = sessions.table.get_mut(&session);
                if started && entry.as_ref().is_some_and(|entry| entry.interrupted) {
                    return Ok(true);
                }
                let Some(entry) = entry else {
                    return Err(format!("REPL session {session} was closed"));
                };
                if !started {
                    // An interrupt from before the evaluation ends nothing.
                    (entry.evaluating, entry.interrupted) = (true, false);
                    entry.message = evaluation.message.map(str::to_owned);
                    if evaluation.restore {
                        restore = Some(entry.namespace.clone());
                    }
                    if let Some(namespace) = evaluation.namespace {
                        entry.namespace = namespace.to_owned();
                    }
                }
                (entry.build, entry.namespace.clone())
            };
            started = true;
            let asked = site
                .mount_of(build)
                .compiler()
                .ok_or_else(|| "this dev server compiles no REPL forms".to_owned())
                .and_then(|compiler| {
                    let (respond, compiled) = channel();
                    let request = ReplRequest::Compile {
                        namespace: namespace.clone(),
                        file: file.to_owned(),
                        text: text.to_owned(),
                        cursor,
                        respond,
                    };
                    let stopped = || "the compiler stopped".to_owned();
                    compiler.send(request).map_err(|_| stopped())?;
                    compiled.recv().map_err(|_| stopped())
                });
            let form = match asked {
                Ok(ReplForm::End) => return Ok(false),
                Ok(ReplForm::Form(form)) => form,
                // A build that cannot compile (its restart failed, say)
                // still lets the session switch to another.
                Err(why) => repl_command_at(file, text, cursor, &namespace).ok_or(why)?,
            };
            cursor = form.cursor;
            site.set_session_namespace(session, &form.namespace);
            // A form that fails leaves the next to run, as under `--repl`;
            // one that does not read moves the cursor to the end.
            let result = run(site, build, session, &form, file, text, print);
            if matches!(&result.outcome, Err(error) if error.phase == "interrupted") {
                return Ok(true);
            }
            // `light.dev/repl` moved the session to its build's namespace.
            let namespace = site
                .session_namespace(session)
                .unwrap_or_else(|| form.namespace.clone());
            each(&result, &namespace);
        }
    })();
    let mut sessions = site.sessions();
    let entry = sessions.table.get_mut(&session);
    let namespace = entry.map(|entry| {
        if started {
            (entry.evaluating, entry.interrupted, entry.pending) = (false, false, None);
            entry.message = None;
        }
        if let Some(namespace) = restore.take() {
            entry.namespace = namespace;
        }
        entry.namespace.clone()
    });
    result.map(|interrupted| Evaluated {
        namespace: namespace.unwrap_or_default(),
        interrupted,
    })
}

/// The form at `cursor` of `text`, read by the server alone, when it is a
/// `(light.dev/repl :ID)` command, as the compiler would have compiled it in
/// `namespace`: for a session whose build has no compiler to answer.
fn repl_command_at(
    file: &str,
    text: &str,
    cursor: usize,
    namespace: &str,
) -> Option<Box<CompiledForm>> {
    let rest = text.get(cursor..)?;
    let source = crate::compiler_data::Source::new(file, rest);
    let view = crate::compiler_reader::NamespaceView::new(namespace, &source);
    let form = crate::compiler_reader::read_form(&source, &view).ok()??;
    let Some(Ok(command @ Command::Repl(_))) = crate::dev_repl::command(&form) else {
        return None;
    };
    let bytes = form.span().bytes();
    let start = cursor + bytes.start;
    let (line, column) = crate::LineIndex::new(text).line_column(start)?;
    Some(Box::new(CompiledForm {
        namespace: namespace.to_owned(),
        cursor: cursor + bytes.end,
        start,
        line,
        column,
        warnings: Vec::new(),
        outcome: Outcome::Command(command),
    }))
}

/// A form's result as `POST /repl` answers it: `{"value"}` with a `ref` for
/// a value that is not finite data, or `{"error"}`, with its `warnings`.
fn result_json(form: &FormResult) -> String {
    let mut result = String::from("{\"warnings\":[");
    for (index, warning) in form.warnings.iter().enumerate() {
        if index > 0 {
            result.push(',');
        }
        write_diagnostic(&mut result, warning);
    }
    result.push(']');
    match &form.outcome {
        Ok((value, reference)) => {
            result.push_str(",\"value\":");
            json_string(&mut result, value);
            if let Some(reference) = reference {
                write!(result, ",\"ref\":{reference}").unwrap();
            }
        }
        Err(error) => {
            write!(
                result,
                ",\"error\":{{\"phase\":\"{}\",\"diagnostic\":",
                error.phase
            )
            .unwrap();
            write_diagnostic(&mut result, &error.diagnostic);
            for (key, value) in [("stack", &error.stack), ("data", &error.data)] {
                if let Some(value) = value {
                    write!(result, ",\"{key}\":").unwrap();
                    json_string(&mut result, value);
                }
            }
            result.push('}');
        }
    }
    result.push('}');
    result
}

/// Run one compiled REPL form of session `session`, which build `build`
/// compiled.
fn run(
    site: &Site,
    build: MountId,
    session: u64,
    form: &CompiledForm,
    file: &str,
    text: &str,
    print: &PrintOptions,
) -> FormResult {
    let location = (Some(file.to_owned()), Some(form.line), Some(form.column));
    let outcome = match &form.outcome {
        Outcome::Error { error, loads } => {
            if !loads.is_empty() {
                let mut message = String::from("{\"op\":\"run\",\"js\":");
                json_string(&mut message, loads);
                message.push('}');
                site.mount_of(build).state().broadcast(&message);
            }
            Err(("compile", error.clone(), None, None))
        }
        Outcome::Command(command) => command_value(site, session, command)
            .map(|value| (value, None))
            .map_err(|message| ("command", Diagnostic::at(message, &location), None, None)),
        Outcome::Run {
            setup, unit: None, ..
        } if setup.is_empty() => Ok(("nil".to_owned(), None)),
        Outcome::Run { setup, loads, unit } => invoke_form(
            site,
            build,
            session,
            form,
            setup,
            loads,
            unit.as_ref(),
            file,
            text,
            print,
        )
        .map_err(|(phase, message, stack, data)| {
            (phase, Diagnostic::at(message, &location), stack, data)
        }),
    };
    FormResult {
        warnings: form.warnings.clone(),
        outcome: outcome.map_err(|(phase, diagnostic, stack, data)| FormError {
            phase,
            diagnostic,
            stack,
            data,
        }),
    }
}

impl Diagnostic {
    fn at(
        message: String,
        (file, line, column): &(Option<String>, Option<usize>, Option<usize>),
    ) -> Self {
        Self {
            message,
            file: file.clone(),
            line: *line,
            column: *column,
        }
    }
}

/// The value of a REPL command, printed, or why it failed.
fn command_value(site: &Site, session: u64, command: &Command) -> Result<String, String> {
    let mut sessions = site.sessions();
    let Some(entry) = sessions.table.get_mut(&session) else {
        return Err(format!("REPL session {session} was closed"));
    };
    let build = entry.build;
    let bound = site.mount_of(build);
    // The build of a named mount, as the watch names it.
    let named = |mount: &Shared| mount.name.as_deref().map(|name| format!(":{name}"));
    match command {
        Command::Runtimes => {
            let current = entry
                .runtime
                .or_else(|| bound.state().runtimes.last().map(|(id, _)| *id));
            drop(sessions);
            // Every build's runtimes, by ID, which is their order of
            // connection; each named with its build when the watch names its
            // builds.
            let mut runtimes = Vec::new();
            for mount in &site.mounts {
                let state = mount.state();
                for (id, _) in &state.runtimes {
                    let agent = state.agents.get(id).cloned().unwrap_or_default();
                    runtimes.push((*id, named(mount), agent));
                }
            }
            runtimes.sort_unstable_by_key(|(id, _, _)| *id);
            let mut value = String::from("[");
            for (index, (id, build, agent)) in runtimes.iter().enumerate() {
                if index > 0 {
                    value.push(' ');
                }
                write!(value, "{{:id {id}, ").unwrap();
                if let Some(build) = build {
                    write!(value, ":build {build}, ").unwrap();
                }
                value.push_str(":agent ");
                edn_string(&mut value, agent);
                write!(value, ", :selected {}}}", current == Some(*id)).unwrap();
            }
            value.push(']');
            Ok(value)
        }
        Command::Select(runtime) => {
            let unknown = runtime.and_then(|id| {
                let target = bound.state().target(Some(id));
                target.err().map(|message| (id, message))
            });
            if let Some((id, message)) = unknown {
                let id = &id;
                // A runtime of another build is that build's to choose.
                let other = site
                    .mounts
                    .iter()
                    .find(|mount| mount.state().runtimes.iter().any(|(known, _)| known == id));
                return Err(match (other.and_then(|mount| named(mount)), named(bound)) {
                    (Some(other), Some(current)) => format!(
                        "runtime {id} runs {other}, and the session is bound to {current}; (light.dev/repl {other}) binds it to {other}"
                    ),
                    _ => format!("{message}; (light.dev/runtimes) lists those that are"),
                });
            }
            entry.runtime = *runtime;
            Ok(runtime.map_or("nil".to_owned(), |id| id.to_string()))
        }
        Command::Repl(name) => {
            // The ids of the builds the watch runs, in the order named.
            let ids = site
                .mounts
                .iter()
                .filter_map(|mount| named(mount))
                .collect::<Vec<_>>();
            if ids.is_empty() {
                return Err(
                    "the watch runs no named builds: light.dev/repl switches between the builds of light-cljs watch ID ..."
                        .to_owned(),
                );
            }
            let Some(id) = site.mount_named(name) else {
                return Err(format!(
                    "the watch runs no build :{name}; its builds are {}",
                    ids.join(", ")
                ));
            };
            // As shadow-cljs's `repl` starts the build's REPL afresh: in the
            // namespace its sessions start in, on its most recently
            // connected runtime.
            let mount = site.mount_of(id);
            entry.build = id;
            entry.runtime = None;
            if let Some(namespace) = mount.start_namespace() {
                entry.namespace = namespace;
            }
            Ok(format!("[:selected :{name}]"))
        }
    }
}

/// Why a REPL form sent to a runtime has no value: the phase, `runtime` or
/// `interrupted`, the error's message, its stack mapped to the sources, and
/// its data, printed.
type Thrown = (&'static str, String, Option<String>, Option<String>);

/// Send a compiled form to the session's runtime, its loads to the others,
/// and wait for its reply: the value printed and its reference number, or
/// the error's message, its stack mapped to the sources, and its data. An
/// interrupt of the session ends the wait.
#[allow(clippy::too_many_arguments)]
fn invoke_form(
    site: &Site,
    build: MountId,
    session: u64,
    form: &CompiledForm,
    setup: &str,
    loads: &str,
    unit: Option<&ReplUnit>,
    file: &str,
    text: &str,
    print: &PrintOptions,
) -> Result<(String, Option<u64>), Thrown> {
    let interrupted = || {
        (
            "interrupted",
            "the evaluation was interrupted".to_owned(),
            None,
            None,
        )
    };
    let shared = site.mount_of(build);
    let invocation = {
        // The session, then the mount: as an interrupt takes them.
        let mut sessions = site.sessions();
        let id = format!("{}-{session}", sessions.incarnation);
        if sessions.table.get(&session).is_some_and(|s| s.interrupted) {
            return Err(interrupted());
        }
        let selected = sessions.table.get(&session).and_then(|s| s.runtime);
        let mut state = shared.state();
        let target = state.target(selected).map_err(|message| {
            let message = match selected {
                Some(id) => format!(
                    "runtime {id}, which (light.dev/select! {id}) chose, is not connected; (light.dev/runtimes) lists those that are, and (light.dev/select! nil) chooses the most recently connected"
                ),
                None => message,
            };
            ("runtime", message, None, None)
        })?;
        if !loads.is_empty() {
            let mut message = String::from("{\"op\":\"run\",\"js\":");
            json_string(&mut message, loads);
            message.push('}');
            let event = event(&message);
            for (id, sender) in &state.runtimes {
                if *id != target.0 {
                    let _ = sender.send(event.clone());
                }
            }
        }
        let js = unit.map(|unit| {
            let url = format!("light-cljs-repl-{}.js", state.next_unit);
            state.next_unit += 1;
            if let Some(mapping) = &unit.mapping {
                if state.sent.len() == SENT_UNITS {
                    state.sent.pop_front();
                }
                state.sent.push_back(SentUnit {
                    url: url.clone(),
                    mapping: mapping.clone(),
                    file: file.to_owned(),
                    text: text.to_owned(),
                    start: form.start,
                });
            }
            format!("{}\n//# sourceURL={url}\n", unit.js)
        });
        let namespace = unit.map_or(form.namespace.as_str(), |unit| unit.namespace.as_str());
        let invocation = state
            .send_invoke(
                target,
                &InvokeMessage {
                    session: Some(&id),
                    namespace,
                    setup,
                    js: js.as_deref(),
                    print,
                },
            )
            .map_err(|message| ("runtime", message, None, None))?;
        if let Some(entry) = sessions.table.get_mut(&session) {
            entry.pending = Some(invocation.id.clone());
        }
        invocation
    };
    let runtime = invocation.runtime;
    let reply = invocation.reply.recv();
    if let Some(entry) = site.sessions().table.get_mut(&session) {
        entry.pending = None;
    }
    let reply = reply.map_err(|_| {
        (
            "runtime",
            format!("runtime {runtime} disconnected before it replied"),
            None,
            None,
        )
    })?;
    if reply.get("interrupted").is_some() {
        return Err(interrupted());
    }
    if let Some(error) = reply.get("error") {
        let text = |key: &str| error.get(key).and_then(Json::as_str).map(str::to_owned);
        let message = text("message").unwrap_or_else(|| "the form threw".to_owned());
        let stack = text("stack").map(|stack| {
            let mut state = shared.state();
            map_stack(&stack, state.sent.make_contiguous(), &shared.out_dir())
        });
        return Err(("runtime", message, stack, text("data")));
    }
    let value = reply
        .get("value")
        .and_then(Json::as_str)
        .unwrap_or("nil")
        .to_owned();
    let reference = match reply.get("ref") {
        Some(Json::Number(n)) if *n >= 1.0 => Some(*n as u64),
        _ => None,
    };
    Ok((value, reference))
}

/// `GET` of a file of a mount's build directory or of a source root, or of
/// a mount's `/`: `path` is the request's, `within` the part of it within
/// the mount.
fn file(
    stream: &mut TcpStream,
    site: &Site,
    shared: &Shared,
    request: &Request,
    path: &str,
    within: &str,
    head_only: bool,
) -> io::Result<()> {
    let found = if within == "/" || within == "/index.html" {
        let page = shared.out_dir().join("index.html");
        if page.is_file() {
            Some(page)
        } else {
            return send(
                stream,
                request,
                "text/html; charset=utf-8",
                INDEX_PAGE.as_bytes(),
                head_only,
            );
        }
    } else {
        resolve(site, shared, path, within)
    };
    let Some(found) = found else {
        return respond_text(stream, "404 Not Found", &format!("no file {path}"));
    };
    match std::fs::read(&found) {
        Ok(bytes) => send(stream, request, content_type(&found), &bytes, head_only),
        Err(_) => respond_text(stream, "404 Not Found", &format!("no file {path}")),
    }
}

/// Send `bytes`, or `304 Not Modified` when the request's `If-None-Match`
/// names their ETag.
fn send(
    stream: &mut TcpStream,
    request: &Request,
    content_type: &str,
    bytes: &[u8],
    head_only: bool,
) -> io::Result<()> {
    let etag = format!("\"{:016x}\"", fnv1a(bytes));
    let matches = request.header("if-none-match").is_some_and(|tags| {
        tags.split(',').any(|tag| {
            let tag = tag.trim();
            tag == "*" || tag.strip_prefix("W/").unwrap_or(tag) == etag
        })
    });
    let headers = [
        ("Content-Type", content_type),
        ("ETag", etag.as_str()),
        ("Cache-Control", "no-cache"),
    ];
    if matches {
        respond(stream, "304 Not Modified", &headers, &[], head_only)
    } else {
        respond(stream, "200 OK", &headers, bytes, head_only)
    }
}

fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// The file a request path names: `within`, the part of `path` within the
/// mount, under the mount's build directory, or else the whole path under a
/// source root at its served prefix, where every mount's source maps name
/// it. A path with an empty, `.` or `..` segment, or a segment that is not a
/// plain file name, names nothing.
fn resolve(site: &Site, shared: &Shared, path: &str, within: &str) -> Option<PathBuf> {
    let join = |base: &Path, segments: &[String]| {
        let mut path = base.to_path_buf();
        path.extend(segments);
        path.is_file().then_some(path)
    };
    join(&shared.out_dir(), &segments(within)?).or_else(|| {
        let segments = segments(path)?;
        let roots = site
            .roots
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        roots.iter().find_map(|(root, prefix)| {
            segments
                .strip_prefix(prefix.as_slice())
                .filter(|rest| !rest.is_empty())
                .and_then(|rest| join(root, rest))
        })
    })
}

/// The percent-decoded segments of a request path, when every one is a
/// plain file name.
fn segments(path: &str) -> Option<Vec<String>> {
    let segments = path
        .strip_prefix('/')?
        .split('/')
        .map(percent_decode)
        .collect::<Option<Vec<_>>>()?;
    segments
        .iter()
        .all(|segment| plain_segment(segment))
        .then_some(segments)
}

/// Whether `segment` names a file in a directory: not empty, `.` or `..`,
/// and without a separator, a drive's colon or a NUL.
fn plain_segment(segment: &str) -> bool {
    !(segment.is_empty()
        || segment == "."
        || segment == ".."
        || segment.contains(['/', '\\', '\0', ':']))
}

fn percent_decode(segment: &str) -> Option<String> {
    let bytes = segment.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = segment.get(index + 1..index + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(out).ok()
}

fn content_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("js" | "mjs" | "cjs") => "text/javascript; charset=utf-8",
        Some("json" | "map") => "application/json; charset=utf-8",
        Some("html" | "htm") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("cljs" | "cljc" | "clj" | "edn" | "txt" | "md") => "text/plain; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("ico") => "image/x-icon",
        Some("wasm") => "application/wasm",
        Some("woff2") => "font/woff2",
        _ => "application/octet-stream",
    }
}

/// The page `GET /` serves when the build directory has no `index.html`:
/// it shows console output, loads `index.js`, and calls the program's value
/// when it is a function, as a host calls an application's start function.
/// The dev client draws its overlay over it.
const INDEX_PAGE: &str = r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>light-cljs dev</title>
<style>
  body { margin: 0; font: 14px/1.4 ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; background: #fafafa; color: #222; }
  header { padding: 8px 16px; background: #222; color: #eee; }
  #light-cljs-console { margin: 0; padding: 8px 16px; white-space: pre-wrap; }
  #light-cljs-console .warn { color: #8a6100; }
  #light-cljs-console .error { color: #b00020; }
</style>
</head>
<body>
<header>light-cljs dev build: console</header>
<pre id="light-cljs-console"></pre>
<script>
  (() => {
    const out = document.getElementById("light-cljs-console");
    const text = value => {
      if (typeof value === "string") return value;
      if (value instanceof Error) return value.stack || String(value);
      try { return String(value); } catch { return Object.prototype.toString.call(value); }
    };
    for (const level of ["log", "info", "warn", "error", "debug"]) {
      const original = console[level].bind(console);
      console[level] = (...values) => {
        original(...values);
        const line = document.createElement("div");
        line.className = level;
        line.textContent = values.map(text).join(" ");
        out.append(line);
      };
    }
  })();
</script>
<script src="index.js"></script>
<script>
  globalThis.__lightDevLoaded.then(
    value => { if (typeof value === "function") value(); },
    error => console.error(error));
</script>
</body>
</html>
"#;

/// A JSON value, as [`Json::parse`] reads it.
#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Json>),
    /// Members in their order in the text.
    Object(Vec<(String, Json)>),
}

/// How deeply arrays and objects may nest, so that a hostile body cannot
/// exhaust the stack.
const MAX_JSON_DEPTH: usize = 512;

impl Json {
    /// Read `text`, one JSON value with only whitespace around it.
    pub fn parse(text: &str) -> Result<Json, String> {
        let mut parser = JsonParser {
            text: text.as_bytes(),
            at: 0,
        };
        let value = parser.value(0)?;
        parser.whitespace();
        if parser.at != parser.text.len() {
            return Err(parser.error("unexpected text after the value"));
        }
        Ok(value)
    }

    /// The member `key` of an object.
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Object(members) => members
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::String(text) => Some(text),
            _ => None,
        }
    }

    /// Append the value as JSON text.
    pub fn write(&self, out: &mut String) {
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(value) => write!(out, "{value}").unwrap(),
            Json::Number(value) if !value.is_finite() => out.push_str("null"),
            Json::Number(value) => write!(out, "{value}").unwrap(),
            Json::String(text) => json_string(out, text),
            Json::Array(items) => {
                out.push('[');
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    item.write(out);
                }
                out.push(']');
            }
            Json::Object(members) => {
                out.push('{');
                for (index, (key, value)) in members.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    json_string(out, key);
                    out.push(':');
                    value.write(out);
                }
                out.push('}');
            }
        }
    }
}

struct JsonParser<'a> {
    text: &'a [u8],
    at: usize,
}

impl JsonParser<'_> {
    fn error(&self, message: &str) -> String {
        format!("invalid JSON at byte {}: {message}", self.at)
    }

    fn whitespace(&mut self) {
        while let Some(b' ' | b'\t' | b'\n' | b'\r') = self.text.get(self.at) {
            self.at += 1;
        }
    }

    fn literal(&mut self, word: &str, value: Json) -> Result<Json, String> {
        if self.text[self.at..].starts_with(word.as_bytes()) {
            self.at += word.len();
            Ok(value)
        } else {
            Err(self.error("unexpected character"))
        }
    }

    fn value(&mut self, depth: usize) -> Result<Json, String> {
        if depth > MAX_JSON_DEPTH {
            return Err(self.error("nested too deeply"));
        }
        self.whitespace();
        match self.text.get(self.at) {
            None => Err(self.error("unexpected end")),
            Some(b'n') => self.literal("null", Json::Null),
            Some(b't') => self.literal("true", Json::Bool(true)),
            Some(b'f') => self.literal("false", Json::Bool(false)),
            Some(b'"') => self.string().map(Json::String),
            Some(b'[') => {
                self.at += 1;
                let mut items = Vec::new();
                self.whitespace();
                if self.text.get(self.at) == Some(&b']') {
                    self.at += 1;
                    return Ok(Json::Array(items));
                }
                loop {
                    items.push(self.value(depth + 1)?);
                    self.whitespace();
                    match self.text.get(self.at) {
                        Some(b',') => self.at += 1,
                        Some(b']') => {
                            self.at += 1;
                            return Ok(Json::Array(items));
                        }
                        _ => return Err(self.error("expected , or ]")),
                    }
                }
            }
            Some(b'{') => {
                self.at += 1;
                let mut members = Vec::new();
                self.whitespace();
                if self.text.get(self.at) == Some(&b'}') {
                    self.at += 1;
                    return Ok(Json::Object(members));
                }
                loop {
                    self.whitespace();
                    if self.text.get(self.at) != Some(&b'"') {
                        return Err(self.error("expected a member name"));
                    }
                    let key = self.string()?;
                    self.whitespace();
                    if self.text.get(self.at) != Some(&b':') {
                        return Err(self.error("expected :"));
                    }
                    self.at += 1;
                    members.push((key, self.value(depth + 1)?));
                    self.whitespace();
                    match self.text.get(self.at) {
                        Some(b',') => self.at += 1,
                        Some(b'}') => {
                            self.at += 1;
                            return Ok(Json::Object(members));
                        }
                        _ => return Err(self.error("expected , or }")),
                    }
                }
            }
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(_) => Err(self.error("unexpected character")),
        }
    }

    fn number(&mut self) -> Result<Json, String> {
        let start = self.at;
        let digits = |parser: &mut Self| {
            let begun = parser.at;
            while parser.text.get(parser.at).is_some_and(u8::is_ascii_digit) {
                parser.at += 1;
            }
            parser.at > begun
        };
        if self.text.get(self.at) == Some(&b'-') {
            self.at += 1;
        }
        if self.text.get(self.at) == Some(&b'0') {
            self.at += 1;
        } else if !digits(self) {
            return Err(self.error("expected a digit"));
        }
        if self.text.get(self.at) == Some(&b'.') {
            self.at += 1;
            if !digits(self) {
                return Err(self.error("expected a digit"));
            }
        }
        if let Some(b'e' | b'E') = self.text.get(self.at) {
            self.at += 1;
            if let Some(b'+' | b'-') = self.text.get(self.at) {
                self.at += 1;
            }
            if !digits(self) {
                return Err(self.error("expected a digit"));
            }
        }
        let text = std::str::from_utf8(&self.text[start..self.at]).unwrap();
        text.parse()
            .map(Json::Number)
            .map_err(|_| self.error("invalid number"))
    }

    fn hex4(&mut self) -> Result<u16, String> {
        let hex = self
            .text
            .get(self.at..self.at + 4)
            .and_then(|hex| std::str::from_utf8(hex).ok())
            .and_then(|hex| u16::from_str_radix(hex, 16).ok())
            .ok_or_else(|| self.error("expected four hex digits"))?;
        self.at += 4;
        Ok(hex)
    }

    fn string(&mut self) -> Result<String, String> {
        self.at += 1;
        let mut out = String::new();
        loop {
            let start = self.at;
            while let Some(&byte) = self.text.get(self.at) {
                if byte == b'"' || byte == b'\\' || byte < 0x20 {
                    break;
                }
                self.at += 1;
            }
            // The text is a `str`, and the run stops at ASCII bytes only.
            out.push_str(std::str::from_utf8(&self.text[start..self.at]).unwrap());
            match self.text.get(self.at) {
                None => return Err(self.error("unterminated string")),
                Some(b'"') => {
                    self.at += 1;
                    return Ok(out);
                }
                Some(b'\\') => {
                    self.at += 1;
                    let escape = *self
                        .text
                        .get(self.at)
                        .ok_or_else(|| self.error("unterminated string"))?;
                    self.at += 1;
                    match escape {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let unit = self.hex4()?;
                            let code = if (0xd800..0xdc00).contains(&unit)
                                && self.text[self.at..].starts_with(b"\\u")
                            {
                                let at = self.at;
                                self.at += 2;
                                let low = self.hex4()?;
                                if (0xdc00..0xe000).contains(&low) {
                                    0x10000
                                        + ((u32::from(unit) - 0xd800) << 10)
                                        + (u32::from(low) - 0xdc00)
                                } else {
                                    // A lone high surrogate; the next escape
                                    // is read on its own.
                                    self.at = at;
                                    0xfffd
                                }
                            } else {
                                u32::from(unit)
                            };
                            // A lone surrogate, which a JavaScript string
                            // may hold, becomes U+FFFD.
                            out.push(char::from_u32(code).unwrap_or('\u{fffd}'));
                        }
                        _ => return Err(self.error("invalid escape")),
                    }
                }
                Some(_) => return Err(self.error("control character in a string")),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_reads_what_javascript_writes() {
        let text = r#" {"op":"reply","id":"7","value":"[1 \"two\" \\ \/]","n":-1.5e3,
            "list":[true,false,null,[],{}],"text":"\u00e9\ud83d\ude00\ud800x\u2028"} "#;
        let json = Json::parse(text).unwrap();
        assert_eq!(json.get("id").and_then(Json::as_str), Some("7"));
        assert_eq!(
            json.get("value").and_then(Json::as_str),
            Some(r#"[1 "two" \ /]"#)
        );
        assert_eq!(json.get("n"), Some(&Json::Number(-1500.0)));
        assert_eq!(
            json.get("text").and_then(Json::as_str),
            Some("\u{e9}\u{1f600}\u{fffd}x\u{2028}")
        );
        let mut written = String::new();
        json.write(&mut written);
        assert_eq!(Json::parse(&written).unwrap(), json);
        for bad in [
            "",
            "{",
            "[1,]",
            "{\"a\" 1}",
            "01",
            "1.",
            "\"\u{1}\"",
            "nul",
            "1 2",
            "\"\\x\"",
        ] {
            assert!(Json::parse(bad).is_err(), "{bad:?}");
        }
        assert!(Json::parse(&"[".repeat(10_000)).is_err());
    }

    #[test]
    fn source_roots_are_served_where_source_maps_name_them() {
        let prefix = |root: &str, out: &str| served_prefix(Path::new(root), Path::new(out));
        assert_eq!(
            prefix("/work/app/src", "/work/app/dev"),
            Some(vec!["src".to_owned()])
        );
        assert_eq!(prefix("/work", "/work/dev"), Some(Vec::new()));
        assert_eq!(
            prefix("/other/lib", "/work/dev"),
            Some(vec!["other".to_owned(), "lib".to_owned()])
        );
        assert!(loopback_host("localhost:9630"));
        assert!(loopback_host("127.0.0.1:9630"));
        assert!(loopback_host("[::1]:9630"));
        assert!(loopback_host("app.localhost"));
        assert!(!loopback_host("example.com:9630"));
        assert!(!loopback_host("192.168.1.2:9630"));
        assert_eq!(percent_decode("a%20b"), Some("a b".to_owned()));
        assert_eq!(percent_decode("a%2"), None);
    }
}
