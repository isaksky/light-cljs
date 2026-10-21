//! `light-cljs watch features` (docs/build-configuration.md, "Commands" and
//! "Top-level keys"), run as a subprocess on a temporary copy of
//! examples/features, with its working directory and `LIGHT_CLJS` set
//! explicitly, for the watch, server and nREPL suites.

use crate::temp_dir::TempDir;
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    sync::mpsc::{Receiver, RecvTimeoutError, channel},
    time::Duration,
};

const FEATURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/features");
/// Far above a build's or a step's time, so a loaded machine does not fail
/// a test.
pub const TIMEOUT: Duration = Duration::from_secs(60);

/// A temporary copy of examples/features, without any build of it, and its
/// canonical path, which is how the child names it.
pub fn features(label: &str) -> (TempDir, PathBuf) {
    project(label, Path::new(FEATURES))
}

/// A temporary copy of the project `source`, as [`features`] copies
/// examples/features.
pub fn project(label: &str, source: &Path) -> (TempDir, PathBuf) {
    let dir = TempDir::new(label);
    let project = fs::canonicalize(&*dir).unwrap().join("project");
    copy_tree(source, &project);
    (dir, project)
}

/// Every file under `dir` with its bytes, by path relative to `dir`.
pub fn tree(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut files = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(next) = pending.pop() {
        for entry in fs::read_dir(&next).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else {
                files.push((
                    path.strip_prefix(dir).unwrap().to_path_buf(),
                    fs::read(&path).unwrap(),
                ));
            }
        }
    }
    files.sort();
    files
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            if entry.file_name() != "out" && entry.file_name() != ".light-cljs" {
                copy_tree(&entry.path(), &target);
            }
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// A listener on a free port of 127.0.0.1, which the test holds so that the
/// port is in use, and the port. The port is well below 65535 and the next
/// one was free when it was picked, so a server that passes over the held
/// port to the next free one upward finds one even when other processes
/// take some of the ports after it (light-cljs-412: a server that passed
/// over a held port found 65535 held by another process, and no port after).
pub fn held_port() -> (TcpListener, u16) {
    loop {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        if port < 65000 && TcpListener::bind(("127.0.0.1", port + 1)).is_ok() {
            return (listener, port);
        }
    }
}

/// The error binding `127.0.0.1:port`, which the test holds, gives: what a
/// server that binds it reports.
pub fn bind_error(port: u16) -> String {
    TcpListener::bind(("127.0.0.1", port))
        .err()
        .unwrap_or_else(|| panic!("port {port} is free"))
        .to_string()
}

/// A `light-cljs ARGS` process, killed when dropped, and its stderr lines.
pub struct Light {
    pub child: Child,
    pub lines: Receiver<String>,
}

impl Light {
    /// `light-cljs ARGS` in `dir`, with `LIGHT_CLJS` set to `light_cljs`
    /// or unset.
    pub fn spawn(dir: &Path, args: &[&str], light_cljs: Option<&str>) -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_light-cljs"));
        command
            .args(args)
            .current_dir(dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        match light_cljs {
            Some(text) => command.env("LIGHT_CLJS", text),
            None => command.env_remove("LIGHT_CLJS"),
        };
        let mut child = command.spawn().unwrap();
        let stderr = child.stderr.take().unwrap();
        let (send, lines) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines() {
                let Ok(line) = line else { break };
                if send.send(line).is_err() {
                    break;
                }
            }
        });
        Self { child, lines }
    }

    /// The stderr lines up to the first one that starts with `prefix`, and
    /// that line.
    pub fn until(&mut self, prefix: &str) -> (Vec<String>, String) {
        let mut before = Vec::new();
        loop {
            match self.lines.recv_timeout(TIMEOUT) {
                Ok(line) if line.starts_with(prefix) => return (before, line),
                Ok(line) => before.push(line),
                Err(RecvTimeoutError::Timeout) => {
                    panic!("no line {prefix:?} in {TIMEOUT:?}; got {before:#?}")
                }
                Err(RecvTimeoutError::Disconnected) => {
                    panic!("the process exited before a line {prefix:?}; got {before:#?}")
                }
            }
        }
    }

    /// The address of `light-cljs: serving DIR at http://ADDRESS/`, the
    /// lines before it and that line.
    pub fn serving(&mut self) -> (String, Vec<String>, String) {
        let (before, line) = self.until("light-cljs: serving ");
        let address = line
            .rsplit_once(" at http://")
            .and_then(|(_, url)| url.strip_suffix('/'))
            .unwrap_or_else(|| panic!("no address in {line:?}"))
            .to_owned();
        (address, before, line)
    }

    /// The stderr lines until the process exits, and how it exited.
    pub fn exit(&mut self) -> (Vec<String>, ExitStatus) {
        let mut lines = Vec::new();
        loop {
            match self.lines.recv_timeout(TIMEOUT) {
                Ok(line) => lines.push(line),
                Err(RecvTimeoutError::Timeout) => panic!("still running; got {lines:#?}"),
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
        (lines, self.child.wait().unwrap())
    }

    /// End the process with SIGINT, as a developer's Ctrl-C does, and wait
    /// for it.
    #[cfg(unix)]
    pub fn interrupt(&mut self) -> ExitStatus {
        let pid = self.child.id().to_string();
        let status = Command::new("kill").args(["-INT", &pid]).status().unwrap();
        assert!(status.success());
        self.child.wait().unwrap()
    }
}

impl Drop for Light {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// `GET path` from the server at `address`: its status line and body.
pub fn get(address: &str, path: &str) -> (String, String) {
    let mut stream = TcpStream::connect(address).unwrap();
    stream.set_read_timeout(Some(TIMEOUT)).unwrap();
    write!(stream, "GET {path} HTTP/1.1\r\nHost: {address}\r\n\r\n").unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    let (head, body) = response.split_once("\r\n\r\n").unwrap();
    (head.lines().next().unwrap().to_owned(), body.to_owned())
}

/// What an nREPL client that sends `describe` to `address` reads before the
/// server answers it in full or closes the connection: the bencoded
/// response, or nothing.
pub fn nrepl_describe(address: &str) -> Vec<u8> {
    let mut stream = TcpStream::connect(address).unwrap();
    stream.set_read_timeout(Some(TIMEOUT)).unwrap();
    stream.write_all(b"d2:op8:describe2:id1:1e").unwrap();
    let mut response = Vec::new();
    let mut buffer = [0; 4096];
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => {
                response.extend_from_slice(&buffer[..count]);
                // The response ends with its `done` status.
                if response.windows(6).any(|window| window == b"4:done") {
                    break;
                }
            }
            // A server that closes the connection unread resets it.
            Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => break,
            Err(error) => panic!("{error}"),
        }
    }
    response
}

