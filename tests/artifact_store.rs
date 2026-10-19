//! Process-level tests of the on-disk artifact store (`src/artifact_store.rs`,
//! docs/compiler-cache.md, "Loading, diagnostics, and storage").
//!
//! Each test runs this binary again as child processes that compile a
//! generated source root, publish its artifacts, read or trim the store
//! ([`child`], driven by `LIGHT_STORE_CHILD`), so that writers, readers and
//! trimmers are separate processes sharing one cache directory.

use light_cljs::MacroHost;
use light_cljs::analyzer::AnalysisOptions;
use light_cljs::artifact_codec::{self, DecodeLimits, ManifestFields};
use light_cljs::artifact_store::{Entry, IndexError, Inputs, Publish, ReadError, Retention, Store};
use light_cljs::cache_key::{KeyEnvironment, NamespaceKeys};
use light_cljs::compiler::{EntryRecorders, analyze_namespace_entry_recorded};
use light_cljs::dependency_manifest::DependencyManifest;
use light_cljs::generated_names::Reservation;
use light_cljs::namespace_artifact::NamespaceCapture;
use light_cljs::sha256::hex;
use light_cljs::source_provider::FileSourceProvider;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, SystemTime};

#[path = "support/temp_dir.rs"]
#[allow(dead_code)]
mod temp_dir;
use temp_dir::TempDir;

const CHILD: &str = "LIGHT_STORE_CHILD";
const LEAVES: usize = 6;

// -- Fixtures -----------------------------------------------------------------

/// Write a source root under `dir` whose entry `PREFIX.main` requires
/// `PREFIX.n0` to `PREFIX.n5`, each defining values that depend on
/// `variant`, and, when `large`, `PREFIX.data` with a literal of about a
/// megabyte, so that writing its artifact takes a while.
fn source_root(dir: &Path, prefix: &str, variant: u32, large: bool) -> PathBuf {
    let root = dir.join(format!("src-{prefix}-{variant}"));
    let package = root.join(prefix);
    fs::create_dir_all(&package).unwrap();
    let mut requires = String::new();
    for leaf in 0..LEAVES {
        fs::write(
            package.join(format!("n{leaf}.cljs")),
            format!(
                "(ns {prefix}.n{leaf})\n(def value {})\n(defn f [x] (+ x value {leaf}))\n(defmacro twice [x] `(* 2 ~x))\n",
                variant * 100 + leaf as u32
            ),
        )
        .unwrap();
        requires.push_str(&format!(" [{prefix}.n{leaf} :as n{leaf}]"));
    }
    let mut body = String::from("(+");
    for leaf in 0..LEAVES {
        body.push_str(&format!(" (n{leaf}/f {variant})"));
    }
    body.push(')');
    if large {
        let items: Vec<String> = (0..40_000)
            .map(|item| format!("\"item-{variant}-{item}\""))
            .collect();
        fs::write(
            package.join("data.cljs"),
            format!("(ns {prefix}.data)\n(def items [{}])\n", items.join(" ")),
        )
        .unwrap();
        requires.push_str(&format!(" [{prefix}.data :as data]"));
        body = format!("(+ {body} (count data/items))");
    }
    fs::write(
        package.join("main.cljs"),
        format!("(ns {prefix}.main (:require{requires}))\n{body}\n"),
    )
    .unwrap();
    root
}

fn namespaces(prefix: &str, large: bool) -> Vec<String> {
    let mut names: Vec<String> = (0..LEAVES)
        .map(|leaf| format!("{prefix}.n{leaf}"))
        .collect();
    if large {
        names.push(format!("{prefix}.data"));
    }
    names.push(format!("{prefix}.main"));
    names
}

/// One compilation's artifacts with what publishing them needs.
struct Built {
    manifest: DependencyManifest,
    files: Vec<(PathBuf, String)>,
    captures: Vec<NamespaceCapture>,
    keys: NamespaceKeys,
    metadata: Vec<light_cljs::cache_policy::NamespaceMetadata>,
    provider: FileSourceProvider,
}

