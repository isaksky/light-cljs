//! The on-disk store of encoded namespace artifacts (docs/compiler-cache.md,
//! "Loading, diagnostics, and storage").
//!
//! A cache directory holds one directory per store format version, so a
//! compiler with another layout ignores this one instead of misreading it:
//!
//! ```text
//! DIR/v1/lock                  advisory lock for index updates and trimming
//! DIR/v1/objects/<sha256>      an encoded artifact, named by the SHA-256 of its bytes
//! DIR/v1/index/<sha256>        a namespace's index, named by the SHA-256 of its name
//! DIR/v1/outputs/<sha256>      a whole build's output, named by its output key
//! DIR/v1/*/.tmp-<pid>-<n>-<t>  a file being written, never read
//! ```
//!
//! **Outputs** (light-cljs-418). A build whose every namespace was
//! installed from or published as an artifact stores the files it wrote
//! under [`crate::cache_key::output_key`] of those artifacts and its
//! emission options, and a later build with the same key writes them again
//! instead of preparing and emitting the application
//! ([`Store::read_output`], [`Store::write_output`]). An output file is
//! written as an artifact is and checks its own SHA-256 when read; a file
//! that fails is a miss. Writing one keeps the [`Retention::per_namespace`]
//! most recently written or used outputs and removes the others.
//!
//! **Publication.** Artifacts are immutable and content-addressed. Each is
//! written in full to a temporary file in the directory it is published in,
//! then renamed over its name, so a reader opens either nothing or the whole
//! file. A namespace's index lists its candidate artifacts, newest first,
//! each with the front-end key it was published under
//! ([`crate::cache_key::FrontEndKey::digest`]), its size, its publication
//! time and its generated names. The artifact's rename and the index's
//! rewrite, again through a temporary file and a rename, both hold the
//! store's lock, the index's after the artifact is in place, so two
//! processes that publish at once never lose each other's entries, and
//! under the lock every artifact no index names is an orphan; an entry with the same key or the
//! same artifact as the new one is replaced, so the last complete write of a
//! key wins. Readers take no lock. Every read checks the index's own
//! SHA-256 and the artifact's content address, and a file that fails either
//! is a miss. Files are not synced to disk: a crash can at worst leave one
//! that fails its hash, which is a miss and is replaced by the next publish,
//! and syncing costs a full flush per file on macOS.
//!
//! **Lookup.** A compilation reads the indexes it may use once, into a
//! [`Snapshot`], reserves the generated names of every candidate in it, and
//! looks up only there, so no artifact another process publishes meanwhile
//! can hand it names it did not reserve.
//!
//! **Refusal.** [`Store::publish`] re-validates the namespace's inputs first
//! ([`crate::cache_key::revalidate`]) and refuses an artifact whose inputs
//! changed while it compiled, since its key would describe inputs it did
//! not see.
//!
//! **Failure.** The first write that fails (a read-only directory, a full
//! disk) prints one warning to stderr and turns writing off for the store's
//! lifetime; later publishes return [`Publish::Unwritable`] without trying.
//! The compiled output does not depend on the store, and what the store held
//! stays readable: a failed write leaves at most a temporary file.
//!
//! **Retention.** Publishing never evicts. [`Store::trim`], which a
//! compilation calls after it succeeds and never during it, keeps the
//! [`Retention::per_namespace`] most recently published artifacts of each
//! namespace, those the finished build used or published (`in_use`) first
//! among them, then evicts the oldest by publication time, those not in use
//! first, until the store's artifacts fit in [`Retention::max_bytes`].
//! Recency is publication, not use: a hit does not rewrite its index, so
//! an artifact a build keeps reusing ages until a build trims with it in
//! use. It also removes every unindexed artifact (one a newer artifact of
//! its key replaced, or a killed writer's), and temporary files older than
//! [`Retention::grace`]; a younger one may be a running writer's.
//!
//! The store is disposable derived data: it is never committed, and deleting
//! the directory is always safe, when no compiler is writing to it or
//! otherwise. Deleting it clears nothing a running compiler holds in memory.

use crate::artifact_codec::{self, DecodeLimits, ManifestFields};
use crate::cache_key::{Digest, Mismatch};
use crate::dependency_manifest::NamespaceId;
use crate::generated_names::{Counters, GeneratedNames, Reservation};
use crate::namespace_artifact::NamespaceArtifact;
use crate::sha256::{Sha256, digest, hex};
use crate::source_provider::SourceProvider;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The layout's version, which names its directory. Bump it with any change
/// to the layout or the index format.
pub const STORE_VERSION: u32 = 1;
const INDEX_MAGIC: [u8; 8] = *b"LCLJSIDX";
/// No stored output is read or written past this size, as no artifact is
/// decoded past [`DecodeLimits::max_bytes`].
pub const MAX_OUTPUT_BYTES: u64 = 1 << 30;
const TEMP_PREFIX: &str = ".tmp-";
/// No index is read past this size.
const MAX_INDEX_BYTES: u64 = 64 << 20;

