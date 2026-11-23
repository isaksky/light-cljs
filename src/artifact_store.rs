//! The on-disk store of encoded namespace artifacts (docs/compiler-cache.md,
//! "Loading, diagnostics, and storage").
//!
//! A cache directory holds one directory per store format version, so a
//! compiler with another layout ignores this one instead of misreading it:
//!
//! ```text
//! DIR/v1/lock                  advisory lock for index updates and trimming
//! DIR/v1/.lease-<token>        advisory staging lease held while a writer has staged files
//! DIR/v1/objects/<sha256>      an encoded artifact, named by the SHA-256 of its bytes
//! DIR/v1/index/<sha256>        a namespace's index, named by the SHA-256 of its name
//! DIR/v1/outputs/<sha256>      a whole build's output, named by its output key
//! DIR/v1/*/.tmp-*              a file being written, never read
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
//! [`Retention::grace`] whose advisory staging lease is no longer held.
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
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The layout's version, which names its directory. Bump it with any change
/// to the layout or the index format.
pub const STORE_VERSION: u32 = 1;
const INDEX_MAGIC: [u8; 8] = *b"LCLJSIDX";
/// No stored output is read or written past this size, as no artifact is
/// decoded past [`DecodeLimits::max_bytes`].
pub const MAX_OUTPUT_BYTES: u64 = 1 << 30;
const TEMP_PREFIX: &str = ".tmp-";
const LEASED_TEMP_PREFIX: &str = ".tmp-l";
const LEASE_PREFIX: &str = ".lease-";
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

/// An encoded artifact written to a temporary file in `objects/`, ready to be
/// committed under the store lock with [`Store::commit_batch`]. Dropping it
/// without committing removes its temporary file.
#[derive(Debug)]
pub struct StagedArtifact {
    namespace: String,
    key: Digest,
    artifact: Digest,
    bytes: u64,
    names: GeneratedNames,
    temporary: Temporary,
}

/// What [`Store::stage`] produced before committing under the store lock.
#[derive(Debug)]
pub enum Stage {
    Staged(StagedArtifact),
    /// Its inputs changed while it compiled, so nothing was written.
    Refused(Mismatch),
    /// It could not be encoded ([`artifact_codec::encode`]).
    Unencodable(String),
    /// The store cannot be written. The first failure printed a warning.
    Unwritable,
}

/// Internal staging result that reports a staging I/O error without latching
/// [`Store::fail`] immediately, so a batched caller can commit earlier staged
/// artifacts before disabling writes.
pub(crate) enum StageAttempt {
    Staged(StagedArtifact),
    Refused(Mismatch),
    Unencodable(String),
    Io(io::Error),
    Unwritable,
}

impl StagedArtifact {
    /// Approximate heap bytes retained by a staged artifact while waiting to
    /// be committed (its generated names and path metadata).
    pub(crate) fn approximate_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.namespace.len()
            + self.temporary.approximate_bytes()
            + self
                .names
                .gensyms
                .iter()
                .map(|g| std::mem::size_of::<Vec<u16>>() + g.len() * 2)
                .sum::<usize>()
    }
}

/// An artifact [`Store::begin_read`] read up to its manifest's end.
pub struct Reading {
    file: fs::File,
    bytes: Vec<u8>,
    length: u64,
    artifact: Digest,
}

impl Reading {
    /// The bytes read so far: the header and manifest.
    pub fn prefix(&self) -> &[u8] {
        &self.bytes
    }

    /// The artifact's size, as its index records and its file has.
    pub fn length(&self) -> u64 {
        self.length
    }

    /// The whole artifact, checked against its size and content hash.
    pub fn finish(mut self) -> Result<Vec<u8>, ReadError> {
        self.bytes
            .reserve_exact((self.length as usize).saturating_sub(self.bytes.len()));
        io::Read::read_to_end(&mut self.file, &mut self.bytes).map_err(read_error)?;
        if self.bytes.len() as u64 != self.length {
            return Err(ReadError::Size {
                expected: self.length,
                found: self.bytes.len() as u64,
            });
        }
        if digest(&self.bytes) != self.artifact {
            return Err(ReadError::Integrity);
        }
        Ok(self.bytes)
    }
}