fn compile(root: &Path, entry: &str) -> Built {
    let roots = vec![root.to_path_buf()];
    let mut host = MacroHost::new();
    let options = AnalysisOptions::default();
    let environment = KeyEnvironment::new(&host, options);
    let mut manifest = DependencyManifest::default();
    let mut captures = Vec::new();
    let mut sink = |capture: NamespaceCapture| captures.push(capture);
    let mut warnings = Vec::new();
    let (_, files) = analyze_namespace_entry_recorded(
        entry,
        &roots,
        &mut host,
        options,
        &mut warnings,
        EntryRecorders {
            manifest: Some(&mut manifest),
            capture: Some(&mut sink),
            ..Default::default()
        },
    )
    .unwrap_or_else(|error| panic!("{entry} compiles: {error:?}"));
    let keys = NamespaceKeys::compute(&environment, &manifest, &files, entry);
    let metadata = light_cljs::cache_policy::source_metadata(&manifest, &host);
    let provider = FileSourceProvider::new(vec![fs::canonicalize(root).unwrap()]);
    Built {
        manifest,
        files,
        captures,
        keys,
        metadata,
        provider,
    }
}

/// Publish every artifact of `built`, printing one line per namespace.
fn publish_all(store: &Store, built: &Built) {
    for capture in &built.captures {
        let artifact = capture.artifact.as_ref().expect("captured");
        let id = built
            .manifest
            .id(&capture.namespace)
            .expect("in the manifest");
        let fields = ManifestFields::of(
            &built.manifest,
            id,
            built.metadata[id.0 as usize],
            built.keys.key(id).clone(),
            [0; 32],
        );
        let outcome = store.publish(
            artifact,
            &fields,
            Inputs {
                manifest: &built.manifest,
                id,
                text: &built.files[id.0 as usize].1,
                sources: &built.provider,
            },
        );
        let namespace = &capture.namespace;
        match outcome {
            Publish::Published(entry) => println!(
                "published {namespace} {} {}",
                hex(&entry.artifact),
                hex(&entry.key)
            ),
            Publish::Refused(mismatch) => println!("refused {namespace} {mismatch}"),
            Publish::Unencodable(reason) => println!("unencodable {namespace} {reason}"),
            Publish::Unwritable => println!("unwritable {namespace}"),
        }
    }
}

/// Read and decode every candidate of `namespace`, checking that each is
/// the namespace's artifact under its key. A missing file is the only
/// acceptable failure.
fn check_candidates(store: &Store, namespace: &str) -> Result<usize, String> {
    let entries = match store.read_candidates(namespace) {
        Ok(entries) => entries,
        Err(IndexError::Missing) => return Ok(0),
        Err(error) => return Err(format!("{namespace}: {error}")),
    };
    let mut read = 0;
    for entry in &entries {
        match store.read(entry, &DecodeLimits::default()) {
            Ok(bytes) => {
                let (manifest, artifact) =
                    artifact_codec::decode(&bytes, &DecodeLimits::default(), &|| false)
                        .map_err(|error| format!("{namespace}: {error}"))?;
                if manifest.namespace != namespace
                    || manifest.fields.front_end_key.digest != entry.key
                    || artifact.names != entry.names
                {
                    return Err(format!("{namespace}: the artifact is another's"));
                }
                read += 1;
            }
            Err(ReadError::Missing) => {}
            Err(error) => return Err(format!("{namespace}: {error}")),
        }
    }
    Ok(read)
}

fn wait_for(path: &Path) {
    while !path.exists() {
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).map(PathBuf::from)
}

// -- The child process ----------------------------------------------------------