/// One candidate artifact of a namespace's index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// The front-end key it was published under.
    pub key: Digest,
    /// The SHA-256 of its encoded bytes, which names its file.
    pub artifact: Digest,
    pub bytes: u64,
    /// When it was published, in nanoseconds since the Unix epoch.
    pub published: u64,
    /// The names its analysis generated, so that a compilation can reserve
    /// every candidate's names ([`Snapshot::reservation`]) without reading the
    /// artifacts.
    pub names: GeneratedNames,
}

/// What [`Store::trim`] keeps.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Retention {
    /// The most recently published artifacts kept per namespace.
    pub per_namespace: usize,
    /// The total size of retained artifacts.
    pub max_bytes: u64,
    /// How old a temporary file must be before it is removed.
    pub grace: Duration,
}

impl Default for Retention {
    /// Three artifacts per namespace, 1 GiB, and ten minutes. Penpot's 922
    /// artifacts are 94 MB, so the cap holds about ten whole builds.
    fn default() -> Self {
        Self {
            per_namespace: 3,
            max_bytes: 1 << 30,
            grace: Duration::from_secs(10 * 60),
        }
    }
}

/// The inputs of the compilation an artifact came from, which
/// [`Store::publish`] re-validates.
pub struct Inputs<'a> {
    pub manifest: &'a dyn crate::manifest_prefix::NamespaceRecords,
    pub id: NamespaceId,
    /// The namespace's source text as it compiled.
    pub text: &'a str,
    pub sources: &'a dyn SourceProvider,
}

/// What [`Store::publish`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Publish {
    Published(Entry),
    /// Its inputs changed while it compiled, so nothing was written.
    Refused(Mismatch),
    /// It could not be encoded ([`artifact_codec::encode`]).
    Unencodable(String),
    /// The store cannot be written. The first failure printed a warning.
    Unwritable,
}

/// Why [`Store::read`] returned no bytes. Each is a miss.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadError {
    /// No file, as when another process evicted it.
    Missing,
    /// Larger than the decode limit allows.
    TooLarge(u64),
    /// Not the size its index entry records.
    Size {
        expected: u64,
        found: u64,
    },
    /// Its bytes do not hash to its name.
    Integrity,
    Io(String),
}

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing => f.write_str("the artifact is missing"),
            Self::TooLarge(bytes) => write!(f, "the artifact's {bytes} bytes exceed the limit"),
            Self::Size { expected, found } => {
                write!(
                    f,
                    "the artifact is {found} bytes, its index says {expected}"
                )
            }
            Self::Integrity => f.write_str("the artifact fails its content hash"),
            Self::Io(error) => write!(f, "the artifact cannot be read: {error}"),
        }
    }
}

/// Why a namespace's index gave no candidates. Each is a miss.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IndexError {
    /// Nothing was published for the namespace.
    Missing,
    /// It is not one index of this version for the namespace whose hash
    /// checks.
    Damaged,
    Io(String),
}

impl fmt::Display for IndexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing => f.write_str("no artifact was published"),
            Self::Damaged => f.write_str("its index is damaged"),
            Self::Io(error) => write!(f, "its index cannot be read: {error}"),
        }
    }
}

/// What one [`Store::trim`] removed and kept.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Trimmed {
    pub evicted: usize,
    pub evicted_bytes: u64,
    pub retained: usize,
    pub retained_bytes: u64,
    pub temporaries: usize,
    pub orphans: usize,
}

/// A store under a cache directory.
pub struct Store {
    dir: PathBuf,
    root: PathBuf,
    /// Why writing was turned off, once a write failed.
    failure: OnceLock<String>,
    prepared: OnceLock<()>,
}

/// The candidates a compilation may look up, read once before its first
/// namespace, and the [`Reservation`] of their generated names, which it
/// reserves then ([`crate::compiler::EntryRecorders::reserve`]). Looking up
/// only here keeps every artifact a lookup can return inside the
/// reservation: an artifact another process publishes later, whose names
/// earlier misses of this compilation may already have taken, is not a
/// candidate until the next compilation.
pub struct Snapshot {
    indexes: HashMap<String, Result<Vec<Entry>, IndexError>>,
    reservation: Reservation,
}

impl Snapshot {
    fn new(indexes: HashMap<String, Result<Vec<Entry>, IndexError>>) -> Self {
        let reservation = Reservation::of(
            indexes
                .values()
                .flatten()
                .flat_map(|entries| entries.iter().map(|entry| &entry.names)),
        );
        Self {
            indexes,
            reservation,
        }
    }

    /// The candidates of `namespace`, newest first, or why there are none.
    pub fn candidates(&self, namespace: &str) -> Result<&[Entry], IndexError> {
        match self.indexes.get(namespace) {
            Some(Ok(entries)) => Ok(entries),
            Some(Err(error)) => Err(error.clone()),
            None => Err(IndexError::Missing),
        }
    }

    /// The candidate of `namespace` published under front-end key `key`.
    pub fn lookup(&self, namespace: &str, key: &Digest) -> Option<&Entry> {
        self.candidates(namespace)
            .ok()?
            .iter()
            .find(|entry| &entry.key == key)
    }

    pub fn reservation(&self) -> &Reservation {
        &self.reservation
    }
}

/// A namespace's index: its name and its entries, newest first.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Index {
    namespace: String,
    entries: Vec<Entry>,
}