/// The `load` a runtime that connects to the event stream at `path` of
/// `address` receives first, the build as it is, as its raw JSON.
pub fn replay(address: &str, path: &str) -> String {
    let mut stream = TcpStream::connect(address).unwrap();
    stream.set_read_timeout(Some(TIMEOUT)).unwrap();
    write!(stream, "GET {path} HTTP/1.1\r\nHost: {address}\r\n\r\n").unwrap();
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    loop {
        line.clear();
        assert!(reader.read_line(&mut line).unwrap() > 0, "{path} ended");
        if let Some(data) = line.strip_prefix("data: {\"op\":\"load\"") {
            return format!("{{\"op\":\"load\"{}", data.trim_end());
        }
    }
}

/// Replace the file at `path` with `text` at once, as an editor saves it: a
/// poll reads the old text or the new, never a part.
pub fn save(path: &Path, text: &str) {
    let saving = path.with_file_name(".saving");
    fs::write(&saving, text).unwrap();
    fs::rename(&saving, path).unwrap();
}

/// A runtime's event stream at `path` of the server at `address`, read one
/// message at a time.
pub struct Events {
    reader: BufReader<TcpStream>,
}

impl Events {
    pub fn connect(address: &str, path: &str) -> Self {
        let mut stream = TcpStream::connect(address).unwrap();
        stream.set_read_timeout(Some(TIMEOUT)).unwrap();
        write!(stream, "GET {path} HTTP/1.1\r\nHost: {address}\r\n\r\n").unwrap();
        Self {
            reader: BufReader::new(stream),
        }
    }

    /// The next message's JSON, or `None` once the server ends the stream.
    pub fn next(&mut self) -> Option<String> {
        let mut line = String::new();
        loop {
            line.clear();
            if self.reader.read_line(&mut line).unwrap() == 0 {
                return None;
            }
            if let Some(data) = line.strip_prefix("data: ") {
                return Some(data.trim_end().to_owned());
            }
        }
    }
}

/// The Node driver of a dev build, tests/support/dev_execution.mjs, which
/// loads the build in `build` and reads the event stream at `url` with the
/// build's client until it has handled `messages` messages after `hello`,
/// with its stdout piped for the record it prints at the end.
pub fn node_runtime(build: &Path, url: &str, messages: usize) -> Light {
    let driver = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/support/dev_execution.mjs"
    );
    let mut child = Command::new(std::env::var_os("NODE").unwrap_or_else(|| "node".into()))
        .arg(driver)
        .arg(build)
        .arg("--connect")
        .arg(url)
        .arg("--messages")
        .arg(messages.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the watch tests require Node");
    let stderr = child.stderr.take().unwrap();
    let (send, lines) = channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines() {
            let Ok(line) = line else { break };
            if send.send(line).is_err() {
                break;
            }
        }
    });
    Light { child, lines }
}