/// What a child process does, by `LIGHT_STORE_CHILD`:
///
/// - `publish`: compile each root of `ROOTS` (`;`-separated, entry `ENTRY`)
///   and publish its artifacts to `STORE`. It creates `READY` after the
///   first compilation and waits for `GO` before publishing; with `LOOP`,
///   it publishes the roots again until killed; with `EDIT=PATH`, it
///   rewrites that source between compiling and publishing.
/// - `read`: until `STOP` exists, read and decode every candidate of
///   `NAMESPACES` (`,`-separated), then print the reads and failures.
/// - `trim`: trim `STORE` with `PER_NAMESPACE`, `MAX_BYTES` and `GRACE_MS`,
///   keeping `IN_USE` (`,`-separated artifact hashes), and print the result.
#[test]
#[ignore = "a child process of the other tests"]
fn child() {
    let Ok(mode) = std::env::var(CHILD) else {
        return;
    };
    let store = Store::open(env_path("STORE").expect("STORE"));
    let var = |name: &str| std::env::var(name).unwrap_or_default();
    match mode.as_str() {
        "publish" => {
            let entry = var("ENTRY");
            let roots: Vec<PathBuf> = var("ROOTS").split(';').map(PathBuf::from).collect();
            let mut first = true;
            loop {
                for root in &roots {
                    let built = compile(root, &entry);
                    if first {
                        first = false;
                        if let Some(ready) = env_path("READY") {
                            fs::write(ready, "").unwrap();
                        }
                        if let Some(go) = env_path("GO") {
                            wait_for(&go);
                        }
                    }
                    if let Some(edit) = env_path("EDIT") {
                        fs::write(&edit, "(ns edited)\n(def value :edited)\n").unwrap();
                    }
                    publish_all(&store, &built);
                }
                if std::env::var_os("LOOP").is_none() {
                    break;
                }
                // Each round republishes its artifacts under the same keys
                // with other bytes: a single-source compilation allocates
                // from the process-wide counters, whose windows the next
                // round's artifacts record (light-cljs-332 made a
                // namespace's own names the same in every round).
                light_cljs::compile("`x#").unwrap();
            }
            if var("TRIM_AFTER") == "1" {
                println!("trim {:?}", store.trim(&Retention::default(), &[]).is_ok());
            }
        }
        "read" => {
            let stop = env_path("STOP").expect("STOP");
            let names = var("NAMESPACES");
            let (mut passes, mut reads, mut failures) = (0u64, 0u64, Vec::new());
            loop {
                let last = stop.exists();
                for namespace in names.split(',') {
                    match check_candidates(&store, namespace) {
                        Ok(read) => reads += read as u64,
                        Err(failure) => failures.push(failure),
                    }
                }
                passes += 1;
                if last {
                    break;
                }
            }
            println!("passes {passes} reads {reads} failures {}", failures.len());
            for failure in failures {
                println!("failure {failure}");
            }
        }
        "trim" => {
            let number = |name: &str| var(name).parse::<u64>().unwrap();
            let retention = Retention {
                per_namespace: number("PER_NAMESPACE") as usize,
                max_bytes: number("MAX_BYTES"),
                grace: Duration::from_millis(number("GRACE_MS")),
            };
            let in_use: Vec<[u8; 32]> = var("IN_USE")
                .split(',')
                .filter(|hash| !hash.is_empty())
                .map(|hash| {
                    let mut digest = [0; 32];
                    for (at, byte) in digest.iter_mut().enumerate() {
                        *byte = u8::from_str_radix(&hash[at * 2..at * 2 + 2], 16).unwrap();
                    }
                    digest
                })
                .collect();
            println!("trimmed {:?}", store.trim(&retention, &in_use));
        }
        other => panic!("unknown child mode {other}"),
    }
}

/// The child process `mode` with environment `vars`.
fn spawn(mode: &str, vars: &[(&str, String)]) -> Child {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["child", "--exact", "--ignored", "--nocapture", "--quiet"])
        .env(CHILD, mode)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (name, value) in vars {
        command.env(name, value);
    }
    command.spawn().unwrap()
}

fn finish(child: Child) -> (String, String) {
    let Output {
        status,
        stdout,
        stderr,
    } = child.wait_with_output().unwrap();
    let (stdout, stderr) = (
        String::from_utf8(stdout).unwrap(),
        String::from_utf8(stderr).unwrap(),
    );
    assert!(status.success(), "child failed: {stdout}\n{stderr}");
    (stdout, stderr)
}