impl Store {
    /// The store in cache directory `dir`. Nothing is created until the
    /// first write, and a directory that does not exist reads as empty.
    pub fn open(dir: impl Into<PathBuf>) -> Self {
        let dir = dir.into();
        let root = dir.join(format!("v{STORE_VERSION}"));
        Self {
            dir,
            root,
            failure: OnceLock::new(),
            prepared: OnceLock::new(),
        }
    }

    /// The directory of this layout version.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Why writing is off, after a write failed.
    pub fn write_failure(&self) -> Option<&str> {
        self.failure.get().map(String::as_str)
    }

    fn objects(&self) -> PathBuf {
        self.root.join("objects")
    }

    fn indexes(&self) -> PathBuf {
        self.root.join("index")
    }

    fn object_path(&self, artifact: &Digest) -> PathBuf {
        self.objects().join(hex(artifact))
    }

    fn index_path(&self, namespace: &str) -> PathBuf {
        self.indexes().join(hex(&digest(namespace.as_bytes())))
    }

    fn outputs(&self) -> PathBuf {
        self.root.join("outputs")
    }

    // -- Outputs ------------------------------------------------------------

    /// The files of the output stored under `key`, each a name and its
    /// text, or `None` when there is none or it fails its checks. A use
    /// counts as recent for [`Store::write_output`]'s retention.
    /// A file larger than `limit` bytes is a miss and is not read.
    pub fn read_output(&self, key: &Digest, limit: u64) -> Option<Vec<(String, String)>> {
        let path = self.outputs().join(hex(key));
        let file = fs::File::open(&path).ok()?;
        if file.metadata().ok()?.len() > limit {
            return None;
        }
        let mut bytes = Vec::new();
        io::Read::read_to_end(&mut io::Read::take(file, limit + 1), &mut bytes).ok()?;
        if bytes.len() as u64 > limit {
            return None;
        }
        let files = decode_output(&bytes, key)?;
        if let Ok(file) = fs::File::options().write(true).open(&path) {
            let _ = file.set_modified(SystemTime::now());
        }
        Some(files)
    }

    /// Store `files`, each a name and its text, as the output of `key`,
    /// then keep the `retention.per_namespace` most recent outputs. Returns
    /// the bytes written, or `None` when the store cannot be written (the
    /// first failure prints a warning) or the output would exceed `limit`
    /// bytes, which [`Store::read_output`] would refuse.
    pub fn write_output(
        &self,
        key: &Digest,
        files: &[(&str, &str)],
        retention: &Retention,
        limit: u64,
    ) -> Option<u64> {
        if self.failure.get().is_some() {
            return None;
        }
        let bytes = encode_output(key, files);
        if bytes.len() as u64 > limit {
            return None;
        }
        let written = (|| -> io::Result<()> {
            fs::create_dir_all(self.outputs())?;
            let temporary = Temporary::write(&self.outputs(), &bytes)?;
            let _lock = self.lock()?;
            temporary.publish(&self.outputs().join(hex(key)), |existing| existing == bytes)?;
            self.trim_outputs(retention, key)
        })();
        match written {
            Ok(()) => Some(bytes.len() as u64),
            Err(error) => {
                self.fail(&error);
                None
            }
        }
    }

    /// Keep the `retention.per_namespace` most recently written or used
    /// outputs, `key`'s among them, and temporary files younger than
    /// `retention.grace`. Holds the lock.
    fn trim_outputs(&self, retention: &Retention, key: &Digest) -> io::Result<()> {
        let mut outputs = Vec::new();
        for file in fs::read_dir(self.outputs())?.flatten() {
            let name = file.file_name().into_string().unwrap_or_default();
            let modified = file.metadata().and_then(|metadata| metadata.modified());
            if name.starts_with(TEMP_PREFIX) {
                let old = modified
                    .ok()
                    .and_then(|time| time.elapsed().ok())
                    .is_some_and(|age| age >= retention.grace);
                if old {
                    let _ = fs::remove_file(file.path());
                }
            } else if let Some(output) = parse_hex(&name) {
                let current = output == *key;
                outputs.push((current, modified.unwrap_or(UNIX_EPOCH), file.path()));
            }
        }
        outputs.sort_by(|a, b| b.cmp(a));
        for (_, _, path) in outputs.iter().skip(retention.per_namespace.max(1)) {
            let _ = fs::remove_file(path);
        }
        Ok(())
    }

    // -- Reading ------------------------------------------------------------

    /// The candidate artifacts of `namespace`, newest first; none when its
    /// index is missing or fails its checks.
    pub fn candidates(&self, namespace: &str) -> Vec<Entry> {
        self.read_candidates(namespace).unwrap_or_default()
    }

    /// [`Store::candidates`], or why the index could not be used.
    pub fn read_candidates(&self, namespace: &str) -> Result<Vec<Entry>, IndexError> {
        self.read_index(namespace).map(|index| index.entries)
    }