fn read_error(error: io::Error) -> ReadError {
    match error.kind() {
        io::ErrorKind::NotFound => ReadError::Missing,
        _ => ReadError::Io(error.to_string()),
    }
}

/// Read `file` on until `bytes` holds `end` bytes of the `length` its
/// index records; a file that ends sooner is the wrong size.
fn read_up_to(
    file: &mut fs::File,
    bytes: &mut Vec<u8>,
    end: u64,
    length: u64,
) -> Result<(), ReadError> {
    let wanted = end.saturating_sub(bytes.len() as u64);
    io::Read::read_to_end(&mut io::Read::take(&mut *file, wanted), bytes).map_err(read_error)?;
    if (bytes.len() as u64) < end {
        return Err(ReadError::Size {
            expected: length,
            found: bytes.len() as u64,
        });
    }
    Ok(())
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

/// A store under a cache directory. Cheap to clone: clones share the store's
/// unwritable flag and directory-preparation state across threads.
#[derive(Clone)]
pub struct Store {
    inner: Arc<StoreInner>,
}

struct StoreInner {
    dir: PathBuf,
    root: PathBuf,
    /// Why writing was turned off, once a write failed.
    failure: OnceLock<String>,
    prepared: OnceLock<()>,
    /// Shared advisory staging lease reused by live staged files of this store,
    /// bounding open descriptors to one lease handle regardless of batch size.
    lease: Mutex<Option<Weak<StagingLease>>>,
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
            inner: Arc::new(StoreInner {
                dir,
                root,
                failure: OnceLock::new(),
                prepared: OnceLock::new(),
                lease: Mutex::new(None),
            }),
        }
    }

    /// The directory of this layout version.
    pub fn root(&self) -> &Path {
        &self.inner.root
    }

    /// Why writing is off, after a write failed.
    pub fn write_failure(&self) -> Option<&str> {
        self.inner.failure.get().map(String::as_str)
    }

    fn objects(&self) -> PathBuf {
        self.inner.root.join("objects")
    }

    fn indexes(&self) -> PathBuf {
        self.inner.root.join("index")
    }

    fn object_path(&self, artifact: &Digest) -> PathBuf {
        self.objects().join(hex(artifact))
    }

    fn index_path(&self, namespace: &str) -> PathBuf {
        self.indexes().join(hex(&digest(namespace.as_bytes())))
    }

    fn outputs(&self) -> PathBuf {
        self.inner.root.join("outputs")
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
        if self.inner.failure.get().is_some() {
            return None;
        }
        let bytes = encode_output(key, files);
        if bytes.len() as u64 > limit {
            return None;
        }
        let written = (|| -> io::Result<()> {
            fs::create_dir_all(self.outputs())?;
            let temporary = self.write_temporary(&self.outputs(), &bytes)?;
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
    /// outputs, `key`'s among them, and temporary files that are younger than
    /// `retention.grace` or still hold an advisory staging lease. Holds the
    /// lock.
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
                    let _ = remove_unheld_temporary(&self.inner.root, &file.path());
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
        self.trim_leases(retention.grace);
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
        self.begin_read(entry, limits)?.finish()
    }

    /// `entry`'s artifact with only its manifest's prefix read
    /// ([`artifact_codec::manifest_end`]), checked against its size: what a
    /// lookup compares a candidate's key with before reading the rest and
    /// checking its content hash ([`Reading::finish`]).
    pub fn begin_read(&self, entry: &Entry, limits: &DecodeLimits) -> Result<Reading, ReadError> {
        if entry.bytes > limits.max_bytes {
            return Err(ReadError::TooLarge(entry.bytes));
        }
        let mut file = fs::File::open(self.object_path(&entry.artifact)).map_err(read_error)?;
        let found = file.metadata().map_err(read_error)?.len();
        if found != entry.bytes {
            return Err(ReadError::Size {
                expected: entry.bytes,
                found,
            });
        }
        let mut bytes = Vec::new();
        let header = (artifact_codec::HEADER_LENGTH as u64).min(found);
        read_up_to(&mut file, &mut bytes, header, found)?;
        // A header that claims more than the file holds is the manifest
        // reader's to report, from the bytes there are.
        let end = artifact_codec::manifest_end(&bytes).map_or(header, |end| end.min(found));
        read_up_to(&mut file, &mut bytes, end, found)?;
        Ok(Reading {
            file,
            bytes,
            length: found,
            artifact: entry.artifact,
        })
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

    /// Validate, encode, hash, and write `artifact` to a temporary file in
    /// `objects/` without taking the store lock or latching [`Store::fail`] on
    /// I/O error. The encoded bytes are dropped before this returns.
    pub(crate) fn stage_attempt(
        &self,
        artifact: &NamespaceArtifact,
        fields: &ManifestFields,
        inputs: Inputs<'_>,
    ) -> StageAttempt {
        if self.inner.failure.get().is_some() {
            return StageAttempt::Unwritable;
        }
        if let Err(mismatch) =
            crate::cache_key::revalidate(inputs.manifest, inputs.id, inputs.text, inputs.sources)
        {
            return StageAttempt::Refused(mismatch);
        }
        let bytes = match artifact_codec::encode(artifact, fields) {
            Ok(bytes) => bytes,
            Err(reason) => return StageAttempt::Unencodable(reason),
        };
        match self.stage_encoded(
            artifact.header.namespace.clone(),
            fields.front_end_key.digest,
            artifact.names.clone(),
            bytes,
        ) {
            Ok(staged) => StageAttempt::Staged(staged),
            Err(error) => StageAttempt::Io(error),
        }
    }

    /// Validate, encode, hash, and write `artifact` to a temporary file in
    /// `objects/` without taking the store lock. The encoded bytes are dropped
    /// before this returns. Commit the staged artifact with
    /// [`Store::commit_batch`].
    pub fn stage(
        &self,
        artifact: &NamespaceArtifact,
        fields: &ManifestFields,
        inputs: Inputs<'_>,
    ) -> Stage {
        match self.stage_attempt(artifact, fields, inputs) {
            StageAttempt::Staged(staged) => Stage::Staged(staged),
            StageAttempt::Refused(mismatch) => Stage::Refused(mismatch),
            StageAttempt::Unencodable(reason) => Stage::Unencodable(reason),
            StageAttempt::Io(error) => {
                self.fail(&error);
                Stage::Unwritable
            }
            StageAttempt::Unwritable => Stage::Unwritable,
        }
    }

    /// Hash `bytes` and write them to a temporary file in `objects/` without
    /// taking the store lock, dropping `bytes` before returning.
    pub(crate) fn stage_encoded(
        &self,
        namespace: String,
        key: Digest,
        names: GeneratedNames,
        bytes: Vec<u8>,
    ) -> io::Result<StagedArtifact> {
        self.prepare()?;
        let artifact = digest(&bytes);
        let temporary = self.write_temporary(&self.objects(), &bytes)?;
        let length = bytes.len() as u64;
        drop(bytes);
        Ok(StagedArtifact {
            namespace,
            key,
            artifact,
            bytes: length,
            names,
            temporary,
        })
    }

    /// Commit a batch of [`StagedArtifact`]s under a single store lock
    /// acquisition: coalesce updates per namespace, prepare each index update
    /// before renaming that namespace's objects (rolling back newly introduced
    /// unindexed objects if the index rename fails), and write each touched
    /// index once. Any staged artifact that cannot be committed has its
    /// temporary file removed on drop and returns [`Publish::Unwritable`].
    pub fn commit_batch(&self, staged: Vec<StagedArtifact>) -> Vec<Publish> {
        let count = staged.len();
        if count == 0 {
            return Vec::new();
        }
        let mut results = vec![Publish::Unwritable; count];
        if self.inner.failure.get().is_some() {
            return results;
        }
        let _lock = match self.prepare().and_then(|()| self.lock()) {
            Ok(lock) => lock,
            Err(error) => {
                self.fail(&error);
                return results;
            }
        };
        let mut by_namespace: HashMap<String, Vec<(usize, StagedArtifact)>> = HashMap::new();
        let mut ns_order: Vec<String> = Vec::new();
        for (idx, item) in staged.into_iter().enumerate() {
            if !by_namespace.contains_key(&item.namespace) {
                ns_order.push(item.namespace.clone());
            }
            by_namespace
                .entry(item.namespace.clone())
                .or_default()
                .push((idx, item));
        }
        let mut published_objects: HashSet<Digest> = HashSet::with_capacity(count);
        let mut last_ts = 0u64;
        for namespace in ns_order {
            let items = by_namespace.remove(&namespace).unwrap_or_default();
            // Read and prepare the updated index before renaming any objects of
            // this namespace, so an unwritable index directory fails before
            // introducing unindexed object orphans.
            let mut index = match self.read_index(&namespace) {
                Ok(index) => index,
                Err(IndexError::Io(error)) => {
                    self.fail(&io::Error::other(error));
                    break;
                }
                Err(IndexError::Missing | IndexError::Damaged) => Index {
                    namespace: namespace.clone(),
                    entries: Vec::new(),
                },
            };
            let mut prepared: Vec<(usize, Entry, Temporary)> = Vec::with_capacity(items.len());
            for (idx, item) in items {
                let ts = now().max(last_ts);
                last_ts = ts;
                let entry = Entry {
                    key: item.key,
                    artifact: item.artifact,
                    bytes: item.bytes,
                    published: ts,
                    names: item.names,
                };
                index
                    .entries
                    .retain(|old| old.key != entry.key && old.artifact != entry.artifact);
                index.entries.insert(0, entry.clone());
                prepared.push((idx, entry, item.temporary));
            }
            let index_bytes = encode_index(&index);
            let index_temp = match self.write_temporary(&self.indexes(), &index_bytes) {
                Ok(temp) => temp,
                Err(error) => {
                    self.fail(&error);
                    break;
                }
            };
            let mut introduced: Vec<(Digest, PathBuf)> = Vec::new();
            let mut object_error = None;
            let mut entries_out: Vec<(usize, Entry)> = Vec::with_capacity(prepared.len());
            for (idx, entry, temporary) in prepared {
                if published_objects.insert(entry.artifact) {
                    let path = self.object_path(&entry.artifact);
                    let existed = path.exists();
                    // Always written, so that a damaged file of the same name is
                    // replaced. The rename and index update hold the lock, so under
                    // it every artifact is indexed, or is an orphan to remove.
                    if let Err(error) =
                        temporary.publish(&path, |existing| digest(existing) == entry.artifact)
                    {
                        published_objects.remove(&entry.artifact);
                        object_error = Some(error);
                        break;
                    }
                    if !existed {
                        introduced.push((entry.artifact, path));
                    }
                } else {
                    // Identical object already published under this lock hold.
                    drop(temporary);
                }
                entries_out.push((idx, entry));
            }
            if let Some(error) = object_error {
                drop(index_temp);
                for (digest, path) in introduced {
                    let _ = fs::remove_file(&path);
                    published_objects.remove(&digest);
                }
                self.fail(&error);
                break;
            }
            let index_path = self.index_path(&index.namespace);
            match index_temp.publish(&index_path, |existing| existing == index_bytes) {
                Ok(()) => {
                    for (idx, entry) in entries_out {
                        results[idx] = Publish::Published(entry);
                    }
                }
                Err(error) => {
                    for (digest, path) in introduced {
                        let _ = fs::remove_file(&path);
                        published_objects.remove(&digest);
                    }
                    self.fail(&error);
                    break;
                }
            }
        }
        results
    }

    /// Publish `artifact` with `fields` in its manifest, unless its inputs
    /// changed while it compiled. The encoded bytes are written and dropped
    /// before this returns.
    pub fn publish(
        &self,
        artifact: &NamespaceArtifact,
        fields: &ManifestFields,
        inputs: Inputs<'_>,
    ) -> Publish {
        match self.stage(artifact, fields, inputs) {
            Stage::Staged(staged) => self
                .commit_batch(vec![staged])
                .into_iter()
                .next()
                .unwrap_or(Publish::Unwritable),
            Stage::Refused(mismatch) => Publish::Refused(mismatch),
            Stage::Unencodable(reason) => Publish::Unencodable(reason),
            Stage::Unwritable => Publish::Unwritable,
        }
    }

    /// Turn writing off, with one warning.
    pub(crate) fn fail(&self, error: &io::Error) {
        let reason = format!(
            "cannot write the compiler cache in {}: {error}; the build continues without writing to it",
            self.inner.dir.display()
        );
        if self.inner.failure.set(reason.clone()).is_ok() {
            eprintln!("warning: {reason}");
        }
    }

    fn prepare(&self) -> io::Result<()> {
        if self.inner.prepared.get().is_none() {
            fs::create_dir_all(self.objects())?;
            fs::create_dir_all(self.indexes())?;
            let _ = self.inner.prepared.set(());
        }
        Ok(())
    }

    fn lock(&self) -> io::Result<fs::File> {
        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(self.inner.root.join("lock"))?;
        file.lock()?;
        Ok(file)
    }

    fn staging_lease(&self) -> io::Result<Arc<StagingLease>> {
        let mut guard = self
            .inner
            .lease
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(existing) = guard.as_ref().and_then(Weak::upgrade) {
            return Ok(existing);
        }
        let lease = Arc::new(StagingLease::acquire(&self.inner.root)?);
        *guard = Some(Arc::downgrade(&lease));
        Ok(lease)
    }

    fn write_temporary(&self, dir: &Path, bytes: &[u8]) -> io::Result<Temporary> {
        let lease = self.staging_lease()?;
        Temporary::write_with_lease(dir, bytes, lease)
    }

    fn trim_leases(&self, grace: Duration) {
        let Ok(entries) = fs::read_dir(&self.inner.root) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if !name.starts_with(LEASE_PREFIX) {
                continue;
            }
            let old = entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .ok()
                .and_then(|time| time.elapsed().ok())
                .is_some_and(|age| age >= grace);
            if old {
                let _ = remove_unheld_lease(&entry.path());
            }
        }
    }

    fn write_index(&self, index: &Index) -> io::Result<()> {
        let bytes = encode_index(index);
        let path = self.index_path(&index.namespace);
        self.write_temporary(&self.indexes(), &bytes)?
            .publish(&path, |existing| existing == bytes)
    }

    // -- Retention ----------------------------------------------------------

    /// Apply `retention` after a successful build, which used or published
    /// the artifacts `in_use`, the last to be evicted. Holds the store's
    /// lock, so publishes of other processes wait for it.
    pub fn trim(&self, retention: &Retention, in_use: &[Digest]) -> io::Result<Trimmed> {
        if self.inner.failure.get().is_some() || !self.inner.root.exists() {
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
        // go once they are old and their advisory staging lease is unheld: a
        // younger or leased file may belong to a running writer.
        let mut objects: HashMap<Digest, u64> = HashMap::new();
        for file in fs::read_dir(self.objects())?.flatten() {
            let name = file.file_name();
            let name = name.to_string_lossy();
            if name.starts_with(TEMP_PREFIX) {
                if old(&file) && remove_unheld_temporary(&self.inner.root, &file.path()) {
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
                if old(&file) && remove_unheld_temporary(&self.inner.root, &file.path()) {
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
        self.trim_leases(retention.grace);
        Ok(trimmed)
    }
}

/// Nanoseconds since the Unix epoch.
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |time| time.as_nanos() as u64)
}

#[cfg(test)]
type AfterLeaseCreateHook = Box<dyn FnMut(&Path)>;

#[cfg(test)]
std::thread_local! {
    static TEST_AFTER_LEASE_CREATE: std::cell::RefCell<Option<AfterLeaseCreateHook>> =
        const { std::cell::RefCell::new(None) };
}

/// A shared exclusive advisory lease file (`DIR/v1/.lease-<token>`) held while
/// one or more [`Temporary`] files are live, bounding open descriptors to one
/// handle per store regardless of batch size.
#[derive(Debug)]
struct StagingLease {
    token: String,
    path: PathBuf,
    file: Option<fs::File>,
}

impl StagingLease {
    fn acquire(root: &Path) -> io::Result<Self> {
        static NEXT_LEASE: AtomicU64 = AtomicU64::new(0);
        let mut last_error = None;
        for _ in 0..8 {
            let token = format!(
                "{}-{}-{}",
                std::process::id(),
                NEXT_LEASE.fetch_add(1, Ordering::Relaxed),
                now()
            );
            let path = root.join(format!("{LEASE_PREFIX}{token}"));
            let file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)?;
            let mut lease = Self {
                token,
                path,
                file: None,
            };
            #[cfg(test)]
            TEST_AFTER_LEASE_CREATE.with(|hook| {
                if let Some(callback) = hook.borrow_mut().as_mut() {
                    callback(&lease.path);
                }
            });
            match file.try_lock() {
                Ok(()) => {
                    lease.file = Some(file);
                    // A concurrent zero-grace trimmer could unlink this lease
                    // file in the window between `create_new` and `try_lock`.
                    if !lease.path.try_exists()? {
                        last_error = Some(io::Error::new(
                            io::ErrorKind::NotFound,
                            "staging lease file was removed before its lock was acquired",
                        ));
                        continue;
                    }
                    return Ok(lease);
                }
                Err(fs::TryLockError::WouldBlock) => {
                    drop(file);
                    last_error = Some(io::Error::new(
                        io::ErrorKind::WouldBlock,
                        "staging lease file was locked before its lock was acquired",
                    ));
                    continue;
                }
                Err(fs::TryLockError::Error(error)) => {
                    drop(file);
                    return Err(error);
                }
            }
        }
        Err(last_error.unwrap_or_else(|| io::Error::other("could not acquire staging lease")))
    }
}

impl Drop for StagingLease {
    fn drop(&mut self) {
        #[cfg(windows)]
        let _ = self.file.take();
        let _ = fs::remove_file(&self.path);
        // Release the exclusive lock only after the lease file is unlinked so a
        // concurrent trimmer never observes an unheld lease file for a live
        // writer.
        let _ = self.file.take();
    }
}

#[derive(Debug)]
struct StagedTemp {
    path: PathBuf,
    _lease: Arc<StagingLease>,
}

/// A complete temporary file protected by a shared advisory staging lease,
/// removed when dropped unless published.
#[derive(Debug)]
struct Temporary(Option<Box<StagedTemp>>);

impl Temporary {
    /// Write `bytes` to a new temporary file in `dir` under `lease`, closing
    /// the data file immediately after flushing while keeping `lease` held
    /// until rename or drop.
    fn write_with_lease(dir: &Path, bytes: &[u8], lease: Arc<StagingLease>) -> io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = dir.join(format!(
            "{LEASED_TEMP_PREFIX}{}_{}",
            lease.token,
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        let temporary = Self(Some(Box::new(StagedTemp {
            path,
            _lease: lease,
        })));
        let result = file.write_all(bytes).and_then(|()| file.flush());
        drop(file);
        result?;
        Ok(temporary)
    }

    fn approximate_bytes(&self) -> usize {
        self.0.as_ref().map_or(0, |inner| {
            std::mem::size_of::<StagedTemp>() + inner.path.as_os_str().len()
        })
    }

    /// Rename it to `target` and release its hold on the staging lease. Where
    /// the rename fails because `target` is open (Windows), a `target` that
    /// already holds what `holds` accepts counts as written.
    fn publish(mut self, target: &Path, holds: impl Fn(&[u8]) -> bool) -> io::Result<()> {
        let Some(inner) = self.0.as_ref() else {
            return Ok(());
        };
        match fs::rename(&inner.path, target) {
            Ok(()) => {
                // Disarm unlink-on-drop and release our hold on the lease.
                drop(self.0.take());
                Ok(())
            }
            Err(_) if fs::read(target).is_ok_and(|existing| holds(&existing)) => Ok(()),
            Err(error) => Err(error),
        }
    }
}

impl Drop for Temporary {
    fn drop(&mut self) {
        if let Some(inner) = self.0.take() {
            let _ = fs::remove_file(&inner.path);
            // Drop `inner.lease` only after `&inner.path` is unlinked so a
            // concurrent trimmer never observes an unleased temporary.
            drop(inner);
        }
    }
}

fn lease_token_from_temp_name(name: &str) -> Option<&str> {
    let rest = name.strip_prefix(LEASED_TEMP_PREFIX)?;
    let (token, _) = rest.split_once('_')?;
    (!token.is_empty() && token.bytes().all(|b| b.is_ascii_digit() || b == b'-')).then_some(token)
}

/// Probe whether `lease_path` is a regular file currently locked by a live
/// writer. Uses a shared lock on a read-only handle (safe on Linux NFS as well
/// as local filesystems and Windows) and never opens non-regular entries.
fn is_lease_held(lease_path: &Path) -> bool {
    let Ok(metadata) = fs::symlink_metadata(lease_path) else {
        return false;
    };
    if !metadata.file_type().is_file() {
        return false;
    }
    let Ok(file) = fs::File::open(lease_path) else {
        return false;
    };
    file.try_lock_shared().is_err()
}

/// Remove an aged lease file only when no live writer holds its exclusive lock.
/// Non-regular entries (such as FIFOs or symlinks) are unlinked directly
/// without opening them.
fn remove_unheld_lease(path: &Path) -> bool {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return false;
    };
    if !metadata.file_type().is_file() {
        return fs::remove_file(path).is_ok();
    }
    let Ok(file) = fs::File::open(path) else {
        return false;
    };
    match file.try_lock_shared() {
        Ok(()) => {
            let removed = fs::remove_file(path).is_ok();
            drop(file);
            removed
        }
        Err(_) => false,
    }
}

/// Remove an aged temporary file only when no live writer holds its advisory
/// staging lease. Non-regular entries (such as stale FIFOs or symlinks) are
/// unlinked directly without opening them.
fn remove_unheld_temporary(root: &Path, path: &Path) -> bool {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return false;
    };
    if !metadata.file_type().is_file() {
        return fs::remove_file(path).is_ok();
    }
    if let Some(name) = path.file_name().and_then(|name| name.to_str())
        && let Some(token) = lease_token_from_temp_name(name)
    {
        let lease_path = root.join(format!("{LEASE_PREFIX}{token}"));
        if is_lease_held(&lease_path) {
            return false;
        }
        return fs::remove_file(path).is_ok();
    }
    let Ok(file) = fs::File::open(path) else {
        return false;
    };
    match file.try_lock_shared() {
        Ok(()) => {
            let removed = fs::remove_file(path).is_ok();
            drop(file);
            removed
        }
        Err(_) => false,
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

    #[test]
    fn temporary_lease_blocks_trim_and_releases_after_publish() {
        let dir = std::env::temp_dir().join(format!(
            "light-cljs-temp-lease-{}-{}",
            std::process::id(),
            now()
        ));
        let store = Store::open(&dir);
        fs::create_dir_all(store.outputs()).unwrap();

        let live = store
            .write_temporary(&store.outputs(), b"in-flight")
            .unwrap();
        let lease_path = live.0.as_ref().unwrap()._lease.path.clone();
        let stale = store.outputs().join(format!("{TEMP_PREFIX}stale-0-0"));
        fs::write(&stale, b"abandoned").unwrap();
        std::thread::sleep(Duration::from_millis(20));

        let zero_grace = Retention {
            grace: Duration::ZERO,
            ..Retention::default()
        };
        let key = digest(b"out");
        assert!(
            store
                .write_output(&key, &[("a.js", "1")], &zero_grace, MAX_OUTPUT_BYTES)
                .is_some()
        );
        assert!(
            live.0.as_ref().unwrap().path.exists(),
            "leased output temporary must survive zero-grace trim"
        );
        assert!(
            lease_path.exists(),
            "active staging lease file must survive zero-grace trim"
        );
        assert!(
            !stale.exists(),
            "unheld output temporary must be removed by zero-grace trim"
        );

        let target = store.outputs().join("published");
        live.publish(&target, |existing| existing == b"in-flight")
            .unwrap();
        // The published file is not locked and the shared lease file is cleaned
        // up once the last live temporary finishes.
        let file = fs::File::open(&target).unwrap();
        assert!(
            file.try_lock_shared().is_ok(),
            "staging lease leaked onto published target"
        );
        drop(file);
        assert!(
            !lease_path.exists(),
            "shared lease file must be removed when the last temporary drops"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn staging_survives_zero_grace_trim_during_creation() {
        struct ResetHook;
        impl Drop for ResetHook {
            fn drop(&mut self) {
                TEST_AFTER_LEASE_CREATE.with(|hook| {
                    *hook.borrow_mut() = None;
                });
            }
        }

        let dir = std::env::temp_dir().join(format!(
            "light-cljs-lease-race-{}-{}",
            std::process::id(),
            now()
        ));
        let store = Store::open(&dir);
        store.prepare().unwrap();
        let zero_grace = Retention {
            grace: Duration::ZERO,
            ..Retention::default()
        };

        // Case 1: a zero-grace trim removes the brand-new lease file in the
        // window between `create_new` and `try_lock`.
        let trim_store = store.clone();
        let mut fired_unlink = false;
        let _reset = ResetHook;
        TEST_AFTER_LEASE_CREATE.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move |lease_path: &Path| {
                if !fired_unlink {
                    fired_unlink = true;
                    std::thread::sleep(Duration::from_millis(20));
                    trim_store.trim(&zero_grace, &[]).unwrap();
                    assert!(
                        !lease_path.exists(),
                        "zero-grace trim must have unlinked the unleased file"
                    );
                }
            }));
        });
        let staged_1 = store
            .stage_encoded(
                "race.unlink".into(),
                digest(b"k1"),
                GeneratedNames::default(),
                b"artifact-1".to_vec(),
            )
            .unwrap();
        let results_1 = store.commit_batch(vec![staged_1]);
        assert!(matches!(results_1.as_slice(), [Publish::Published(_)]));
        assert_eq!(store.candidates("race.unlink").len(), 1);

        // Case 2: a concurrent trimmer holds a shared probe lock on the
        // brand-new lease file when `StagingLease::acquire` calls `try_lock`.
        let mut held_probe: Option<fs::File> = None;
        TEST_AFTER_LEASE_CREATE.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move |lease_path: &Path| {
                if held_probe.is_none() {
                    let probe = fs::File::open(lease_path).unwrap();
                    probe.try_lock_shared().unwrap();
                    held_probe = Some(probe);
                }
            }));
        });
        let staged_2 = store
            .stage_encoded(
                "race.contested".into(),
                digest(b"k2"),
                GeneratedNames::default(),
                b"artifact-2".to_vec(),
            )
            .unwrap();
        // Clear the hook so `held_probe` drops before final cleanup.
        TEST_AFTER_LEASE_CREATE.with(|hook| {
            *hook.borrow_mut() = None;
        });
        let results_2 = store.commit_batch(vec![staged_2]);
        assert!(matches!(results_2.as_slice(), [Publish::Published(_)]));
        assert_eq!(store.candidates("race.contested").len(), 1);

        let _ = fs::remove_dir_all(&dir);
    }
}