fn run(mode: &str, vars: &[(&str, String)]) -> (String, String) {
    finish(spawn(mode, vars))
}

fn lines<'a>(stdout: &'a str, prefix: &str) -> Vec<&'a str> {
    stdout
        .lines()
        .filter(|line| line.starts_with(prefix))
        .collect()
}

fn path(path: &Path) -> String {
    path.to_str().unwrap().to_owned()
}

fn roots(roots: &[&PathBuf]) -> String {
    roots
        .iter()
        .map(|root| path(root))
        .collect::<Vec<_>>()
        .join(";")
}

/// The files of `dir` whose names start with `.tmp-`.
fn temporaries(dir: &Path) -> Vec<PathBuf> {
    fs::read_dir(dir)
        .map(|files| {
            files
                .flatten()
                .filter(|file| file.file_name().to_string_lossy().starts_with(".tmp-"))
                .map(|file| file.path())
                .collect()
        })
        .unwrap_or_default()
}

fn object_count(store: &Store) -> usize {
    fs::read_dir(store.root().join("objects"))
        .map(|files| files.flatten().count())
        .unwrap_or(0)
}

// -- Tests ----------------------------------------------------------------------

#[test]
fn concurrent_writers_of_the_same_and_of_different_namespaces_keep_every_entry() {
    let dir = TempDir::new("store-writers");
    let cache = dir.join("cache");
    let first = source_root(&dir, "shared", 1, false);
    let second = source_root(&dir, "shared", 2, false);
    let other = source_root(&dir, "other", 1, false);
    let go = dir.join("go");
    let writers: Vec<(Child, PathBuf)> =
        [(&first, "shared"), (&second, "shared"), (&other, "other")]
            .into_iter()
            .enumerate()
            .map(|(at, (root, prefix))| {
                let ready = dir.join(format!("ready-{at}"));
                let child = spawn(
                    "publish",
                    &[
                        ("STORE", path(&cache)),
                        ("ROOTS", path(root)),
                        ("ENTRY", format!("{prefix}.main")),
                        ("READY", path(&ready)),
                        ("GO", path(&go)),
                    ],
                );
                (child, ready)
            })
            .collect();
    for (_, ready) in &writers {
        wait_for(ready);
    }
    // All three publish at once.
    fs::write(&go, "").unwrap();
    let outputs: Vec<String> = writers
        .into_iter()
        .map(|(child, _)| finish(child).0)
        .collect();
    for output in &outputs {
        assert_eq!(lines(output, "published").len(), LEAVES + 1, "{output}");
    }

    let store = Store::open(&cache);
    for namespace in namespaces("shared", false) {
        let entries = store.read_candidates(&namespace).unwrap();
        assert_eq!(entries.len(), 2, "{namespace} keeps both writers' entries");
        assert_ne!(entries[0].key, entries[1].key);
        assert_eq!(check_candidates(&store, &namespace), Ok(2));
        for output in &outputs[..2] {
            let line = lines(output, &format!("published {namespace} "))[0];
            let artifact = line.split(' ').nth(2).unwrap();
            assert!(entries.iter().any(|entry| hex(&entry.artifact) == artifact));
        }
    }
    for namespace in namespaces("other", false) {
        assert_eq!(check_candidates(&store, &namespace), Ok(1), "{namespace}");
    }
    assert_eq!(object_count(&store), 3 * (LEAVES + 1));
    assert!(temporaries(&store.root().join("objects")).is_empty());
    assert!(temporaries(&store.root().join("index")).is_empty());
}