    /// The encoded bytes of `entry`'s artifact, checked against its size and
    /// content hash. The caller owns them and releases them by dropping them.
    pub fn read(&self, entry: &Entry, limits: &DecodeLimits) -> Result<Vec<u8>, ReadError> {
        if entry.bytes > limits.max_bytes {
            return Err(ReadError::TooLarge(entry.bytes));
        }
        let io = |error: io::Error| match error.kind() {
            io::ErrorKind::NotFound => ReadError::Missing,
            _ => ReadError::Io(error.to_string()),
        };
        let mut file = fs::File::open(self.object_path(&entry.artifact)).map_err(io)?;
        let found = file.metadata().map_err(io)?.len();
        if found != entry.bytes {
            return Err(ReadError::Size {
                expected: entry.bytes,
                found,
            });
        }
        let mut bytes = Vec::with_capacity(found as usize);
        io::Read::read_to_end(&mut file, &mut bytes).map_err(io)?;
        if bytes.len() as u64 != found {
            return Err(ReadError::Size {
                expected: entry.bytes,
                found: bytes.len() as u64,
            });
        }
        if digest(&bytes) != entry.artifact {
            return Err(ReadError::Integrity);
        }
        Ok(bytes)
    }

    /// The candidates of `namespaces` as their indexes are now, which is all
    /// a compilation looks up, with their generated names reserved.
    pub fn snapshot<'a>(&self, namespaces: impl IntoIterator<Item = &'a str>) -> Snapshot {
        Snapshot::new(
            namespaces
                .into_iter()
                .map(|namespace| {
                    let entries = self.read_index(namespace).map(|index| index.entries);
                    (namespace.to_owned(), entries)
                })
                .collect(),
        )
    }

    /// [`Store::snapshot`] of every namespace the store indexes.
    pub fn snapshot_all(&self) -> Snapshot {
        Snapshot::new(
            self.all_indexes()
                .into_iter()
                .map(|(_, index)| (index.namespace, Ok(index.entries)))
                .collect(),
        )
    }

    fn read_index(&self, namespace: &str) -> Result<Index, IndexError> {
        let index = read_index_file(&self.index_path(namespace))?;
        if index.namespace != namespace {
            return Err(IndexError::Damaged);
        }
        Ok(index)
    }

    /// Every valid index, with its file name.
    fn all_indexes(&self) -> Vec<(String, Index)> {
        let Ok(files) = fs::read_dir(self.indexes()) else {
            return Vec::new();
        };
        files
            .flatten()
            .filter_map(|file| {
                let name = file.file_name().into_string().ok()?;
                let index = read_index_file(&file.path()).ok()?;
                (hex(&digest(index.namespace.as_bytes())) == name).then_some((name, index))
            })
            .collect()
    }

    // -- Writing ------------------------------------------------------------

    /// Publish `artifact` with `fields` in its manifest, unless its inputs
    /// changed while it compiled. The encoded bytes are written and dropped
    /// before this returns.
    pub fn publish(
        &self,
        artifact: &NamespaceArtifact,
        fields: &ManifestFields,
        inputs: Inputs<'_>,
    ) -> Publish {
        if self.failure.get().is_some() {
            return Publish::Unwritable;
        }
        if let Err(mismatch) =
            crate::cache_key::revalidate(inputs.manifest, inputs.id, inputs.text, inputs.sources)
        {
            return Publish::Refused(mismatch);
        }
        let bytes = match artifact_codec::encode(artifact, fields) {
            Ok(bytes) => bytes,
            Err(reason) => return Publish::Unencodable(reason),
        };
        match self.write(
            &artifact.header.namespace,
            fields.front_end_key.digest,
            &artifact.names,
            bytes,
        ) {
            Ok(entry) => Publish::Published(entry),
            Err(error) => {
                self.fail(&error);
                Publish::Unwritable
            }
        }
    }

    /// Turn writing off, with one warning.
    fn fail(&self, error: &io::Error) {
        let reason = format!(
            "cannot write the compiler cache in {}: {error}; the build continues without writing to it",
            self.dir.display()
        );
        if self.failure.set(reason.clone()).is_ok() {
            eprintln!("warning: {reason}");
        }
    }

    fn prepare(&self) -> io::Result<()> {
        if self.prepared.get().is_none() {
            fs::create_dir_all(self.objects())?;
            fs::create_dir_all(self.indexes())?;
            let _ = self.prepared.set(());
        }
        Ok(())
    }

    fn lock(&self) -> io::Result<fs::File> {
        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(self.root.join("lock"))?;
        file.lock()?;
        Ok(file)
    }

    fn write(
        &self,
        namespace: &str,
        key: Digest,
        names: &GeneratedNames,
        bytes: Vec<u8>,
    ) -> io::Result<Entry> {
        self.prepare()?;
        let artifact = digest(&bytes);
        let path = self.object_path(&artifact);
        // Always written, so that a damaged file of the same name is
        // replaced. The rename and the index update hold the lock, so that
        // under it every artifact is indexed, or is an orphan to remove.
        let temporary = Temporary::write(&self.objects(), &bytes)?;
        let length = bytes.len() as u64;
        drop(bytes);
        let _lock = self.lock()?;
        temporary.publish(&path, |existing| digest(existing) == artifact)?;
        let entry = Entry {
            key,
            artifact,
            bytes: length,
            published: now(),
            names: names.clone(),
        };
        // A damaged index is replaced; one that cannot be read is not.
        let mut index = match self.read_index(namespace) {
            Ok(index) => index,
            Err(IndexError::Io(error)) => return Err(io::Error::other(error)),
            Err(IndexError::Missing | IndexError::Damaged) => Index {
                namespace: namespace.to_owned(),
                entries: Vec::new(),
            },
        };
        index
            .entries
            .retain(|old| old.key != key && old.artifact != artifact);
        index.entries.insert(0, entry.clone());
        self.write_index(&index)?;
        Ok(entry)
    }

    fn write_index(&self, index: &Index) -> io::Result<()> {
        let bytes = encode_index(index);
        let path = self.index_path(&index.namespace);
        Temporary::write(&self.indexes(), &bytes)?.publish(&path, |existing| existing == bytes)
    }

    // -- Retention ----------------------------------------------------------

    /// Apply `retention` after a successful build, which used or published
    /// the artifacts `in_use`, the last to be evicted. Holds the store's
    /// lock, so publishes of other processes wait for it.
    pub fn trim(&self, retention: &Retention, in_use: &[Digest]) -> io::Result<Trimmed> {
        if self.failure.get().is_some() || !self.root.exists() {
            return Ok(Trimmed::default());
        }
        let result = self.trim_locked(retention, in_use);
        if let Err(error) = &result {
            self.fail(error);
        }
        result
    }

    fn trim_locked(&self, retention: &Retention, in_use: &[Digest]) -> io::Result<Trimmed> {
        self.prepare()?;
        let _lock = self.lock()?;
        let in_use: &HashSet<&Digest> = &in_use.iter().collect();
        let mut trimmed = Trimmed::default();
        let old = |file: &fs::DirEntry| {
            file.metadata()
                .and_then(|metadata| metadata.modified())
                .ok()
                .and_then(|time| time.elapsed().ok())
                .is_some_and(|age| age >= retention.grace)
        };

        // The complete artifacts, by name, with their sizes. Temporary files
        // go once they are old: a younger one may be a running writer's.
        let mut objects: HashMap<Digest, u64> = HashMap::new();
        for file in fs::read_dir(self.objects())?.flatten() {
            let name = file.file_name();
            let name = name.to_string_lossy();
            if name.starts_with(TEMP_PREFIX) {
                if old(&file) && fs::remove_file(file.path()).is_ok() {
                    trimmed.temporaries += 1;
                }
            } else if let Some(artifact) = parse_hex(&name) {
                let bytes = file.metadata().map_or(0, |metadata| metadata.len());
                objects.insert(artifact, bytes);
            }
        }
        let mut indexes = Vec::new();
        for file in fs::read_dir(self.indexes())?.flatten() {
            let name = file.file_name().into_string().unwrap_or_default();
            if name.starts_with(TEMP_PREFIX) {
                if old(&file) && fs::remove_file(file.path()).is_ok() {
                    trimmed.temporaries += 1;
                }
                continue;
            }
            if parse_hex(&name).is_none() {
                continue;
            }
            match read_index_file(&file.path()) {
                Ok(index) if hex(&digest(index.namespace.as_bytes())) == name => {
                    indexes.push((index.clone(), index))
                }
                // Damaged: nothing reads it, so it goes.
                _ => {
                    let _ = fs::remove_file(file.path());
                }
            }
        }

        // Each namespace keeps its newest entries whose artifacts exist,
        // those in use counted first.
        for (_, index) in &mut indexes {
            index
                .entries
                .retain(|entry| objects.get(&entry.artifact) == Some(&entry.bytes));
            index
                .entries
                .sort_by_key(|entry| std::cmp::Reverse((entry.published, entry.artifact)));
            let used = index
                .entries
                .iter()
                .filter(|entry| in_use.contains(&entry.artifact))
                .count();
            let mut room = retention.per_namespace.saturating_sub(used);
            index.entries.retain(|entry| {
                if in_use.contains(&entry.artifact) {
                    return true;
                }
                let keep = room > 0;
                room = room.saturating_sub(1);
                keep
            });
        }

        // Then the oldest go until the store fits the cap: those the build
        // did not use first, then those it did.
        let mut total: u64 = indexes
            .iter()
            .flat_map(|(_, index)| &index.entries)
            .map(|entry| entry.bytes)
            .sum();
        let mut by_age: Vec<(bool, u64, Digest, usize)> = indexes
            .iter()
            .enumerate()
            .flat_map(|(at, (_, index))| {
                index.entries.iter().map(move |entry| {
                    let used = in_use.contains(&entry.artifact);
                    (used, entry.published, entry.artifact, at)
                })
            })
            .collect();
        by_age.sort();
        for (_, _, artifact, at) in by_age {
            if total <= retention.max_bytes {
                break;
            }
            let entries = &mut indexes[at].1.entries;
            if let Some(position) = entries.iter().position(|e| e.artifact == artifact) {
                total -= entries.remove(position).bytes;
            }
        }

        // Indexes first, so that none names a removed artifact. Under the
        // lock every artifact no index names is evicted or an orphan, since
        // a writer renames an artifact into place and indexes it under it.
        let mut referenced: HashSet<Digest> = HashSet::new();
        let mut evicted: HashSet<Digest> = HashSet::new();
        for (before, after) in &indexes {
            referenced.extend(after.entries.iter().map(|entry| entry.artifact));
            evicted.extend(before.entries.iter().map(|entry| entry.artifact));
            if after.entries.is_empty() {
                fs::remove_file(self.index_path(&after.namespace))?;
            } else if before != after {
                self.write_index(after)?;
            }
        }
        for (artifact, bytes) in &objects {
            if referenced.contains(artifact) {
                trimmed.retained += 1;
                trimmed.retained_bytes += bytes;
            } else if fs::remove_file(self.object_path(artifact)).is_ok() {
                if evicted.contains(artifact) {
                    trimmed.evicted += 1;
                    trimmed.evicted_bytes += bytes;
                } else {
                    trimmed.orphans += 1;
                }
            }
        }
        Ok(trimmed)
    }
}