#[test]
fn a_reader_racing_a_writer_sees_only_complete_artifacts() {
    let dir = TempDir::new("store-race");
    let cache = dir.join("cache");
    let variants: Vec<PathBuf> = (1..=4)
        .map(|variant| source_root(&dir, "race", variant, true))
        .collect();
    let stop = dir.join("stop");
    let reader = spawn(
        "read",
        &[
            ("STORE", path(&cache)),
            ("NAMESPACES", namespaces("race", true).join(",")),
            ("STOP", path(&stop)),
        ],
    );
    let (written, _) = run(
        "publish",
        &[
            ("STORE", path(&cache)),
            ("ROOTS", roots(&variants.iter().collect::<Vec<_>>())),
            ("ENTRY", "race.main".into()),
        ],
    );
    fs::write(&stop, "").unwrap();
    let (read, _) = finish(reader);
    assert_eq!(
        lines(&written, "published").len(),
        4 * (LEAVES + 2),
        "{written}"
    );
    let summary = lines(&read, "passes")[0];
    let fields: Vec<u64> = summary
        .split(' ')
        .filter_map(|field| field.parse().ok())
        .collect();
    let [passes, reads, failures] = fields[..] else {
        panic!("{read}")
    };
    assert_eq!(failures, 0, "{read}");
    assert!(passes > 1 && reads >= 4 * (LEAVES as u64 + 2), "{read}");
}

#[test]
fn a_killed_writers_temporary_file_is_ignored_and_later_cleaned() {
    let dir = TempDir::new("store-killed");
    let cache = dir.join("cache");
    let root = source_root(&dir, "killed", 1, true);
    let mut writer = spawn(
        "publish",
        &[
            ("STORE", path(&cache)),
            ("ROOTS", path(&root)),
            ("ENTRY", "killed.main".into()),
            ("LOOP", "1".into()),
        ],
    );
    let pid = writer.id();
    // Kill it in the middle of publishing, after one complete round.
    let mut stdout = BufReader::new(writer.stdout.take().unwrap());
    let mut published = 0;
    let mut line = String::new();
    while published < LEAVES + 2 + 3 {
        line.clear();
        assert!(stdout.read_line(&mut line).unwrap() > 0, "the writer ended");
        if line.starts_with("published") {
            published += 1;
        }
    }
    writer.kill().unwrap();
    writer.wait().unwrap();

    let store = Store::open(&cache);
    let objects = store.root().join("objects");
    let indexes = store.root().join("index");
    for namespace in namespaces("killed", true) {
        assert_eq!(check_candidates(&store, &namespace), Ok(1), "{namespace}");
    }
    // What a writer killed while writing leaves, if this kill did not.
    let data = store.candidates("killed.data").remove(0);
    let bytes = store.read(&data, &DecodeLimits::default()).unwrap();
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    fs::write(
        objects.join(format!(".tmp-{pid}-1000-{now}")),
        &bytes[..bytes.len() / 2],
    )
    .unwrap();
    let index = fs::read_dir(&indexes).unwrap().flatten().next().unwrap();
    let index_bytes = fs::read(index.path()).unwrap();
    fs::write(
        indexes.join(format!(".tmp-{pid}-1001-{now}")),
        &index_bytes[..index_bytes.len() - 7],
    )
    .unwrap();
    let left = [temporaries(&objects), temporaries(&indexes)].concat();
    assert!(left.len() >= 2);

    // Ignored: every namespace still reads its one complete artifact.
    for namespace in namespaces("killed", true) {
        assert_eq!(check_candidates(&store, &namespace), Ok(1), "{namespace}");
    }
    let trim = |grace_ms: u64| {
        run(
            "trim",
            &[
                ("STORE", path(&cache)),
                ("PER_NAMESPACE", "3".into()),
                ("MAX_BYTES", (1u64 << 30).to_string()),
                ("GRACE_MS", grace_ms.to_string()),
            ],
        )
        .0
    };
    // A young temporary file may be a live writer's, so it stays, but the
    // artifacts the looping writer replaced under the same keys go, and the
    // directory then holds exactly what the store retains.
    let orphans = object_count(&store) - temporaries(&objects).len() - (LEAVES + 2);
    assert!(orphans > 0);
    let output = trim(600_000);
    assert!(
        output.contains(&format!("temporaries: 0, orphans: {orphans}")),
        "{output}"
    );
    assert!(left.iter().all(|file| file.exists()));
    let on_disk: u64 = fs::read_dir(&objects)
        .unwrap()
        .flatten()
        .filter(|file| !file.file_name().to_string_lossy().starts_with(".tmp-"))
        .map(|file| file.metadata().unwrap().len())
        .sum();
    assert!(
        output.contains(&format!("retained_bytes: {on_disk}")),
        "{output}"
    );
    // Once it is older than the grace period, it goes.
    let hour_ago = SystemTime::now() - Duration::from_secs(3600);
    for file in &left {
        fs::File::options()
            .write(true)
            .open(file)
            .unwrap()
            .set_modified(hour_ago)
            .unwrap();
    }
    let output = trim(600_000);
    assert!(
        output.contains(&format!("temporaries: {}, orphans: 0", left.len())),
        "{output}"
    );
    assert!(left.iter().all(|file| !file.exists()));
    for namespace in namespaces("killed", true) {
        assert_eq!(check_candidates(&store, &namespace), Ok(1), "{namespace}");
    }
    assert_eq!(object_count(&store), LEAVES + 2);
}

/// Makes directories read-only, and writable again when dropped, before
/// the temporary directory holding them is removed.
#[cfg(unix)]
struct ReadOnly(Vec<PathBuf>);

#[cfg(unix)]
impl ReadOnly {
    fn new(dirs: Vec<PathBuf>) -> Self {
        use std::os::unix::fs::PermissionsExt;
        for dir in &dirs {
            fs::set_permissions(dir, fs::Permissions::from_mode(0o555)).unwrap();
        }
        Self(dirs)
    }
}

#[cfg(unix)]
impl Drop for ReadOnly {
    fn drop(&mut self) {
        use std::os::unix::fs::PermissionsExt;
        for dir in &self.0 {
            let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o755));
        }
    }
}

#[cfg(unix)]
#[test]
fn a_read_only_cache_directory_warns_once_and_stays_readable() {
    let dir = TempDir::new("store-read-only");
    let cache = dir.join("cache");
    let first = source_root(&dir, "ro", 1, false);
    let second = source_root(&dir, "ro", 2, false);
    let publish = |root: &PathBuf| {
        run(
            "publish",
            &[
                ("STORE", path(&cache)),
                ("ROOTS", path(root)),
                ("ENTRY", "ro.main".into()),
                ("TRIM_AFTER", "1".into()),
            ],
        )
    };
    publish(&first);
    let store = Store::open(&cache);
    let before = object_count(&store);
    let _read_only = ReadOnly::new(vec![
        store.root().to_path_buf(),
        store.root().join("objects"),
        store.root().join("index"),
    ]);
    let (stdout, stderr) = publish(&second);
    assert_eq!(
        lines(&stdout, "unwritable").len(),
        LEAVES + 1,
        "every publish fails: {stdout}"
    );
    let warnings = lines(&stderr, "warning:");
    assert_eq!(warnings.len(), 1, "one warning: {stderr}");
    assert!(
        warnings[0].contains("cannot write the compiler cache"),
        "{stderr}"
    );
    assert!(lines(&stdout, "trim").contains(&"trim true"), "{stdout}");
    // What was there still reads, and nothing was left behind.
    for namespace in namespaces("ro", false) {
        assert_eq!(check_candidates(&store, &namespace), Ok(1), "{namespace}");
    }
    assert_eq!(object_count(&store), before);
}

#[test]
fn an_input_changed_between_compile_and_publish_is_not_published() {
    let dir = TempDir::new("store-changed");
    let cache = dir.join("cache");
    let root = source_root(&dir, "changed", 1, false);
    let (stdout, _) = run(
        "publish",
        &[
            ("STORE", path(&cache)),
            ("ROOTS", path(&root)),
            ("ENTRY", "changed.main".into()),
            ("EDIT", path(&root.join("changed/n2.cljs"))),
        ],
    );
    assert_eq!(
        lines(&stdout, "refused"),
        ["refused changed.n2 source changed"],
        "{stdout}"
    );
    assert_eq!(lines(&stdout, "published").len(), LEAVES, "{stdout}");
    let store = Store::open(&cache);
    assert_eq!(
        store.read_candidates("changed.n2"),
        Err(IndexError::Missing)
    );
    assert_eq!(object_count(&store), LEAVES);
    assert_eq!(check_candidates(&store, "changed.n1"), Ok(1));
}