/// Nanoseconds since the Unix epoch.
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |time| time.as_nanos() as u64)
}

/// A complete temporary file, removed when dropped unless published.
struct Temporary(PathBuf);

impl Temporary {
    /// Write `bytes` to a new temporary file in `dir`.
    fn write(dir: &Path, bytes: &[u8]) -> io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let temporary = Self(dir.join(format!(
            "{TEMP_PREFIX}{}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
            now()
        )));
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary.0)?;
        file.write_all(bytes)?;
        file.flush()?;
        Ok(temporary)
    }

    /// Rename it to `target`. Where the rename fails because `target` is
    /// open (Windows), a `target` that already holds what `holds` accepts
    /// counts as written.
    fn publish(self, target: &Path, holds: impl Fn(&[u8]) -> bool) -> io::Result<()> {
        match fs::rename(&self.0, target) {
            Ok(()) => {
                std::mem::forget(self);
                Ok(())
            }
            Err(_) if fs::read(target).is_ok_and(|existing| holds(&existing)) => Ok(()),
            Err(error) => Err(error),
        }
    }
}

impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn parse_hex(name: &str) -> Option<Digest> {
    if name.len() != 64 {
        return None;
    }
    let mut out = [0; 32];
    for (at, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(name.get(at * 2..at * 2 + 2)?, 16).ok()?;
    }
    (hex(&out) == name).then_some(out)
}