#[test]
fn retention_keeps_the_newest_per_namespace_and_evicts_the_oldest_to_the_cap() {
    let dir = TempDir::new("store-retention");
    let cache = dir.join("cache");
    let variants: Vec<PathBuf> = (1..=4)
        .map(|variant| source_root(&dir, "kept", variant, false))
        .collect();
    let (stdout, _) = run(
        "publish",
        &[
            ("STORE", path(&cache)),
            ("ROOTS", roots(&variants.iter().collect::<Vec<_>>())),
            ("ENTRY", "kept.main".into()),
        ],
    );
    assert_eq!(lines(&stdout, "published").len(), 4 * (LEAVES + 1));
    let store = Store::open(&cache);
    let names = namespaces("kept", false);
    // Publishing evicts nothing; entries are newest first, variant 4 first.
    let by_variant = |variant: usize| -> Vec<Entry> {
        names
            .iter()
            .map(|namespace| {
                let entries = store.read_candidates(namespace).unwrap();
                assert_eq!(entries.len(), 4, "{namespace}");
                entries[4 - variant].clone()
            })
            .collect()
    };
    let (newest, third, second, first) =
        (by_variant(4), by_variant(3), by_variant(2), by_variant(1));
    assert_eq!(object_count(&store), 4 * names.len());
    let bytes = |entries: &[Entry]| entries.iter().map(|entry| entry.bytes).sum::<u64>();
    let in_use: Vec<String> = newest.iter().map(|entry| hex(&entry.artifact)).collect();
    // Three per namespace, the newest in use among them, and the cap holds
    // exactly the newest two variants' artifacts.
    let (output, _) = run(
        "trim",
        &[
            ("STORE", path(&cache)),
            ("PER_NAMESPACE", "3".into()),
            ("MAX_BYTES", (bytes(&newest) + bytes(&third)).to_string()),
            ("GRACE_MS", "600000".into()),
            ("IN_USE", in_use.join(",")),
        ],
    );
    assert!(
        output.contains(&format!(
            "evicted: {}, evicted_bytes: {}",
            2 * names.len(),
            bytes(&first) + bytes(&second)
        )),
        "{output}"
    );
    for (at, namespace) in names.iter().enumerate() {
        let entries = store.read_candidates(namespace).unwrap();
        assert_eq!(
            entries,
            [newest[at].clone(), third[at].clone()],
            "{namespace}"
        );
    }
    assert_eq!(object_count(&store), 2 * names.len());
    let retained = bytes(&newest) + bytes(&third);
    assert!(
        output.contains(&format!("retained_bytes: {retained}")),
        "{output}"
    );

    // Those in use go last: with the older third variant in use, a cap of
    // its size evicts the newer fourth.
    let trim = |max_bytes: u64, in_use: &[Entry]| {
        let in_use: Vec<String> = in_use.iter().map(|entry| hex(&entry.artifact)).collect();
        run(
            "trim",
            &[
                ("STORE", path(&cache)),
                ("PER_NAMESPACE", "3".into()),
                ("MAX_BYTES", max_bytes.to_string()),
                ("GRACE_MS", "600000".into()),
                ("IN_USE", in_use.join(",")),
            ],
        )
        .0
    };
    let output = trim(bytes(&third), &third);
    assert!(
        output.contains(&format!(
            "evicted: {}, evicted_bytes: {}",
            names.len(),
            bytes(&newest)
        )),
        "{output}"
    );
    for (at, namespace) in names.iter().enumerate() {
        assert_eq!(
            store.read_candidates(namespace).unwrap(),
            [third[at].clone()]
        );
    }
    assert_eq!(check_candidates(&store, "kept.main"), Ok(1));

    // The cap holds for artifacts in use too.
    let output = trim(0, &third);
    assert!(
        output.contains(&format!("evicted: {}", names.len())),
        "{output}"
    );
    assert_eq!(object_count(&store), 0);
    assert!(
        fs::read_dir(store.root().join("index"))
            .unwrap()
            .next()
            .is_none()
    );
}