// -- The index format -------------------------------------------------------
//
// magic "LCLJSIDX", STORE_VERSION u32, the namespace (u32 length, UTF-8),
// the entry count u32, then per entry its key, its artifact, its bytes u64,
// its publication time u64, its generated names (the eight window counters
// and the gensym mark as u64, a u32 count of spellings, each a u32 length
// and UTF-16 code units), and last the SHA-256 of everything before it.
// Numbers are little-endian.

fn encode_index(index: &Index) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&INDEX_MAGIC);
    out.extend_from_slice(&STORE_VERSION.to_le_bytes());
    out.extend_from_slice(&(index.namespace.len() as u32).to_le_bytes());
    out.extend_from_slice(index.namespace.as_bytes());
    out.extend_from_slice(&(index.entries.len() as u32).to_le_bytes());
    for entry in &index.entries {
        out.extend_from_slice(&entry.key);
        out.extend_from_slice(&entry.artifact);
        out.extend_from_slice(&entry.bytes.to_le_bytes());
        out.extend_from_slice(&entry.published.to_le_bytes());
        let names = &entry.names;
        for counters in [&names.from, &names.to] {
            for (_, value) in counters.each() {
                out.extend_from_slice(&value.to_le_bytes());
            }
        }
        out.extend_from_slice(&names.gensym_to.to_le_bytes());
        out.extend_from_slice(&(names.gensyms.len() as u32).to_le_bytes());
        for gensym in &names.gensyms {
            out.extend_from_slice(&(gensym.len() as u32).to_le_bytes());
            for unit in gensym {
                out.extend_from_slice(&unit.to_le_bytes());
            }
        }
    }
    let hash = digest(&out);
    out.extend_from_slice(&hash);
    out
}

fn read_index_file(path: &Path) -> Result<Index, IndexError> {
    let io = |error: io::Error| match error.kind() {
        io::ErrorKind::NotFound => IndexError::Missing,
        _ => IndexError::Io(error.to_string()),
    };
    let file = fs::File::open(path).map_err(io)?;
    if file.metadata().map_err(io)?.len() > MAX_INDEX_BYTES {
        return Err(IndexError::Damaged);
    }
    let mut bytes = Vec::new();
    io::Read::read_to_end(&mut io::Read::take(file, MAX_INDEX_BYTES + 1), &mut bytes)
        .map_err(io)?;
    decode_index(&bytes).ok_or(IndexError::Damaged)
}

/// The index `bytes` hold, or `None` if they are not exactly one index of
/// this version whose hash checks.
fn decode_index(bytes: &[u8]) -> Option<Index> {
    let (body, hash) = bytes.split_at_checked(bytes.len().checked_sub(32)?)?;
    let mut check = Sha256::new();
    check.update(body);
    if check.finish() != hash {
        return None;
    }
    let mut input = IndexReader { bytes: body };
    if input.take(8)? != INDEX_MAGIC || input.u32()? != STORE_VERSION {
        return None;
    }
    let length = input.u32()? as usize;
    let namespace = String::from_utf8(input.take(length)?.to_vec()).ok()?;
    let count = input.u32()? as usize;
    // Each entry takes at least 32 + 32 + 8 + 8 + 9 * 8 + 4 bytes.
    const ENTRY: usize = 156;
    if count > input.bytes.len() / ENTRY {
        return None;
    }
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        let key = input.digest()?;
        let artifact = input.digest()?;
        let bytes = input.u64()?;
        let published = input.u64()?;
        let mut counters = || -> Option<Counters> {
            Some(Counters {
                syntax_quote: input.u64()?,
                anonymous: input.u64()?,
                binding: input.u64()?,
                destructure: input.u64()?,
            })
        };
        let from = counters()?;
        let to = counters()?;
        let gensym_to = input.u64()?;
        let spellings = input.u32()? as usize;
        if spellings > input.bytes.len() / 4 {
            return None;
        }
        let mut gensyms = Vec::with_capacity(spellings);
        for _ in 0..spellings {
            let units = input.u32()? as usize;
            let raw = input.take(units.checked_mul(2)?)?;
            gensyms.push(
                raw.as_chunks::<2>()
                    .0
                    .iter()
                    .map(|pair| u16::from_le_bytes(*pair))
                    .collect(),
            );
        }
        entries.push(Entry {
            key,
            artifact,
            bytes,
            published,
            names: GeneratedNames {
                from,
                to,
                gensym_to,
                gensyms,
            },
        });
    }
    input
        .bytes
        .is_empty()
        .then_some(Index { namespace, entries })
}

// -- The output format ------------------------------------------------------
//
// magic "LCLJSOUT", STORE_VERSION u32, the output key, the file count u32,
// then per file its name (u32 length, UTF-8) and its text (u64 length,
// UTF-8), and last the SHA-256 of everything before it. Numbers are
// little-endian.

const OUTPUT_MAGIC: [u8; 8] = *b"LCLJSOUT";

fn encode_output(key: &Digest, files: &[(&str, &str)]) -> Vec<u8> {
    let length = files
        .iter()
        .map(|(name, text)| 12 + name.len() + text.len())
        .sum::<usize>();
    let mut out = Vec::with_capacity(80 + length);
    out.extend_from_slice(&OUTPUT_MAGIC);
    out.extend_from_slice(&STORE_VERSION.to_le_bytes());
    out.extend_from_slice(key);
    out.extend_from_slice(&(files.len() as u32).to_le_bytes());
    for (name, text) in files {
        out.extend_from_slice(&(name.len() as u32).to_le_bytes());
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(&(text.len() as u64).to_le_bytes());
        out.extend_from_slice(text.as_bytes());
    }
    let hash = digest(&out);
    out.extend_from_slice(&hash);
    out
}

/// The files `bytes` hold, or `None` if they are not exactly one output of
/// this version for `key` whose hash checks.
fn decode_output(bytes: &[u8], key: &Digest) -> Option<Vec<(String, String)>> {
    let (body, hash) = bytes.split_at_checked(bytes.len().checked_sub(32)?)?;
    if digest(body) != hash {
        return None;
    }
    let mut input = IndexReader { bytes: body };
    if input.take(8)? != OUTPUT_MAGIC || input.u32()? != STORE_VERSION || input.digest()? != *key {
        return None;
    }
    let count = input.u32()? as usize;
    if count > input.bytes.len() / 12 {
        return None;
    }
    let mut files = Vec::with_capacity(count);
    for _ in 0..count {
        let length = input.u32()? as usize;
        let name = String::from_utf8(input.take(length)?.to_vec()).ok()?;
        let length = usize::try_from(input.u64()?).ok()?;
        let text = String::from_utf8(input.take(length)?.to_vec()).ok()?;
        files.push((name, text));
    }
    input.bytes.is_empty().then_some(files)
}

struct IndexReader<'a> {
    bytes: &'a [u8],
}