#[test]
fn a_snapshot_returns_only_the_candidates_whose_names_it_reserved() {
    let dir = TempDir::new("store-snapshot");
    let cache = dir.join("cache");
    let publish = |variant: u32| {
        let root = source_root(&dir, "snap", variant, false);
        run(
            "publish",
            &[
                ("STORE", path(&cache)),
                ("ROOTS", path(&root)),
                ("ENTRY", "snap.main".into()),
            ],
        )
    };
    publish(1);
    let store = Store::open(&cache);
    let names = namespaces("snap", false);
    let snapshot = store.snapshot(names.iter().map(String::as_str));
    let before: Vec<Entry> = names
        .iter()
        .map(|namespace| store.read_candidates(namespace).unwrap().remove(0))
        .collect();
    assert_eq!(
        snapshot.reservation(),
        &Reservation::of(before.iter().map(|entry| &entry.names))
    );
    // Another process publishes after the snapshot was taken.
    publish(2);
    for (at, namespace) in names.iter().enumerate() {
        let now = store.read_candidates(namespace).unwrap();
        assert_eq!(now.len(), 2, "{namespace}");
        assert_eq!(snapshot.candidates(namespace), Ok(&before[at..at + 1]));
        assert_eq!(snapshot.lookup(namespace, &now[0].key), None);
        assert_eq!(
            snapshot.lookup(namespace, &before[at].key),
            Some(&before[at])
        );
    }
    assert_eq!(snapshot.candidates("snap.absent"), Err(IndexError::Missing));
    let all = store.snapshot_all();
    assert_eq!(all.candidates("snap.main").unwrap().len(), 2);
}

#[test]
fn reads_reject_damaged_truncated_over_budget_and_missing_artifacts() {
    let dir = TempDir::new("store-damaged");
    let cache = dir.join("cache");
    let root = source_root(&dir, "bad", 1, false);
    run(
        "publish",
        &[
            ("STORE", path(&cache)),
            ("ROOTS", path(&root)),
            ("ENTRY", "bad.main".into()),
        ],
    );
    let store = Store::open(&cache);
    let entry = store.candidates("bad.n0").remove(0);
    let file = store.root().join("objects").join(hex(&entry.artifact));
    let limits = DecodeLimits::default();
    let bytes = fs::read(&file).unwrap();
    assert_eq!(store.read(&entry, &limits), Ok(bytes.clone()));
    let mut damaged = bytes.clone();
    damaged[bytes.len() / 2] ^= 1;
    fs::write(&file, &damaged).unwrap();
    assert_eq!(store.read(&entry, &limits), Err(ReadError::Integrity));
    fs::write(&file, &bytes[..bytes.len() - 1]).unwrap();
    assert_eq!(
        store.read(&entry, &limits),
        Err(ReadError::Size {
            expected: entry.bytes,
            found: entry.bytes - 1
        })
    );
    fs::write(&file, &bytes).unwrap();
    let small = DecodeLimits {
        max_bytes: entry.bytes - 1,
        ..limits
    };
    assert_eq!(
        store.read(&entry, &small),
        Err(ReadError::TooLarge(entry.bytes))
    );
    fs::remove_file(&file).unwrap();
    assert_eq!(store.read(&entry, &limits), Err(ReadError::Missing));
    // A damaged index is no candidates, and the other namespaces still read.
    let index = store
        .root()
        .join("index")
        .join(hex(&light_cljs::sha256::digest(b"bad.n1")));
    let mut index_bytes = fs::read(&index).unwrap();
    index_bytes[20] ^= 1;
    fs::write(&index, &index_bytes).unwrap();
    assert_eq!(store.read_candidates("bad.n1"), Err(IndexError::Damaged));
    for namespace in ["bad.n2", "bad.main"] {
        assert_eq!(check_candidates(&store, namespace), Ok(1), "{namespace}");
    }
}