impl<'a> IndexReader<'a> {
    fn take(&mut self, length: usize) -> Option<&'a [u8]> {
        let (taken, rest) = self.bytes.split_at_checked(length)?;
        self.bytes = rest;
        Some(taken)
    }

    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }

    fn u64(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }

    fn digest(&mut self) -> Option<Digest> {
        self.take(32)?.try_into().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(seed: u8, gensyms: &[&str]) -> Entry {
        Entry {
            key: [seed; 32],
            artifact: [seed.wrapping_add(1); 32],
            bytes: u64::from(seed) * 1000,
            published: u64::from(seed) << 40,
            names: GeneratedNames {
                from: Counters {
                    syntax_quote: 1,
                    anonymous: 2,
                    binding: 3,
                    destructure: 4,
                },
                to: Counters {
                    syntax_quote: 10,
                    anonymous: 20,
                    binding: 30,
                    destructure: 40,
                },
                gensym_to: 7,
                gensyms: gensyms
                    .iter()
                    .map(|text| text.encode_utf16().collect())
                    .collect(),
            },
        }
    }

    #[test]
    fn an_index_round_trips_and_any_damage_is_rejected() {
        let index = Index {
            namespace: "app.ünï".into(),
            entries: vec![entry(3, &["G__1", "x\u{1F600}"]), entry(9, &[])],
        };
        let bytes = encode_index(&index);
        assert_eq!(decode_index(&bytes), Some(index.clone()));
        assert_eq!(
            decode_index(&encode_index(&Index::default())),
            Some(Index::default())
        );
        for at in 0..bytes.len() {
            let mut damaged = bytes.clone();
            damaged[at] ^= 0x40;
            assert_eq!(decode_index(&damaged), None, "byte {at}");
        }
        for length in 0..bytes.len() {
            assert_eq!(decode_index(&bytes[..length]), None, "length {length}");
        }
    }

    #[test]
    fn an_output_round_trips_and_any_damage_is_rejected() {
        let key = digest(b"output");
        let files = [
            ("app.main.js", "export default 1;\n"),
            ("chunks.json", "{}"),
        ];
        let bytes = encode_output(&key, &files);
        let owned = files
            .iter()
            .map(|(name, text)| (name.to_string(), text.to_string()))
            .collect::<Vec<_>>();
        assert_eq!(decode_output(&bytes, &key), Some(owned));
        assert_eq!(decode_output(&bytes, &digest(b"other")), None);
        for at in 0..bytes.len() {
            let mut damaged = bytes.clone();
            damaged[at] ^= 0x40;
            assert_eq!(decode_output(&damaged, &key), None, "byte {at}");
        }
        for length in 0..bytes.len() {
            assert_eq!(
                decode_output(&bytes[..length], &key),
                None,
                "length {length}"
            );
        }
    }

    #[test]
    fn writing_an_output_keeps_the_most_recent_ones() {
        let dir = std::env::temp_dir().join(format!(
            "light-cljs-outputs-{}-{}",
            std::process::id(),
            now()
        ));
        let store = Store::open(&dir);
        let retention = Retention {
            per_namespace: 2,
            ..Retention::default()
        };
        let keys = [b"a", b"b", b"c"].map(|seed| digest(seed));
        for (at, key) in keys.iter().enumerate() {
            let text = format!("output {at}");
            assert!(
                store
                    .write_output(key, &[("-", &text)], &retention, MAX_OUTPUT_BYTES)
                    .is_some()
            );
            // Modification times order the outputs.
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(
            store.read_output(&keys[0], MAX_OUTPUT_BYTES),
            None,
            "the oldest went"
        );
        assert_eq!(
            store.read_output(&keys[1], MAX_OUTPUT_BYTES),
            Some(vec![("-".to_owned(), "output 1".to_owned())])
        );
        std::thread::sleep(Duration::from_millis(20));
        // Reading `b` made it recent, so writing `a` again removes `c`.
        assert!(
            store
                .write_output(&keys[0], &[("-", "again")], &retention, MAX_OUTPUT_BYTES)
                .is_some()
        );
        assert_eq!(store.read_output(&keys[2], MAX_OUTPUT_BYTES), None);
        assert!(store.read_output(&keys[1], MAX_OUTPUT_BYTES).is_some());
        assert_eq!(
            store.read_output(&keys[0], MAX_OUTPUT_BYTES),
            Some(vec![("-".to_owned(), "again".to_owned())])
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn outputs_past_the_limit_are_neither_written_nor_read() {
        let dir = std::env::temp_dir().join(format!(
            "light-cljs-outputs-limit-{}-{}",
            std::process::id(),
            now()
        ));
        let store = Store::open(&dir);
        let retention = Retention::default();
        let key = digest(b"large");
        let text = "x".repeat(1000);
        assert_eq!(
            store.write_output(&key, &[("-", &text)], &retention, 1000),
            None,
            "too large to store"
        );
        assert!(!store.outputs().join(hex(&key)).exists());
        let written = store.write_output(&key, &[("-", &text)], &retention, 2000);
        assert!(written.is_some());
        assert!(store.read_output(&key, 2000).is_some());
        assert_eq!(store.read_output(&key, 1000), None, "too large to read");
        // A file extended past the limit, sparse, is refused unread.
        let file = fs::File::options()
            .write(true)
            .open(store.outputs().join(hex(&key)))
            .unwrap();
        file.set_len(MAX_OUTPUT_BYTES + 1).unwrap();
        assert_eq!(store.read_output(&key, MAX_OUTPUT_BYTES), None);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_index_of_another_version_is_not_read() {
        let mut bytes = encode_index(&Index::default());
        bytes.truncate(bytes.len() - 32);
        bytes[8..12].copy_from_slice(&(STORE_VERSION + 1).to_le_bytes());
        let hash = digest(&bytes);
        bytes.extend_from_slice(&hash);
        assert_eq!(decode_index(&bytes), None);
    }

    #[test]
    fn hex_names_parse_only_in_their_own_spelling() {
        let digest = digest(b"x");
        assert_eq!(parse_hex(&hex(&digest)), Some(digest));
        assert_eq!(parse_hex(&hex(&digest).to_uppercase()), None);
        assert_eq!(parse_hex(".tmp-1-2-3"), None);
        assert_eq!(parse_hex(&"é".repeat(32)), None);
    }
}
